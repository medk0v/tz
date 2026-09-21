//! Shared, project-scoped Markdown notes with nested pages and versioned writes.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError};

const MAX_BODY_CHARS: usize = 200_000;
const MAX_BODY_BYTES: usize = 800_000;

/// Routes for project notes. The selected project supplies the authorization scope.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/notes", get(list_notes).post(create_note))
        .route(
            "/api/v1/notes/{note_id}",
            get(get_note).patch(update_note).delete(delete_note),
        )
        .route("/api/v1/notes/{note_id}/move", post(move_note))
}

#[derive(Debug, FromRow, Serialize)]
struct NoteSummary {
    id: Uuid,
    parent_id: Option<Uuid>,
    sort_order: i32,
    title: String,
    icon: String,
    is_favorite: bool,
    version: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
struct Note {
    #[serde(flatten)]
    #[sqlx(flatten)]
    summary: NoteSummary,
    body: String,
}

#[derive(Serialize)]
struct NoteList {
    items: Vec<NoteSummary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateNote {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    parent_id: Option<Uuid>,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    is_favorite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateNote {
    title: String,
    body: String,
    #[serde(deserialize_with = "required_parent")]
    parent_id: Option<Uuid>,
    icon: String,
    is_favorite: bool,
    expected_version: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveNote {
    #[serde(deserialize_with = "required_parent")]
    parent_id: Option<Uuid>,
    #[serde(deserialize_with = "required_parent")]
    before_id: Option<Uuid>,
    expected_version: i32,
}

#[derive(Serialize)]
struct MovedNote {
    note: Note,
    items: Vec<NoteSummary>,
}

// PATCH is a complete editable snapshot: an omitted parent must not move a page to root.
fn required_parent<'de, D>(deserializer: D) -> Result<Option<Uuid>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Uuid>::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteNote {
    expected_version: i32,
}

async fn list_notes(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<NoteList>, AppError> {
    let project_id = require_scope(&actor, "notes:read")?;
    let items = sqlx::query_as::<_, NoteSummary>(
        "SELECT id, parent_id, sort_order, title, icon, is_favorite, version, created_at, updated_at
         FROM notes WHERE tenant_id = $1 AND project_id = $2
         ORDER BY parent_id NULLS FIRST, sort_order, id",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(NoteList { items }))
}

async fn get_note(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(note_id): Path<Uuid>,
) -> Result<Json<Note>, AppError> {
    let project_id = require_scope(&actor, "notes:read")?;
    let note = sqlx::query_as::<_, Note>(
        "SELECT id, parent_id, sort_order, title, body, icon, is_favorite, version, created_at, updated_at
         FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(note_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(note))
}

async fn create_note(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(mut input): Json<CreateNote>,
) -> Result<(StatusCode, Json<Note>), AppError> {
    let project_id = require_scope(&actor, "notes:write")?;
    validate_content(&mut input.title, &input.body, &input.icon)?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, &actor, project_id).await?;
    let note_id = Uuid::now_v7();
    validate_parent(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        input.parent_id,
    )
    .await?;
    let sort_order = next_order(&mut transaction, &actor, project_id, input.parent_id).await?;
    let note = sqlx::query_as::<_, Note>(
        "INSERT INTO notes (id, tenant_id, project_id, parent_id, title, body, icon, is_favorite, sort_order)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, parent_id, sort_order, title, body, icon, is_favorite, version, created_at, updated_at",
    )
    .bind(note_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(input.parent_id)
    .bind(input.title)
    .bind(input.body)
    .bind(input.icon)
    .bind(input.is_favorite)
    .bind(sort_order)
    .fetch_one(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        "note.created",
    )
    .await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(note)))
}

async fn update_note(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(note_id): Path<Uuid>,
    Json(mut input): Json<UpdateNote>,
) -> Result<Json<Note>, AppError> {
    let project_id = require_scope(&actor, "notes:write")?;
    validate_content(&mut input.title, &input.body, &input.icon)?;
    validate_version(input.expected_version)?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, &actor, project_id).await?;
    check_version(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        input.expected_version,
    )
    .await?;
    validate_parent(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        input.parent_id,
    )
    .await?;
    let (current_parent, current_order): (Option<Uuid>, i32) = sqlx::query_as(
        "SELECT parent_id, sort_order FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(note_id)
    .fetch_one(&mut *transaction)
    .await?;
    let sort_order = if current_parent == input.parent_id {
        current_order
    } else {
        next_order(&mut transaction, &actor, project_id, input.parent_id).await?
    };
    let note = sqlx::query_as::<_, Note>(
        "UPDATE notes SET title = $4, body = $5, parent_id = $6, icon = $7,
             is_favorite = $8, sort_order = $9, version = version + 1, updated_at = clock_timestamp()
         WHERE tenant_id = $1 AND project_id = $2 AND id = $3
         RETURNING id, parent_id, sort_order, title, body, icon, is_favorite, version, created_at, updated_at",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(note_id)
    .bind(input.title)
    .bind(input.body)
    .bind(input.parent_id)
    .bind(input.icon)
    .bind(input.is_favorite)
    .bind(sort_order)
    .fetch_one(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        "note.updated",
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(note))
}

async fn delete_note(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(note_id): Path<Uuid>,
    Query(input): Query<DeleteNote>,
) -> Result<StatusCode, AppError> {
    let project_id = require_scope(&actor, "notes:write")?;
    validate_version(input.expected_version)?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, &actor, project_id).await?;
    check_version(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        input.expected_version,
    )
    .await?;
    let has_children: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM notes
         WHERE tenant_id = $1 AND project_id = $2 AND parent_id = $3)",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(note_id)
    .fetch_one(&mut *transaction)
    .await?;
    if has_children {
        return Err(AppError::Conflict(
            "move or delete child notes before deleting this note".to_owned(),
        ));
    }
    sqlx::query("DELETE FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(note_id)
        .execute(&mut *transaction)
        .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        "note.deleted",
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn move_note(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(note_id): Path<Uuid>,
    Json(input): Json<MoveNote>,
) -> Result<Json<MovedNote>, AppError> {
    let project_id = require_scope(&actor, "notes:write")?;
    validate_version(input.expected_version)?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, &actor, project_id).await?;
    check_version(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        input.expected_version,
    )
    .await?;
    validate_parent(
        &mut transaction,
        &actor,
        project_id,
        note_id,
        input.parent_id,
    )
    .await?;
    if input.before_id == Some(note_id) {
        return Err(AppError::BadRequest(
            "a note cannot be placed before itself".to_owned(),
        ));
    }
    let previous_parent: Option<Uuid> = sqlx::query_scalar(
        "SELECT parent_id FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(note_id)
    .fetch_one(&mut *transaction)
    .await?;
    let mut siblings = sibling_ids(
        &mut transaction,
        &actor,
        project_id,
        input.parent_id,
        note_id,
    )
    .await?;
    let position = if let Some(before_id) = input.before_id {
        siblings
            .iter()
            .position(|id| *id == before_id)
            .ok_or(AppError::NotFound)?
    } else {
        siblings.len()
    };
    siblings.insert(position, note_id);
    sqlx::query(
        "UPDATE notes SET parent_id = $4, version = version + 1, updated_at = clock_timestamp()
         WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(note_id)
    .bind(input.parent_id)
    .execute(&mut *transaction)
    .await?;
    assign_order(&mut transaction, &actor, project_id, &siblings).await?;
    if previous_parent != input.parent_id {
        let previous = sibling_ids(
            &mut transaction,
            &actor,
            project_id,
            previous_parent,
            note_id,
        )
        .await?;
        assign_order(&mut transaction, &actor, project_id, &previous).await?;
    }
    insert_audit(&mut transaction, &actor, project_id, note_id, "note.moved").await?;
    let note = sqlx::query_as::<_, Note>(
        "SELECT id, parent_id, sort_order, title, body, icon, is_favorite, version, created_at, updated_at
         FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    ).bind(actor.tenant_id).bind(project_id).bind(note_id).fetch_one(&mut *transaction).await?;
    let items = sqlx::query_as::<_, NoteSummary>(
        "SELECT id, parent_id, sort_order, title, icon, is_favorite, version, created_at, updated_at
         FROM notes WHERE tenant_id = $1 AND project_id = $2 ORDER BY parent_id NULLS FIRST, sort_order, id",
    ).bind(actor.tenant_id).bind(project_id).fetch_all(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(Json(MovedNote { note, items }))
}

async fn sibling_ids(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    parent_id: Option<Uuid>,
    excluded_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    sqlx::query_scalar(
        "SELECT id FROM notes WHERE tenant_id = $1 AND project_id = $2
         AND parent_id IS NOT DISTINCT FROM $3 AND id <> $4 ORDER BY sort_order, id",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(parent_id)
    .bind(excluded_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(AppError::from)
}

// Reordering changes placement only. Neighbouring content snapshots remain valid.
async fn assign_order(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    ids: &[Uuid],
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE notes AS note SET sort_order = (ordered.position - 1)::integer
         FROM unnest($3::uuid[]) WITH ORDINALITY AS ordered(id, position)
         WHERE note.tenant_id = $1 AND note.project_id = $2 AND note.id = ordered.id
           AND note.sort_order <> ordered.position - 1",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(ids)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn next_order(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    parent_id: Option<Uuid>,
) -> Result<i32, AppError> {
    sqlx::query_scalar(
        "SELECT COALESCE(max(sort_order) + 1, 0) FROM notes
         WHERE tenant_id = $1 AND project_id = $2 AND parent_id IS NOT DISTINCT FROM $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(parent_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(AppError::from)
}

pub(crate) fn require_scope(actor: &ActorContext, permission: &str) -> Result<Uuid, AppError> {
    actor.require(permission)?;
    let project_id = actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))?;
    actor.require_project(project_id)?;
    Ok(project_id)
}

pub(crate) fn validate_content(title: &mut String, body: &str, icon: &str) -> Result<(), AppError> {
    *title = title.trim().to_owned();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(AppError::BadRequest(
            "title must contain between 1 and 200 characters".to_owned(),
        ));
    }
    if body.len() > MAX_BODY_BYTES || body.chars().count() > MAX_BODY_CHARS {
        return Err(AppError::BadRequest(
            "body must not exceed 200000 characters or 800000 UTF-8 bytes".to_owned(),
        ));
    }
    if icon.chars().count() > 32 {
        return Err(AppError::BadRequest(
            "icon must not exceed 32 characters".to_owned(),
        ));
    }
    if title.contains('\0') || body.contains('\0') || icon.contains('\0') {
        return Err(AppError::BadRequest(
            "note content must not contain null characters".to_owned(),
        ));
    }
    Ok(())
}

fn validate_version(version: i32) -> Result<(), AppError> {
    if version < 1 {
        return Err(AppError::BadRequest(
            "expected_version must be positive".to_owned(),
        ));
    }
    Ok(())
}

// All note mutations lock this common row before reading the tree. Concurrent moves
// cannot both pass the cycle check, and parent deletion cannot race child creation.
async fn lock_project(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM projects WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(())
}

async fn check_version(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    note_id: Uuid,
    expected_version: i32,
) -> Result<(), AppError> {
    let version = sqlx::query_scalar::<_, i32>(
        "SELECT version FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(note_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if version != expected_version {
        return Err(AppError::Conflict(
            "note changed; reload it before saving or deleting".to_owned(),
        ));
    }
    Ok(())
}

async fn validate_parent(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    note_id: Uuid,
    parent_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    if parent_id == note_id {
        return Err(AppError::BadRequest(
            "a note cannot be its own parent".to_owned(),
        ));
    }
    let ancestors = sqlx::query_scalar::<_, Uuid>(
        "WITH RECURSIVE ancestors AS (
             SELECT id, parent_id FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3
             UNION
             SELECT note.id, note.parent_id FROM notes AS note
             JOIN ancestors ON ancestors.parent_id = note.id
             WHERE note.tenant_id = $1 AND note.project_id = $2
         ) SELECT id FROM ancestors",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(parent_id)
    .fetch_all(&mut **transaction)
    .await?;
    if ancestors.is_empty() {
        return Err(AppError::NotFound);
    }
    if ancestors.contains(&note_id) {
        return Err(AppError::BadRequest(
            "a note cannot be moved inside its descendants".to_owned(),
        ));
    }
    Ok(())
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    note_id: Uuid,
    action: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO audit_log
             (id, tenant_id, project_id, actor_id, action, resource_kind, resource_id, metadata)
         VALUES ($1, $2, $3, $4, $5, 'note', $6, '{}'::jsonb)",
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(action)
    .bind(note_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_BODY_CHARS, MoveNote, UpdateNote, validate_content};

    #[test]
    fn validates_unicode_lengths_without_modifying_markdown() {
        let mut title = "  Заметка  ".to_owned();
        let body = "🙂".repeat(MAX_BODY_CHARS);
        validate_content(&mut title, &body, "📘").unwrap();
        assert_eq!(title, "Заметка");
        assert!(validate_content(&mut title, &(body + "a"), "").is_err());
        assert!(validate_content(&mut "  ".to_owned(), "", "").is_err());
        assert!(validate_content(&mut "a".repeat(201), "", "").is_err());
        assert!(validate_content(&mut title, "", &"a".repeat(33)).is_err());
        assert!(validate_content(&mut title, "before\0after", "").is_err());
    }

    #[test]
    fn complete_update_requires_an_explicit_nullable_parent() {
        let mut input = serde_json::json!({
            "title": "Page", "body": "", "icon": "", "is_favorite": false,
            "expected_version": 1
        });
        assert!(serde_json::from_value::<UpdateNote>(input.clone()).is_err());
        input["parent_id"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<UpdateNote>(input).is_ok());
    }

    #[test]
    fn moving_requires_explicit_parent_and_anchor_without_editable_content() {
        let input = serde_json::json!({"parent_id":null,"before_id":null,"expected_version":1});
        assert!(serde_json::from_value::<MoveNote>(input.clone()).is_ok());
        for key in ["parent_id", "before_id", "expected_version"] {
            let mut missing = input.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<MoveNote>(missing).is_err());
        }
        let mut injected = input;
        injected["body"] = serde_json::json!("overwrite");
        assert!(serde_json::from_value::<MoveNote>(injected).is_err());
    }
}
