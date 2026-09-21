//! Company positions and reporting lines, independent of authorization roles.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError};

const MAX_POSITIONS: i64 = 500;
const MAX_OPTIONS: usize = 1000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects/{project_id}/company-structure",
            get(get_structure).post(create_position),
        )
        .route(
            "/api/v1/projects/{project_id}/company-structure/{position_id}",
            put(update_position).delete(delete_position),
        )
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum OccupantKind {
    Vacant,
    Human,
    Agent,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PositionDraft {
    title: String,
    job_position_id: Option<Uuid>,
    department_id: Option<Uuid>,
    reports_to_position_id: Option<Uuid>,
    is_department_head: bool,
    occupant_kind: OccupantKind,
    occupant_name: Option<String>,
    user_id: Option<Uuid>,
    ai_profile_id: Option<Uuid>,
}

impl PositionDraft {
    fn normalize(mut self) -> Result<Self, AppError> {
        self.title = normalize_name(self.title, "position title", 200)?;
        self.occupant_name = self
            .occupant_name
            .map(|name| normalize_name(name, "occupant name", 160))
            .transpose()?;
        if self.is_department_head && self.department_id.is_none() {
            return Err(AppError::BadRequest(
                "a department head requires a department".to_owned(),
            ));
        }
        let valid_occupant = match self.occupant_kind {
            OccupantKind::Vacant => {
                self.occupant_name.is_none()
                    && self.user_id.is_none()
                    && self.ai_profile_id.is_none()
            }
            OccupantKind::Human => {
                self.ai_profile_id.is_none()
                    && (self.user_id.is_some() != self.occupant_name.is_some())
            }
            OccupantKind::Agent => {
                self.occupant_name.is_none()
                    && self.user_id.is_none()
                    && self.ai_profile_id.is_some()
            }
        };
        if !valid_occupant {
            return Err(AppError::BadRequest(
                "vacant positions have no occupant; humans require a user or a name; agents require an AI profile".to_owned(),
            ));
        }
        Ok(self)
    }
}

#[derive(Serialize, FromRow)]
struct CompanyPosition {
    id: Uuid,
    title: String,
    job_position_id: Option<Uuid>,
    department_id: Option<Uuid>,
    reports_to_position_id: Option<Uuid>,
    is_department_head: bool,
    occupant_kind: OccupantKind,
    occupant_name: Option<String>,
    user_id: Option<Uuid>,
    ai_profile_id: Option<Uuid>,
}

#[derive(Serialize, FromRow)]
struct NamedOption {
    id: Uuid,
    name: String,
}

#[derive(Serialize, FromRow)]
struct PersonOption {
    id: Uuid,
    display_name: String,
}

#[derive(Serialize, FromRow)]
struct JobPositionOption {
    id: Uuid,
    name: String,
    department_id: Option<Uuid>,
}

#[derive(Serialize)]
struct CompanyStructure {
    positions: Vec<CompanyPosition>,
    departments: Vec<NamedOption>,
    people: Vec<PersonOption>,
    agents: Vec<NamedOption>,
    job_positions: Vec<JobPositionOption>,
    can_manage: bool,
}

fn normalize_name(value: String, label: &str, max_length: usize) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_length || value.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(format!(
            "{label} must contain 1 to {max_length} characters without control characters"
        )));
    }
    Ok(value.to_owned())
}

