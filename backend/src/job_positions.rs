//! Project job descriptions, approved KPI definitions, and human/AI assignments.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post, put},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json as SqlJson};
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError, projects::insert_audit};

mod generation;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/projects/{project_id}/positions",
            get(list_positions).post(create_position),
        )
        .route(
            "/api/v1/projects/{project_id}/positions/{position_id}",
            patch(update_position),
        )
        .route(
            "/api/v1/projects/{project_id}/positions/{position_id}/approve-kpis",
            post(approve_kpis),
        )
        .route("/api/v1/projects/{project_id}/team", get(list_team))
        .route(
            "/api/v1/projects/{project_id}/team/members/{user_id}/position",
            put(assign_member),
        )
        .route(
            "/api/v1/projects/{project_id}/team/agents/{profile_id}/position",
            put(assign_agent),
        )
        .merge(generation::router())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KpiMetric {
    pub name: String,
    pub formula: String,
    pub data_source: String,
    pub target: String,
    pub evaluation_period: String,
    pub data_owner: String,
}

impl KpiMetric {
    pub(crate) fn normalize(self, approved: bool) -> Result<Self, AppError> {
        Ok(Self {
            name: normalize_text(self.name, 200, "KPI name", approved)?,
            formula: normalize_text(self.formula, 2000, "KPI formula", approved)?,
            data_source: normalize_text(self.data_source, 1000, "KPI data source", approved)?,
            target: normalize_text(self.target, 500, "KPI target", approved)?,
            evaluation_period: normalize_text(
                self.evaluation_period,
                200,
                "KPI evaluation period",
                approved,
            )?,
            data_owner: normalize_text(self.data_owner, 200, "KPI data owner", approved)?,
        })
    }
}

#[derive(Debug, Serialize, FromRow)]
pub(crate) struct JobPosition {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub department_id: Option<Uuid>,
    pub kpi_goal: String,
    pub draft_kpis: SqlJson<Vec<KpiMetric>>,
    pub approved_kpis: SqlJson<Vec<KpiMetric>>,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<Uuid>,
    pub revision: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PositionInput {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    instructions: String,
    department_id: Option<Uuid>,
    #[serde(default)]
    kpi_goal: String,
    #[serde(default)]
    draft_kpis: Vec<KpiMetric>,
    revision: Option<i32>,
}

impl PositionInput {
    fn normalize(self) -> Result<Self, AppError> {
        Ok(Self {
            name: normalize_text(self.name, 200, "position name", true)?,
            description: normalize_text(self.description, 10000, "description", false)?,
            instructions: normalize_text(self.instructions, 50000, "instructions", false)?,
            kpi_goal: normalize_text(self.kpi_goal, 10000, "KPI goal", false)?,
            draft_kpis: validate_kpis(self.draft_kpis, false)?,
            department_id: self.department_id,
            revision: self.revision,
        })
    }
}

#[derive(Serialize)]
struct PositionList {
    items: Vec<JobPosition>,
}

#[derive(Serialize, FromRow)]
struct TeamMember {
    user_id: Uuid,
    display_name: String,
    department_id: Option<Uuid>,
    position_id: Option<Uuid>,
}

#[derive(Serialize, FromRow)]
struct TeamAgent {
    id: Uuid,
    name: String,
    department_id: Option<Uuid>,
    position_id: Option<Uuid>,
}

#[derive(Serialize)]
struct TeamResponse {
    company_project: bool,
    members: Vec<TeamMember>,
    agents: Vec<TeamAgent>,
    can_manage: bool,
}

fn can_manage(actor: &ActorContext) -> bool {
    !actor.is_demo()
        && !actor.department_restricted()
        && !actor.has_restricted_inbox_scope()
        && actor.require("teams:manage").is_ok()
}

async fn require_read(db: &PgPool, actor: &ActorContext, project_id: Uuid) -> Result<(), AppError> {
    actor.require_password_session()?;
    actor.require_project(project_id)?;
    if actor.project_id != Some(project_id) {
        return Err(AppError::Forbidden);
    }
    let accessible = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM projects p JOIN memberships m ON m.tenant_id=p.tenant_id AND (m.project_id=p.id OR m.project_id IS NULL) WHERE p.tenant_id=$1 AND p.id=$2 AND m.user_id=$3 AND m.revoked_at IS NULL)",
    )
    .bind(actor.tenant_id).bind(project_id).bind(actor.actor_id).fetch_one(db).await?;
    if !accessible {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub(super) async fn require_write(
    db: &PgPool,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<(), AppError> {
    require_read(db, actor, project_id).await?;
    if !can_manage(actor) {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub(super) async fn load_position(
    db: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    position_id: Uuid,
) -> Result<JobPosition, AppError> {
    sqlx::query_as::<_, JobPosition>(
        "SELECT * FROM job_positions WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(position_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn list_positions(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
) -> Result<Json<PositionList>, AppError> {
    require_read(&state.db, &actor, project_id).await?;
    let mut items = sqlx::query_as::<_, JobPosition>(
        "SELECT * FROM job_positions WHERE tenant_id=$1 AND project_id=$2 AND ($3::uuid IS NULL OR department_id IS NULL OR department_id=$3) ORDER BY lower(name),id",
    ).bind(actor.tenant_id).bind(project_id).bind(actor.department_id()).fetch_all(&state.db).await?;
    if !can_manage(&actor) {
        for position in &mut items {
            position.draft_kpis.0.clear();
            position.kpi_goal.clear();
        }
    }
    Ok(Json(PositionList { items }))
}

async fn validate_department(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    department_id: Option<Uuid>,
) -> Result<(), AppError> {
    if let Some(department_id) = department_id {
        let exists = sqlx::query_scalar::<_, Uuid>("SELECT id FROM departments WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR KEY SHARE")
            .bind(actor.tenant_id).bind(project_id).bind(department_id).fetch_optional(&mut **transaction).await?;
        if exists.is_none() {
            return Err(AppError::BadRequest(
                "department must belong to this project".to_owned(),
            ));
        }
    }
    Ok(())
}

// Acquire the project before child rows, matching organization-structure writes.
async fn lock_project(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM projects WHERE tenant_id=$1 AND id=$2 FOR KEY SHARE",
    )
    .bind(tenant_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(())
}

async fn create_position(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
    Json(input): Json<PositionInput>,
) -> Result<(StatusCode, Json<JobPosition>), AppError> {
    require_write(&state.db, &actor, project_id).await?;
    let input = input.normalize()?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id).await?;
    validate_department(&mut transaction, &actor, project_id, input.department_id).await?;
    let position = sqlx::query_as::<_, JobPosition>(
        "INSERT INTO job_positions (id,tenant_id,project_id,department_id,name,description,instructions,kpi_goal,draft_kpis) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *",
    ).bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project_id).bind(input.department_id).bind(input.name).bind(input.description).bind(input.instructions).bind(input.kpi_goal).bind(SqlJson(input.draft_kpis)).fetch_one(&mut *transaction).await?;
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "job_position.created",
        "job_position",
        Some(position.id),
        serde_json::json!({}),
    )
    .await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(position)))
}

