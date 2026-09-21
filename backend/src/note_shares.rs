//! Revocable project/subtree publication, independent of authenticated workspace sessions.

use std::collections::{HashMap, HashSet};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError, notes};

static PASSWORD_WORK: Semaphore = Semaphore::const_new(4);

/// Management and public routes share transport privacy headers, but not authentication.
pub fn router() -> Router<AppState> {
    let public = Router::new()
        .route("/resolve", post(resolve).layer(DefaultBodyLimit::max(4096)))
        .route("/unlock", post(unlock).layer(DefaultBodyLimit::max(4096)))
        .route("/page", patch(edit_page))
        .route("/databases", post(crate::note_databases::public::dispatch))
        .fallback(public_not_found)
        .layer(middleware::from_fn(private_response));
    Router::new()
        .route(
            "/api/v1/notes/shares",
            get(list_shares)
                .post(create_share)
                .layer(DefaultBodyLimit::max(262_144)),
        )
        .route(
            "/api/v1/notes/shares/{share_id}",
            patch(update_share)
                .delete(revoke_share)
                .layer(DefaultBodyLimit::max(262_144)),
        )
        .layer(middleware::from_fn(private_response))
        .nest("/api/v1/public/notes", public)
}

async fn private_response(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}

async fn public_not_found() -> AppError {
    AppError::NotFound
}

#[derive(Debug, thiserror::Error)]
enum ShareError {
    #[error(transparent)]
    App(#[from] AppError),
    #[error("too many password operations; try again shortly")]
    RateLimited,
}

impl From<sqlx::Error> for ShareError {
    fn from(value: sqlx::Error) -> Self {
        Self::App(AppError::Database(value))
    }
}

impl IntoResponse for ShareError {
    fn into_response(self) -> Response {
        match self {
            Self::App(error) => error.into_response(),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, "60")],
                Json(serde_json::json!({"error": {
                    "code": "too_many_requests",
                    "message": "too many password operations; try again shortly",
                    "request_id": Uuid::now_v7()
                }})),
            )
                .into_response(),
        }
    }
}

