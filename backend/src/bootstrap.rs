//! First administrator and internal workspace provisioning.

use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

use anyhow::{Context, Result, bail};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use uuid::Uuid;

use crate::{
    Config,
    auth::Role,
    departments::{DepartmentDraft, insert_department},
};

/// Creates the fresh workspace and its administrator, saving the generated password
/// to a new owner-only file. Repeated successful runs never rotate credentials.
///
/// # Errors
///
/// Rejects a config without a fixed project, an invalid email/path, a populated
/// non-demo database,
/// conflicting identity/project, or any database or credentials-file failure.
pub async fn create_admin(config: &Config, email: &str, credentials: &Path) -> Result<bool> {
    let project_id = config
        .product
        .fixed_project_id()
        .filter(|id| !id.is_nil())
        .context("product configuration with a non-nil project-id is required")?;
    let email = email.trim().to_lowercase();
    if email.is_empty()
        || email.len() > 320
        || !email.contains('@')
        || email.chars().any(char::is_whitespace)
        || email.chars().any(char::is_control)
        || email == crate::demo::DEMO_EMAIL
    {
        bail!("a valid non-demo administrator email is required");
    }
    if !credentials.is_absolute() || credentials.file_name().is_none() {
        bail!("credentials path must be an absolute file path");
    }

    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&config.pg.url)
        .await
        .context("failed to connect for bootstrap")?;
    sqlx::migrate!("./migrations").run(&db).await?;
    let mut tx = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('workspace-bootstrap', 0))")
        .execute(&mut *tx)
        .await?;
    let initialized: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM users u JOIN memberships m ON m.user_id=u.id \
         JOIN projects p ON p.tenant_id=m.tenant_id \
         WHERE lower(u.email)=$1 AND u.status='active' AND u.password_hash IS NOT NULL \
         AND m.role='admin' AND m.project_id IS NULL AND m.revoked_at IS NULL \
         AND p.id=$2 AND p.status='active')",
    )
    .bind(&email)
    .bind(project_id)
    .fetch_one(&mut *tx)
    .await?;
    if initialized {
        tx.commit().await?;
        return Ok(false);
    }
    let populated: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM projects WHERE id<>$1) \
         OR EXISTS (SELECT 1 FROM users WHERE NOT is_demo)",
    )
    .bind(crate::demo::DEMO_PROJECT_ID)
    .fetch_one(&mut *tx)
    .await?;
    if populated {
        bail!("bootstrap requires a fresh database; existing non-demo records were found");
    }

    let mut random = [0u8; 32];
    rand::rng().fill_bytes(&mut random);
    let password = URL_SAFE_NO_PAD.encode(random);
    let password_for_hash = password.clone();
    let password_hash = tokio::task::spawn_blocking(move || -> Result<String> {
        let mut salt = [0u8; 16];
        rand::rng().fill_bytes(&mut salt);
        let salt = SaltString::encode_b64(&salt).map_err(anyhow::Error::msg)?;
        Argon2::default()
            .hash_password(password_for_hash.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(anyhow::Error::msg)
    })
    .await??;
    let tenant_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id,name) VALUES ($1,'Support')")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO users (id,email,display_name,status,password_hash) VALUES ($1,$2,'Administrator','active',$3)")
        .bind(user_id).bind(&email).bind(password_hash).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO projects (id,tenant_id,name,slug,status) VALUES ($1,$2,'Support','support','active')")
        .bind(project_id).bind(tenant_id).execute(&mut *tx).await?;
    for (role, name, system) in [
        (Role::Admin, "Administrator", true),
        (Role::Manager, "Manager", false),
        (Role::Operator, "Operator", false),
    ] {
        sqlx::query("INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions,is_system,updated_by_user_id) VALUES ($1,$2,$3,$4,$3,$5,$6,$7)")
            .bind(tenant_id).bind(project_id).bind(role.as_str()).bind(name)
            .bind(role.permissions()).bind(system).bind(user_id).execute(&mut *tx).await?;
    }
    let department = DepartmentDraft::support().normalize()?;
    let department_id = insert_department(&mut tx, tenant_id, project_id, &department, 0).await?;
    sqlx::query("INSERT INTO inboxes (id,tenant_id,project_id,department_id,name,status) VALUES ($1,$2,$3,$4,'Support','active')")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(project_id).bind(department_id)
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE projects SET default_department_id=$2 WHERE id=$1")
        .bind(project_id)
        .bind(department_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships (id,tenant_id,project_id,user_id,role,permissions) VALUES ($1,$2,NULL,$3,'admin',$4)")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(user_id).bind(Role::Admin.permissions())
        .execute(&mut *tx).await?;

    // Refuse to overwrite any file (including symlinks). Persist credentials
    // before committing the account so a filesystem error leaves no locked-out user.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(credentials)
        .context("failed to create new owner-only credentials file")?;
    writeln!(file, "EMAIL={email}\nPASSWORD={password}")?;
    file.sync_all()?;
    tx.commit().await.context(
        "bootstrap commit failed; retain the credentials file while checking database state",
    )?;
    Ok(true)
}
