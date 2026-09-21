//! Project administration, resource summaries, and project-scoped membership.

use std::str::FromStr;

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{ActorContext, Role, lock_project_role},
    error::AppError,
};

/// Routes for project lifecycle and project member access.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects", get(list_projects))
        .route("/api/v1/projects/{project_id}", patch(update_project))
        .route(
            "/api/v1/projects/{project_id}/users",
            post(create_project_user),
        )
        .route(
            "/api/v1/projects/{project_id}/members",
            get(list_project_members).post(grant_project_member),
        )
        .route(
            "/api/v1/projects/{project_id}/members/{membership_id}",
            patch(update_project_member).delete(revoke_project_member),
        )
}

#[derive(Debug, FromRow)]
struct ProjectRow {
    id: Uuid,
    parent_project_id: Option<Uuid>,
    name: String,
    slug: String,
    status: String,
    project_kind: ProjectKind,
    company_profile: Option<sqlx::types::Json<CompanyProfile>>,
    default_department_id: Option<Uuid>,
    director_enabled: bool,
    default_director_workspace: bool,
    director_menu: sqlx::types::Json<crate::departments::DirectorMenu>,
    actor_role: String,
    inbox_count: i64,
    channel_count: i64,
    conversation_count: i64,
    team_count: i64,
    member_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectResponse {
    id: Uuid,
    parent_project_id: Option<Uuid>,
    name: String,
    slug: String,
    status: String,
    project_kind: ProjectKind,
    company_profile: Option<CompanyProfile>,
    default_department_id: Option<Uuid>,
    director_enabled: bool,
    default_director_workspace: bool,
    director_menu: sqlx::types::Json<crate::departments::DirectorMenu>,
    role: Role,
    current: bool,
    inbox_count: i64,
    channel_count: i64,
    conversation_count: i64,
    team_count: i64,
    member_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ProjectResponse {
    fn try_from_row(row: ProjectRow, current_project_id: Option<Uuid>) -> Result<Self, AppError> {
        Ok(Self {
            current: current_project_id == Some(row.id),
            id: row.id,
            parent_project_id: row.parent_project_id,
            name: row.name,
            slug: row.slug,
            status: row.status,
            project_kind: row.project_kind,
            company_profile: row.company_profile.map(|profile| profile.0),
            default_department_id: row.default_department_id,
            director_enabled: row.director_enabled,
            default_director_workspace: row.default_director_workspace,
            director_menu: row.director_menu,
            role: Role::from_str(&row.actor_role)?,
            inbox_count: row.inbox_count,
            channel_count: row.channel_count,
            conversation_count: row.conversation_count,
            team_count: row.team_count,
            member_count: row.member_count,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug, Serialize)]
struct ProjectListResponse {
    items: Vec<ProjectResponse>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum ProjectKind {
    #[default]
    Project,
    Company,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct CompanyProfile {
    description: String,
    industry: String,
    country: String,
    website: String,
    employee_count: Option<i64>,
    products_services: String,
    goals: String,
}

impl CompanyProfile {
    fn normalize(mut self) -> Result<Self, AppError> {
        for (field, value, limit) in [
            ("description", &mut self.description, 10_000),
            ("industry", &mut self.industry, 200),
            ("country", &mut self.country, 200),
            ("website", &mut self.website, 200),
            ("products_services", &mut self.products_services, 10_000),
            ("goals", &mut self.goals, 10_000),
        ] {
            *value = value.trim().to_owned();
            if value.chars().count() > limit {
                return Err(AppError::BadRequest(format!(
                    "company_profile.{field} must contain at most {limit} characters"
                )));
            }
        }
        if self
            .employee_count
            .is_some_and(|count| !(0..=10_000_000).contains(&count))
        {
            return Err(AppError::BadRequest(
                "company_profile.employee_count must be between 0 and 10000000".to_owned(),
            ));
        }
        Ok(self)
    }
}

async fn list_projects(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<ProjectListResponse>, AppError> {
    if !actor.access_all_projects() {
        actor.require("projects:read")?;
    }
    let mut rows = load_accessible_project_rows(&state.db, &actor).await?;
    if actor.access_all_projects() {
        for row in &mut rows {
            actor.role().as_str().clone_into(&mut row.actor_role);
            if actor.require("projects:read").is_err() {
                row.inbox_count = 0;
                row.channel_count = 0;
                row.conversation_count = 0;
                row.team_count = 0;
                row.member_count = 0;
            }
        }
    }
    let items = rows
        .into_iter()
        .map(|row| ProjectResponse::try_from_row(row, actor.project_id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ProjectListResponse { items }))
}

async fn load_accessible_project_rows(
    db: &PgPool,
    actor: &ActorContext,
) -> Result<Vec<ProjectRow>, AppError> {
    sqlx::query_as::<_, ProjectRow>(
        r#"
        SELECT project.id, project.parent_project_id, project.name, project.slug, project.status, project.project_kind,
               CASE WHEN $11::boolean AND EXISTS (
                   SELECT 1 FROM memberships AS profile_access
                   WHERE profile_access.tenant_id = project.tenant_id
                     AND profile_access.user_id = $2
                     AND profile_access.role = 'admin'
                     AND profile_access.department_id IS NULL
                     AND profile_access.revoked_at IS NULL
                     AND (profile_access.project_id = project.id OR profile_access.project_id IS NULL)
               ) THEN project.company_profile ELSE NULL END AS company_profile,
               COALESCE(actor_scope.department_id, project.default_department_id) AS default_department_id,
               project.director_enabled, project.default_director_workspace, project.director_menu,
               COALESCE((
                   SELECT COALESCE(project_role.base_role, membership.role)
                   FROM memberships AS membership
                   LEFT JOIN project_roles AS project_role
                     ON project_role.tenant_id = membership.tenant_id
                    AND project_role.project_id = project.id
                    AND project_role.id = membership.role
                   WHERE membership.tenant_id = project.tenant_id
                     AND membership.user_id = $2
                     AND membership.revoked_at IS NULL
                     AND (membership.project_id = project.id OR membership.project_id IS NULL)
                   ORDER BY
                       CASE COALESCE(project_role.base_role, membership.role)
                           WHEN 'admin' THEN 0 ELSE 1 END,
                       CASE WHEN membership.project_id IS NULL THEN 0 ELSE 1 END,
                       membership.created_at ASC,
                       membership.id ASC
                   LIMIT 1
               ), $5::text) AS actor_role,
               (SELECT COUNT(*) FROM inboxes
                WHERE tenant_id = project.tenant_id AND project_id = project.id
                  AND (actor_scope.department_id IS NULL OR department_id = actor_scope.department_id)
                  AND ($9::uuid[] IS NULL OR id = ANY($9))) AS inbox_count,
               (SELECT COUNT(*) FROM channel_connections
                WHERE tenant_id = project.tenant_id AND project_id = project.id
                  AND deleted_at IS NULL
                  AND ($9::uuid[] IS NULL OR inbox_id = ANY($9))
                  AND (actor_scope.department_id IS NULL OR inbox_id IN (
                      SELECT id FROM inboxes WHERE tenant_id = project.tenant_id AND department_id = actor_scope.department_id
                  ))) AS channel_count,
               (SELECT COUNT(*) FROM conversations AS conversation
                WHERE conversation.tenant_id = project.tenant_id
                  AND conversation.project_id = project.id
                  AND ($9::uuid[] IS NULL OR conversation.inbox_id = ANY($9))
                  AND (actor_scope.department_id IS NULL OR conversation.inbox_id IN (
                      SELECT id FROM inboxes WHERE tenant_id = project.tenant_id AND department_id = actor_scope.department_id
                  ))
                  AND (NOT $6::boolean OR EXISTS (
                      SELECT 1 FROM widget_sessions AS demo_session
                      WHERE demo_session.tenant_id = conversation.tenant_id
                        AND demo_session.project_id = conversation.project_id
                        AND demo_session.channel_connection_id = conversation.channel_connection_id
                        AND demo_session.contact_id = conversation.contact_id
                        AND conversation.channel_connection_id = $8
                        AND demo_session.client_ip = $7::text::inet
                        AND demo_session.revoked_at IS NULL
                        AND demo_session.visitor_data_expires_at > now()
                  ))) AS conversation_count,
               (SELECT COUNT(*) FROM teams
                WHERE tenant_id = project.tenant_id AND project_id = project.id
                  AND actor_scope.department_id IS NULL AND $9::uuid[] IS NULL) AS team_count,
               (SELECT COUNT(*) FROM memberships
                WHERE tenant_id = project.tenant_id AND project_id = project.id
                  AND revoked_at IS NULL
                  AND (actor_scope.department_id IS NULL OR department_id = actor_scope.department_id OR department_id IS NULL)) AS member_count,
               project.created_at, project.updated_at
        FROM projects AS project
        LEFT JOIN LATERAL (
            SELECT membership.department_id
            FROM memberships AS membership
            LEFT JOIN project_roles AS role ON role.tenant_id = membership.tenant_id
              AND role.project_id = project.id AND role.id = membership.role
            WHERE membership.tenant_id = project.tenant_id AND membership.user_id = $2
              AND membership.revoked_at IS NULL
              AND (membership.project_id = project.id OR membership.project_id IS NULL)
            ORDER BY CASE COALESCE(role.base_role, membership.role) WHEN 'admin' THEN 0 ELSE 1 END,
                     CASE WHEN membership.project_id IS NULL THEN 0 ELSE 1 END,
                     membership.created_at, membership.id
            LIMIT 1
        ) AS actor_scope ON true
        WHERE project.tenant_id = $1
          AND project.deleted_at IS NULL
          AND ($12::uuid IS NULL OR project.id = $12)
          AND (
              (
                  $4::boolean
                  AND EXISTS (
                      SELECT 1
                      FROM memberships AS accessible
                      WHERE accessible.tenant_id = project.tenant_id
                        AND accessible.user_id = $2
                        AND accessible.revoked_at IS NULL
                        AND (accessible.project_id = project.id OR accessible.project_id IS NULL)
                  )
              )
              OR (
                  NOT $4::boolean
                  AND (($10 AND project.status = 'active') OR (NOT $10 AND ($3::uuid IS NULL OR project.id = $3)))
              )
          )
        ORDER BY project.name ASC, project.id ASC
        LIMIT 500
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(actor.project_id)
    .bind(actor.is_password_session())
    .bind(actor.role().as_str())
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .bind(if actor.is_password_session() { None } else { actor.inbox_scope() })
    .bind(actor.access_all_projects())
    .bind(actor.require_session_admin().is_ok() && !actor.is_demo())
    .bind(actor.deployment_project_id())
    .fetch_all(db)
    .await
    .map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateProjectRequest {
    name: Option<String>,
    slug: Option<String>,
    status: Option<String>,
    // A PATCH distinguishes an omitted field from an explicit null (detach).
    #[allow(clippy::option_option)]
    #[serde(default, deserialize_with = "deserialize_parent_project_id")]
    parent_project_id: Option<Option<Uuid>>,
    default_department_id: Option<Uuid>,
    director_enabled: Option<bool>,
    default_director_workspace: Option<bool>,
    director_menu: Option<crate::departments::DirectorMenu>,
    project_kind: Option<ProjectKind>,
    company_profile: Option<CompanyProfile>,
}

#[allow(clippy::option_option)]
fn deserialize_parent_project_id<'de, D>(deserializer: D) -> Result<Option<Option<Uuid>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Uuid>::deserialize(deserializer).map(Some)
}

async fn update_project(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
    Json(request): Json<UpdateProjectRequest>,
) -> Result<Json<ProjectResponse>, AppError> {
    require_project_admin_access(&state.db, &actor, project_id).await?;
    if request.name.is_none()
        && request.slug.is_none()
        && request.status.is_none()
        && request.parent_project_id.is_none()
        && request.default_department_id.is_none()
        && request.director_enabled.is_none()
        && request.default_director_workspace.is_none()
        && request.director_menu.is_none()
        && request.project_kind.is_none()
        && request.company_profile.is_none()
    {
        return Err(AppError::BadRequest(
            "at least one project field must be provided".to_owned(),
        ));
    }
    if let Some(menu) = &request.director_menu {
        menu.validate()?;
    }
    let name = request
        .name
        .map(|value| normalize_required_text(value, 200, "name"))
        .transpose()?;
    let slug = request.slug.map(normalize_slug).transpose()?;
    let status = request.status.map(normalize_project_status).transpose()?;
    let supplied_profile = request
        .company_profile
        .map(CompanyProfile::normalize)
        .transpose()?;
    let profile_changed = supplied_profile.is_some();
    if status.as_deref() == Some("disabled") && actor.project_id == Some(project_id) {
        return Err(AppError::Conflict(
            "switch to another project before disabling the active project".to_owned(),
        ));
    }

    let mut transaction = state.db.begin().await?;
    if request.parent_project_id.is_some() || status.is_some() {
        lock_project_hierarchy(&mut transaction, actor.tenant_id).await?;
    }
    let (current_kind, current_profile, current_parent, current_director): (
        ProjectKind,
        Option<sqlx::types::Json<CompanyProfile>>,
        Option<Uuid>,
        bool,
    ) =
        sqlx::query_as(
            "SELECT project_kind, company_profile, parent_project_id, director_enabled FROM projects WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    if request.default_director_workspace == Some(true)
        && !request.director_enabled.unwrap_or(current_director)
    {
        return Err(AppError::BadRequest(
            "default_director_workspace requires an enabled control center".to_owned(),
        ));
    }
    if request.director_enabled == Some(false) {
        let has_departments: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM departments WHERE tenant_id = $1 AND project_id = $2)",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !has_departments {
            return Err(AppError::BadRequest(
                "create a department before disabling the control center".to_owned(),
            ));
        }
    }
    if let Some(parent_project_id) = request.parent_project_id {
        actor.require("projects:manage")?;
        if let Some(parent_project_id) = parent_project_id {
            validate_parent_project(&mut transaction, &actor, project_id, parent_project_id)
                .await?;
        }
    }
    let parent_project_id = request.parent_project_id.unwrap_or(current_parent);
    if status.as_deref() == Some("disabled") {
        let has_active_children: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM projects WHERE tenant_id = $1 AND parent_project_id = $2 AND status = 'active')",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
        if has_active_children {
            return Err(AppError::Conflict(
                "move or disable active subprojects before disabling their parent project"
                    .to_owned(),
            ));
        }
    }
    if status.as_deref() == Some("active")
        && let Some(parent_project_id) = parent_project_id
    {
        let parent_active: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM projects WHERE tenant_id = $1 AND id = $2 AND status = 'active')",
        )
        .bind(actor.tenant_id)
        .bind(parent_project_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !parent_active {
            return Err(AppError::Conflict(
                "activate the parent project or detach this subproject before activating it"
                    .to_owned(),
            ));
        }
    }
    let project_kind = request.project_kind.unwrap_or(current_kind);
    if current_kind == ProjectKind::Company && project_kind == ProjectKind::Project {
        return Err(AppError::Conflict(
            "a company cannot be converted back to an ordinary project".to_owned(),
        ));
    }
    if project_kind == ProjectKind::Project && supplied_profile.is_some() {
        return Err(AppError::BadRequest(
            "company_profile requires project_kind company".to_owned(),
        ));
    }
    let company_profile = if project_kind == ProjectKind::Company {
        Some(
            supplied_profile
                .map(sqlx::types::Json)
                .or(current_profile)
                .unwrap_or_default(),
        )
    } else {
        None
    };
    if let Some(department_id) = request.default_department_id {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM departments WHERE tenant_id = $1 AND project_id = $2 AND id = $3)")
            .bind(actor.tenant_id).bind(project_id).bind(department_id).fetch_one(&mut *transaction).await?;
        if !exists {
            return Err(AppError::BadRequest(
                "default department must belong to this project".to_owned(),
            ));
        }
    }
    let updated = sqlx::query(
        r#"
        UPDATE projects
        SET name = COALESCE($3, name),
            slug = COALESCE($4, slug),
            status = COALESCE($5, status),
            default_department_id = COALESCE($6, default_department_id),
            director_enabled = COALESCE($7, director_enabled),
            default_director_workspace = COALESCE($11, default_director_workspace)
                AND COALESCE($7, director_enabled),
            director_menu = COALESCE($12, director_menu),
            project_kind = $8,
            company_profile = $9,
            parent_project_id = $10,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(name.as_deref())
    .bind(slug.as_deref())
    .bind(status.as_deref())
    .bind(request.default_department_id)
    .bind(request.director_enabled)
    .bind(project_kind)
    .bind(&company_profile)
    .bind(parent_project_id)
    .bind(request.default_director_workspace)
    .bind(request.director_menu.as_ref().map(sqlx::types::Json))
    .execute(&mut *transaction)
    .await
    .map_err(project_write_error)?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "project.updated",
        "project",
        Some(project_id),
        serde_json::json!({ "name": name, "slug": slug, "status": status, "parent_project_id": parent_project_id, "parent_project_changed": request.parent_project_id.is_some() && parent_project_id != current_parent, "default_department_id": request.default_department_id, "director_enabled": request.director_enabled, "default_director_workspace": request.default_director_workspace, "director_menu": request.director_menu, "project_kind": request.project_kind, "company_profile_changed": profile_changed }),
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_project_response(&state, &actor, project_id).await?,
    ))
}

async fn lock_project_hierarchy(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('project-hierarchy:' || $1::uuid::text, 0))",
    )
    .bind(tenant_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn validate_parent_project(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    parent_project_id: Uuid,
) -> Result<(), AppError> {
    actor.require("projects:manage")?;
    if parent_project_id == project_id {
        return Err(AppError::BadRequest(
            "a project cannot be its own parent".to_owned(),
        ));
    }
    let (parent_parent_id, status): (Option<Uuid>, String) = sqlx::query_as(
        r#"
        SELECT project.parent_project_id, project.status
        FROM projects AS project
        WHERE project.tenant_id = $1 AND project.id = $2
          AND EXISTS (
              SELECT 1 FROM memberships AS membership
              LEFT JOIN project_roles AS role
                ON role.tenant_id = membership.tenant_id
               AND role.project_id = project.id AND role.id = membership.role
              WHERE membership.tenant_id = project.tenant_id
                AND membership.user_id = $3 AND membership.role = 'admin'
                AND membership.department_id IS NULL AND membership.revoked_at IS NULL
                AND (membership.project_id = project.id OR membership.project_id IS NULL)
                AND 'projects:manage' = ANY(COALESCE(role.permissions, membership.permissions))
          )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(parent_project_id)
    .bind(actor.actor_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if status != "active" {
        return Err(AppError::Conflict(
            "the parent project must be active".to_owned(),
        ));
    }
    if parent_parent_id.is_some() {
        return Err(AppError::Conflict(
            "a subproject cannot contain other subprojects".to_owned(),
        ));
    }
    let has_children: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM projects WHERE tenant_id = $1 AND parent_project_id = $2)",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    if has_children {
        return Err(AppError::Conflict(
            "a project with subprojects cannot become a subproject".to_owned(),
        ));
    }
    Ok(())
}

/// The project as `actor` sees it in the project list.
pub(crate) async fn load_project_response(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<ProjectResponse, AppError> {
    let project = load_accessible_project_rows(&state.db, actor)
        .await?
        .into_iter()
        .find(|project| project.id == project_id)
        .ok_or(AppError::NotFound)?;
    ProjectResponse::try_from_row(project, actor.project_id)
}

#[derive(Debug, Serialize)]
struct ProjectMemberResponse {
    membership_id: Uuid,
    user_id: Uuid,
    email: String,
    display_name: String,
    chat_display_name: String,
    avatar_url: Option<String>,
    role: Role,
    role_id: String,
    role_name: String,
    department_id: Option<Uuid>,
    director_access: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct ProjectMemberRow {
    membership_id: Uuid,
    user_id: Uuid,
    email: String,
    display_name: String,
    chat_display_name: String,
    avatar_url: Option<String>,
    role: String,
    role_id: String,
    role_name: String,
    department_id: Option<Uuid>,
    director_access: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ProjectMemberRow> for ProjectMemberResponse {
    type Error = AppError;

    fn try_from(row: ProjectMemberRow) -> Result<Self, Self::Error> {
        Ok(Self {
            membership_id: row.membership_id,
            user_id: row.user_id,
            email: row.email,
            display_name: row.display_name,
            chat_display_name: row.chat_display_name,
            avatar_url: row.avatar_url,
            role: Role::from_str(&row.role)?,
            role_id: row.role_id,
            role_name: row.role_name,
            department_id: row.department_id,
            director_access: row.director_access,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug, Serialize)]
struct ProjectMemberListResponse {
    items: Vec<ProjectMemberResponse>,
}

async fn list_project_members(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ProjectMemberListResponse>, AppError> {
    require_project_admin_access(&state.db, &actor, project_id).await?;
    let rows = sqlx::query_as::<_, ProjectMemberRow>(
        r#"
        SELECT membership.id AS membership_id, app_user.id AS user_id,
               app_user.email, app_user.display_name,
               COALESCE(profile.display_name, app_user.display_name) AS chat_display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS avatar_url,
               project_role.base_role AS role, membership.role AS role_id,
               project_role.name AS role_name, membership.department_id, membership.director_access,
               membership.created_at, membership.updated_at
        FROM memberships AS membership
        JOIN users AS app_user ON app_user.id = membership.user_id
        JOIN project_roles AS project_role
          ON project_role.tenant_id = membership.tenant_id
         AND project_role.project_id = membership.project_id
         AND project_role.id = membership.role
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = membership.tenant_id
         AND profile.project_id = membership.project_id
         AND profile.user_id = membership.user_id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = membership.tenant_id
         AND stored_avatar.project_id = membership.project_id
         AND stored_avatar.user_id = membership.user_id
        WHERE membership.tenant_id = $1
          AND membership.project_id = $2
          AND membership.revoked_at IS NULL
        ORDER BY app_user.display_name ASC, app_user.email ASC, membership.id ASC
        LIMIT 500
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let items = rows
        .into_iter()
        .map(ProjectMemberResponse::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ProjectMemberListResponse { items }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProjectUserRequest {
    display_name: String,
    email: String,
    password: String,
    role_id: String,
    department_id: Option<Uuid>,
    #[serde(default)]
    director_access: bool,
}

async fn create_project_user(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateProjectUserRequest>,
) -> Result<(StatusCode, Json<ProjectMemberResponse>), AppError> {
    require_project_admin_access(&state.db, &actor, project_id).await?;
    let email = normalize_email(request.email)?;
    let display_name = normalize_required_text(request.display_name, 200, "display_name")?;
    if request.password.chars().count() < 8 || request.password.len() > 128 {
        return Err(AppError::BadRequest(
            "password must contain at least 8 characters and no more than 128 bytes".to_owned(),
        ));
    }
    let password_hash = tokio::task::spawn_blocking(move || {
        let mut salt_bytes = [0_u8; 16];
        rand::rng().fill_bytes(&mut salt_bytes);
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|error| AppError::internal(anyhow::anyhow!(error.to_string())))?;
        Argon2::default()
            .hash_password(request.password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| AppError::internal(anyhow::anyhow!(error.to_string())))
    })
    .await
    .map_err(AppError::internal)??;
    let mut transaction = state.db.begin().await?;
    let role = lock_project_role(
        &mut transaction,
        actor.tenant_id,
        project_id,
        &request.role_id,
    )
    .await?;
    crate::departments::validate_member_scope(
        &mut transaction,
        actor.tenant_id,
        project_id,
        request.department_id,
        request.director_access,
        role.base_role()? == Role::Admin,
    )
    .await?;
    let user_id = Uuid::now_v7();
    let created = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO users (id, email, display_name, password_hash)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (lower(email)) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(&email)
    .bind(display_name)
    .bind(password_hash)
    .fetch_optional(&mut *transaction)
    .await?;
    if created.is_none() {
        return Err(AppError::Conflict(
            "a user with this email already exists".to_owned(),
        ));
    }
    let membership_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO memberships (
            id, tenant_id, project_id, user_id, role, permissions, department_id, director_access
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(membership_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(user_id)
    .bind(&role.id)
    .bind(&role.permissions)
    .bind(request.department_id)
    .bind(request.director_access)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "project.user_created",
        "membership",
        Some(membership_id),
        serde_json::json!({ "user_id": user_id, "role_id": role.id, "role_name": role.name, "department_id": request.department_id, "director_access": request.director_access }),
    )
    .await?;
    transaction.commit().await?;
    let member = load_project_member(&state.db, actor.tenant_id, project_id, membership_id).await?;
    Ok((StatusCode::CREATED, Json(member)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantProjectMemberRequest {
    email: String,
    role_id: String,
    department_id: Option<Uuid>,
    #[serde(default)]
    director_access: bool,
}

async fn grant_project_member(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
    Json(request): Json<GrantProjectMemberRequest>,
) -> Result<(StatusCode, Json<ProjectMemberResponse>), AppError> {
    require_project_admin_access(&state.db, &actor, project_id).await?;
    let email = normalize_email(request.email)?;
    let mut transaction = state.db.begin().await?;
    let role = lock_project_role(
        &mut transaction,
        actor.tenant_id,
        project_id,
        &request.role_id,
    )
    .await?;
    crate::departments::validate_member_scope(
        &mut transaction,
        actor.tenant_id,
        project_id,
        request.department_id,
        request.director_access,
        role.base_role()? == Role::Admin,
    )
    .await?;
    let user_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT app_user.id
        FROM users AS app_user
        WHERE lower(app_user.email) = $1
          AND app_user.status = 'active'
          AND EXISTS (
              SELECT 1
              FROM memberships
              WHERE tenant_id = $2 AND user_id = app_user.id
          )
        "#,
    )
    .bind(&email)
    .bind(actor.tenant_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if user_id == actor.actor_id {
        return Err(AppError::Conflict(
            "use another administrator to change your own project access".to_owned(),
        ));
    }
    if request.department_id.is_some() {
        require_no_tenant_wide_access(&mut transaction, actor.tenant_id, user_id).await?;
    }
    let membership_id = Uuid::now_v7();
    let returned_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO memberships (
            id, tenant_id, project_id, user_id, role, permissions, revoked_at, department_id, director_access
        ) VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8)
        ON CONFLICT (tenant_id, project_id, user_id)
        DO UPDATE SET role = EXCLUDED.role, permissions = EXCLUDED.permissions,
                      revoked_at = NULL, department_id = EXCLUDED.department_id,
                      position_id = CASE WHEN memberships.revoked_at IS NOT NULL
                          OR memberships.department_id IS DISTINCT FROM EXCLUDED.department_id
                          THEN NULL ELSE memberships.position_id END,
                      director_access = EXCLUDED.director_access, updated_at = now()
        RETURNING id
        "#,
    )
    .bind(membership_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(user_id)
    .bind(&role.id)
    .bind(&role.permissions)
    .bind(request.department_id)
    .bind(request.director_access)
    .fetch_one(&mut *transaction)
    .await?;
    if let Some(department_id) = request.department_id {
        release_other_department_assignments(
            &mut transaction,
            actor.tenant_id,
            project_id,
            user_id,
            department_id,
        )
        .await?;
    }
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "project.member_granted",
        "membership",
        Some(returned_id),
        serde_json::json!({ "user_id": user_id, "role_id": role.id, "role_name": role.name, "department_id": request.department_id, "director_access": request.director_access }),
    )
    .await?;
    transaction.commit().await?;
    let member = load_project_member(&state.db, actor.tenant_id, project_id, returned_id).await?;
    Ok((StatusCode::CREATED, Json(member)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateProjectMemberRequest {
    role_id: Option<String>,
    #[serde(default)]
    department_id: DepartmentScopeUpdate,
    director_access: Option<bool>,
}

#[derive(Debug, Default)]
enum DepartmentScopeUpdate {
    #[default]
    Unchanged,
    Assign(Option<Uuid>),
}

impl<'de> Deserialize<'de> for DepartmentScopeUpdate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<Uuid>::deserialize(deserializer).map(Self::Assign)
    }
}

async fn update_project_member(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, membership_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateProjectMemberRequest>,
) -> Result<Json<ProjectMemberResponse>, AppError> {
    require_project_admin_access(&state.db, &actor, project_id).await?;
    let mut transaction = state.db.begin().await?;
    let current =
        lock_project_member(&mut transaction, actor.tenant_id, project_id, membership_id).await?;
    let role = lock_project_role(
        &mut transaction,
        actor.tenant_id,
        project_id,
        request.role_id.as_deref().unwrap_or(&current.role_id),
    )
    .await?;
    let base_role = role.base_role()?;
    let department_id = match request.department_id {
        DepartmentScopeUpdate::Unchanged => current.department_id,
        DepartmentScopeUpdate::Assign(department_id) => department_id,
    };
    let director_access = request.director_access.unwrap_or(current.director_access);
    crate::departments::validate_member_scope(
        &mut transaction,
        actor.tenant_id,
        project_id,
        department_id,
        director_access,
        base_role == Role::Admin,
    )
    .await?;
    if current.user_id == actor.actor_id {
        return Err(AppError::Conflict(
            "use another administrator to change your own project access".to_owned(),
        ));
    }
    if department_id.is_some() {
        require_no_tenant_wide_access(&mut transaction, actor.tenant_id, current.user_id).await?;
    }
    if current.role == "admin" && base_role != Role::Admin {
        require_another_admin(&mut transaction, actor.tenant_id, project_id, membership_id).await?;
    }
    sqlx::query(
        r#"
        UPDATE memberships
        SET role = $4, permissions = $5, department_id = $6, director_access = $7, updated_at = now(),
            position_id = CASE WHEN memberships.department_id IS DISTINCT FROM $6::uuid
                THEN NULL ELSE memberships.position_id END
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
          AND revoked_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(membership_id)
    .bind(&role.id)
    .bind(&role.permissions)
    .bind(department_id)
    .bind(director_access)
    .execute(&mut *transaction)
    .await?;
    if let Some(department_id) = department_id {
        release_other_department_assignments(
            &mut transaction,
            actor.tenant_id,
            project_id,
            current.user_id,
            department_id,
        )
        .await?;
    }
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "project.member_updated",
        "membership",
        Some(membership_id),
        serde_json::json!({ "user_id": current.user_id, "role_id": role.id, "role_name": role.name, "department_id": department_id, "director_access": director_access }),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(
        load_project_member(&state.db, actor.tenant_id, project_id, membership_id).await?,
    ))
}

async fn revoke_project_member(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, membership_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_project_admin_access(&state.db, &actor, project_id).await?;
    let mut transaction = state.db.begin().await?;
    let current =
        lock_project_member(&mut transaction, actor.tenant_id, project_id, membership_id).await?;
    if current.user_id == actor.actor_id {
        return Err(AppError::Conflict(
            "an administrator cannot revoke their own active project access".to_owned(),
        ));
    }
    if current.role == "admin" {
        require_another_admin(&mut transaction, actor.tenant_id, project_id, membership_id).await?;
    }
    sqlx::query(
        r#"
        UPDATE memberships
        SET revoked_at = now(), updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
          AND revoked_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(membership_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE operator_sessions
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE tenant_id = $1 AND membership_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(membership_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE api_keys
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE tenant_id = $1 AND project_id = $2
          AND actor_user_id = $3 AND revoked_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(current.user_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "project.member_revoked",
        "membership",
        Some(membership_id),
        serde_json::json!({ "user_id": current.user_id }),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, FromRow)]
struct LockedProjectMember {
    user_id: Uuid,
    role: String,
    role_id: String,
    department_id: Option<Uuid>,
    director_access: bool,
}

async fn require_no_tenant_wide_access(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let has_tenant_access: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memberships WHERE tenant_id = $1 AND user_id = $2 AND project_id IS NULL AND revoked_at IS NULL)")
        .bind(tenant_id).bind(user_id).fetch_one(&mut **transaction).await?;
    if has_tenant_access {
        return Err(AppError::Conflict(
            "remove tenant-wide access before assigning a single department".to_owned(),
        ));
    }
    Ok(())
}

async fn release_other_department_assignments(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
    department_id: Uuid,
) -> Result<(), AppError> {
    // Lock conversations before changing their active assignments and participants.
    let conversations = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT conversation.id
           FROM conversations AS conversation
           JOIN inboxes AS inbox ON inbox.tenant_id = conversation.tenant_id
             AND inbox.project_id = conversation.project_id AND inbox.id = conversation.inbox_id
           WHERE conversation.tenant_id = $1 AND conversation.project_id = $2
             AND inbox.department_id IS DISTINCT FROM $4
             AND EXISTS (SELECT 1 FROM conversation_assignments AS assignment
                 WHERE assignment.tenant_id = conversation.tenant_id AND assignment.conversation_id = conversation.id
                   AND assignment.user_id = $3 AND assignment.unassigned_at IS NULL)
           ORDER BY conversation.id FOR UPDATE OF conversation"#,
    ).bind(tenant_id).bind(project_id).bind(user_id).bind(department_id).fetch_all(&mut **transaction).await?;
    for conversation_id in conversations {
        release_department_assignment(transaction, tenant_id, conversation_id, user_id).await?;
    }
    Ok(())
}

/// Releases one inaccessible operator and notifies clients after commit through the outbox.
pub(crate) async fn release_department_assignment(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    conversation_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let (project_id, inbox_id, contact_id) = sqlx::query_as::<_, (Uuid, Uuid, Uuid)>(
        "SELECT project_id, inbox_id, contact_id FROM conversations WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    ).bind(tenant_id).bind(conversation_id).fetch_one(&mut **transaction).await?;
    let now = Utc::now();
    let changed = sqlx::query("UPDATE conversation_assignments SET unassigned_at = $4 WHERE tenant_id = $1 AND conversation_id = $2 AND user_id = $3 AND unassigned_at IS NULL")
        .bind(tenant_id).bind(conversation_id).bind(user_id).bind(now).execute(&mut **transaction).await?;
    if changed.rows_affected() == 0 {
        return Ok(());
    }
    sqlx::query("UPDATE conversation_participants SET left_at = $4 WHERE tenant_id = $1 AND conversation_id = $2 AND user_id = $3 AND participant_kind = 'operator' AND left_at IS NULL")
        .bind(tenant_id).bind(conversation_id).bind(user_id).bind(now).execute(&mut **transaction).await?;
    sqlx::query("UPDATE conversations SET updated_at = $3, version = version + 1 WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id).bind(conversation_id).bind(now).execute(&mut **transaction).await?;
    let event = crate::realtime::RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id,
        project_id,
        inbox_id,
        contact_id: Some(contact_id),
        event_type: "conversation.operator_left".to_owned(),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: now,
        data: serde_json::json!({"conversation_id":conversation_id}),
    };
    sqlx::query("INSERT INTO outbox_events (id,tenant_id,aggregate_type,aggregate_id,event_type,payload,status,available_at,created_at) VALUES ($1,$2,'realtime',$3,$4,$5,'pending',now(),now())")
        .bind(event.event_id).bind(tenant_id).bind(conversation_id).bind(&event.event_type)
        .bind(serde_json::to_value(&event).map_err(AppError::internal)?).execute(&mut **transaction).await?;
    Ok(())
}

async fn lock_project_member(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    membership_id: Uuid,
) -> Result<LockedProjectMember, AppError> {
    sqlx::query_as::<_, LockedProjectMember>(
        r#"
        SELECT membership.user_id, project_role.base_role AS role, membership.role AS role_id,
               membership.department_id, membership.director_access
        FROM memberships AS membership
        JOIN project_roles AS project_role
          ON project_role.tenant_id = membership.tenant_id
         AND project_role.project_id = membership.project_id
         AND project_role.id = membership.role
        WHERE membership.tenant_id = $1 AND membership.project_id = $2
          AND membership.id = $3 AND membership.revoked_at IS NULL
        FOR UPDATE OF membership
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(membership_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

async fn require_another_admin(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    excluded_membership_id: Uuid,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM memberships
            WHERE tenant_id = $1
              AND role = 'admin'
              AND revoked_at IS NULL
              AND id <> $3
              AND (project_id = $2 OR project_id IS NULL)
        )
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(excluded_membership_id)
    .fetch_one(&mut **transaction)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::Conflict(
            "a project must retain at least one administrator".to_owned(),
        ))
    }
}

async fn load_project_member(
    db: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    membership_id: Uuid,
) -> Result<ProjectMemberResponse, AppError> {
    let row = sqlx::query_as::<_, ProjectMemberRow>(
        r#"
        SELECT membership.id AS membership_id, app_user.id AS user_id,
               app_user.email, app_user.display_name,
               COALESCE(profile.display_name, app_user.display_name) AS chat_display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS avatar_url,
               project_role.base_role AS role, membership.role AS role_id,
               project_role.name AS role_name, membership.department_id, membership.director_access,
               membership.created_at, membership.updated_at
        FROM memberships AS membership
        JOIN users AS app_user ON app_user.id = membership.user_id
        JOIN project_roles AS project_role
          ON project_role.tenant_id = membership.tenant_id
         AND project_role.project_id = membership.project_id
         AND project_role.id = membership.role
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = membership.tenant_id
         AND profile.project_id = membership.project_id
         AND profile.user_id = membership.user_id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = membership.tenant_id
         AND stored_avatar.project_id = membership.project_id
         AND stored_avatar.user_id = membership.user_id
        WHERE membership.tenant_id = $1
          AND membership.project_id = $2
          AND membership.id = $3
          AND membership.revoked_at IS NULL
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(membership_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    ProjectMemberResponse::try_from(row)
}

pub(crate) async fn require_project_admin_access(
    db: &PgPool,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<(), AppError> {
    actor.require_deployment_project(project_id)?;
    actor.require_session_admin()?;
    if actor.is_demo() {
        actor.require_project(project_id)?;
        return Ok(());
    }
    let accessible = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM projects AS project
            JOIN memberships AS membership
             ON membership.tenant_id = project.tenant_id
             AND membership.user_id = $3
             AND membership.role = 'admin'
             AND membership.department_id IS NULL
             AND membership.revoked_at IS NULL
             AND (membership.project_id = project.id OR membership.project_id IS NULL)
            WHERE project.tenant_id = $1 AND project.id = $2 AND project.deleted_at IS NULL
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .fetch_one(db)
    .await?;
    if accessible {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

pub(crate) async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Option<Uuid>,
    action: &str,
    resource_kind: &str,
    resource_id: Option<Uuid>,
    metadata: serde_json::Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(action)
    .bind(resource_kind)
    .bind(resource_id)
    .bind(metadata)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn normalize_required_text(
    value: String,
    max_chars: usize,
    field: &str,
) -> Result<String, AppError> {
    let value = value.trim();
    let length = value.chars().count();
    if length == 0 || length > max_chars {
        return Err(AppError::BadRequest(format!(
            "{field} must contain between 1 and {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_slug(value: String) -> Result<String, AppError> {
    let slug = value.trim().to_ascii_lowercase();
    let valid = (1..=63).contains(&slug.len())
        && slug.bytes().enumerate().all(|(index, value)| {
            value.is_ascii_lowercase() || value.is_ascii_digit() || (index > 0 && value == b'-')
        });
    if !valid {
        return Err(AppError::BadRequest(
            "slug must contain 1 to 63 lowercase Latin letters, digits, or hyphens and start with a letter or digit"
                .to_owned(),
        ));
    }
    Ok(slug)
}

fn normalize_project_status(value: String) -> Result<String, AppError> {
    match value.trim() {
        "active" => Ok("active".to_owned()),
        "disabled" => Ok("disabled".to_owned()),
        _ => Err(AppError::BadRequest(
            "status must be one of: active, disabled".to_owned(),
        )),
    }
}

fn normalize_email(value: String) -> Result<String, AppError> {
    let email = value.trim().to_lowercase();
    if email.is_empty()
        || email.len() > 320
        || email.contains(char::is_whitespace)
        || !email.contains('@')
    {
        return Err(AppError::BadRequest("email is invalid".to_owned()));
    }
    Ok(email)
}

fn project_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("a project with this slug already exists".to_owned())
    } else {
        AppError::Database(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        UpdateProjectRequest, normalize_email, normalize_project_status, normalize_required_text,
        normalize_slug,
    };

    #[test]
    fn project_parent_patch_distinguishes_omitted_null_and_uuid() {
        let omitted: UpdateProjectRequest = serde_json::from_str(r#"{"name":"Renamed"}"#).unwrap();
        assert_eq!(omitted.parent_project_id, None);
        let detached: UpdateProjectRequest =
            serde_json::from_str(r#"{"parent_project_id":null}"#).unwrap();
        assert_eq!(detached.parent_project_id, Some(None));
        let parent_id = uuid::Uuid::now_v7();
        let attached: UpdateProjectRequest =
            serde_json::from_value(serde_json::json!({ "parent_project_id": parent_id })).unwrap();
        assert_eq!(attached.parent_project_id, Some(Some(parent_id)));
        assert!(
            serde_json::from_str::<UpdateProjectRequest>(r#"{"parent_project_id":""}"#).is_err()
        );
    }

    #[test]
    fn validates_project_identity_fields() {
        assert_eq!(
            normalize_slug(" Support-EU ".to_owned()).unwrap(),
            "support-eu"
        );
        assert!(normalize_slug("-support".to_owned()).is_err());
        assert!(normalize_slug("поддержка".to_owned()).is_err());
        assert_eq!(
            normalize_required_text(" Customer support ".to_owned(), 200, "name").unwrap(),
            "Customer support"
        );
    }

    #[test]
    fn accepts_only_explicit_project_statuses_and_basic_emails() {
        assert_eq!(
            normalize_project_status("active".to_owned()).unwrap(),
            "active"
        );
        assert!(normalize_project_status("archived".to_owned()).is_err());
        assert_eq!(
            normalize_email(" Operator@Example.com ".to_owned()).unwrap(),
            "operator@example.com"
        );
        assert!(normalize_email("operator example.com".to_owned()).is_err());
    }
}