#[derive(Serialize, FromRow)]
#[allow(clippy::struct_excessive_bools)] // Independent publication settings and password status in the API response.
struct ShareSummary {
    id: Uuid,
    note_id: Option<Uuid>,
    custom_slug: Option<String>,
    has_password: bool,
    can_edit: bool,
    theme_palette: Option<String>,
    theme_mode: Option<String>,
    allow_theme_change: bool,
    include_descendants: bool,
    included_note_ids: Option<Vec<Uuid>>,
    version: i32,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ShareList {
    items: Vec<ShareSummary>,
}

#[derive(Serialize)]
struct CreatedShare {
    #[serde(flatten)]
    share: ShareSummary,
    token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateShare {
    #[serde(default)]
    note_id: Option<Uuid>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    custom_slug: Option<String>,
    #[serde(default)]
    can_edit: bool,
    #[serde(default)]
    theme_palette: Option<String>,
    #[serde(default)]
    theme_mode: Option<String>,
    #[serde(default = "enabled")]
    allow_theme_change: bool,
    #[serde(default = "enabled")]
    include_descendants: bool,
    #[serde(default)]
    included_note_ids: Option<Vec<Uuid>>,
}

fn enabled() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateShare {
    #[serde(deserialize_with = "required_nullable")]
    theme_palette: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    theme_mode: Option<String>,
    allow_theme_change: bool,
    include_descendants: bool,
    #[serde(deserialize_with = "required_nullable")]
    included_note_ids: Option<Vec<Uuid>>,
    can_edit: bool,
    expected_version: i32,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(FromRow)]
struct Share {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    note_id: Option<Uuid>,
    password_hash: Option<String>,
    can_edit: bool,
    theme_palette: Option<String>,
    theme_mode: Option<String>,
    allow_theme_change: bool,
    include_descendants: bool,
    included_note_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveRequest {
    token: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    note_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnlockRequest {
    token: String,
    password: String,
}

#[derive(Serialize)]
struct UnlockedShare {
    access_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    token: String,
    #[serde(default)]
    access_token: Option<String>,
    note_id: Uuid,
    title: String,
    body: String,
    icon: String,
    expected_version: i32,
}

#[derive(Serialize, FromRow)]
struct PublicNoteSummary {
    id: Uuid,
    parent_id: Option<Uuid>,
    sort_order: i32,
    title: String,
    icon: String,
    version: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, FromRow)]
struct PublicNote {
    #[serde(flatten)]
    #[sqlx(flatten)]
    summary: PublicNoteSummary,
    body: String,
}

#[derive(Serialize)]
struct ResolvedShare {
    can_edit: bool,
    theme_palette: Option<String>,
    theme_mode: Option<String>,
    allow_theme_change: bool,
    include_descendants: bool,
    root_note_id: Option<Uuid>,
    items: Vec<PublicNoteSummary>,
    note: Option<PublicNote>,
}

async fn list_shares(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<ShareList>, AppError> {
    let project_id = notes::require_scope(&actor, "notes:write")?;
    let items = sqlx::query_as::<_, ShareSummary>(
        "SELECT id, note_id, custom_slug, password_hash IS NOT NULL AS has_password, can_edit,
             theme_palette, theme_mode, allow_theme_change, include_descendants, included_note_ids, version, created_at
         FROM note_shares WHERE tenant_id = $1 AND project_id = $2 AND revoked_at IS NULL
         ORDER BY created_at DESC, id",
    ).bind(actor.tenant_id).bind(project_id).fetch_all(&state.db).await?;
    Ok(Json(ShareList { items }))
}

async fn create_share(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(input): Json<CreateShare>,
) -> Result<(StatusCode, Json<CreatedShare>), ShareError> {
    let project_id = notes::require_scope(&actor, "notes:write")?;
    validate_settings(
        input.theme_palette.as_deref(),
        input.theme_mode.as_deref(),
        input.included_note_ids.as_deref(),
    )?;
    let slug = normalize_slug(input.custom_slug)?;
    if let Some(password) = &input.password {
        validate_password(password)?;
    }
    let password_hash = match input.password {
        Some(password) => Some(hash_password(password).await?),
        None => None,
    };
    let token = slug.clone().unwrap_or_else(random_token);
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id, true).await?;
    if let Some(note_id) = input.note_id {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(note_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    }
    validate_selection(
        &mut transaction,
        actor.tenant_id,
        project_id,
        input.note_id,
        input.included_note_ids.as_deref(),
    )
    .await?;
    let share = sqlx::query_as::<_, ShareSummary>(
        "INSERT INTO note_shares (id, tenant_id, project_id, note_id, token_hash, custom_slug, password_hash, can_edit,
             theme_palette, theme_mode, allow_theme_change, include_descendants, included_note_ids)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         RETURNING id, note_id, custom_slug, password_hash IS NOT NULL AS has_password, can_edit,
             theme_palette, theme_mode, allow_theme_change, include_descendants, included_note_ids, version, created_at",
    ).bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project_id).bind(input.note_id)
        .bind(token_hash(&token)).bind(slug).bind(password_hash).bind(input.can_edit)
        .bind(input.theme_palette).bind(input.theme_mode).bind(input.allow_theme_change)
        .bind(input.include_descendants).bind(input.included_note_ids)
        .fetch_one(&mut *transaction).await.map_err(share_write_error)?;
    insert_audit(
        &mut transaction,
        actor.tenant_id,
        project_id,
        Some(actor.actor_id),
        share.id,
        "note_share.created",
        None,
    )
    .await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(CreatedShare { share, token })))
}

