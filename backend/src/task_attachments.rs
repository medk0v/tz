//! Private task attachments, including short-lived uploads for unsaved drafts.

use std::collections::HashSet;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    attachments::{self, AttachmentObject, AttachmentStorage, PreparedAttachment},
    auth::ActorContext,
    error::AppError,
    task_board,
};

const MAX_TASK_ATTACHMENTS: usize = 5;
const MAX_DRAFT_ATTACHMENTS: i64 = 20;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/task-attachments",
            post(upload).layer(DefaultBodyLimit::max(attachments::MAX_UPLOAD_REQUEST_BYTES)),
        )
        .route("/api/v1/ai/task-attachments/{attachment_id}", get(download))
}

#[derive(Serialize)]
struct UploadedAttachment {
    id: Uuid,
}

async fn upload(
    State(state): State<AppState>,
    actor: ActorContext,
    multipart: Multipart,
) -> Result<(StatusCode, Json<UploadedAttachment>), AppError> {
    let project_id = task_board::require_manage(&actor)?;
    let mut prepared = state.attachment_storage.receive_task(multipart).await?;
    // Finish publishing even if the client disconnects. Abandoned drafts expire after a day.
    tokio::spawn(async move {
        let result = store_upload(&state, &actor, project_id, &mut prepared).await;
        if result.is_err() && !AttachmentStorage::is_published(&prepared) {
            state.attachment_storage.discard(&prepared).await;
        }
        result.map(|()| {
            (
                StatusCode::CREATED,
                Json(UploadedAttachment { id: prepared.id }),
            )
        })
    })
    .await
    .map_err(AppError::internal)?
}

async fn store_upload(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    prepared: &mut PreparedAttachment,
) -> Result<(), AppError> {
    let mut tx = state.db.begin().await?;
    // Share the storage quota lock with conversation attachments and recordings.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("attachments:{}", actor.tenant_id))
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_task_attachments WHERE tenant_id=$1 AND project_id=$2 AND uploaded_by=$3 AND task_id IS NULL AND created_at > now()-interval '24 hours'",
    ).bind(actor.tenant_id).bind(project_id).bind(actor.actor_id).fetch_one(&mut *tx).await?;
    if count >= MAX_DRAFT_ATTACHMENTS {
        return Err(AppError::BadRequest(
            "too many unattached files; save your existing task drafts first".into(),
        ));
    }
    let used_bytes: i64 = sqlx::query_scalar(
        r"SELECT (
            COALESCE((SELECT SUM(byte_size) FROM message_attachments WHERE tenant_id=$1 AND purged_at IS NULL AND scan_status IN ('pending','clean','expired')), 0) +
            COALESCE((SELECT SUM(byte_size) FROM ai_task_attachments WHERE tenant_id=$1), 0)
        )::bigint",
    ).bind(actor.tenant_id).fetch_one(&mut *tx).await?;
    let max_bytes =
        i64::try_from(state.config.attachments.max_tenant_bytes).map_err(AppError::internal)?;
    if used_bytes.saturating_add(prepared.byte_size) > max_bytes {
        return Err(AppError::PayloadTooLarge(
            "the workspace has reached its attachment storage quota".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO ai_task_attachments (id,tenant_id,project_id,uploaded_by,file_name,content_type,byte_size,checksum_sha256) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    ).bind(prepared.id).bind(actor.tenant_id).bind(project_id).bind(actor.actor_id)
        .bind(&prepared.file_name).bind(prepared.content_type).bind(prepared.byte_size)
        .bind(&prepared.checksum_sha256).execute(&mut *tx).await?;
    state.attachment_storage.publish(prepared).await?;
    tx.commit().await?;
    Ok(())
}

/// Attach only this actor's fresh draft uploads or files already on this task.
/// The task and attachment changes share one transaction, so failed saves remain retryable.
pub(crate) async fn replace(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    task_id: Uuid,
    ids: Option<&[Uuid]>,
) -> Result<(), AppError> {
    let Some(ids) = ids else {
        return Ok(());
    };
    if ids.len() > MAX_TASK_ATTACHMENTS || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(AppError::BadRequest(
            "a task may have up to 5 different files".into(),
        ));
    }
    for (position, id) in ids.iter().enumerate() {
        let updated = sqlx::query(
            r"UPDATE ai_task_attachments SET task_id=$4, project_id=$3, position=$7
              WHERE tenant_id=$1 AND id=$2 AND (
                (task_id=$4 AND project_id=$3) OR
                (task_id IS NULL AND uploaded_by=$5 AND project_id=$6 AND created_at > now()-interval '24 hours')
              )",
        ).bind(actor.tenant_id).bind(id).bind(project_id).bind(task_id).bind(actor.actor_id)
            .bind(actor.project_id).bind(i32::try_from(position).map_err(AppError::internal)?)
            .execute(&mut **tx).await?;
        if updated.rows_affected() != 1 {
            return Err(AppError::BadRequest(
                "an attachment is unavailable; upload it again".into(),
            ));
        }
    }
    // The existing storage reconciler removes unreferenced files after its grace period.
    sqlx::query(
        "DELETE FROM ai_task_attachments WHERE tenant_id=$1 AND task_id=$2 AND NOT (id=ANY($3))",
    )
    .bind(actor.tenant_id)
    .bind(task_id)
    .bind(ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[derive(FromRow)]
struct AttachmentRow {
    id: Uuid,
    task_id: Option<Uuid>,
    project_id: Uuid,
    uploaded_by: Uuid,
    file_name: String,
    content_type: String,
    byte_size: i64,
    checksum_sha256: String,
}

async fn download(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(attachment_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let access = task_board::task_actor(&actor)?;
    let attachment = sqlx::query_as::<_, AttachmentRow>(
        "SELECT id,task_id,project_id,uploaded_by,file_name,content_type,byte_size,checksum_sha256 FROM ai_task_attachments WHERE tenant_id=$1 AND id=$2 AND (task_id IS NOT NULL OR created_at > now()-interval '24 hours')",
    ).bind(actor.tenant_id).bind(attachment_id).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    if let Some(task_id) = attachment.task_id {
        task_board::task_project(&state, &actor, access.access, task_id).await?;
    } else if attachment.uploaded_by != actor.actor_id || attachment.project_id != access.project_id
    {
        return Err(AppError::NotFound);
    }
    let mut response = attachments::attachment_response(
        &state,
        AttachmentObject {
            id: attachment.id,
            content_type: attachment.content_type,
            byte_size: attachment.byte_size,
            checksum_sha256: attachment.checksum_sha256,
        },
        &headers,
    )
    .await?;
    let disposition = format!(
        "attachment; filename=\"attachment-{}\"; filename*=UTF-8''{}",
        attachment.id,
        utf8_percent_encode(&attachment.file_name, NON_ALPHANUMERIC)
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(AppError::internal)?,
    );
    Ok(response)
}
