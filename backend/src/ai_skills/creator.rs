//! Temporary source ingestion for an editable, unsaved skill draft.

use std::time::Duration;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    routing::post,
};
use tokio::sync::Semaphore;
use uuid::Uuid;

use super::{
    require_write,
    sources::{SkillSource, parse_material},
};
use crate::{AppState, auth::ActorContext, error::AppError, provider_reply};

const MAX_FILES: usize = 10;
const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 30 * 1024 * 1024;
const MAX_SOURCE_CHARS: usize = 1_000_000;
static GENERATIONS: Semaphore = Semaphore::const_new(2);

pub(super) fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/ai/skills/generate",
        post(generate).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES + 1024 * 1024)),
    )
}

struct MaterialUpload {
    name: String,
    content_type: String,
    bytes: Vec<u8>,
}

async fn generate(
    State(state): State<AppState>,
    actor: ActorContext,
    multipart: Multipart,
) -> Result<Json<provider_reply::GeneratedSkillDraft>, AppError> {
    let project_id = require_write(&actor)?;
    let _permit = GENERATIONS
        .try_acquire()
        .map_err(|_| AppError::TooManyRequests)?;
    tokio::time::timeout(
        Duration::from_secs(600),
        generate_inner(&state, &actor, project_id, multipart),
    )
    .await
    .map_err(|_| {
        AppError::BadRequest("Skill generation timed out. Try fewer source files.".to_owned())
    })?
    .map(Json)
}

async fn generate_inner(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    mut multipart: Multipart,
) -> Result<provider_reply::GeneratedSkillDraft, AppError> {
    let tenant_id = actor.tenant_id;
    let mut provider_id = None;
    let mut goal = None;
    let mut source_text = None;
    let mut uploads = Vec::new();
    let mut request_bytes = 0_usize;
    while let Some(mut field) = multipart.next_field().await.map_err(|_| invalid_form())? {
        let key = field.name().ok_or_else(invalid_form)?.to_owned();
        if !matches!(
            key.as_str(),
            "provider_connection_id" | "goal" | "source_text" | "file"
        ) {
            return Err(invalid_form());
        }
        let file_name = field.file_name().map(str::to_owned);
        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_owned();
        let limit = match key.as_str() {
            "file" => MAX_FILE_BYTES,
            "source_text" => MAX_SOURCE_CHARS * 4,
            "goal" => 40_000,
            _ => 100,
        };
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|_| invalid_form())? {
            request_bytes = request_bytes.saturating_add(chunk.len());
            if request_bytes > MAX_REQUEST_BYTES || bytes.len().saturating_add(chunk.len()) > limit
            {
                return Err(AppError::PayloadTooLarge(
                    "Sources exceed the limit: 20 MiB per file and 30 MiB per request.".to_owned(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if key == "file" {
            if uploads.len() >= MAX_FILES {
                return Err(AppError::BadRequest(
                    "Upload at most 10 source files.".to_owned(),
                ));
            }
            uploads.push(MaterialUpload {
                name: file_name.ok_or_else(invalid_form)?,
                content_type,
                bytes,
            });
            continue;
        }
        if file_name.is_some() {
            return Err(invalid_form());
        }
        let value = String::from_utf8(bytes).map_err(|_| invalid_form())?;
        match key.as_str() {
            "provider_connection_id" if provider_id.is_none() => {
                provider_id = Some(value.parse::<Uuid>().map_err(|_| invalid_form())?);
            }
            "goal" if goal.is_none() => goal = Some(value),
            "source_text" if source_text.is_none() => source_text = Some(value),
            _ => return Err(invalid_form()),
        }
    }
    let provider_id = provider_id.ok_or_else(invalid_form)?;
    let goal = goal.ok_or_else(invalid_form)?;
    let goal = goal.trim();
    if goal.is_empty() || goal.chars().count() > 10_000 || goal.contains('\0') {
        return Err(AppError::BadRequest(
            "Describe the skill's purpose in 1 to 10000 characters.".to_owned(),
        ));
    }
    // Validate model visibility before spending time parsing uploaded books.
    let provider_project_id = crate::resource_visibility::owner_project(
        &state.db,
        actor,
        crate::resource_visibility::Resource::Provider,
        provider_id,
    )
    .await?;
    let available: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_provider_connections WHERE tenant_id=$1 AND project_id=$2 AND id=$3 AND status='active' AND model_type='chat' AND provider_kind IN ('openai','openai_compatible'))")
        .bind(tenant_id).bind(provider_project_id).bind(provider_id).fetch_one(&state.db).await?;
    if !available {
        return Err(AppError::NotFound);
    }
    let mut sources = Vec::new();
    let mut source_chars = 0_usize;
    if let Some(text) = source_text.filter(|text| !text.trim().is_empty()) {
        if text.contains('\0') {
            return Err(invalid_form());
        }
        source_chars = text.chars().count();
        if source_chars > MAX_SOURCE_CHARS {
            return Err(AppError::PayloadTooLarge(
                "Source text exceeds 1000000 characters.".to_owned(),
            ));
        }
        sources.push(SkillSource::Text {
            name: "Pasted text".to_owned(),
            text,
        });
    }
    for upload in uploads {
        let source = parse_material(&upload.name, &upload.content_type, upload.bytes).await?;
        if let SkillSource::Text { text, .. } = &source {
            source_chars += text.chars().count();
        }
        if source_chars > MAX_SOURCE_CHARS {
            return Err(AppError::PayloadTooLarge(
                "Source text exceeds 1000000 characters. Split the materials into separate skills."
                    .to_owned(),
            ));
        }
        sources.push(source);
    }
    provider_reply::generate_skill_draft(state, tenant_id, project_id, provider_id, goal, &sources)
        .await
}

fn invalid_form() -> AppError {
    AppError::BadRequest("Invalid skill source form. Provide one model connection, one purpose, optional text, and source files.".to_owned())
}