async fn require_admin(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<(), AppError> {
    if actor.is_demo() {
        return Err(AppError::Forbidden);
    }
    crate::projects::require_project_admin_access(&state.db, actor, project_id).await
}

/// All mutations lock the same project row before validating reporting lines and
/// department heads, so concurrent edits cannot create cycles or duplicate heads.
async fn lock_company(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<(), AppError> {
    let kind = sqlx::query_scalar::<_, String>(
        "SELECT project_kind FROM projects WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    require_company(&kind)
}

fn require_company(kind: &str) -> Result<(), AppError> {
    if kind != "company" {
        return Err(AppError::BadRequest(
            "company structure is only available for company projects".to_owned(),
        ));
    }
    Ok(())
}

const POSITIONS_SELECT: &str = r#"
    SELECT position.id, position.title, position.job_position_id, position.department_id,
           position.reports_to_position_id, position.is_department_head,
           position.occupant_kind,
           CASE position.occupant_kind
               WHEN 'agent' THEN agent.name
               WHEN 'human' THEN COALESCE(person.display_name, position.occupant_name)
               ELSE NULL
           END AS occupant_name,
           position.user_id, position.ai_profile_id
    FROM company_positions AS position
    LEFT JOIN ai_profiles AS agent ON agent.tenant_id=position.tenant_id
        AND agent.project_id=position.project_id AND agent.id=position.ai_profile_id
    LEFT JOIN users AS person ON person.id=position.user_id
        AND EXISTS (SELECT 1 FROM memberships AS membership
            WHERE membership.tenant_id=position.tenant_id AND membership.user_id=person.id
              AND (membership.project_id=position.project_id OR membership.project_id IS NULL)
              AND membership.revoked_at IS NULL)
    WHERE position.tenant_id=$1 AND position.project_id=$2
"#;

async fn get_structure(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
) -> Result<Json<CompanyStructure>, AppError> {
    require_admin(&state, &actor, project_id).await?;
    let kind: String =
        sqlx::query_scalar("SELECT project_kind FROM projects WHERE tenant_id=$1 AND id=$2")
            .bind(actor.tenant_id)
            .bind(project_id)
            .fetch_one(&state.db)
            .await?;
    require_company(&kind)?;
    let positions = sqlx::query_as::<_, CompanyPosition>(&format!(
        "{POSITIONS_SELECT} ORDER BY position.created_at, position.id LIMIT 501"
    ))
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let departments = sqlx::query_as::<_, NamedOption>(
        "SELECT id,name FROM departments WHERE tenant_id=$1 AND project_id=$2 ORDER BY position,name,id LIMIT 1001",
    ).bind(actor.tenant_id).bind(project_id).fetch_all(&state.db).await?;
    let people = sqlx::query_as::<_, PersonOption>(
        r#"SELECT person.id,person.display_name FROM users AS person
           WHERE person.status='active' AND EXISTS (SELECT 1 FROM memberships AS membership
               WHERE membership.tenant_id=$1 AND membership.user_id=person.id
                 AND (membership.project_id=$2 OR membership.project_id IS NULL)
                 AND membership.revoked_at IS NULL)
           ORDER BY person.display_name,person.id LIMIT 1001"#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let agents = sqlx::query_as::<_, NamedOption>(
        "SELECT id,name FROM ai_profiles WHERE tenant_id=$1 AND project_id=$2 AND status <> 'disabled' ORDER BY name,id LIMIT 1001",
    ).bind(actor.tenant_id).bind(project_id).fetch_all(&state.db).await?;
    let job_positions = sqlx::query_as::<_, JobPositionOption>(
        "SELECT id,name,department_id FROM job_positions WHERE tenant_id=$1 AND project_id=$2 ORDER BY name,id LIMIT 1001",
    ).bind(actor.tenant_id).bind(project_id).fetch_all(&state.db).await?;
    if positions.len() > 500
        || [
            departments.len(),
            people.len(),
            agents.len(),
            job_positions.len(),
        ]
        .into_iter()
        .any(|count| count > MAX_OPTIONS)
    {
        return Err(AppError::BadRequest(
            "company structure supports up to 500 positions and 1000 options of each kind"
                .to_owned(),
        ));
    }
    Ok(Json(CompanyStructure {
        positions,
        departments,
        people,
        agents,
        job_positions,
        can_manage: true,
    }))
}

async fn validate_references(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    position_id: Uuid,
    draft: &PositionDraft,
) -> Result<(), AppError> {
    if let Some(job_position_id) = draft.job_position_id {
        let department_id = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT department_id FROM job_positions WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR SHARE",
        ).bind(tenant_id).bind(project_id).bind(job_position_id)
            .fetch_optional(&mut **transaction).await?
            .ok_or_else(|| AppError::BadRequest("job position must belong to this company".to_owned()))?;
        if department_id.is_some() && department_id != draft.department_id {
            return Err(AppError::BadRequest(
                "job position must belong to the selected department or be shared".to_owned(),
            ));
        }
    }
    if let Some(department_id) = draft.department_id {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM departments WHERE tenant_id=$1 AND project_id=$2 AND id=$3)",
        ).bind(tenant_id).bind(project_id).bind(department_id).fetch_one(&mut **transaction).await?;
        if !exists {
            return Err(AppError::BadRequest(
                "department must belong to this company".to_owned(),
            ));
        }
    }
    if let Some(manager_id) = draft.reports_to_position_id {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM company_positions WHERE tenant_id=$1 AND project_id=$2 AND id=$3)",
        ).bind(tenant_id).bind(project_id).bind(manager_id).fetch_one(&mut **transaction).await?;
        if !exists {
            return Err(AppError::BadRequest(
                "manager position must belong to this company".to_owned(),
            ));
        }
        let cycle: bool = sqlx::query_scalar(
            r#"WITH RECURSIVE ancestors AS (
                   SELECT id,reports_to_position_id FROM company_positions
                   WHERE tenant_id=$1 AND project_id=$2 AND id=$3
                   UNION
                   SELECT parent.id,parent.reports_to_position_id FROM company_positions AS parent
                   JOIN ancestors ON ancestors.reports_to_position_id=parent.id
                   WHERE parent.tenant_id=$1 AND parent.project_id=$2
               ) SELECT EXISTS (SELECT 1 FROM ancestors WHERE id=$4)"#,
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(manager_id)
        .bind(position_id)
        .fetch_one(&mut **transaction)
        .await?;
        if cycle {
            return Err(AppError::BadRequest(
                "reporting lines cannot contain a cycle".to_owned(),
            ));
        }
    }
    if draft.is_department_head {
        let occupied: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM company_positions WHERE tenant_id=$1 AND project_id=$2 AND department_id=$3 AND is_department_head AND id<>$4)",
        ).bind(tenant_id).bind(project_id).bind(draft.department_id).bind(position_id).fetch_one(&mut **transaction).await?;
        if occupied {
            return Err(AppError::Conflict(
                "the department already has a head position".to_owned(),
            ));
        }
    }
    if let Some(user_id) = draft.user_id {
        let assigned_position = sqlx::query_scalar::<_, Option<Uuid>>(
            r#"SELECT membership.position_id FROM users AS person JOIN memberships AS membership
                   ON membership.user_id=person.id
                   WHERE person.id=$3 AND person.status='active' AND membership.tenant_id=$1
                     AND (membership.project_id=$2 OR membership.project_id IS NULL)
                     AND membership.revoked_at IS NULL
                   ORDER BY membership.project_id IS NULL, membership.id LIMIT 1 FOR SHARE OF membership,person"#,
        ).bind(tenant_id).bind(project_id).bind(user_id).fetch_optional(&mut **transaction).await?
            .ok_or_else(|| AppError::BadRequest("user must be an active member of this company".to_owned()))?;
        check_existing_assignment(draft.job_position_id, assigned_position)?;
    }
    if let Some(profile_id) = draft.ai_profile_id {
        let assigned_position = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT position_id FROM ai_profiles WHERE tenant_id=$1 AND project_id=$2 AND id=$3 AND status <> 'disabled' FOR SHARE",
        ).bind(tenant_id).bind(project_id).bind(profile_id).fetch_optional(&mut **transaction).await?
            .ok_or_else(|| AppError::BadRequest("AI agent must belong to this company and must not be disabled".to_owned()))?;
        check_existing_assignment(draft.job_position_id, assigned_position)?;
    }
    Ok(())
}