async fn lock_position(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    position_id: Uuid,
) -> Result<JobPosition, AppError> {
    sqlx::query_as::<_, JobPosition>(
        "SELECT * FROM job_positions WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(position_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

fn check_revision(actual: i32, expected: Option<i32>) -> Result<(), AppError> {
    match expected {
        None => Err(AppError::BadRequest("revision is required".to_owned())),
        Some(expected) if expected != actual => Err(AppError::Conflict(
            "Position changed. Reload it before saving.".to_owned(),
        )),
        Some(_) => Ok(()),
    }
}

async fn update_position(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, position_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<PositionInput>,
) -> Result<Json<JobPosition>, AppError> {
    require_write(&state.db, &actor, project_id).await?;
    let input = input.normalize()?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id).await?;
    let existing =
        lock_position(&mut transaction, actor.tenant_id, project_id, position_id).await?;
    check_revision(existing.revision, input.revision)?;
    validate_department(&mut transaction, &actor, project_id, input.department_id).await?;
    if let Some(department_id) = input.department_id {
        let incompatible = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM memberships WHERE tenant_id=$1 AND project_id=$2 AND position_id=$3 AND revoked_at IS NULL AND department_id IS NOT NULL AND department_id<>$4) OR EXISTS(SELECT 1 FROM ai_profiles WHERE tenant_id=$1 AND project_id=$2 AND position_id=$3 AND NOT resource_visible(visibility,$2,$4)) OR EXISTS(SELECT 1 FROM company_positions WHERE tenant_id=$1 AND project_id=$2 AND job_position_id=$3 AND department_id IS DISTINCT FROM $4)",
        ).bind(actor.tenant_id).bind(project_id).bind(position_id).bind(department_id).fetch_one(&mut *transaction).await?;
        if incompatible {
            return Err(AppError::Conflict(
                "Position department is incompatible with its assigned team members.".to_owned(),
            ));
        }
    }
    let position = sqlx::query_as::<_, JobPosition>(
        "UPDATE job_positions SET name=$4,description=$5,instructions=$6,department_id=$7,kpi_goal=$8,draft_kpis=$9,revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND id=$3 RETURNING *",
    ).bind(actor.tenant_id).bind(project_id).bind(position_id).bind(input.name).bind(input.description).bind(input.instructions).bind(input.department_id).bind(input.kpi_goal).bind(SqlJson(input.draft_kpis)).fetch_one(&mut *transaction).await?;
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "job_position.updated",
        "job_position",
        Some(position_id),
        serde_json::json!({"revision":position.revision}),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(position))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalInput {
    revision: i32,
}

