//! Project-scoped typed databases embedded into notes.

pub mod embeds;
pub mod fields;
pub mod model;
pub mod public;
pub mod records;
pub mod relations;
pub mod views;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError, notes};
pub use model::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/notes/databases",
            get(list_route).post(create_route),
        )
        .route(
            "/api/v1/notes/databases/{database_id}",
            get(detail_route).patch(update_route).delete(delete_route),
        )
        .merge(fields::router())
        .merge(records::router())
        .merge(embeds::router())
        .merge(views::router())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub fn require_scope(actor: &ActorContext, permission: &str) -> Result<Scope, AppError> {
    Ok(Scope {
        tenant_id: actor.tenant_id,
        project_id: notes::require_scope(actor, permission)?,
    })
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn lock_project(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
) -> Result<(), AppError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM projects WHERE tenant_id=$1 AND id=$2 AND status='active' FOR UPDATE",
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub fn check_version(current: i32, expected: i32) -> Result<(), AppError> {
    if expected < 1 {
        return Err(AppError::BadRequest(
            "expected_version must be positive".to_owned(),
        ));
    }
    if current != expected {
        return Err(AppError::Conflict(
            "database content changed; reload before saving".to_owned(),
        ));
    }
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn get_database(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    id: Uuid,
) -> Result<Database, AppError> {
    sqlx::query_as("SELECT id,project_id,name,description,icon,version,created_at,updated_at FROM note_databases WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
        .bind(scope.tenant_id).bind(scope.project_id).bind(id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn database_detail(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    id: Uuid,
) -> Result<DatabaseDetail, AppError> {
    Ok(DatabaseDetail {
        database: get_database(tx, scope, id).await?,
        fields: fields::list(tx, scope, id).await?,
        views: views::fetch_views(tx, scope, id).await?,
    })
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn bump_database(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    id: Uuid,
) -> Result<Database, AppError> {
    sqlx::query_as("UPDATE note_databases SET version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND id=$3 RETURNING id,project_id,name,description,icon,version,created_at,updated_at")
        .bind(scope.tenant_id).bind(scope.project_id).bind(id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    actor_id: Option<Uuid>,
    kind: &str,
    id: Uuid,
    action: &str,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO audit_log(id,tenant_id,project_id,actor_id,resource_kind,resource_id,action) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(Uuid::now_v7()).bind(scope.tenant_id).bind(scope.project_id).bind(actor_id).bind(kind).bind(id).bind(action).execute(&mut **tx).await?;
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub fn validate_text(
    value: &str,
    minimum: usize,
    maximum: usize,
    name: &str,
) -> Result<(), AppError> {
    if !(minimum..=maximum).contains(&value.chars().count()) || value.contains('\0') {
        return Err(AppError::BadRequest(format!(
            "{name} must contain {minimum} to {maximum} characters without null characters"
        )));
    }
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub fn validate_order(existing: &[Uuid], provided: &[Uuid]) -> Result<(), AppError> {
    let current = existing
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let next = provided
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    if current != next || provided.len() != next.len() {
        return Err(AppError::Conflict(
            "order must contain every current item exactly once".to_owned(),
        ));
    }
    Ok(())
}

async fn list_route(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<List<Database>>, AppError> {
    let scope = require_scope(&actor, "notes:read")?;
    let items=sqlx::query_as("SELECT id,project_id,name,description,icon,version,created_at,updated_at FROM note_databases WHERE tenant_id=$1 AND project_id=$2 ORDER BY updated_at DESC,id")
        .bind(scope.tenant_id).bind(scope.project_id).fetch_all(&state.db).await?;
    Ok(Json(List { items }))
}

async fn create_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(mut input): Json<CreateDatabase>,
) -> Result<(StatusCode, Json<Database>), AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    input.name = input.name.trim().to_owned();
    validate_text(&input.name, 1, 160, "name")?;
    validate_text(&input.description, 0, 4000, "description")?;
    validate_text(&input.icon, 0, 32, "icon")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let row=sqlx::query_as::<_,Database>("INSERT INTO note_databases(id,tenant_id,project_id,name,description,icon) VALUES($1,$2,$3,$4,$5,$6) RETURNING id,project_id,name,description,icon,version,created_at,updated_at")
        .bind(Uuid::now_v7()).bind(scope.tenant_id).bind(scope.project_id).bind(input.name).bind(input.description).bind(input.icon).fetch_one(&mut *tx).await?;
    audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database",
        row.id,
        "note_database.created",
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn detail_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(id): Path<Uuid>,
) -> Result<Json<DatabaseDetail>, AppError> {
    let scope = require_scope(&actor, "notes:read")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let detail = database_detail(&mut tx, &scope, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

async fn update_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(id): Path<Uuid>,
    Json(mut input): Json<UpdateDatabase>,
) -> Result<Json<Database>, AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    input.name = input.name.trim().to_owned();
    validate_text(&input.name, 1, 160, "name")?;
    validate_text(&input.description, 0, 4000, "description")?;
    validate_text(&input.icon, 0, 32, "icon")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    check_version(
        get_database(&mut tx, &scope, id).await?.version,
        input.expected_version,
    )?;
    sqlx::query("UPDATE note_databases SET name=$4,description=$5,icon=$6 WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
        .bind(scope.tenant_id).bind(scope.project_id).bind(id).bind(input.name).bind(input.description).bind(input.icon).execute(&mut *tx).await?;
    let row = bump_database(&mut tx, &scope, id).await?;
    audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database",
        id,
        "note_database.updated",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(row))
}

async fn delete_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(id): Path<Uuid>,
    Query(input): Query<VersionQuery>,
) -> Result<StatusCode, AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    check_version(
        get_database(&mut tx, &scope, id).await?.version,
        input.expected_version,
    )?;
    let referenced:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM note_database_embeds WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3) OR EXISTS(SELECT 1 FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id<>$3 AND (relation_target_database_id=$3 OR rollup_target_field_id IN(SELECT id FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3)))")
        .bind(scope.tenant_id).bind(scope.project_id).bind(id).fetch_one(&mut *tx).await?;
    if referenced {
        return Err(AppError::Conflict(
            "remove database embeds and related fields before deleting this database".to_owned(),
        ));
    }
    sqlx::query("DELETE FROM note_databases WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
        .bind(scope.tenant_id)
        .bind(scope.project_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database",
        id,
        "note_database.deleted",
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