fn check_existing_assignment(
    requested: Option<Uuid>,
    assigned: Option<Uuid>,
) -> Result<(), AppError> {
    if requested.is_some() && assigned.is_some() && requested != assigned {
        return Err(AppError::Conflict(
            "the occupant is assigned to a different job position; reconcile the team assignment before linking this position".to_owned(),
        ));
    }
    Ok(())
}

async fn load_position(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    position_id: Uuid,
) -> Result<CompanyPosition, AppError> {
    sqlx::query_as::<_, CompanyPosition>(&format!("{POSITIONS_SELECT} AND position.id=$3"))
        .bind(tenant_id)
        .bind(project_id)
        .bind(position_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(AppError::NotFound)
}

async fn create_position(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
    Json(draft): Json<PositionDraft>,
) -> Result<(StatusCode, Json<CompanyPosition>), AppError> {
    require_admin(&state, &actor, project_id).await?;
    let draft = draft.normalize()?;
    let mut transaction = state.db.begin().await?;
    lock_company(&mut transaction, actor.tenant_id, project_id).await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM company_positions WHERE tenant_id=$1 AND project_id=$2",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await?;
    if count >= MAX_POSITIONS {
        return Err(AppError::BadRequest(
            "a company supports up to 500 positions".to_owned(),
        ));
    }
    let position_id = Uuid::now_v7();
    validate_references(
        &mut transaction,
        actor.tenant_id,
        project_id,
        position_id,
        &draft,
    )
    .await?;
    sqlx::query(
        r#"INSERT INTO company_positions
           (id,tenant_id,project_id,title,department_id,reports_to_position_id,is_department_head,occupant_kind,occupant_name,user_id,ai_profile_id,job_position_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"#,
    ).bind(position_id).bind(actor.tenant_id).bind(project_id).bind(&draft.title)
        .bind(draft.department_id).bind(draft.reports_to_position_id).bind(draft.is_department_head)
        .bind(draft.occupant_kind).bind(&draft.occupant_name).bind(draft.user_id).bind(draft.ai_profile_id)
        .bind(draft.job_position_id)
        .execute(&mut *transaction).await.map_err(write_error)?;
    crate::projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "company_position.created",
        "company_position",
        Some(position_id),
        serde_json::json!({"title":draft.title}),
    )
    .await?;
    let position =
        load_position(&mut transaction, actor.tenant_id, project_id, position_id).await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(position)))
}

