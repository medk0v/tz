//! Project-scoped knowledge bases and manually maintained articles.

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
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    auth::ActorContext,
    error::AppError,
    provider_reply::{self, ContentDraftAgentList, ContentDraftRequest, ContentDraftResponse},
};

/// Routes for knowledge-base and article administration.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/knowledge-bases",
            get(list_knowledge_bases).post(create_knowledge_base),
        )
        .route(
            "/api/v1/knowledge-bases/{knowledge_base_id}",
            patch(update_knowledge_base).delete(delete_knowledge_base),
        )
        .route(
            "/api/v1/knowledge-bases/{knowledge_base_id}/articles",
            get(list_articles).post(create_article),
        )
        .route(
            "/api/v1/knowledge-bases/{knowledge_base_id}/articles/{article_id}",
            patch(update_article).delete(delete_article),
        )
        .route(
            "/api/v1/knowledge-bases/{knowledge_base_id}/article-draft-agents",
            get(list_article_draft_agents),
        )
        .route(
            "/api/v1/knowledge-bases/{knowledge_base_id}/article-drafts",
            post(generate_article_draft),
        )
}

#[derive(Debug, FromRow)]
struct KnowledgeBaseRow {
    visibility: sqlx::types::Json<ResourceVisibility>,
    id: Uuid,
    name: String,
    description: String,
    status: String,
    article_count: i64,
    published_article_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct KnowledgeBaseResponse {
    visibility: ResourceVisibility,
    id: Uuid,
    name: String,
    description: String,
    status: String,
    article_count: i64,
    published_article_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<KnowledgeBaseRow> for KnowledgeBaseResponse {
    fn from(row: KnowledgeBaseRow) -> Self {
        Self {
            visibility: row.visibility.0,
            id: row.id,
            name: row.name,
            description: row.description,
            status: row.status,
            article_count: row.article_count,
            published_article_count: row.published_article_count,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct KnowledgeBaseListResponse {
    items: Vec<KnowledgeBaseResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KnowledgeBaseRequest {
    visibility: Option<ResourceVisibility>,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default, rename = "default_language")]
    _legacy_default_language: Option<String>,
    status: String,
}

async fn list_knowledge_bases(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<KnowledgeBaseListResponse>, AppError> {
    let project_id = require_management(&actor)?;
    let items = load_knowledge_bases(&state, &actor, project_id, None).await?;
    Ok(Json(KnowledgeBaseListResponse { items }))
}

async fn create_knowledge_base(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<KnowledgeBaseRequest>,
) -> Result<(StatusCode, Json<KnowledgeBaseResponse>), AppError> {
    let project_id = require_management(&actor)?;
    let visibility = request.visibility.clone();
    let input = normalize_knowledge_base(request)?;
    let knowledge_base_id = Uuid::now_v7();
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO knowledge_bases (
            id, tenant_id, project_id, name, description, status, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(knowledge_base_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.status)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await
    .map_err(knowledge_write_error)?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
        visibility.as_ref(),
        true,
    )
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "knowledge_base.created",
        "knowledge_base",
        knowledge_base_id,
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_knowledge_base(&state, &actor, project_id, knowledge_base_id).await?),
    ))
}

async fn update_knowledge_base(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(knowledge_base_id): Path<Uuid>,
    Json(request): Json<KnowledgeBaseRequest>,
) -> Result<Json<KnowledgeBaseResponse>, AppError> {
    require_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
    )
    .await?;
    let visibility = request.visibility.clone();
    let input = normalize_knowledge_base(request)?;
    let mut transaction = state.db.begin().await?;
    lock_knowledge_base_for_task_scope(&mut transaction, &actor, project_id, knowledge_base_id)
        .await?;
    let updated = sqlx::query(
        r#"
        UPDATE knowledge_bases
        SET name = $4, description = $5, status = $6, updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.status)
    .execute(&mut *transaction)
    .await
    .map_err(knowledge_write_error)?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
        visibility.as_ref(),
        false,
    )
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "knowledge_base.updated",
        "knowledge_base",
        knowledge_base_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(
        load_knowledge_base(&state, &actor, project_id, knowledge_base_id).await?,
    ))
}

async fn delete_knowledge_base(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(knowledge_base_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
    )
    .await?;
    let mut transaction = state.db.begin().await?;
    lock_knowledge_base_for_task_scope(&mut transaction, &actor, project_id, knowledge_base_id)
        .await?;
    let deleted = sqlx::query(
        "DELETE FROM knowledge_bases WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .execute(&mut *transaction)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "knowledge_base.deleted",
        "knowledge_base",
        knowledge_base_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, FromRow, Serialize)]