async fn update_share(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(share_id): Path<Uuid>,
    Json(input): Json<UpdateShare>,
) -> Result<Json<ShareSummary>, AppError> {
    let project_id = notes::require_scope(&actor, "notes:write")?;
    validate_settings(
        input.theme_palette.as_deref(),
        input.theme_mode.as_deref(),
        input.included_note_ids.as_deref(),
    )?;
    if input.expected_version < 1 {
        return Err(AppError::BadRequest(
            "expected_version must be positive".to_owned(),
        ));
    }
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id, true).await?;
    let (note_id, version): (Option<Uuid>, i32) = sqlx::query_as(
        "SELECT note_id, version FROM note_shares WHERE tenant_id=$1 AND project_id=$2 AND id=$3 AND revoked_at IS NULL",
    ).bind(actor.tenant_id).bind(project_id).bind(share_id).fetch_optional(&mut *transaction).await?.ok_or(AppError::NotFound)?;
    if version != input.expected_version {
        return Err(AppError::Conflict(
            "share settings changed; reload before saving".to_owned(),
        ));
    }
    validate_selection(
        &mut transaction,
        actor.tenant_id,
        project_id,
        note_id,
        input.included_note_ids.as_deref(),
    )
    .await?;
    let share = sqlx::query_as::<_, ShareSummary>(
        "UPDATE note_shares SET theme_palette=$4, theme_mode=$5, allow_theme_change=$6,
             include_descendants=$7, included_note_ids=$8, can_edit=$9, version=version+1
         WHERE tenant_id=$1 AND project_id=$2 AND id=$3
         RETURNING id, note_id, custom_slug, password_hash IS NOT NULL AS has_password, can_edit,
             theme_palette, theme_mode, allow_theme_change, include_descendants, included_note_ids, version, created_at",
    ).bind(actor.tenant_id).bind(project_id).bind(share_id).bind(input.theme_palette).bind(input.theme_mode)
        .bind(input.allow_theme_change).bind(input.include_descendants).bind(input.included_note_ids).bind(input.can_edit)
        .fetch_one(&mut *transaction).await?;
    insert_audit(
        &mut transaction,
        actor.tenant_id,
        project_id,
        Some(actor.actor_id),
        share_id,
        "note_share.updated",
        None,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(share))
}

