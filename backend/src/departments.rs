//! Project departments, department menus, and the director's project overview.

use std::collections::HashSet;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects/{project_id}/departments",
            get(list_departments).post(create_department),
        )
        .route(
            "/api/v1/projects/{project_id}/departments/{department_id}",
            patch(update_department).delete(delete_department),
        )
        .route(
            "/api/v1/projects/{project_id}/overview",
            get(director_overview),
        )
}

const SIDEBAR_ITEMS: &[&str] = &[
    "conversations",
    "contacts",
    "online_visitors",
    "support_quality",
    "inbox_routing",
    "channels",
    "team",
    "ai",
    "tasks",
    "processes",
    "notes",
    "knowledge_base",
    "reply_templates",
    "integrations",
];

/// Presentation only: category membership never grants access to a page.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SidebarCategory {
    pub id: String,
    pub name: String,
    pub items: Vec<String>,
}

fn validate_categories(categories: &[SidebarCategory], director: bool) -> Result<(), AppError> {
    let mut ids = HashSet::new();
    let mut items = HashSet::new();
    if categories.len() > 32 {
        return Err(AppError::BadRequest(
            "sidebar_categories cannot exceed 32 categories".to_owned(),
        ));
    }
    for category in categories {
        if category.id.is_empty()
            || category.id.len() > 80
            || !category
                .id
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b"-_:".contains(&ch))
            || !ids.insert(&category.id)
            || category.name.trim().is_empty()
            || category.name.chars().count() > 80
        {
            return Err(AppError::BadRequest(
                "sidebar_categories require unique valid IDs and names of 1 to 80 characters"
                    .to_owned(),
            ));
        }
        for item in &category.items {
            let supported = SIDEBAR_ITEMS.contains(&item.as_str())
                || ["departments", "projects", "users", "roles", "access_tokens"]
                    .contains(&item.as_str())
                || (director && item == "director");
            if !supported || !items.insert(item) {
                return Err(AppError::BadRequest(
                    "sidebar_categories require supported pages assigned to at most one category"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DepartmentDraft {
    pub name: String,
    pub icon: Option<String>,
    pub sidebar_items: Option<Vec<String>>,
    pub sidebar_categories: Option<Vec<SidebarCategory>>,
    pub default_page: Option<String>,
    pub show_default_channels: Option<bool>,
}

impl DepartmentDraft {
    pub(crate) fn normalize(self) -> Result<Self, AppError> {
        let name = normalize_text(self.name, 100, "department name")?;
        let icon = normalize_text(
            self.icon.unwrap_or_else(|| "headset".to_owned()),
            32,
            "icon",
        )?;
        let sidebar_items = self.sidebar_items.unwrap_or_else(|| {
            SIDEBAR_ITEMS
                .iter()
                .map(|item| (*item).to_owned())
                .collect()
        });
        let default_page = self
            .default_page
            .unwrap_or_else(|| sidebar_items.first().cloned().unwrap_or_default());
        validate_menu(&sidebar_items, &default_page)?;
        validate_categories(
            self.sidebar_categories.as_deref().unwrap_or_default(),
            false,
        )?;
        Ok(Self {
            name,
            icon: Some(icon),
            sidebar_items: Some(sidebar_items),
            sidebar_categories: self.sidebar_categories,
            default_page: Some(default_page),
            show_default_channels: self.show_default_channels,
        })
    }

    pub(crate) fn support() -> Self {
        Self {
            name: "Support".to_owned(),
            icon: None,
            sidebar_items: None,
            sidebar_categories: None,
            default_page: None,
            show_default_channels: None,
        }
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct DepartmentResponse {
    pub id: Uuid,
    pub name: String,
    pub icon: String,
    pub sidebar_items: Vec<String>,
    pub sidebar_categories: sqlx::types::Json<Vec<SidebarCategory>>,
    pub default_page: String,
    pub show_default_channels: bool,
    pub position: i32,
    pub inbox_ids: Vec<Uuid>,
    pub member_count: i64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectorMenu {
    pub sidebar_items: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sidebar_categories: Vec<SidebarCategory>,
    pub default_page: String,
    pub show_default_channels: bool,
}

impl DirectorMenu {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if self.sidebar_items.is_empty()
            || self.sidebar_items.len() > SIDEBAR_ITEMS.len() + 1
            || self
                .sidebar_items
                .iter()
                .any(|item| item != "director" && !SIDEBAR_ITEMS.contains(&item.as_str()))
            || self.sidebar_items.iter().collect::<HashSet<_>>().len() != self.sidebar_items.len()
            || !self.sidebar_items.contains(&self.default_page)
        {
            return Err(AppError::BadRequest(
                "director_menu requires distinct supported sections and an enabled default_page"
                    .to_owned(),
            ));
        }
        validate_categories(&self.sidebar_categories, true)
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct DepartmentInboxOption {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectDepartmentList {
    pub items: Vec<DepartmentResponse>,
    pub default_department_id: Option<Uuid>,
    pub director_enabled: bool,
    pub default_director_workspace: bool,
    pub(crate) director_menu: sqlx::types::Json<DirectorMenu>,
    pub can_access_director: bool,
    pub inbox_options: Vec<DepartmentInboxOption>,
}

#[derive(FromRow)]
struct DepartmentAccess {
    department_id: Option<Uuid>,
    default_department_id: Option<Uuid>,
    director_enabled: bool,
    default_director_workspace: bool,
    can_access_director: bool,
}

async fn project_access(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<DepartmentAccess, AppError> {
    if !actor.is_password_session() {
        actor.require_project(project_id)?;
        let department_id = actor.department_id().ok_or(AppError::Forbidden)?;
        let (default_department_id, director_enabled, default_director_workspace): (Option<Uuid>, bool, bool) = sqlx::query_as("SELECT default_department_id, director_enabled, default_director_workspace FROM projects WHERE tenant_id = $1 AND id = $2")
            .bind(actor.tenant_id).bind(project_id).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
        return Ok(DepartmentAccess {
            department_id: Some(department_id),
            default_department_id,
            director_enabled,
            default_director_workspace,
            can_access_director: false,
        });
    }
    if actor.is_demo() {
        actor.require_project(project_id)?;
    }
    sqlx::query_as::<_, DepartmentAccess>(
        r#"SELECT membership.department_id, project.default_department_id, project.director_enabled,
                  project.default_director_workspace,
                  (membership.department_id IS NULL AND project.director_enabled
                   AND (COALESCE(role.base_role, membership.role) = 'admin' OR membership.director_access)) AS can_access_director
           FROM projects AS project
           JOIN memberships AS membership ON membership.tenant_id = project.tenant_id
             AND membership.user_id = $3 AND membership.revoked_at IS NULL
             AND (membership.project_id = project.id OR membership.project_id IS NULL)
           LEFT JOIN project_roles AS role ON role.tenant_id = membership.tenant_id
             AND role.project_id = project.id AND role.id = membership.role
           WHERE project.tenant_id = $1 AND project.id = $2
           ORDER BY CASE COALESCE(role.base_role, membership.role) WHEN 'admin' THEN 0 ELSE 1 END,
                    CASE WHEN membership.project_id IS NULL THEN 0 ELSE 1 END,
                    membership.created_at, membership.id
           LIMIT 1"#,
    ).bind(actor.tenant_id).bind(project_id).bind(actor.actor_id)
        .fetch_optional(&state.db).await?.ok_or(AppError::NotFound)
}

async fn load_departments(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    department_id: Option<Uuid>,
) -> Result<Vec<DepartmentResponse>, AppError> {
    Ok(sqlx::query_as::<_, DepartmentResponse>(
        r#"SELECT department.id, department.name, department.icon, department.sidebar_items, department.sidebar_categories,
                  department.default_page, department.show_default_channels, department.position,
                  ARRAY(SELECT id FROM inboxes WHERE tenant_id = department.tenant_id
                    AND project_id = department.project_id AND department_id = department.id ORDER BY name, id) AS inbox_ids,
                  (SELECT COUNT(DISTINCT user_id) FROM memberships WHERE tenant_id = department.tenant_id
                    AND (project_id = department.project_id OR project_id IS NULL) AND revoked_at IS NULL
                    AND (department_id = department.id OR department_id IS NULL)) AS member_count
           FROM departments AS department WHERE tenant_id = $1 AND project_id = $2
             AND ($3::uuid IS NULL OR id = $3) ORDER BY position, name, id"#,
    ).bind(tenant_id).bind(project_id).bind(department_id).fetch_all(&state.db).await?)
}

async fn list_departments(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ProjectDepartmentList>, AppError> {
    let access = project_access(&state, &actor, project_id).await?;
    let mut items =
        load_departments(&state, actor.tenant_id, project_id, access.department_id).await?;
    let mut inbox_options = sqlx::query_as::<_, DepartmentInboxOption>(
        "SELECT id, name FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND ($3::uuid IS NULL OR department_id = $3) ORDER BY name, id",
    ).bind(actor.tenant_id).bind(project_id).bind(access.department_id).fetch_all(&state.db).await?;
    if !actor.is_password_session()
        && let Some(scope) = actor.inbox_scope()
    {
        for department in &mut items {
            department.inbox_ids.retain(|id| scope.contains(id));
        }
        inbox_options.retain(|inbox| scope.contains(&inbox.id));
    }
    Ok(Json(ProjectDepartmentList {
        items,
        inbox_options,
        default_department_id: access.department_id.or(access.default_department_id),
        director_enabled: access.director_enabled,
        default_director_workspace: access.default_director_workspace,
        director_menu: sqlx::query_scalar(
            "SELECT director_menu FROM projects WHERE tenant_id=$1 AND id=$2",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .fetch_one(&state.db)
        .await?,
        can_access_director: access.can_access_director,
    }))
}

pub(crate) async fn insert_department(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    draft: &DepartmentDraft,
    position: i32,
) -> Result<Uuid, AppError> {
    let department_id = Uuid::now_v7();
    sqlx::query("INSERT INTO departments (id, tenant_id, project_id, name, icon, sidebar_items, default_page, position, show_default_channels, sidebar_categories) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(department_id).bind(tenant_id).bind(project_id).bind(&draft.name).bind(&draft.icon)
        .bind(&draft.sidebar_items).bind(&draft.default_page).bind(position)
        .bind(draft.show_default_channels.unwrap_or(true))
        .bind(sqlx::types::Json(draft.sidebar_categories.as_deref().unwrap_or_default()))
        .execute(&mut **transaction).await.map_err(department_write_error)?;
    Ok(department_id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateDepartmentRequest {
    name: String,
    icon: Option<String>,
    sidebar_items: Option<Vec<String>>,
    sidebar_categories: Option<Vec<SidebarCategory>>,
    default_page: Option<String>,
    show_default_channels: Option<bool>,
    inbox_ids: Option<Vec<Uuid>>,
}

async fn create_department(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
    Json(request): Json<CreateDepartmentRequest>,
) -> Result<(StatusCode, Json<DepartmentResponse>), AppError> {
    crate::projects::require_project_admin_access(&state.db, &actor, project_id).await?;
    let draft = DepartmentDraft {
        name: request.name,
        icon: request.icon,
        sidebar_items: request.sidebar_items,
        sidebar_categories: request.sidebar_categories,
        default_page: request.default_page,
        show_default_channels: request.show_default_channels,
    }
    .normalize()?;
    let mut transaction = state.db.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE tenant_id = $1 AND id = $2 FOR UPDATE")
        .bind(actor.tenant_id)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    let (count, last_position): (i64, i32) = sqlx::query_as("SELECT COUNT(*), COALESCE(MAX(position), -1) FROM departments WHERE tenant_id = $1 AND project_id = $2")
        .bind(actor.tenant_id).bind(project_id).fetch_one(&mut *transaction).await?;
    if count >= 100 {
        return Err(AppError::BadRequest(
            "a project can have at most 100 departments".to_owned(),
        ));
    }
    let position = (last_position + 1).min(999);
    let department_id = insert_department(
        &mut transaction,
        actor.tenant_id,
        project_id,
        &draft,
        position,
    )
    .await?;
    if let Some(inbox_ids) = request.inbox_ids {
        assign_inboxes(
            &mut transaction,
            actor.tenant_id,
            project_id,
            department_id,
            &inbox_ids,
        )
        .await?;
    } else {
        sqlx::query("INSERT INTO inboxes (id, tenant_id, project_id, department_id, name) VALUES ($1,$2,$3,$4,$5)")
            .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project_id).bind(department_id).bind(&draft.name)
            .execute(&mut *transaction).await.map_err(department_write_error)?;
    }
    sqlx::query("UPDATE projects SET default_department_id = COALESCE(default_department_id, $3), updated_at = now() WHERE tenant_id = $1 AND id = $2")
        .bind(actor.tenant_id).bind(project_id).bind(department_id).execute(&mut *transaction).await?;
    crate::projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "department.created",
        "department",
        Some(department_id),
        serde_json::json!({"name":draft.name}),
    )
    .await?;
    transaction.commit().await?;
    let department = load_departments(&state, actor.tenant_id, project_id, Some(department_id))
        .await?
        .pop()
        .ok_or(AppError::NotFound)?;
    Ok((StatusCode::CREATED, Json(department)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateDepartmentRequest {
    name: Option<String>,
    icon: Option<String>,
    sidebar_items: Option<Vec<String>>,
    sidebar_categories: Option<Vec<SidebarCategory>>,
    default_page: Option<String>,
    show_default_channels: Option<bool>,
    position: Option<i32>,
    inbox_ids: Option<Vec<Uuid>>,
}

async fn update_department(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, department_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateDepartmentRequest>,
) -> Result<Json<DepartmentResponse>, AppError> {
    crate::projects::require_project_admin_access(&state.db, &actor, project_id).await?;
    let mut transaction = state.db.begin().await?;
    let (name, icon, sidebar_items, default_page, position): (String, String, Vec<String>, String, i32) = sqlx::query_as(
        "SELECT name, icon, sidebar_items, default_page, position FROM departments WHERE tenant_id = $1 AND project_id = $2 AND id = $3 FOR UPDATE",
    ).bind(actor.tenant_id).bind(project_id).bind(department_id).fetch_optional(&mut *transaction).await?.ok_or(AppError::NotFound)?;
    let draft = DepartmentDraft {
        name: request.name.unwrap_or(name),
        icon: request.icon.or(Some(icon)),
        sidebar_items: request.sidebar_items.or(Some(sidebar_items)),
        sidebar_categories: request.sidebar_categories,
        default_page: request.default_page.or(Some(default_page)),
        show_default_channels: request.show_default_channels,
    }
    .normalize()?;
    let position = request.position.unwrap_or(position);
    if !(0..=999).contains(&position) {
        return Err(AppError::BadRequest(
            "position must be between 0 and 999".to_owned(),
        ));
    }
    sqlx::query("UPDATE departments SET name = $4, icon = $5, sidebar_items = $6, default_page = $7, position = $8, show_default_channels = COALESCE($9, show_default_channels), sidebar_categories = COALESCE($10, sidebar_categories), updated_at = now() WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
        .bind(actor.tenant_id).bind(project_id).bind(department_id).bind(&draft.name).bind(&draft.icon)
        .bind(&draft.sidebar_items).bind(&draft.default_page).bind(position).bind(draft.show_default_channels)
        .bind(draft.sidebar_categories.as_ref().map(sqlx::types::Json))
        .execute(&mut *transaction).await.map_err(department_write_error)?;
    if let Some(inbox_ids) = request.inbox_ids {
        assign_inboxes(
            &mut transaction,
            actor.tenant_id,
            project_id,
            department_id,
            &inbox_ids,
        )
        .await?;
    }
    crate::projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "department.updated",
        "department",
        Some(department_id),
        serde_json::json!({"name":draft.name,"position":position}),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(
        load_departments(&state, actor.tenant_id, project_id, Some(department_id))
            .await?
            .pop()
            .ok_or(AppError::NotFound)?,
    ))
}

async fn delete_department(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, department_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProjectDepartmentList>, AppError> {
    crate::projects::require_project_admin_access(&state.db, &actor, project_id).await?;
    if actor.is_demo() {
        return Err(AppError::Forbidden);
    }
    let mut transaction = state.db.begin().await?;
    // Serialize deletion with creation and changes to the project's default workspace.
    let director_enabled: bool = sqlx::query_scalar(
        "SELECT director_enabled FROM projects WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM departments WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(department_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let fallback: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM departments WHERE tenant_id=$1 AND project_id=$2 AND id<>$3 ORDER BY position, name, id LIMIT 1",
    ).bind(actor.tenant_id).bind(project_id).bind(department_id)
        .fetch_optional(&mut *transaction).await?;
    if fallback.is_none() && !director_enabled {
        return Err(AppError::Conflict(
            "enable the control center before deleting the last department".to_owned(),
        ));
    }
    // Removing a department must never turn a restricted member or resource into an unrestricted one.
    let has_members: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM memberships WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3 AND revoked_at IS NULL)",
    ).bind(actor.tenant_id).bind(project_id).bind(department_id)
        .fetch_one(&mut *transaction).await?;
    if has_members {
        return Err(AppError::Conflict(
            "reassign department members before deleting the department".to_owned(),
        ));
    }
    let has_positions: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM job_positions WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3 UNION ALL SELECT 1 FROM company_positions WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3)",
    ).bind(actor.tenant_id).bind(project_id).bind(department_id)
        .fetch_one(&mut *transaction).await?;
    if has_positions {
        return Err(AppError::Conflict(
            "reassign department positions before deleting the department".to_owned(),
        ));
    }
    ensure_department_resources_unbound(&mut transaction, actor.tenant_id, department_id).await?;
    sqlx::query("UPDATE projects SET default_department_id=CASE WHEN default_department_id=$3 THEN $4 ELSE default_department_id END, default_director_workspace=default_director_workspace OR $4::uuid IS NULL, updated_at=now() WHERE tenant_id=$1 AND id=$2")
        .bind(actor.tenant_id).bind(project_id).bind(department_id).bind(fallback)
        .execute(&mut *transaction).await?;
    sqlx::query("UPDATE inboxes SET department_id=NULL, updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3")
        .bind(actor.tenant_id).bind(project_id).bind(department_id).execute(&mut *transaction).await?;
    sqlx::query("UPDATE memberships SET department_id=NULL WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3 AND revoked_at IS NOT NULL")
        .bind(actor.tenant_id).bind(project_id).bind(department_id).execute(&mut *transaction).await?;
    for table in ["operator_sessions", "operator_session_projects"] {
        sqlx::query(&format!("UPDATE {table} SET department_id=NULL, director_mode=false WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3"))
            .bind(actor.tenant_id).bind(project_id).bind(department_id).execute(&mut *transaction).await?;
    }
    sqlx::query("DELETE FROM user_project_workspaces WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3")
        .bind(actor.tenant_id).bind(project_id).bind(department_id).execute(&mut *transaction).await?;
    sqlx::query("DELETE FROM departments WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
        .bind(actor.tenant_id).bind(project_id).bind(department_id).execute(&mut *transaction).await
        .map_err(|error| {
            if error.as_database_error().is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation) {
                AppError::Conflict("department references changed; reload and reassign linked records before deleting it".to_owned())
            } else { AppError::Database(error) }
        })?;
    crate::projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "department.deleted",
        "department",
        Some(department_id),
        serde_json::json!({"default_department_id":fallback}),
    )
    .await?;
    transaction.commit().await?;
    list_departments(State(state), actor, Path(project_id)).await
}

async fn ensure_department_resources_unbound(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    department_id: Uuid,
) -> Result<(), AppError> {
    for table in [
        "ai_provider_connections",
        "knowledge_bases",
        "ai_tasks",
        "ai_profiles",
        "reply_templates",
        "channel_connections",
        "api_integrations",
        "company_processes",
    ] {
        let bound: bool = sqlx::query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE tenant_id=$1 AND visibility->'department_ids' ? $2)"))
            .bind(tenant_id).bind(department_id.to_string()).fetch_one(&mut **transaction).await?;
        if bound {
            return Err(AppError::Conflict(
                "reassign department resource visibility before deleting the department".to_owned(),
            ));
        }
    }
    let bound: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM company_processes WHERE tenant_id=$1 AND ($2=ANY(edit_department_ids) OR $2=ANY(approve_department_ids)) UNION ALL SELECT 1 FROM company_governance WHERE tenant_id=$1 AND content->'teams' @> jsonb_build_array(jsonb_build_object('department_id', $2::text)))",
    ).bind(tenant_id).bind(department_id).fetch_one(&mut **transaction).await?;
    if bound {
        return Err(AppError::Conflict(
            "reassign department process access and working teams before deleting the department"
                .to_owned(),
        ));
    }
    Ok(())
}

async fn assign_inboxes(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    department_id: Uuid,
    inbox_ids: &[Uuid],
) -> Result<(), AppError> {
    if inbox_ids.len() > 500 || inbox_ids.iter().collect::<HashSet<_>>().len() != inbox_ids.len() {
        return Err(AppError::BadRequest(
            "inbox_ids must contain at most 500 distinct inboxes".to_owned(),
        ));
    }
    let missing_current: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND department_id = $3 AND NOT (id = ANY($4)))")
        .bind(tenant_id).bind(project_id).bind(department_id).bind(inbox_ids).fetch_one(&mut **transaction).await?;
    if missing_current {
        return Err(AppError::BadRequest(
            "move excluded inboxes to another department before removing them".to_owned(),
        ));
    }
    let updated = sqlx::query("UPDATE inboxes SET department_id = $3, updated_at = now() WHERE tenant_id = $1 AND project_id = $2 AND id = ANY($4)")
        .bind(tenant_id).bind(project_id).bind(department_id).bind(inbox_ids).execute(&mut **transaction).await?;
    if updated.rows_affected() != inbox_ids.len() as u64 {
        return Err(AppError::BadRequest(
            "every inbox must belong to this project".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize, FromRow)]
pub struct DepartmentOverview {
    pub id: Uuid,
    pub name: String,
    pub icon: String,
    pub open_conversations: i64,
    pub conversation_count: i64,
    pub member_count: i64,
    pub inbox_count: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct OverviewTotals {
    pub open_conversations: i64,
    pub conversation_count: i64,
    pub member_count: i64,
    pub inbox_count: i64,
}

#[derive(Debug, Serialize)]
pub struct DirectorOverview {
    pub project_id: Uuid,
    pub departments: Vec<DepartmentOverview>,
    pub totals: OverviewTotals,
}

async fn director_overview(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
) -> Result<Json<DirectorOverview>, AppError> {
    let access = project_access(&state, &actor, project_id).await?;
    if !access.can_access_director || actor.is_demo() {
        return Err(AppError::Forbidden);
    }
    let departments = sqlx::query_as::<_, DepartmentOverview>(
        r#"SELECT department.id, department.name, department.icon,
                  (SELECT COUNT(*) FROM conversations AS conversation JOIN inboxes AS inbox ON inbox.tenant_id = conversation.tenant_id AND inbox.id = conversation.inbox_id
                   WHERE inbox.department_id = department.id AND conversation.last_message_sequence > 0 AND conversation.status IN ('new','open','waiting_customer')) AS open_conversations,
                  (SELECT COUNT(*) FROM conversations AS conversation JOIN inboxes AS inbox ON inbox.tenant_id = conversation.tenant_id AND inbox.id = conversation.inbox_id
                   WHERE inbox.department_id = department.id AND conversation.last_message_sequence > 0) AS conversation_count,
                  (SELECT COUNT(DISTINCT user_id) FROM memberships WHERE tenant_id = department.tenant_id AND (project_id = department.project_id OR project_id IS NULL)
                   AND revoked_at IS NULL AND (department_id = department.id OR department_id IS NULL)) AS member_count,
                  (SELECT COUNT(*) FROM inboxes WHERE tenant_id = department.tenant_id AND project_id = department.project_id AND department_id = department.id) AS inbox_count
           FROM departments AS department WHERE tenant_id = $1 AND project_id = $2 ORDER BY position, name, id"#,
    ).bind(actor.tenant_id).bind(project_id).fetch_all(&state.db).await?;
    let totals = sqlx::query_as::<_, OverviewTotals>(
        r#"SELECT (SELECT COUNT(*) FROM conversations WHERE tenant_id = $1 AND project_id = $2 AND last_message_sequence > 0 AND status IN ('new','open','waiting_customer')) AS open_conversations,
                  (SELECT COUNT(*) FROM conversations WHERE tenant_id = $1 AND project_id = $2 AND last_message_sequence > 0) AS conversation_count,
                  (SELECT COUNT(DISTINCT user_id) FROM memberships WHERE tenant_id = $1 AND (project_id = $2 OR project_id IS NULL) AND revoked_at IS NULL) AS member_count,
                  (SELECT COUNT(*) FROM inboxes WHERE tenant_id = $1 AND project_id = $2) AS inbox_count"#,
    ).bind(actor.tenant_id).bind(project_id).fetch_one(&state.db).await?;
    Ok(Json(DirectorOverview {
        project_id,
        departments,
        totals,
    }))
}

pub(crate) async fn validate_member_scope(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    department_id: Option<Uuid>,
    director_access: bool,
    admin: bool,
) -> Result<(), AppError> {
    if let Some(department_id) = department_id {
        if admin || director_access {
            return Err(AppError::BadRequest(
                "department-scoped members cannot be administrators or directors".to_owned(),
            ));
        }
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM departments WHERE tenant_id = $1 AND project_id = $2 AND id = $3)")
            .bind(tenant_id).bind(project_id).bind(department_id).fetch_one(&mut **transaction).await?;
        if !exists {
            return Err(AppError::BadRequest(
                "department must belong to this project".to_owned(),
            ));
        }
    }
    Ok(())
}

fn normalize_text(value: String, max: usize, field: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(AppError::BadRequest(format!(
            "{field} must contain between 1 and {max} characters"
        )));
    }
    Ok(value.to_owned())
}

fn validate_menu(items: &[String], default_page: &str) -> Result<(), AppError> {
    if items.is_empty()
        || items.len() > SIDEBAR_ITEMS.len()
        || items
            .iter()
            .any(|item| !SIDEBAR_ITEMS.contains(&item.as_str()))
        || items.iter().collect::<HashSet<_>>().len() != items.len()
    {
        return Err(AppError::BadRequest(
            "sidebar_items must contain distinct supported sections".to_owned(),
        ));
    }
    if !items.iter().any(|item| item == default_page) {
        return Err(AppError::BadRequest(
            "default_page must be enabled in sidebar_items".to_owned(),
        ));
    }
    Ok(())
}

fn department_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("a department or inbox with this name already exists".to_owned())
    } else {
        AppError::Database(error)
    }
}

#[cfg(test)]
mod tests {
    use super::validate_menu;

    #[test]
    fn menu_requires_unique_known_sections_and_visible_start_page() {
        assert!(validate_menu(&["conversations".into(), "contacts".into()], "contacts").is_ok());
        assert!(validate_menu(&[], "conversations").is_err());
        assert!(validate_menu(&["contacts".into(), "contacts".into()], "contacts").is_err());
        assert!(validate_menu(&["unknown".into()], "unknown").is_err());
        assert!(validate_menu(&["contacts".into()], "conversations").is_err());
    }
}