struct KnowledgeArticleResponse {
    id: Uuid,
    knowledge_base_id: Uuid,
    title: String,
    body: String,
    status: String,
    source_url: Option<String>,
    version: i64,
    published_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct KnowledgeArticleListResponse {
    items: Vec<KnowledgeArticleResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KnowledgeArticleRequest {
    title: String,
    #[serde(default)]
    body: String,
    status: String,
    source_url: Option<String>,
}

async fn list_articles(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(knowledge_base_id): Path<Uuid>,
) -> Result<Json<KnowledgeArticleListResponse>, AppError> {
    require_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
    )
    .await?;
    ensure_knowledge_base(&state, &actor, project_id, knowledge_base_id).await?;
    let items = sqlx::query_as::<_, KnowledgeArticleResponse>(
        r#"
        SELECT id, knowledge_base_id, title, body, status, source_url,
               version, published_at, created_at, updated_at
        FROM knowledge_articles
        WHERE tenant_id = $1 AND project_id = $2 AND knowledge_base_id = $3
        ORDER BY updated_at DESC, id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(KnowledgeArticleListResponse { items }))
}

async fn create_article(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(knowledge_base_id): Path<Uuid>,
    Json(request): Json<KnowledgeArticleRequest>,
) -> Result<(StatusCode, Json<KnowledgeArticleResponse>), AppError> {
    require_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
    )
    .await?;
    let input = normalize_article(request)?;
    let article_id = Uuid::now_v7();
    let mut transaction = state.db.begin().await?;
    lock_knowledge_base_for_task_scope(&mut transaction, &actor, project_id, knowledge_base_id)
        .await?;
    sqlx::query(
        r#"
        INSERT INTO knowledge_articles (
            id, tenant_id, project_id, knowledge_base_id, title, body,
            status, source_url, created_by, updated_by, published_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $9,
            CASE WHEN $7 = 'published' THEN now() ELSE NULL END
        )
        "#,
    )
    .bind(article_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .bind(&input.title)
    .bind(&input.body)
    .bind(&input.status)
    .bind(&input.source_url)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "knowledge_article.created",
        "knowledge_article",
        article_id,
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_article(&state, &actor, project_id, knowledge_base_id, article_id).await?),
    ))
}

