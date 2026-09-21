//! Project-owned reusable instructions assigned to visible AI profiles.

mod creator;
pub(crate) mod sources;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, put},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, PgExecutor, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState, ai_settings, auth::ActorContext, error::AppError, projects, resource_visibility,
};

/// Routes for the project skill library and agent assignments.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(creator::router())
        .route("/api/v1/ai/skills", get(list_skills).post(create_skill))
        .route(
            "/api/v1/ai/profiles/{profile_id}/skills",
            put(update_profile_skills),
        )
        .route(
            "/api/v1/ai/skills/{skill_id}",
            patch(update_skill).delete(delete_skill),
        )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillRequest {
    name: String,
    description: String,
    instructions: String,
    ai_profile_ids: Vec<Uuid>,
}

impl SkillRequest {
    fn normalize(mut self) -> Result<Self, AppError> {
        for (value, field, minimum, maximum) in [
            (&mut self.name, "name", 1, 200),
            (&mut self.description, "description", 0, 2_000),
            (&mut self.instructions, "instructions", 1, 50_000),
        ] {
            *value = value.trim().to_owned();
            if !(minimum..=maximum).contains(&value.chars().count()) || value.contains('\0') {
                return Err(AppError::BadRequest(format!(
                    "{field} must contain between {minimum} and {maximum} characters without null bytes"
                )));
            }
        }
        if self.ai_profile_ids.len() > 64 {
            return Err(AppError::BadRequest(
                "assign a skill to at most 64 AI agents".to_owned(),
            ));
        }
        self.ai_profile_ids.sort_unstable();
        let count = self.ai_profile_ids.len();
        self.ai_profile_ids.dedup();
        if self.ai_profile_ids.len() != count {
            return Err(AppError::BadRequest(
                "AI agent assignments must be unique".to_owned(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, FromRow, Serialize)]
struct SkillResponse {
    id: Uuid,
    name: String,
    description: String,
    instructions: String,
    ai_profile_ids: Vec<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct SkillListResponse {
    items: Vec<SkillResponse>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileSkills {
    skill_ids: Vec<Uuid>,
}

const SELECT_SKILL: &str = r#"
    SELECT skill.id, skill.name, skill.description, skill.instructions,
           skill.created_at, skill.updated_at,
           ARRAY(SELECT assignment.ai_profile_id FROM ai_profile_skills assignment
                 JOIN ai_profiles profile ON profile.tenant_id=assignment.tenant_id
                   AND profile.id=assignment.ai_profile_id
                 WHERE assignment.tenant_id=skill.tenant_id
                   AND assignment.project_id=skill.project_id
                   AND assignment.skill_id=skill.id
                   AND resource_visible(profile.visibility,$2,$3)
                   AND ($4::uuid IS NULL OR profile.project_id=$4)
                 ORDER BY assignment.ai_profile_id) AS ai_profile_ids
    FROM ai_skills skill
    WHERE skill.tenant_id=$1 AND skill.project_id=$2
"#;

fn require_write(actor: &ActorContext) -> Result<Uuid, AppError> {
    let project_id = ai_settings::require_management(actor)?;
    if actor.is_demo() || actor.has_restricted_inbox_scope() {
        return Err(AppError::Forbidden);
    }
    Ok(project_id)
}

async fn list_skills(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<SkillListResponse>, AppError> {
    let project_id = ai_settings::require_management(&actor)?;
    let query = format!("{SELECT_SKILL} ORDER BY skill.name, skill.id");
    let items = sqlx::query_as(&query)
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(actor.department_id())
        .bind(resource_visibility::owner_limit(&actor))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(SkillListResponse { items }))
}

async fn update_profile_skills(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile_id): Path<Uuid>,
    Json(mut input): Json<ProfileSkills>,
) -> Result<Json<ProfileSkills>, AppError> {
    let project_id = require_write(&actor)?;
    input.skill_ids.sort_unstable();
    let count = input.skill_ids.len();
    input.skill_ids.dedup();
    if count != input.skill_ids.len() {
        return Err(AppError::BadRequest(
            "skill assignments must be unique".to_owned(),
        ));
    }
    let mut tx = state.db.begin().await?;
    // Match skill CRUD's skill -> profile lock order. Locking the project
    // library also serializes edits from different agent cards and protects
    // each skill's assignment limit without rewriting its other assignments.
    let available: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM ai_skills WHERE tenant_id=$1 AND project_id=$2 ORDER BY id FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    validate_profiles(&mut tx, &actor, &[profile_id]).await?;
    if input
        .skill_ids
        .iter()
        .any(|id| available.binary_search(id).is_err())
    {
        return Err(AppError::NotFound);
    }
    let full: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ai_profile_skills \
         WHERE tenant_id=$1 AND project_id=$2 AND skill_id=ANY($3) AND ai_profile_id<>$4 \
         GROUP BY skill_id HAVING count(*)>=64)",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&input.skill_ids)
    .bind(profile_id)
    .fetch_one(&mut *tx)
    .await?;
    if full {
        return Err(AppError::Conflict(
            "a selected skill is already assigned to 64 AI agents".to_owned(),
        ));
    }
    sqlx::query(
        "DELETE FROM ai_profile_skills WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO ai_profile_skills (tenant_id,project_id,skill_id,ai_profile_id) \
         SELECT $1,$2,skill_id,$3 FROM unnest($4::uuid[]) AS skill_id",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(&input.skill_ids)
    .execute(&mut *tx)
    .await?;
    projects::insert_audit(
        &mut tx,
        &actor,
        Some(project_id),
        "ai_profile.skills_updated",
        "ai_profile",
        Some(profile_id),
        json!({"skill_ids": input.skill_ids}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(input))
}

async fn load_skill(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    skill_id: Uuid,
) -> Result<SkillResponse, AppError> {
    let query = format!("{SELECT_SKILL} AND skill.id=$5 FOR UPDATE OF skill");
    sqlx::query_as(&query)
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(actor.department_id())
        .bind(resource_visibility::owner_limit(actor))
        .bind(skill_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(AppError::NotFound)
}

async fn validate_profiles(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    profile_ids: &[Uuid],
) -> Result<(), AppError> {
    let found: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id FROM ai_profiles
        WHERE tenant_id=$1 AND id=ANY($2)
          AND resource_visible(visibility,$3,$4)
          AND ($5::uuid IS NULL OR project_id=$5)
        ORDER BY id FOR SHARE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(profile_ids)
    .bind(actor.project_id)
    .bind(actor.department_id())
    .bind(resource_visibility::owner_limit(actor))
    .fetch_all(&mut **tx)
    .await?;
    if found.len() != profile_ids.len() {
        return Err(AppError::NotFound);
    }
    Ok(())
}

async fn save_assignments(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    skill_id: Uuid,
    profile_ids: &[Uuid],
) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM ai_profile_skills WHERE tenant_id=$1 AND project_id=$2 AND skill_id=$3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(skill_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO ai_profile_skills (tenant_id,project_id,skill_id,ai_profile_id) \
         SELECT $1,$2,$3,profile_id FROM unnest($4::uuid[]) AS profile_id",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(skill_id)
    .bind(profile_ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn validate_existing_assignments(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    skill_id: Uuid,
) -> Result<(), AppError> {
    // A filtered list must never let an editor overwrite assignments that they
    // cannot manage, or change instructions for an inaccessible shared profile.
    let profiles: Vec<Uuid> = sqlx::query_scalar(
        "SELECT ai_profile_id FROM ai_profile_skills WHERE tenant_id=$1 AND project_id=$2 AND skill_id=$3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(skill_id)
    .fetch_all(&mut **tx)
    .await?;
    validate_profiles(tx, actor, &profiles)
        .await
        .map_err(|error| {
            if matches!(error, AppError::NotFound) {
                AppError::Forbidden
            } else {
                error
            }
        })
}

async fn create_skill(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<SkillRequest>,
) -> Result<(StatusCode, Json<SkillResponse>), AppError> {
    let project_id = require_write(&actor)?;
    let input = request.normalize()?;
    let skill_id = Uuid::now_v7();
    let mut tx = state.db.begin().await?;
    validate_profiles(&mut tx, &actor, &input.ai_profile_ids).await?;
    sqlx::query(
        "INSERT INTO ai_skills (id,tenant_id,project_id,name,description,instructions) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(skill_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.instructions)
    .execute(&mut *tx)
    .await
    .map_err(write_error)?;
    save_assignments(&mut tx, &actor, project_id, skill_id, &input.ai_profile_ids).await?;
    projects::insert_audit(
        &mut tx,
        &actor,
        Some(project_id),
        "ai_skill.created",
        "ai_skill",
        Some(skill_id),
        json!({"ai_profile_ids": input.ai_profile_ids}),
    )
    .await?;
    let response = load_skill(&mut tx, &actor, project_id, skill_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn update_skill(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(skill_id): Path<Uuid>,
    Json(request): Json<SkillRequest>,
) -> Result<Json<SkillResponse>, AppError> {
    let project_id = require_write(&actor)?;
    let input = request.normalize()?;
    let mut tx = state.db.begin().await?;
    load_skill(&mut tx, &actor, project_id, skill_id).await?;
    validate_existing_assignments(&mut tx, &actor, project_id, skill_id).await?;
    validate_profiles(&mut tx, &actor, &input.ai_profile_ids).await?;
    sqlx::query(
        "UPDATE ai_skills SET name=$4,description=$5,instructions=$6,updated_at=now() \
         WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(skill_id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.instructions)
    .execute(&mut *tx)
    .await
    .map_err(write_error)?;
    save_assignments(&mut tx, &actor, project_id, skill_id, &input.ai_profile_ids).await?;
    projects::insert_audit(
        &mut tx,
        &actor,
        Some(project_id),
        "ai_skill.updated",
        "ai_skill",
        Some(skill_id),
        json!({"ai_profile_ids": input.ai_profile_ids}),
    )
    .await?;
    let response = load_skill(&mut tx, &actor, project_id, skill_id).await?;
    tx.commit().await?;
    Ok(Json(response))
}

async fn delete_skill(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(skill_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let project_id = require_write(&actor)?;
    let mut tx = state.db.begin().await?;
    load_skill(&mut tx, &actor, project_id, skill_id).await?;
    validate_existing_assignments(&mut tx, &actor, project_id, skill_id).await?;
    sqlx::query("DELETE FROM ai_skills WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(skill_id)
        .execute(&mut *tx)
        .await?;
    projects::insert_audit(
        &mut tx,
        &actor,
        Some(project_id),
        "ai_skill.deleted",
        "ai_skill",
        Some(skill_id),
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("a skill with this name already exists in this project".to_owned())
    } else {
        AppError::Database(error)
    }
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub(crate) struct RuntimeSkill {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) instructions: String,
}

/// Loads assignments for the current execution project, including shared agents.
pub(crate) async fn load_profile_skills<'e>(
    db: impl PgExecutor<'e>,
    tenant_id: Uuid,
    project_id: Uuid,
    profile_id: Uuid,
) -> Result<Vec<RuntimeSkill>, AppError> {
    Ok(sqlx::query_as(
        r#"
        SELECT skill.id, skill.name, skill.description, skill.instructions
        FROM ai_skills skill
        JOIN ai_profile_skills assignment
          ON assignment.tenant_id=skill.tenant_id
         AND assignment.project_id=skill.project_id AND assignment.skill_id=skill.id
        JOIN ai_profiles profile
          ON profile.tenant_id=assignment.tenant_id AND profile.id=assignment.ai_profile_id
        WHERE skill.tenant_id=$1 AND skill.project_id=$2 AND assignment.ai_profile_id=$3
          AND resource_visible(profile.visibility,$2,NULL)
        ORDER BY skill.name, skill.id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .fetch_all(db)
    .await?)
}
