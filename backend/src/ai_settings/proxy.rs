//! Agent-owned proxy discovery settings. Credentials never appear in management responses.

use super::{
    decrypt_secret, encrypt_secret, insert_audit, load_secret_encryption_key,
    lock_profile_for_management_scope, require_management,
    require_project_wide_for_autonomous_profile,
};
use crate::{
    AppState,
    auth::ActorContext,
    error::AppError,
    resource_visibility::{self, Resource},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/profiles/{profile_id}/proxy",
            get(settings).put(save),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
}

#[derive(Default, FromRow, Serialize)]
struct Settings {
    enabled: bool,
    service_url: String,
    region: Option<String>,
    country: Option<String>,
    token_configured: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    enabled: bool,
    service_url: String,
    region: Option<String>,
    country: Option<String>,
    token: Option<String>,
    #[serde(default)]
    clear_token: bool,
}

impl Input {
    fn normalize(mut self) -> Result<Self, AppError> {
        self.service_url = self.service_url.trim().to_owned();
        if !self.service_url.is_empty() {
            let url = url::Url::parse(&self.service_url)
                .map_err(|_| invalid("Invalid proxy service URL"))?;
            let host = url.host_str().unwrap_or_default();
            if self.service_url.len() > 2048
                || url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.port().is_some()
                || !host.contains('.')
                || host.parse::<std::net::IpAddr>().is_ok()
                || !host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err(invalid(
                    "Proxy service URL must use HTTPS with a public hostname, no credentials, query, fragment or custom port",
                ));
            }
            self.service_url = url.to_string();
        } else if self.enabled {
            return Err(invalid("Proxy service URL is required"));
        }
        self.region = self
            .region
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        self.country = self
            .country
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty());
        if self.region.is_some() && self.country.is_some() {
            return Err(invalid("Select region or country, not both"));
        }
        if self.region.as_ref().is_some_and(|s| {
            s.len() > 64
                || !s.as_bytes()[0].is_ascii_lowercase()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        }) {
            return Err(invalid("Invalid proxy region"));
        }
        if self
            .country
            .as_ref()
            .is_some_and(|s| s.len() != 2 || !s.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(invalid("Country must be a two-letter code"));
        }
        self.token = self
            .token
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        if self
            .token
            .as_ref()
            .is_some_and(|s| s.len() > 4096 || !s.bytes().all(|b| (33..=126).contains(&b)))
        {
            return Err(invalid("Invalid proxy service token"));
        }
        if self.clear_token && (self.enabled || self.token.is_some()) {
            return Err(invalid("Disable proxy before removing its token"));
        }
        Ok(self)
    }
}

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.to_owned())
}

fn aad(tenant: Uuid, project: Uuid, profile: Uuid) -> Vec<u8> {
    format!("tzomet:ai-profile-proxy:v1:{tenant}:{project}:{profile}").into_bytes()
}

async fn authorized(
    state: &AppState,
    actor: &ActorContext,
    profile: Uuid,
) -> Result<(Uuid, Transaction<'static, Postgres>), AppError> {
    require_management(actor)?;
    let project =
        resource_visibility::owner_project(&state.db, actor, Resource::Profile, profile).await?;
    let mut tx = state.db.begin().await?;
    lock_profile_for_management_scope(&mut tx, actor, project, profile).await?;
    require_project_wide_for_autonomous_profile(&mut tx, actor, project, profile).await?;
    Ok((project, tx))
}

async fn read(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    project: Uuid,
    profile: Uuid,
) -> Result<Settings, AppError> {
    Ok(sqlx::query_as("SELECT enabled,service_url,region,country,encrypted_token IS NOT NULL AS token_configured FROM ai_profile_proxy_settings WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3")
        .bind(tenant).bind(project).bind(profile).fetch_optional(&mut **tx).await?.unwrap_or_default())
}