async fn update_article(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((knowledge_base_id, article_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<KnowledgeArticleRequest>,
) -> Result<Json<KnowledgeArticleResponse>, AppError> {
    require_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
    )
    .await?;
    let input = normalize_article(request)?;
    let mut transaction = state.db.begin().await?;
    lock_knowledge_base_for_task_scope(&mut transaction, &actor, project_id, knowledge_base_id)
        .await?;
    let updated = sqlx::query(
        r#"
        UPDATE knowledge_articles
        SET title = $5, body = $6, status = $7, source_url = $8,
            updated_by = $9, updated_at = now(), version = version + 1,
            published_at = CASE
                WHEN $7 = 'published' THEN COALESCE(published_at, now())
                ELSE NULL
            END
        WHERE tenant_id = $1 AND project_id = $2
          AND knowledge_base_id = $3 AND id = $4
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .bind(article_id)
    .bind(&input.title)
    .bind(&input.body)
    .bind(&input.status)
    .bind(&input.source_url)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "knowledge_article.updated",
        "knowledge_article",
        article_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(
        load_article(&state, &actor, project_id, knowledge_base_id, article_id).await?,
    ))
}

async fn delete_article(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((knowledge_base_id, article_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
    )
    .await?;
    let mut transaction = state.db.begin().await?;
    lock_knowledge_base_for_task_scope(&mut transaction, &actor, project_id, knowledge_base_id)
        .await?;
    let deleted = sqlx::query(
        r#"
        DELETE FROM knowledge_articles
        WHERE tenant_id = $1 AND project_id = $2
          AND knowledge_base_id = $3 AND id = $4
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .bind(article_id)
    .execute(&mut *transaction)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "knowledge_article.deleted",
        "knowledge_article",
        article_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

struct NormalizedKnowledgeBase {
    name: String,
    description: String,
    status: String,
}

fn normalize_knowledge_base(
    request: KnowledgeBaseRequest,
) -> Result<NormalizedKnowledgeBase, AppError> {
    Ok(NormalizedKnowledgeBase {
        name: normalize_required_text(request.name, 200, "knowledge base name")?,
        description: normalize_optional_text(request.description, 2_000, "description")?,
        status: validate_value(request.status, &["active", "archived"], "status")?,
    })
}

struct NormalizedArticle {
    title: String,
    body: String,
    status: String,
    source_url: Option<String>,
}

fn normalize_article(request: KnowledgeArticleRequest) -> Result<NormalizedArticle, AppError> {
    Ok(NormalizedArticle {
        title: normalize_required_text(request.title, 300, "article title")?,
        body: normalize_optional_text(request.body, 200_000, "article body")?,
        status: validate_value(request.status, &["draft", "published"], "status")?,
        source_url: normalize_source_url(request.source_url)?,
    })
}

fn normalize_source_url(value: Option<String>) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > 2_000 {
        return Err(AppError::BadRequest(
            "source URL must not exceed 2000 characters".to_owned(),
        ));
    }
    let parsed = Url::parse(value)
        .map_err(|_| AppError::BadRequest("source URL must be a valid URL".to_owned()))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(AppError::BadRequest(
            "source URL must use http or https and contain no credentials".to_owned(),
        ));
    }
    Ok(Some(parsed.to_string()))
}

fn normalize_required_text(
    value: String,
    max_chars: usize,
    field: &str,
) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(AppError::BadRequest(format!(
            "{field} must contain between 1 and {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_optional_text(
    value: String,
    max_chars: usize,
    field: &str,
) -> Result<String, AppError> {
    let value = value.trim();
    if value.chars().count() > max_chars {
        return Err(AppError::BadRequest(format!(
            "{field} must not exceed {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

fn validate_value(value: String, allowed: &[&str], field: &str) -> Result<String, AppError> {
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(AppError::BadRequest(format!("unsupported {field}")))
    }
}

async fn article_draft_project(
    state: &AppState,
    actor: &ActorContext,
    knowledge_base_id: Uuid,
) -> Result<Uuid, AppError> {
    require_management(actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        actor,
        Resource::KnowledgeBase,
        knowledge_base_id,
    )
    .await?;
    ensure_knowledge_base(state, actor, project_id, knowledge_base_id).await?;
    Ok(project_id)
}

async fn list_article_draft_agents(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(knowledge_base_id): Path<Uuid>,
) -> Result<Json<ContentDraftAgentList>, AppError> {
    let project_id = article_draft_project(&state, &actor, knowledge_base_id).await?;
    Ok(Json(
        provider_reply::list_content_draft_agents(&state, &actor, project_id).await?,
    ))
}

/// Drafts material content from its title; the editor reviews and saves it.
async fn generate_article_draft(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(knowledge_base_id): Path<Uuid>,
    Json(request): Json<ContentDraftRequest>,
) -> Result<Json<ContentDraftResponse>, AppError> {
    let project_id = article_draft_project(&state, &actor, knowledge_base_id).await?;
    Ok(Json(
        provider_reply::generate_content_draft(
            &state,
            &actor,
            project_id,
            provider_reply::ContentDraftKind::KnowledgeArticle,
            request,
        )
        .await?,
    ))
}

fn require_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    actor.require("knowledge:manage")?;
    actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))
}

pub(crate) async fn lock_knowledge_base_for_task_scope(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    knowledge_base_id: Uuid,
) -> Result<(), AppError> {
    let locked = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM knowledge_bases
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if locked.is_none() {
        return Err(AppError::NotFound);
    }
    if !actor.has_restricted_inbox_scope() {
        return Ok(());
    }
    let has_autonomous_usage = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM ai_profile_knowledge_bases AS profile_knowledge
            JOIN ai_task_agents AS task_agent
              ON task_agent.tenant_id = profile_knowledge.tenant_id
             AND task_agent.ai_profile_id = profile_knowledge.ai_profile_id
            JOIN ai_tasks AS task
              ON task.tenant_id = task_agent.tenant_id
             AND task.id = task_agent.task_id
            WHERE profile_knowledge.tenant_id = $1
              AND profile_knowledge.knowledge_base_id = $2
              AND task.project_id = $3
        ) OR EXISTS (
            SELECT 1
            FROM ai_profile_knowledge_bases AS assignment
            JOIN ai_api_endpoints AS endpoint
              ON endpoint.tenant_id = assignment.tenant_id
             AND endpoint.ai_profile_id = assignment.ai_profile_id
            WHERE assignment.tenant_id = $1
              AND assignment.knowledge_base_id = $2
              AND endpoint.project_id = $3
              AND endpoint.deleted_at IS NULL
        ) OR EXISTS (
            SELECT 1
            FROM ai_profile_knowledge_bases AS assignment
            JOIN ai_api_settings AS settings
              ON settings.tenant_id = assignment.tenant_id
             AND settings.ai_profile_id = assignment.ai_profile_id
            WHERE assignment.tenant_id = $1
              AND assignment.knowledge_base_id = $2
              AND settings.project_id = $3
              AND (settings.enabled OR settings.key_id IS NOT NULL)
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(knowledge_base_id)
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    crate::ai_tasks::require_project_wide_task_dependency(actor, has_autonomous_usage)
}

async fn load_knowledge_bases(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    selected_id: Option<Uuid>,
) -> Result<Vec<KnowledgeBaseResponse>, AppError> {
    let rows = sqlx::query_as::<_, KnowledgeBaseRow>(
        r#"
        SELECT knowledge_base.visibility, knowledge_base.id, knowledge_base.name, knowledge_base.description,
               knowledge_base.status,
               COUNT(article.id) AS article_count,
               COUNT(article.id) FILTER (WHERE article.status = 'published')
                   AS published_article_count,
               knowledge_base.created_at, knowledge_base.updated_at
        FROM knowledge_bases AS knowledge_base
        LEFT JOIN knowledge_articles AS article
          ON article.tenant_id = knowledge_base.tenant_id
         AND article.knowledge_base_id = knowledge_base.id
        WHERE knowledge_base.tenant_id = $1
          AND knowledge_base.project_id = $2
          AND (($4::uuid IS NOT NULL AND knowledge_base.id=$4)
            OR ($4 IS NULL AND resource_visible(knowledge_base.visibility,$2,$3)))
        GROUP BY knowledge_base.id
        ORDER BY knowledge_base.name, knowledge_base.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.department_id())
    .bind(selected_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(KnowledgeBaseResponse::from).collect())
}

async fn load_knowledge_base(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    knowledge_base_id: Uuid,
) -> Result<KnowledgeBaseResponse, AppError> {
    load_knowledge_bases(state, actor, project_id, Some(knowledge_base_id))
        .await?
        .into_iter()
        .find(|knowledge_base| knowledge_base.id == knowledge_base_id)
        .ok_or(AppError::NotFound)
}

async fn ensure_knowledge_base(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    knowledge_base_id: Uuid,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM knowledge_bases WHERE tenant_id = $1 AND project_id = $2 AND id = $3)",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .fetch_one(&state.db)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

async fn load_article(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    knowledge_base_id: Uuid,
    article_id: Uuid,
) -> Result<KnowledgeArticleResponse, AppError> {
    sqlx::query_as::<_, KnowledgeArticleResponse>(
        r#"
        SELECT id, knowledge_base_id, title, body, status, source_url,
               version, published_at, created_at, updated_at
        FROM knowledge_articles
        WHERE tenant_id = $1 AND project_id = $2
          AND knowledge_base_id = $3 AND id = $4
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(knowledge_base_id)
    .bind(article_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    action: &str,
    resource_kind: &str,
    resource_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, '{}'::jsonb)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(action)
    .bind(resource_kind)
    .bind(resource_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn knowledge_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("a knowledge base with this name already exists".to_owned())
    } else {
        AppError::Database(error)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        KnowledgeArticleRequest, KnowledgeBaseRequest, normalize_article, normalize_knowledge_base,
    };

    #[test]
    fn validates_knowledge_base_identity_and_article_source() {
        assert!(
            normalize_knowledge_base(KnowledgeBaseRequest {
                visibility: None,
                name: " Support rules ".to_owned(),
                description: "Customer-facing policies".to_owned(),
                _legacy_default_language: None,
                status: "active".to_owned(),
            })
            .is_ok()
        );
        assert!(
            normalize_article(KnowledgeArticleRequest {
                title: "Refunds".to_owned(),
                body: "Ask an operator before promising a refund.".to_owned(),
                status: "published".to_owned(),
                source_url: Some("https://docs.example/refunds".to_owned()),
            })
            .is_ok()
        );
        assert!(
            normalize_article(KnowledgeArticleRequest {
                title: "Unsafe".to_owned(),
                body: String::new(),
                status: "draft".to_owned(),
                source_url: Some("https://user:secret@example.com/private".to_owned()),
            })
            .is_err()
        );
    }

    #[test]
    fn accepts_new_and_legacy_knowledge_base_payloads_without_language_semantics() {
        for payload in [
            json!({
                "name": "Support rules",
                "description": "Policies",
                "status": "active"
            }),
            json!({
                "name": "Support rules",
                "description": "Policies",
                "default_language": "ru",
                "status": "active"
            }),
        ] {
            let request: KnowledgeBaseRequest =
                serde_json::from_value(payload).expect("payload should deserialize");
            let normalized = normalize_knowledge_base(request).expect("payload should normalize");
            assert_eq!(normalized.name, "Support rules");
            assert_eq!(normalized.description, "Policies");
            assert_eq!(normalized.status, "active");
        }
    }
}