async fn revoke_share(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(share_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let project_id = notes::require_scope(&actor, "notes:write")?;
    let mut transaction = state.db.begin().await?;
    lock_project(&mut transaction, actor.tenant_id, project_id, true).await?;
    let revoked_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        "SELECT revoked_at FROM note_shares WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(share_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if revoked_at.is_none() {
        sqlx::query("UPDATE note_shares SET revoked_at = clock_timestamp() WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
            .bind(actor.tenant_id).bind(project_id).bind(share_id).execute(&mut *transaction).await?;
        sqlx::query("DELETE FROM note_share_access_tokens WHERE tenant_id = $1 AND project_id = $2 AND share_id = $3")
            .bind(actor.tenant_id).bind(project_id).bind(share_id).execute(&mut *transaction).await?;
        insert_audit(
            &mut transaction,
            actor.tenant_id,
            project_id,
            Some(actor.actor_id),
            share_id,
            "note_share.revoked",
            None,
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resolve(
    State(state): State<AppState>,
    Json(input): Json<ResolveRequest>,
) -> Result<Json<ResolvedShare>, AppError> {
    let mut transaction = state.db.begin().await?;
    let share = load_locked_share(&mut transaction, &input.token, false).await?;
    require_deployment_project(&state, &share)?;
    authorize_access(&mut transaction, &share, input.access_token.as_deref()).await?;
    let items = public_summaries(&mut transaction, &share).await?;
    let selected_id = input
        .note_id
        .or(share.note_id)
        .or_else(|| items.first().map(|item| item.id));
    let note = if let Some(note_id) = selected_id {
        if !items.iter().any(|item| item.id == note_id) {
            return Err(AppError::NotFound);
        }
        Some(load_public_note(&mut transaction, &share, note_id).await?)
    } else {
        None
    };
    transaction.commit().await?;
    Ok(Json(ResolvedShare {
        can_edit: share.can_edit,
        theme_palette: share.theme_palette,
        theme_mode: share.theme_mode,
        allow_theme_change: share.allow_theme_change,
        include_descendants: share.include_descendants,
        root_note_id: share.note_id,
        items,
        note,
    }))
}

async fn unlock(
    State(state): State<AppState>,
    Json(input): Json<UnlockRequest>,
) -> Result<Json<UnlockedShare>, ShareError> {
    if input.password.len() > 512 || input.password.chars().count() > 128 {
        return Err(AppError::Unauthorized.into());
    }
    let mut lookup = state.db.begin().await?;
    let share = find_share(&mut lookup, &input.token).await?;
    require_deployment_project(&state, &share)?;
    lookup.commit().await?;
    // The counter is an independent short transaction, before expensive Argon2 work.
    // It remains effective across workers and counts successful unlocks as well.
    let attempted = sqlx::query(
        "UPDATE note_shares SET
             unlock_attempts = CASE WHEN unlock_window_start <= now() - interval '1 minute' THEN 1 ELSE unlock_attempts + 1 END,
             unlock_window_start = CASE WHEN unlock_window_start <= now() - interval '1 minute' THEN now() ELSE unlock_window_start END
         WHERE tenant_id = $1 AND project_id = $2 AND id = $3 AND revoked_at IS NULL
           AND (unlock_attempts < 10 OR unlock_window_start <= now() - interval '1 minute')",
    ).bind(share.tenant_id).bind(share.project_id).bind(share.id).execute(&state.db).await?;
    if attempted.rows_affected() == 0 {
        let mut lookup = state.db.begin().await?;
        find_share(&mut lookup, &input.token).await?;
        lookup.commit().await?;
        return Err(ShareError::RateLimited);
    }
    if let Some(hash) = share.password_hash
        && !verify_password(input.password, hash).await?
    {
        return Err(AppError::Unauthorized.into());
    }
    let mut transaction = state.db.begin().await?;
    let live_share = load_locked_share(&mut transaction, &input.token, true).await?;
    if live_share.id != share.id {
        return Err(AppError::NotFound.into());
    }
    let access_token = random_token();
    sqlx::query("DELETE FROM note_share_access_tokens WHERE tenant_id = $1 AND project_id = $2 AND share_id = $3 AND expires_at <= now()")
        .bind(live_share.tenant_id).bind(live_share.project_id).bind(live_share.id).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO note_share_access_tokens (token_hash, tenant_id, project_id, share_id) VALUES ($1, $2, $3, $4)")
        .bind(token_hash(&access_token)).bind(live_share.tenant_id).bind(live_share.project_id).bind(live_share.id)
        .execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(Json(UnlockedShare { access_token }))
}

async fn edit_page(
    State(state): State<AppState>,
    Json(mut input): Json<EditRequest>,
) -> Result<Json<PublicNote>, AppError> {
    notes::validate_content(&mut input.title, &input.body, &input.icon)?;
    if input.expected_version < 1 {
        return Err(AppError::BadRequest(
            "expected_version must be positive".to_owned(),
        ));
    }
    let mut transaction = state.db.begin().await?;
    let share = load_locked_share(&mut transaction, &input.token, true).await?;
    require_deployment_project(&state, &share)?;
    authorize_access(&mut transaction, &share, input.access_token.as_deref()).await?;
    if !share.can_edit {
        return Err(AppError::Forbidden);
    }
    let items = public_summaries(&mut transaction, &share).await?;
    if !items.iter().any(|item| item.id == input.note_id) {
        return Err(AppError::NotFound);
    }
    let changed = sqlx::query(
        "UPDATE notes SET title = $4, body = $5, icon = $6, version = version + 1, updated_at = clock_timestamp()
         WHERE tenant_id = $1 AND project_id = $2 AND id = $3 AND version = $7",
    ).bind(share.tenant_id).bind(share.project_id).bind(input.note_id).bind(input.title).bind(input.body).bind(input.icon)
        .bind(input.expected_version).execute(&mut *transaction).await?;
    if changed.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "note changed; reload it before saving".to_owned(),
        ));
    }
    let note = load_public_note(&mut transaction, &share, input.note_id).await?;
    insert_audit(
        &mut transaction,
        share.tenant_id,
        share.project_id,
        None,
        input.note_id,
        "note.public_updated",
        Some(share.id),
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(note))
}

async fn find_share(
    transaction: &mut Transaction<'_, Postgres>,
    token: &str,
) -> Result<Share, AppError> {
    validate_token(token)?;
    sqlx::query_as::<_, Share>(
        "SELECT share.id, share.tenant_id, share.project_id, share.note_id, share.password_hash, share.can_edit,
             share.theme_palette, share.theme_mode, share.allow_theme_change, share.include_descendants, share.included_note_ids
         FROM note_shares AS share JOIN projects AS project ON project.tenant_id = share.tenant_id AND project.id = share.project_id
         WHERE share.token_hash = $1 AND share.revoked_at IS NULL AND project.status = 'active'
           AND (share.note_id IS NULL OR EXISTS (SELECT 1 FROM notes WHERE tenant_id = share.tenant_id AND project_id = share.project_id AND id = share.note_id))",
    ).bind(token_hash(token)).fetch_optional(&mut **transaction).await?.ok_or(AppError::NotFound)
}

fn require_deployment_project(state: &AppState, share: &Share) -> Result<(), AppError> {
    state
        .config
        .product
        .resolve_project(Some(share.project_id))
        .map(|_| ())
        .map_err(|_| AppError::NotFound)
}

async fn load_locked_share(
    transaction: &mut Transaction<'_, Postgres>,
    token: &str,
    write: bool,
) -> Result<Share, AppError> {
    let initial = find_share(transaction, token).await?;
    lock_project(transaction, initial.tenant_id, initial.project_id, write).await?;
    // Re-read after waiting on the project lock, since a revoke or root deletion may have won.
    let share = find_share(transaction, token).await?;
    if share.id != initial.id {
        return Err(AppError::NotFound);
    }
    Ok(share)
}

async fn lock_project(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    write: bool,
) -> Result<(), AppError> {
    let query = if write {
        "SELECT id FROM projects WHERE tenant_id = $1 AND id = $2 AND status = 'active' FOR UPDATE"
    } else {
        "SELECT id FROM projects WHERE tenant_id = $1 AND id = $2 AND status = 'active' FOR SHARE"
    };
    sqlx::query_scalar::<_, Uuid>(query)
        .bind(tenant_id)
        .bind(project_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(())
}

async fn authorize_access(
    transaction: &mut Transaction<'_, Postgres>,
    share: &Share,
    access_token: Option<&str>,
) -> Result<(), AppError> {
    if share.password_hash.is_none() {
        return Ok(());
    }
    let token = access_token.ok_or(AppError::Unauthorized)?;
    if !is_random_token(token) {
        return Err(AppError::Unauthorized);
    }
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM note_share_access_tokens
         WHERE tenant_id = $1 AND project_id = $2 AND share_id = $3 AND token_hash = $4 AND expires_at > now())",
    ).bind(share.tenant_id).bind(share.project_id).bind(share.id).bind(token_hash(token)).fetch_one(&mut **transaction).await?;
    if !valid {
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

/// Authenticate a publication and hold its project lock for the caller's entire database operation.
/// The caller must use only embeds belonging to the returned, freshly resolved note IDs.
pub(crate) async fn authorize_databases(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    token: &str,
    access_token: Option<&str>,
    write: bool,
) -> Result<(crate::note_databases::model::Scope, Vec<Uuid>), AppError> {
    let share = load_locked_share(transaction, token, write).await?;
    require_deployment_project(state, &share)?;
    authorize_access(transaction, &share, access_token).await?;
    if write && !share.can_edit {
        return Err(AppError::Forbidden);
    }
    let notes = public_summaries(transaction, &share).await?;
    Ok((
        crate::note_databases::model::Scope {
            tenant_id: share.tenant_id,
            project_id: share.project_id,
        },
        notes.into_iter().map(|note| note.id).collect(),
    ))
}

async fn public_summaries(
    transaction: &mut Transaction<'_, Postgres>,
    share: &Share,
) -> Result<Vec<PublicNoteSummary>, AppError> {
    sqlx::query_as::<_, PublicNoteSummary>(
        "WITH RECURSIVE visible AS (
             SELECT id, parent_id, sort_order, title, icon, version, created_at, updated_at FROM notes
             WHERE tenant_id = $1 AND project_id = $2
               AND (($3::uuid IS NOT NULL AND id = $3)
                 OR ($3 IS NULL AND parent_id IS NULL AND ($5::uuid[] IS NULL OR id = ANY($5))))
             UNION
             SELECT note.id, note.parent_id, note.sort_order, note.title, note.icon, note.version, note.created_at, note.updated_at
             FROM notes AS note JOIN visible ON note.parent_id = visible.id
             WHERE note.tenant_id = $1 AND note.project_id = $2 AND $4::boolean
               AND ($5::uuid[] IS NULL OR note.id = ANY($5))
         ) SELECT id, CASE WHEN id = $3 THEN NULL ELSE parent_id END AS parent_id,
             sort_order, title, icon, version, created_at, updated_at FROM visible
             ORDER BY parent_id NULLS FIRST, sort_order, id",
    ).bind(share.tenant_id).bind(share.project_id).bind(share.note_id)
        .bind(share.include_descendants).bind(share.included_note_ids.as_deref())
        .fetch_all(&mut **transaction).await.map_err(AppError::from)
}

fn validate_settings(
    palette: Option<&str>,
    mode: Option<&str>,
    selected: Option<&[Uuid]>,
) -> Result<(), AppError> {
    if palette.is_some_and(|value| {
        !matches!(
            value,
            "ocean" | "ink" | "graphite" | "forest" | "plum" | "copper" | "paper"
        )
    }) {
        return Err(AppError::BadRequest("unsupported theme_palette".to_owned()));
    }
    if mode.is_some_and(|value| !matches!(value, "light" | "dark")) {
        return Err(AppError::BadRequest(
            "theme_mode must be light, dark or null".to_owned(),
        ));
    }
    if selected.is_some_and(|ids| {
        ids.len() > 5000 || ids.iter().collect::<HashSet<_>>().len() != ids.len()
    }) {
        return Err(AppError::BadRequest(
            "included_note_ids must contain at most 5000 unique note IDs".to_owned(),
        ));
    }
    Ok(())
}

// Validate only selected pages and their ancestor paths while holding the project lock.
// The root is implicit; any other unchecked ancestor would make a selection unreachable.
async fn validate_selection(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    root_id: Option<Uuid>,
    selected: Option<&[Uuid]>,
) -> Result<(), AppError> {
    let Some(selected) = selected else {
        return Ok(());
    };
    let parents: HashMap<Uuid, Option<Uuid>> = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
        "WITH RECURSIVE ancestors AS (
             SELECT id, parent_id FROM notes WHERE tenant_id=$1 AND project_id=$2 AND id=ANY($4::uuid[])
             UNION
             SELECT parent.id, parent.parent_id FROM notes AS parent JOIN ancestors ON parent.id=ancestors.parent_id
             WHERE parent.tenant_id=$1 AND parent.project_id=$2 AND ancestors.id IS DISTINCT FROM $3
         ) SELECT id, parent_id FROM ancestors",
    ).bind(tenant_id).bind(project_id).bind(root_id).bind(selected)
        .fetch_all(&mut **transaction).await?.into_iter().collect();
    let selected_set = selected.iter().copied().collect::<HashSet<_>>();
    for id in selected {
        if !parents.contains_key(id) {
            return Err(AppError::NotFound);
        }
        let mut current = Some(*id);
        let mut ancestors = HashSet::new();
        while current != root_id {
            let Some(id) = current else {
                return Err(AppError::NotFound);
            };
            if !ancestors.insert(id) {
                return Err(AppError::BadRequest(
                    "selected notes contain an invalid hierarchy".to_owned(),
                ));
            }
            current = *parents.get(&id).ok_or(AppError::NotFound)?;
        }
        if !ancestors.is_subset(&selected_set) {
            return Err(AppError::BadRequest(
                "include every ancestor between a selected note and the publication root"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

async fn load_public_note(
    transaction: &mut Transaction<'_, Postgres>,
    share: &Share,
    note_id: Uuid,
) -> Result<PublicNote, AppError> {
    sqlx::query_as::<_, PublicNote>(
        "SELECT id, CASE WHEN id = $4 THEN NULL ELSE parent_id END AS parent_id, sort_order, title, body, icon, version, created_at, updated_at
         FROM notes WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    ).bind(share.tenant_id).bind(share.project_id).bind(note_id).bind(share.note_id).fetch_optional(&mut **transaction).await?.ok_or(AppError::NotFound)
}

fn normalize_slug(slug: Option<String>) -> Result<Option<String>, AppError> {
    slug.map(|value| {
        let value = value.trim().to_ascii_lowercase();
        if !is_slug(&value) { return Err(AppError::BadRequest("custom_slug must contain 3 to 64 lowercase letters, digits or hyphens and begin and end with a letter or digit".to_owned())); }
        Ok(value)
    }).transpose()
}

fn is_slug(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn is_random_token(value: &str) -> bool {
    value.len() == 44
        && value.starts_with('_')
        && URL_SAFE_NO_PAD
            .decode(&value[1..])
            .is_ok_and(|bytes| bytes.len() == 32)
}

fn validate_token(value: &str) -> Result<(), AppError> {
    if is_slug(value) || is_random_token(value) {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

fn random_token() -> String {
    let mut random = [0u8; 32];
    rand::rng().fill_bytes(&mut random);
    format!("_{}", URL_SAFE_NO_PAD.encode(random))
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() > 512
        || !(10..=128).contains(&password.chars().count())
        || password.contains('\0')
    {
        return Err(AppError::BadRequest(
            "password must contain between 10 and 128 characters without null characters"
                .to_owned(),
        ));
    }
    Ok(())
}

async fn hash_password(password: String) -> Result<String, ShareError> {
    let permit = PASSWORD_WORK
        .try_acquire()
        .map_err(|_| ShareError::RateLimited)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut salt = [0u8; 16];
        rand::rng().fill_bytes(&mut salt);
        let salt = SaltString::encode_b64(&salt)
            .map_err(|error| AppError::internal(anyhow::anyhow!(error)))?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| AppError::internal(anyhow::anyhow!(error)))
    })
    .await
    .map_err(AppError::internal)?
    .map_err(ShareError::from)
}

async fn verify_password(password: String, stored: String) -> Result<bool, ShareError> {
    let permit = PASSWORD_WORK
        .try_acquire()
        .map_err(|_| ShareError::RateLimited)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let hash = PasswordHash::new(&stored)
            .map_err(|error| AppError::internal(anyhow::anyhow!(error)))?;
        match Argon2::default().verify_password(password.as_bytes(), &hash) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(error) => Err(AppError::internal(anyhow::anyhow!(error))),
        }
    })
    .await
    .map_err(AppError::internal)?
    .map_err(ShareError::from)
}

fn share_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("this public link address is already in use".to_owned())
    } else {
        AppError::Database(error)
    }
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    actor_id: Option<Uuid>,
    resource_id: Uuid,
    action: &str,
    share_id: Option<Uuid>,
) -> Result<(), AppError> {
    let metadata = share_id.map_or_else(
        || serde_json::json!({}),
        |id| serde_json::json!({"share_id": id}),
    );
    sqlx::query("INSERT INTO audit_log (id, tenant_id, project_id, actor_id, action, resource_kind, resource_id, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(Uuid::now_v7()).bind(tenant_id).bind(project_id).bind(actor_id).bind(action)
        .bind(if share_id.is_some() { "note" } else { "note_share" }).bind(resource_id).bind(metadata)
        .execute(&mut **transaction).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_preserve_legacy_defaults_and_require_complete_updates() {
        let create: CreateShare = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(create.allow_theme_change && create.include_descendants);
        assert!(
            create.theme_palette.is_none()
                && create.theme_mode.is_none()
                && create.included_note_ids.is_none()
        );
        let full = serde_json::json!({"theme_palette":null,"theme_mode":null,"allow_theme_change":true,
            "include_descendants":true,"included_note_ids":null,"can_edit":false,"expected_version":1});
        assert!(serde_json::from_value::<UpdateShare>(full.clone()).is_ok());
        for key in [
            "theme_palette",
            "theme_mode",
            "allow_theme_change",
            "include_descendants",
            "included_note_ids",
            "can_edit",
            "expected_version",
        ] {
            let mut input = full.clone();
            input.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<UpdateShare>(input).is_err(),
                "{key}"
            );
        }
        for palette in [
            "ocean", "ink", "graphite", "forest", "plum", "copper", "paper",
        ] {
            assert!(validate_settings(Some(palette), Some("dark"), None).is_ok());
        }
        assert!(validate_settings(Some("unknown"), None, None).is_err());
        assert!(validate_settings(None, Some("auto"), None).is_err());
        let id = Uuid::now_v7();
        assert!(validate_settings(None, None, Some(&[id, id])).is_err());
    }

    #[test]
    fn secrets_and_public_slugs_have_disjoint_bounded_formats() {
        let first = random_token();
        let second = random_token();
        assert_ne!(first, second);
        assert!(is_random_token(&first));
        assert!(!is_slug(&first));
        assert_eq!(token_hash(&first).len(), 32);
        assert_eq!(
            normalize_slug(Some("  My-Notes-12  ".to_owned()))
                .unwrap()
                .as_deref(),
            Some("my-notes-12")
        );
        for slug in ["ab", "_secret", "has space", "-start", "end-", "сообщение"] {
            assert!(normalize_slug(Some(slug.to_owned())).is_err());
        }
        assert!(normalize_slug(Some("a".repeat(65))).is_err());
        assert!(validate_token(&"a".repeat(10_000)).is_err());
    }

    #[tokio::test]
    async fn passwords_are_salted_and_verified_without_accepting_the_hash_as_input() {
        let password = "long secret phrase".to_owned();
        let first = hash_password(password.clone()).await.unwrap();
        let second = hash_password(password.clone()).await.unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with("$argon2id$"));
        assert!(verify_password(password, first.clone()).await.unwrap());
        assert!(
            !verify_password("wrong password".to_owned(), first.clone())
                .await
                .unwrap()
        );
        assert!(!verify_password(first.clone(), first).await.unwrap());
        assert!(validate_password("too short").is_err());
        assert!(validate_password(&"🙂".repeat(128)).is_ok());
        assert!(validate_password(&"🙂".repeat(129)).is_err());
    }

    #[test]
    fn external_editor_rejects_scope_favorites_and_unknown_fields() {
        let mut input = serde_json::json!({"token": "notes", "note_id": Uuid::now_v7(), "title": "Page", "body": "", "icon": "", "expected_version": 1});
        assert!(serde_json::from_value::<EditRequest>(input.clone()).is_ok());
        input["parent_id"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<EditRequest>(input.clone()).is_err());
        input.as_object_mut().unwrap().remove("parent_id");
        input["is_favorite"] = serde_json::json!(true);
        assert!(serde_json::from_value::<EditRequest>(input).is_err());
    }
}