async fn settings(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
) -> Result<Json<Settings>, AppError> {
    let (project, mut tx) = authorized(&state, &actor, profile).await?;
    let result = read(&mut tx, actor.tenant_id, project, profile).await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn save(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
    Json(input): Json<Input>,
) -> Result<Json<Settings>, AppError> {
    let input = input.normalize()?;
    let (project, mut tx) = authorized(&state, &actor, profile).await?;
    let previous = read(&mut tx, actor.tenant_id, project, profile).await?;
    if previous.token_configured
        && previous.service_url != input.service_url
        && input.token.is_none()
        && !input.clear_token
    {
        return Err(invalid(
            "Enter a new token or remove the saved token when changing the service URL",
        ));
    }
    if input.enabled && input.token.is_none() && !previous.token_configured {
        return Err(invalid("Proxy service token is required"));
    }
    let encrypted = input
        .token
        .as_ref()
        .map(|token| {
            encrypt_secret(
                &load_secret_encryption_key(&state)?,
                token.as_bytes(),
                state.config.secrets.key_version.clone(),
                &aad(actor.tenant_id, project, profile),
            )
        })
        .transpose()?;
    sqlx::query("INSERT INTO ai_profile_proxy_settings (tenant_id,project_id,ai_profile_id) VALUES ($1,$2,$3) ON CONFLICT (ai_profile_id) DO NOTHING")
        .bind(actor.tenant_id).bind(project).bind(profile).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_profile_proxy_settings SET enabled=$4,service_url=$5,region=$6,country=$7,encrypted_token=CASE WHEN $11 THEN NULL ELSE COALESCE($8,encrypted_token) END,token_nonce=CASE WHEN $11 THEN NULL ELSE COALESCE($9,token_nonce) END,key_version=CASE WHEN $11 THEN NULL ELSE COALESCE($10,key_version) END,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(input.enabled).bind(&input.service_url).bind(&input.region).bind(&input.country)
        .bind(encrypted.as_ref().map(|s| s.ciphertext.as_slice())).bind(encrypted.as_ref().map(|s| s.nonce.as_slice())).bind(encrypted.as_ref().map(|s| s.key_version.as_str())).bind(input.clear_token)
        .execute(&mut *tx).await?;
    insert_audit(
        &mut tx,
        &actor,
        project,
        "ai_profile.proxy_updated",
        "ai_profile",
        profile,
    )
    .await?;
    let result = read(&mut tx, actor.tenant_id, project, profile).await?;
    tx.commit().await?;
    Ok(Json(result))
}

#[derive(Serialize)]
pub(crate) struct RuntimeProxy {
    service_url: String,
    token: String,
    region: Option<String>,
    country: Option<String>,
}

#[derive(FromRow)]
struct SecretRow {
    service_url: String,
    region: Option<String>,
    country: Option<String>,
    encrypted_token: Vec<u8>,
    token_nonce: Vec<u8>,
    key_version: String,
}

pub(crate) async fn load_runtime(
    state: &AppState,
    tenant: Uuid,
    project: Uuid,
    profile: Uuid,
) -> anyhow::Result<Option<RuntimeProxy>> {
    let row: Option<SecretRow> = sqlx::query_as("SELECT service_url,region,country,encrypted_token,token_nonce,key_version FROM ai_profile_proxy_settings WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3 AND enabled")
        .bind(tenant).bind(project).bind(profile).fetch_optional(&state.db).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    anyhow::ensure!(
        row.key_version == state.config.secrets.key_version,
        "Proxy token key version unavailable"
    );
    let plaintext = decrypt_secret(
        &load_secret_encryption_key(state)?,
        &row.encrypted_token,
        &row.token_nonce,
        &aad(tenant, project, profile),
    )?;
    Ok(Some(RuntimeProxy {
        service_url: row.service_url,
        token: String::from_utf8(plaintext)?,
        region: row.region,
        country: row.country,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn input(value: serde_json::Value) -> Result<Input, AppError> {
        serde_json::from_value::<Input>(value).unwrap().normalize()
    }
    #[test]
    fn proxy_settings_validate_filters_and_secret_destination() {
        for bad in [
            "http://localhost:8080/api/proxies/random",
            "https://user:secret@example.com/random",
            "https://127.0.0.1/random",
            "https://example.com/random?country=DE",
        ] {
            assert!(input(json!({"enabled":true,"service_url":bad})).is_err());
        }
        assert!(input(json!({"enabled":true,"service_url":"https://proxy.example/random","region":"europe","country":"DE"})).is_err());
        assert!(input(json!({"enabled":true,"service_url":"https://proxy.example/random","token":"a\r\nb"})).is_err());
        let valid = input(
            json!({"enabled":true,"service_url":"https://proxy.example/random","country":" de "}),
        )
        .unwrap();
        assert_eq!(valid.country.as_deref(), Some("DE"));
        let public = serde_json::to_value(Settings::default()).unwrap();
        assert!(public.get("token").is_none());
    }
    #[test]
    fn proxy_token_is_bound_to_agent_and_project() {
        let tenant = Uuid::now_v7();
        let project = Uuid::now_v7();
        let profile = Uuid::now_v7();
        let key = [7u8; 32];
        let encrypted =
            encrypt_secret(&key, b"token", "v1".into(), &aad(tenant, project, profile)).unwrap();
        assert_eq!(
            decrypt_secret(
                &key,
                &encrypted.ciphertext,
                &encrypted.nonce,
                &aad(tenant, project, profile)
            )
            .unwrap(),
            b"token"
        );
        assert!(
            decrypt_secret(
                &key,
                &encrypted.ciphertext,
                &encrypted.nonce,
                &aad(tenant, project, Uuid::now_v7())
            )
            .is_err()
        );
    }
}
