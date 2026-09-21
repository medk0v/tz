//! Reusable Markdown replies with optional workspace visibility.

use crate::resource_visibility::{self, Resource, ResourceVisibility};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::ActorContext,
    error::AppError,
    provider_reply::{self, ContentDraftAgentList, ContentDraftRequest, ContentDraftResponse},
};

/// Routes for browsing and managing reusable replies in an authorized Inbox.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/inboxes/{inbox_id}/reply-templates",
            get(list_templates).post(create_template),
        )
        .route(
            "/api/v1/inboxes/{inbox_id}/reply-templates/{template_id}",
            patch(update_template).delete(delete_template),
        )
        .route(
            "/api/v1/inboxes/{inbox_id}/reply-template-draft-agents",
            get(list_draft_agents),
        )
        .route(
            "/api/v1/inboxes/{inbox_id}/reply-template-drafts",
            post(generate_draft),
        )
}

#[derive(Debug, FromRow, Serialize)]
struct ReplyTemplate {
    visibility: sqlx::types::Json<ResourceVisibility>,
    id: Uuid,
    inbox_id: Uuid,
    title: String,
    body: String,
    body_format: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ReplyTemplateList {
    items: Vec<ReplyTemplate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyTemplateInput {
    visibility: Option<ResourceVisibility>,
    title: String,
    body: String,
}

fn normalize_input(input: ReplyTemplateInput) -> Result<ReplyTemplateInput, AppError> {
    let title = input.title.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = input.body.trim().to_owned();
    if !(1..=120).contains(&title.chars().count()) || title.contains('\0') {
        return Err(AppError::BadRequest(
            "title must contain between 1 and 120 characters".to_owned(),
        ));
    }
    if !(1..=10_000).contains(&body.chars().count()) || body.contains('\0') {
        return Err(AppError::BadRequest(
            "body must contain between 1 and 10000 characters".to_owned(),
        ));
    }
    Ok(ReplyTemplateInput {
        title,
        body,
        visibility: input.visibility,
    })
}

async fn require_inbox(
    state: &AppState,
    actor: &ActorContext,
    inbox_id: Uuid,
) -> Result<Uuid, AppError> {
    let project_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT project_id FROM inboxes WHERE tenant_id = $1 AND id = $2",
    )
    .bind(actor.tenant_id)
    .bind(inbox_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(project_id, inbox_id)?;
    Ok(project_id)
}

async fn template_project(
    state: &AppState,
    actor: &ActorContext,
    inbox_id: Uuid,
    template_id: Uuid,
) -> Result<Uuid, AppError> {
    require_inbox(state, actor, inbox_id).await?;
    let project_id =
        resource_visibility::owner_project(&state.db, actor, Resource::ReplyTemplate, template_id)
            .await?;
    if !actor.is_password_session() {
        let matches: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reply_templates WHERE tenant_id=$1 AND id=$2 AND inbox_id=$3)")
            .bind(actor.tenant_id).bind(template_id).bind(inbox_id).fetch_one(&state.db).await?;
        if !matches {
            return Err(AppError::NotFound);
        }
    }
    Ok(project_id)
}

async fn list_templates(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
) -> Result<Json<ReplyTemplateList>, AppError> {
    actor
        .require("conversations:reply")
        .or_else(|_| actor.require("reply_templates:manage"))?;
    let project_id = require_inbox(&state, &actor, inbox_id).await?;
    let items = sqlx::query_as::<_, ReplyTemplate>(
        r#"
        SELECT visibility, id, inbox_id, title, body, body_format, created_at, updated_at
        FROM reply_templates
        WHERE tenant_id = $1 AND resource_visible(visibility,$2,(SELECT department_id FROM inboxes WHERE tenant_id=$1 AND id=$3)) AND ($4::uuid IS NULL OR project_id=$4) AND ($5::boolean OR inbox_id=$3)
        ORDER BY lower(title), id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(resource_visibility::owner_limit(&actor))
    .bind(actor.is_password_session())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(ReplyTemplateList { items }))
}

async fn create_template(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
    Json(request): Json<ReplyTemplateInput>,
) -> Result<(StatusCode, Json<ReplyTemplate>), AppError> {
    actor.require("reply_templates:manage")?;
    let project_id = require_inbox(&state, &actor, inbox_id).await?;
    let input = normalize_input(request)?;
    let mut transaction = state.db.begin().await?;
    let mut template = sqlx::query_as::<_, ReplyTemplate>(
        r#"
        INSERT INTO reply_templates (id, tenant_id, project_id, inbox_id, title, body)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING visibility, id, inbox_id, title, body, body_format, created_at, updated_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(input.title)
    .bind(input.body)
    .fetch_one(&mut *transaction)
    .await?;
    let saved_visibility = resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::ReplyTemplate,
        template.id,
        input.visibility.as_ref(),
        true,
    )
    .await?;
    if let Some(visibility) = saved_visibility {
        template.visibility = sqlx::types::Json(visibility);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        inbox_id,
        template.id,
        "reply_template.created",
    )
    .await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn update_template(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((inbox_id, template_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ReplyTemplateInput>,
) -> Result<Json<ReplyTemplate>, AppError> {
    actor.require("reply_templates:manage")?;
    let project_id = template_project(&state, &actor, inbox_id, template_id).await?;
    let input = normalize_input(request)?;
    let mut transaction = state.db.begin().await?;
    let mut template = sqlx::query_as::<_, ReplyTemplate>(
        r#"
        UPDATE reply_templates
        SET title = $4, body = $5, updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        RETURNING visibility, id, inbox_id, title, body, body_format, created_at, updated_at
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(template_id)
    .bind(input.title)
    .bind(input.body)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let saved_visibility = resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::ReplyTemplate,
        template.id,
        input.visibility.as_ref(),
        false,
    )
    .await?;
    if let Some(visibility) = saved_visibility {
        template.visibility = sqlx::types::Json(visibility);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        inbox_id,
        template_id,
        "reply_template.updated",
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(template))
}

async fn delete_template(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((inbox_id, template_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    actor.require("reply_templates:manage")?;
    let project_id = template_project(&state, &actor, inbox_id, template_id).await?;
    let mut transaction = state.db.begin().await?;
    let deleted = sqlx::query(
        r#"
        DELETE FROM reply_templates
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(template_id)
    .execute(&mut *transaction)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        inbox_id,
        template_id,
        "reply_template.deleted",
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_draft_agents(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
) -> Result<Json<ContentDraftAgentList>, AppError> {
    actor.require("reply_templates:manage")?;
    let project_id = require_inbox(&state, &actor, inbox_id).await?;
    Ok(Json(
        provider_reply::list_content_draft_agents(&state, &actor, project_id).await?,
    ))
}

/// Drafts a template body from its title; the operator reviews and saves it.
async fn generate_draft(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
    Json(request): Json<ContentDraftRequest>,
) -> Result<Json<ContentDraftResponse>, AppError> {
    actor.require("reply_templates:manage")?;
    let project_id = require_inbox(&state, &actor, inbox_id).await?;
    Ok(Json(
        provider_reply::generate_content_draft(
            &state,
            &actor,
            project_id,
            provider_reply::ContentDraftKind::ReplyTemplate,
            request,
        )
        .await?,
    ))
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    inbox_id: Uuid,
    template_id: Uuid,
    action: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action, resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5, 'reply_template', $6, $7)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(action)
    .bind(template_id)
    .bind(serde_json::json!({ "inbox_id": inbox_id }))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ReplyTemplateInput, normalize_input};

    #[test]
    fn input_normalizes_title_and_preserves_markdown() {
        let input = normalize_input(ReplyTemplateInput {
            visibility: None,
            title: " \t  Проверка\nоплаты \u{a0}".to_owned(),
            body: "\n**Проверяю заявку.**\n\n- Оплата\n- Выплата\n".to_owned(),
        })
        .unwrap();
        assert_eq!(input.title, "Проверка оплаты");
        assert_eq!(input.body, "**Проверяю заявку.**\n\n- Оплата\n- Выплата");
    }

    #[test]
    fn input_enforces_unicode_character_limits_and_nonblank_content() {
        for (title, body) in [
            (" ".to_owned(), "Reply".to_owned()),
            ("Title".to_owned(), "\n\t\u{a0}".to_owned()),
            ("Ж".repeat(121), "Reply".to_owned()),
            ("Title".to_owned(), "Ж".repeat(10_001)),
            ("Title\0".to_owned(), "Reply".to_owned()),
            ("Title".to_owned(), "Reply\0".to_owned()),
        ] {
            assert!(
                normalize_input(ReplyTemplateInput {
                    title,
                    body,
                    visibility: None
                })
                .is_err()
            );
        }
        assert!(
            normalize_input(ReplyTemplateInput {
                visibility: None,
                title: "Ж".repeat(120),
                body: "Ж".repeat(10_000),
            })
            .is_ok()
        );
    }
}