async fn approve_kpis(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, position_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<ApprovalInput>,
) -> Result<Json<JobPosition>, AppError> {
    require_write(&state.db, &actor, project_id).await?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id).await?;
    let existing =
        lock_position(&mut transaction, actor.tenant_id, project_id, position_id).await?;
    check_revision(existing.revision, Some(input.revision))?;
    let approved = validate_kpis(existing.draft_kpis.0, true)?;
    let position = sqlx::query_as::<_, JobPosition>(
        "UPDATE job_positions SET approved_kpis=$4,approved_by=$5,approved_at=now(),revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND id=$3 RETURNING *",
    ).bind(actor.tenant_id).bind(project_id).bind(position_id).bind(SqlJson(approved)).bind(actor.actor_id).fetch_one(&mut *transaction).await?;
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "job_position.kpis_approved",
        "job_position",
        Some(position_id),
        serde_json::json!({"revision":position.revision,"count":position.approved_kpis.len()}),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(position))
}

async fn list_team(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(project_id): Path<Uuid>,
) -> Result<Json<TeamResponse>, AppError> {
    require_read(&state.db, &actor, project_id).await?;
    let manage = can_manage(&actor);
    let company_project = sqlx::query_scalar::<_, bool>(
        "SELECT project_kind='company' FROM projects WHERE tenant_id=$1 AND id=$2",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_one(&state.db)
    .await?;
    let members = sqlx::query_as::<_, TeamMember>(
        r#"SELECT m.user_id,u.display_name,m.department_id,p.id AS position_id
        FROM memberships m JOIN users u ON u.id=m.user_id
        LEFT JOIN job_positions p ON p.tenant_id=m.tenant_id AND p.project_id=m.project_id AND p.id=m.position_id
            AND (p.department_id IS NULL OR $4::uuid IS NULL OR p.department_id=$4)
        WHERE m.tenant_id=$1 AND m.project_id=$2 AND m.revoked_at IS NULL AND u.status='active'
            AND ($3 OR m.user_id=$5) AND ($4::uuid IS NULL OR m.department_id IS NULL OR m.department_id=$4)
        ORDER BY lower(u.display_name),m.user_id"#,
    ).bind(actor.tenant_id).bind(project_id).bind(manage).bind(actor.department_id()).bind(actor.actor_id).fetch_all(&state.db).await?;
    let agents = if actor.require("ai:manage").is_ok() {
        sqlx::query_as::<_, TeamAgent>(
            r#"SELECT a.id,a.name,COALESCE(p.department_id,d.department_id) AS department_id,p.id AS position_id
            FROM ai_profiles a
            LEFT JOIN job_positions p ON p.tenant_id=a.tenant_id AND p.project_id=a.project_id AND p.id=a.position_id
                AND (p.department_id IS NULL OR $3::uuid IS NULL OR p.department_id=$3)
            LEFT JOIN LATERAL (SELECT CASE WHEN count(*)=1 THEN (array_agg(id))[1] END AS department_id
                FROM departments WHERE tenant_id=a.tenant_id AND project_id=a.project_id AND a.visibility->'department_ids' ? id::text) d ON true
            WHERE a.tenant_id=$1 AND a.project_id=$2 AND resource_visible(a.visibility,$2,$3)
            ORDER BY lower(a.name),a.id"#,
        ).bind(actor.tenant_id).bind(project_id).bind(actor.department_id()).fetch_all(&state.db).await?
    } else {
        Vec::new()
    };
    Ok(Json(TeamResponse {
        company_project,
        members,
        agents,
        can_manage: manage,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignmentInput {
    position_id: Option<Uuid>,
}

async fn check_organization_assignment(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    position_id: Option<Uuid>,
    user_id: Option<Uuid>,
    profile_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(position_id) = position_id else {
        return Ok(());
    };
    let incompatible = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM company_positions WHERE tenant_id=$1 AND project_id=$2 AND job_position_id IS NOT NULL AND job_position_id<>$3 AND (user_id=$4 OR ai_profile_id=$5))",
    ).bind(tenant_id).bind(project_id).bind(position_id).bind(user_id).bind(profile_id)
        .fetch_one(&mut **transaction).await?;
    if incompatible {
        return Err(AppError::Conflict(
            "The team member occupies an organization seat with a different job position."
                .to_owned(),
        ));
    }
    Ok(())
}

async fn assign_member(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, user_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<AssignmentInput>,
) -> Result<StatusCode, AppError> {
    require_write(&state.db, &actor, project_id).await?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id).await?;
    let position = match input.position_id {
        Some(id) => Some(lock_position(&mut transaction, actor.tenant_id, project_id, id).await?),
        None => None,
    };
    let department = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT m.department_id FROM memberships m JOIN users u ON u.id=m.user_id WHERE m.tenant_id=$1 AND m.project_id=$2 AND m.user_id=$3 AND m.revoked_at IS NULL AND u.status='active' FOR UPDATE OF m",
    ).bind(actor.tenant_id).bind(project_id).bind(user_id).fetch_optional(&mut *transaction).await?.ok_or(AppError::NotFound)?;
    if let (Some(department), Some(position_department)) =
        (department, position.and_then(|p| p.department_id))
        && department != position_department
    {
        return Err(AppError::BadRequest(
            "Position must belong to the member's department or be shared.".to_owned(),
        ));
    }
    check_organization_assignment(
        &mut transaction,
        actor.tenant_id,
        project_id,
        input.position_id,
        Some(user_id),
        None,
    )
    .await?;
    sqlx::query("UPDATE memberships SET position_id=$4,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND user_id=$3 AND revoked_at IS NULL")
        .bind(actor.tenant_id).bind(project_id).bind(user_id).bind(input.position_id).execute(&mut *transaction).await?;
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "job_position.member_assigned",
        "user",
        Some(user_id),
        serde_json::json!({"position_id":input.position_id}),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_agent(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((project_id, profile_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<AssignmentInput>,
) -> Result<StatusCode, AppError> {
    require_write(&state.db, &actor, project_id).await?;
    actor.require("ai:manage")?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id).await?;
    let department_id = match input.position_id {
        Some(id) => {
            lock_position(&mut transaction, actor.tenant_id, project_id, id)
                .await?
                .department_id
        }
        None => None,
    };
    let compatible = sqlx::query_scalar::<_, bool>(
        "SELECT ($5::uuid IS NULL OR resource_visible(visibility,$2,$4)) FROM ai_profiles WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
    ).bind(actor.tenant_id).bind(project_id).bind(profile_id).bind(department_id).bind(input.position_id).fetch_optional(&mut *transaction).await?.ok_or(AppError::NotFound)?;
    if !compatible {
        return Err(AppError::BadRequest(
            "Position department must be visible to the agent.".to_owned(),
        ));
    }
    check_organization_assignment(
        &mut transaction,
        actor.tenant_id,
        project_id,
        input.position_id,
        None,
        Some(profile_id),
    )
    .await?;
    sqlx::query("UPDATE ai_profiles SET position_id=$4,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
        .bind(actor.tenant_id).bind(project_id).bind(profile_id).bind(input.position_id).execute(&mut *transaction).await?;
    insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "job_position.agent_assigned",
        "ai_profile",
        Some(profile_id),
        serde_json::json!({"position_id":input.position_id}),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn validate_kpis(
    metrics: Vec<KpiMetric>,
    approved: bool,
) -> Result<Vec<KpiMetric>, AppError> {
    if metrics.len() > 20 || (approved && metrics.is_empty()) {
        return Err(AppError::BadRequest(
            "KPI set must contain at most 20 metrics and cannot be empty when approved.".to_owned(),
        ));
    }
    metrics
        .into_iter()
        .map(|metric| metric.normalize(approved))
        .collect()
}

fn normalize_text(
    value: String,
    max: usize,
    field: &str,
    required: bool,
) -> Result<String, AppError> {
    let value = value.trim();
    if (required && value.is_empty()) || value.chars().count() > max || value.contains('\0') {
        return Err(AppError::BadRequest(format!(
            "{field} must contain {} to {max} characters",
            usize::from(required)
        )));
    }
    Ok(value.to_owned())
}