async fn update_position(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, position_id)): Path<(Uuid, Uuid)>,
    Json(draft): Json<PositionDraft>,
) -> Result<Json<CompanyPosition>, AppError> {
    require_admin(&state, &actor, project_id).await?;
    let draft = draft.normalize()?;
    let mut transaction = state.db.begin().await?;
    lock_company(&mut transaction, actor.tenant_id, project_id).await?;
    load_position(&mut transaction, actor.tenant_id, project_id, position_id).await?;
    validate_references(
        &mut transaction,
        actor.tenant_id,
        project_id,
        position_id,
        &draft,
    )
    .await?;
    sqlx::query(
        r#"UPDATE company_positions SET title=$4,department_id=$5,reports_to_position_id=$6,
            is_department_head=$7,occupant_kind=$8,occupant_name=$9,user_id=$10,ai_profile_id=$11,job_position_id=$12,updated_at=now()
           WHERE tenant_id=$1 AND project_id=$2 AND id=$3"#,
    ).bind(actor.tenant_id).bind(project_id).bind(position_id).bind(&draft.title)
        .bind(draft.department_id).bind(draft.reports_to_position_id).bind(draft.is_department_head)
        .bind(draft.occupant_kind).bind(&draft.occupant_name).bind(draft.user_id).bind(draft.ai_profile_id)
        .bind(draft.job_position_id)
        .execute(&mut *transaction).await.map_err(write_error)?;
    crate::projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "company_position.updated",
        "company_position",
        Some(position_id),
        serde_json::json!({"title":draft.title}),
    )
    .await?;
    let position =
        load_position(&mut transaction, actor.tenant_id, project_id, position_id).await?;
    transaction.commit().await?;
    Ok(Json(position))
}

async fn delete_position(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, position_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_admin(&state, &actor, project_id).await?;
    let mut transaction = state.db.begin().await?;
    lock_company(&mut transaction, actor.tenant_id, project_id).await?;
    let deleted =
        sqlx::query("DELETE FROM company_positions WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
            .bind(actor.tenant_id)
            .bind(project_id)
            .bind(position_id)
            .execute(&mut *transaction)
            .await
            .map_err(write_error)?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    crate::projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "company_position.deleted",
        "company_position",
        Some(position_id),
        serde_json::json!({}),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn write_error(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database) = &error {
        if database.is_unique_violation() {
            return AppError::Conflict("the department already has a head position".to_owned());
        }
        if database.is_foreign_key_violation() {
            return AppError::Conflict(
                "the position has direct reports or a referenced company resource changed; update the structure and retry".to_owned(),
            );
        }
    }
    AppError::Database(error)
}
