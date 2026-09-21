use super::{
    Embed, Scope, VersionQuery, audit, bump_database, check_version, get_database, lock_project,
    model, require_scope, views,
};
use crate::{AppState, auth::ActorContext, error::AppError};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get as route_get, patch},
};
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateEmbed {
    pub database_id: Uuid,
    pub view_id: Option<Uuid>,
    pub position: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEmbed {
    pub database_id: Uuid,
    #[serde(deserialize_with = "required_view")]
    pub view_id: Option<Uuid>,
    pub position: i32,
    pub expected_version: i32,
}
fn required_view<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Uuid>, D::Error> {
    Option::<Uuid>::deserialize(deserializer)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/notes/{note_id}/databases",
            route_get(list_route).post(create_route),
        )
        .route(
            "/api/v1/notes/{note_id}/databases/{embed_id}",
            patch(update_route).delete(delete_route),
        )
}

async fn require_note(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    note_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM notes WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(note_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn list(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    note_id: Uuid,
) -> Result<Vec<Embed>, AppError> {
    require_note(tx, scope, note_id).await?;
    sqlx::query_as("SELECT id,note_id,database_id,view_id,position,version,created_at,updated_at FROM note_database_embeds WHERE tenant_id=$1 AND project_id=$2 AND note_id=$3 ORDER BY position,id")
        .bind(scope.tenant_id).bind(scope.project_id).bind(note_id).fetch_all(&mut **tx).await.map_err(AppError::from)
}

async fn get(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    note_id: Uuid,
    id: Uuid,
) -> Result<Embed, AppError> {
    sqlx::query_as("SELECT id,note_id,database_id,view_id,position,version,created_at,updated_at FROM note_database_embeds WHERE tenant_id=$1 AND project_id=$2 AND note_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(note_id).bind(id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)
}

async fn validate_target(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    view_id: Option<Uuid>,
    position: i32,
) -> Result<(), AppError> {
    get_database(tx, scope, database_id).await?;
    if let Some(view) = view_id {
        views::fetch_view(tx, scope, database_id, view).await?;
    }
    if !(0..=9999).contains(&position) {
        return Err(AppError::BadRequest(
            "embed position must be between 0 and 9999".to_owned(),
        ));
    }
    Ok(())
}

async fn bump_note(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    note_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("UPDATE notes SET version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
        .bind(scope.tenant_id).bind(scope.project_id).bind(note_id).execute(&mut **tx).await?;
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn create(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    note_id: Uuid,
    input: CreateEmbed,
    actor_id: Option<Uuid>,
) -> Result<Embed, AppError> {
    let existing = list(tx, scope, note_id).await?;
    if existing.len() >= 50 {
        return Err(AppError::Conflict(
            "a note can contain at most 50 database embeds".to_owned(),
        ));
    }
    let count = i32::try_from(existing.len()).map_err(AppError::internal)?;
    let requested = input.position.unwrap_or(count);
    validate_target(tx, scope, input.database_id, input.view_id, requested).await?;
    let position = requested.min(count);
    sqlx::query("UPDATE note_database_embeds SET position=position+1,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND note_id=$3 AND position >= $4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(note_id).bind(position).execute(&mut **tx).await?;
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO note_database_embeds(id,tenant_id,project_id,note_id,database_id,view_id,position) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(scope.tenant_id).bind(scope.project_id).bind(note_id).bind(input.database_id).bind(input.view_id).bind(position).execute(&mut **tx).await?;
    bump_note(tx, scope, note_id).await?;
    bump_database(tx, scope, input.database_id).await?;
    audit(
        tx,
        scope,
        actor_id,
        "note_database_embed",
        id,
        "note_database_embed.created",
    )
    .await?;
    get(tx, scope, note_id, id).await
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn update(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    note_id: Uuid,
    id: Uuid,
    input: UpdateEmbed,
    actor_id: Option<Uuid>,
) -> Result<Embed, AppError> {
    let current = get(tx, scope, note_id, id).await?;
    check_version(current.version, input.expected_version)?;
    validate_target(tx, scope, input.database_id, input.view_id, input.position).await?;
    let count = i32::try_from(list(tx, scope, note_id).await?.len()).map_err(AppError::internal)?;
    let position = input.position.min(count - 1);
    if position != current.position {
        let (lower, upper, delta) = if position < current.position {
            (position, current.position - 1, 1)
        } else {
            (current.position + 1, position, -1)
        };
        sqlx::query("UPDATE note_database_embeds SET position=position+$6,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND note_id=$3 AND position BETWEEN $4 AND $5")
            .bind(scope.tenant_id).bind(scope.project_id).bind(note_id).bind(lower).bind(upper).bind(delta).execute(&mut **tx).await?;
    }
    sqlx::query("UPDATE note_database_embeds SET database_id=$5,view_id=$6,position=$7,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND note_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(note_id).bind(id).bind(input.database_id).bind(input.view_id).bind(position).execute(&mut **tx).await?;
    bump_note(tx, scope, note_id).await?;
    bump_database(tx, scope, input.database_id).await?;
    if current.database_id != input.database_id {
        bump_database(tx, scope, current.database_id).await?;
    }
    audit(
        tx,
        scope,
        actor_id,
        "note_database_embed",
        id,
        "note_database_embed.updated",
    )
    .await?;
    get(tx, scope, note_id, id).await
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn delete(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    note_id: Uuid,
    id: Uuid,
    expected_version: i32,
    actor_id: Option<Uuid>,
) -> Result<(), AppError> {
    let current = get(tx, scope, note_id, id).await?;
    check_version(current.version, expected_version)?;
    sqlx::query("DELETE FROM note_database_embeds WHERE tenant_id=$1 AND project_id=$2 AND note_id=$3 AND id=$4").bind(scope.tenant_id).bind(scope.project_id).bind(note_id).bind(id).execute(&mut **tx).await?;
    sqlx::query("UPDATE note_database_embeds SET position=position-1,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND note_id=$3 AND position > $4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(note_id).bind(current.position).execute(&mut **tx).await?;
    bump_note(tx, scope, note_id).await?;
    bump_database(tx, scope, current.database_id).await?;
    audit(
        tx,
        scope,
        actor_id,
        "note_database_embed",
        id,
        "note_database_embed.deleted",
    )
    .await
}

async fn list_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(note): Path<Uuid>,
) -> Result<Json<model::List<Embed>>, AppError> {
    let scope = require_scope(&actor, "notes:read")?;
    let mut tx = state.db.begin().await?;
    let items = list(&mut tx, &scope, note).await?;
    tx.commit().await?;
    Ok(Json(model::List { items }))
}
async fn create_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(note): Path<Uuid>,
    Json(input): Json<CreateEmbed>,
) -> Result<(StatusCode, Json<Embed>), AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let row = create(&mut tx, &scope, note, input, Some(actor.actor_id)).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}
async fn update_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((note, id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateEmbed>,
) -> Result<Json<Embed>, AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let row = update(&mut tx, &scope, note, id, input, Some(actor.actor_id)).await?;
    tx.commit().await?;
    Ok(Json(row))
}
async fn delete_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((note, id)): Path<(Uuid, Uuid)>,
    Query(input): Query<VersionQuery>,
) -> Result<StatusCode, AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    delete(
        &mut tx,
        &scope,
        note,
        id,
        input.expected_version,
        Some(actor.actor_id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
