//! Durable instruction and knowledge snapshots owned by an AI profile.

use super::{
    AiProfileResponse, insert_audit, load_profile, lock_profile_for_management_scope,
    require_management, require_project_wide_for_autonomous_profile,
};
use crate::{AppState, auth::ActorContext, error::AppError};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction, types::Json as SqlJson};
use std::collections::HashMap;
use uuid::Uuid;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/profiles/{profile_id}/backups/{backup_id}",
            delete(remove),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/backups",
            get(list).post(create),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/backups/{backup_id}/restore",
            post(restore),
        )
}

#[derive(Serialize, FromRow)]
struct BackupSummary {
    id: Uuid,
    reason: String,
    created_at: DateTime<Utc>,
    knowledge_base_count: i64,
    article_count: i64,
}

#[derive(Serialize)]
struct BackupList {
    items: Vec<BackupSummary>,
}

#[derive(Deserialize)]
struct Snapshot {
    instructions: String,
    description: Option<String>,
    #[serde(default)]
    tool_instructions: String,
    blacklist_reply_text: Option<String>,
    blacklist_reply_match_language: Option<bool>,
    knowledge_bases: Vec<BaseSnapshot>,
}

#[derive(Deserialize)]
struct BaseSnapshot {
    visibility: Option<crate::resource_visibility::ResourceVisibility>,
    id: Option<Uuid>,
    name: String,
    description: String,
    status: String,
    articles: Vec<ArticleSnapshot>,
}

#[derive(Deserialize)]
struct ArticleSnapshot {
    id: Option<Uuid>,
    title: String,
    body: String,
    status: String,
    source_url: Option<String>,
    version: i64,
    published_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct RestoreInput {
    #[serde(default = "default_create_backup")]
    create_backup: bool,
}

fn default_create_backup() -> bool {
    true
}

async fn authorized_transaction(
    state: &AppState,
    actor: &ActorContext,
    profile_id: Uuid,
) -> Result<(Uuid, Transaction<'static, Postgres>), AppError> {
    let selected_project_id = require_management(actor)?;
    let project_id = crate::resource_visibility::owner_project(
        &state.db,
        actor,
        crate::resource_visibility::Resource::Profile,
        profile_id,
    )
    .await?;
    if project_id != selected_project_id {
        return Err(AppError::NotFound);
    }
    actor.require("knowledge:manage")?;
    let mut transaction = state.db.begin().await?;
    lock_profile_for_management_scope(&mut transaction, actor, project_id, profile_id).await?;
    Ok((project_id, transaction))
}

async fn list(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile_id): Path<Uuid>,
) -> Result<Json<BackupList>, AppError> {
    let (project_id, mut transaction) = authorized_transaction(&state, &actor, profile_id).await?;
    let items = sqlx::query_as::<_, BackupSummary>(
        r#"
        SELECT id, reason, created_at,
               jsonb_array_length(snapshot->'knowledge_bases')::bigint AS knowledge_base_count,
               (SELECT COALESCE(sum(jsonb_array_length(base->'articles')), 0)::bigint
                FROM jsonb_array_elements(snapshot->'knowledge_bases') base) AS article_count
        FROM ai_profile_backups
        WHERE tenant_id = $1 AND project_id = $2 AND ai_profile_id = $3
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(BackupList { items }))
}

// A single statement takes one consistent MVCC snapshot of the profile and all articles.
async fn save_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    profile_id: Uuid,
    reason: &str,
) -> Result<(), AppError> {
    let backup_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO ai_profile_backups
            (id, tenant_id, project_id, ai_profile_id, reason, created_by, snapshot)
        SELECT $4, profile.tenant_id, profile.project_id, profile.id, $5, $6,
            jsonb_build_object('instructions', profile.instructions, 'tool_instructions', profile.tool_instructions,
                'description', profile.description,
                'blacklist_reply_text', profile.blacklist_reply_text,
                'blacklist_reply_match_language', profile.blacklist_reply_match_language,
                'knowledge_bases', COALESCE((
                SELECT jsonb_agg(jsonb_build_object(
                    'id', base.id, 'name', base.name, 'description', base.description, 'status', base.status, 'visibility', base.visibility,
                    'articles', COALESCE((
                        SELECT jsonb_agg(jsonb_build_object(
                            'id', article.id, 'title', article.title, 'body', article.body, 'status', article.status,
                            'source_url', article.source_url, 'version', article.version,
                            'published_at', article.published_at
                        ) ORDER BY article.id)
                        FROM knowledge_articles article
                        WHERE article.tenant_id = base.tenant_id AND article.project_id = base.project_id
                          AND article.knowledge_base_id = base.id
                    ), '[]'::jsonb)
                ) ORDER BY base.id)
                FROM ai_profile_knowledge_bases assignment
                JOIN knowledge_bases base ON base.tenant_id = assignment.tenant_id
                    AND base.id = assignment.knowledge_base_id
                    AND base.project_id = profile.project_id
                WHERE assignment.tenant_id = profile.tenant_id AND assignment.ai_profile_id = profile.id
            ), '[]'::jsonb))
        FROM ai_profiles profile
        WHERE profile.tenant_id = $1 AND profile.project_id = $2 AND profile.id = $3
        "#,
    ).bind(actor.tenant_id).bind(project_id).bind(profile_id).bind(backup_id)
        .bind(reason).bind(actor.actor_id).execute(&mut **transaction).await?;
    insert_audit(
        transaction,
        actor,
        project_id,
        "ai_profile.backup_created",
        "ai_profile_backup",
        backup_id,
    )
    .await
}

async fn create(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile_id): Path<Uuid>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let (project_id, mut transaction) = authorized_transaction(&state, &actor, profile_id).await?;
    save_snapshot(&mut transaction, &actor, project_id, profile_id, "manual").await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({}))))
}

async fn restore(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile_id, backup_id)): Path<(Uuid, Uuid)>,
    body: Bytes,
) -> Result<Json<AiProfileResponse>, AppError> {
    let create_backup = if body.is_empty() {
        true
    } else {
        serde_json::from_slice::<RestoreInput>(&body)
            .map_err(|_| AppError::BadRequest("invalid backup restore options".to_owned()))?
            .create_backup
    };
    let (project_id, mut transaction) = authorized_transaction(&state, &actor, profile_id).await?;
    require_project_wide_for_autonomous_profile(&mut transaction, &actor, project_id, profile_id)
        .await?;
    let SqlJson(snapshot) = sqlx::query_scalar::<_, SqlJson<Snapshot>>(
        "SELECT snapshot FROM ai_profile_backups WHERE tenant_id = $1 AND project_id = $2 AND ai_profile_id = $3 AND id = $4",
    ).bind(actor.tenant_id).bind(project_id).bind(profile_id).bind(backup_id)
        .fetch_optional(&mut *transaction).await?.ok_or(AppError::NotFound)?;
    if create_backup {
        save_snapshot(
            &mut transaction,
            &actor,
            project_id,
            profile_id,
            "before_restore",
        )
        .await?;
    }
    let current_base_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT knowledge_base_id FROM ai_profile_knowledge_bases WHERE tenant_id = $1 AND ai_profile_id = $2",
    ).bind(actor.tenant_id).bind(profile_id).fetch_all(&mut *transaction).await?;
    sqlx::query(
        "DELETE FROM ai_profile_knowledge_bases WHERE tenant_id = $1 AND ai_profile_id = $2",
    )
    .bind(actor.tenant_id)
    .bind(profile_id)
    .execute(&mut *transaction)
    .await?;
    let mut restored_base_ids = Vec::new();
    let mut replacements = Vec::new();
    for base in snapshot.knowledge_bases {
        let base_id = restore_base(
            &mut transaction,
            &actor,
            project_id,
            profile_id,
            base,
            &current_base_ids,
            &mut replacements,
        )
        .await?;
        restored_base_ids.push(base_id);
        sqlx::query("INSERT INTO ai_profile_knowledge_bases (tenant_id, ai_profile_id, knowledge_base_id) VALUES ($1, $2, $3)")
            .bind(actor.tenant_id).bind(profile_id).bind(base_id).execute(&mut *transaction).await?;
    }
    let mut instructions = snapshot.instructions;
    let mut tool_instructions = snapshot.tool_instructions;
    // Shared or deleted originals need new IDs; keep explicit knowledge references usable.
    for (old, new) in replacements {
        let old = old.to_string();
        let new = new.to_string();
        instructions = instructions.replace(&old, &new);
        tool_instructions = tool_instructions.replace(&old, &new);
        sqlx::query("UPDATE knowledge_articles SET body = replace(body, $3, $4) WHERE tenant_id = $1 AND knowledge_base_id = ANY($2) AND strpos(body, $3) > 0")
            .bind(actor.tenant_id).bind(&restored_base_ids).bind(old).bind(new)
            .execute(&mut *transaction).await?;
    }
    sqlx::query("UPDATE ai_profiles SET instructions = $4, tool_instructions = $5, blacklist_reply_text = COALESCE($6::text, blacklist_reply_text), blacklist_reply_match_language = COALESCE($7::boolean, blacklist_reply_match_language), description = COALESCE($8::text, description), updated_at = now() WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
        .bind(actor.tenant_id).bind(project_id).bind(profile_id).bind(instructions).bind(tool_instructions)
        .bind(snapshot.blacklist_reply_text).bind(snapshot.blacklist_reply_match_language)
        .bind(snapshot.description)
        .execute(&mut *transaction).await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.backup_restored",
        "ai_profile_backup",
        backup_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(
        load_profile(&state, &actor, project_id, profile_id).await?,
    ))
}

async fn remove(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile_id, backup_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let (project_id, mut transaction) = authorized_transaction(&state, &actor, profile_id).await?;
    let deleted = sqlx::query("DELETE FROM ai_profile_backups WHERE tenant_id = $1 AND project_id = $2 AND ai_profile_id = $3 AND id = $4")
        .bind(actor.tenant_id).bind(project_id).bind(profile_id).bind(backup_id)
        .execute(&mut *transaction).await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.backup_deleted",
        "ai_profile_backup",
        backup_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_base(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    profile_id: Uuid,
    base: BaseSnapshot,
    current_base_ids: &[Uuid],
    replacements: &mut Vec<(Uuid, Uuid)>,
) -> Result<Uuid, AppError> {
    // Legacy copies have no IDs. Only an exact name in this project can identify their base.
    let original_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM knowledge_bases WHERE tenant_id = $1 AND project_id = $2 AND (($3::uuid IS NOT NULL AND id = $3) OR ($3 IS NULL AND name = $4)) FOR UPDATE",
    ).bind(actor.tenant_id).bind(project_id).bind(base.id).bind(&base.name)
        .fetch_optional(&mut **transaction).await?;
    let mut target_id = original_id.filter(|id| current_base_ids.contains(id));
    if target_id.is_none() {
        // Restores create this exact suffix. Reuse the connected copy, never overwrite
        // a detached base based only on a coinciding name.
        let candidates: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM knowledge_bases WHERE tenant_id = $1 AND project_id = $2 AND id = ANY($3) AND name = left($4, 160) || ' [' || id::text || ']' FOR UPDATE")
            .bind(actor.tenant_id).bind(project_id).bind(current_base_ids).bind(&base.name)
            .fetch_all(&mut **transaction).await?;
        if let [id] = candidates.as_slice() {
            target_id = Some(*id);
        }
    }
    let shared = if let Some(id) = target_id {
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM ai_profile_knowledge_bases WHERE tenant_id = $1 AND knowledge_base_id = $2 AND ai_profile_id <> $3)")
            .bind(actor.tenant_id).bind(id).bind(profile_id).fetch_one(&mut **transaction).await?
    } else {
        false
    };
    target_id = target_id.filter(|_| !shared);
    let reuse = target_id.is_some();
    let base_id = target_id.unwrap_or_else(Uuid::now_v7);
    if let Some(old) = base.id.or(original_id).filter(|id| *id != base_id) {
        replacements.push((old, base_id));
    }
    // Read before mutation so legacy articles can be matched by an unambiguous title.
    let originals: Vec<(Uuid, String)> = if let Some(id) = original_id {
        sqlx::query_as("SELECT id, title FROM knowledge_articles WHERE tenant_id = $1 AND knowledge_base_id = $2 ORDER BY id FOR UPDATE")
            .bind(actor.tenant_id).bind(id).fetch_all(&mut **transaction).await?
    } else {
        Vec::new()
    };
    let targets: Vec<(Uuid, String)> = if target_id == original_id {
        originals.clone()
    } else if let Some(id) = target_id {
        sqlx::query_as("SELECT id, title FROM knowledge_articles WHERE tenant_id = $1 AND knowledge_base_id = $2 ORDER BY id FOR UPDATE")
            .bind(actor.tenant_id).bind(id).fetch_all(&mut **transaction).await?
    } else {
        Vec::new()
    };
    let name_taken = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM knowledge_bases WHERE tenant_id = $1 AND project_id = $2 AND lower(name) = lower($3) AND id <> $4)")
        .bind(actor.tenant_id).bind(project_id).bind(&base.name).bind(base_id)
        .fetch_one(&mut **transaction).await?;
    let name = if name_taken {
        format!(
            "{} [{}]",
            base.name.chars().take(160).collect::<String>(),
            base_id
        )
    } else {
        base.name
    };
    if reuse {
        sqlx::query("UPDATE knowledge_bases SET name = $4, description = $5, status = $6, updated_at = now() WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
            .bind(actor.tenant_id).bind(project_id).bind(base_id).bind(name).bind(base.description).bind(base.status)
            .execute(&mut **transaction).await?;
    } else {
        sqlx::query("INSERT INTO knowledge_bases (id, tenant_id, project_id, name, description, status, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7)")
            .bind(base_id).bind(actor.tenant_id).bind(project_id).bind(name).bind(base.description).bind(base.status).bind(actor.actor_id)
            .execute(&mut **transaction).await?;
    }
    if !reuse {
        let mut visibility = base.visibility.unwrap_or_default();
        visibility.project_ids = vec![project_id];
        visibility.department_ids = sqlx::query_scalar(
            "SELECT id FROM departments WHERE tenant_id=$1 AND project_id=$2 AND id=ANY($3)",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(&visibility.department_ids)
        .fetch_all(&mut **transaction)
        .await?;
        crate::resource_visibility::save(
            transaction,
            actor,
            crate::resource_visibility::Resource::KnowledgeBase,
            base_id,
            Some(&visibility),
            true,
        )
        .await?;
    }
    let mut title_counts = HashMap::new();
    for article in &base.articles {
        *title_counts.entry(article.title.clone()).or_insert(0) += 1;
    }
    let mut restored_article_ids = Vec::new();
    for article in base.articles {
        let match_article = |articles: &[(Uuid, String)], allow_title: bool| {
            if let Some(id) = article.id
                && let Some((id, _)) = articles.iter().find(|(current, _)| *current == id)
            {
                return Some(*id);
            }
            if allow_title && title_counts[&article.title] == 1 {
                let mut matches = articles.iter().filter(|(_, title)| *title == article.title);
                let first = matches.next();
                first
                    .filter(|_| matches.next().is_none())
                    .map(|(id, _)| *id)
            } else {
                None
            }
        };
        let original_article_id = match_article(&originals, article.id.is_none());
        let existing_id = match_article(&targets, article.id.is_none() || target_id != original_id)
            .filter(|id| reuse && !restored_article_ids.contains(id));
        let mut article_id = existing_id.unwrap_or_else(Uuid::now_v7);
        if let Some(id) = article.id.filter(|_| existing_id.is_none() && reuse) {
            let occupied = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM knowledge_articles WHERE id = $1)",
            )
            .bind(id)
            .fetch_one(&mut **transaction)
            .await?;
            if !occupied {
                article_id = id;
            }
        }
        restored_article_ids.push(article_id);
        if let Some(old) = article
            .id
            .or(original_article_id)
            .filter(|id| *id != article_id)
        {
            replacements.push((old, article_id));
        }
        if existing_id.is_some() {
            sqlx::query("UPDATE knowledge_articles SET title = $4, body = $5, status = $6, source_url = $7, published_at = $8, updated_by = $9, updated_at = now(), version = version + 1 WHERE tenant_id = $1 AND knowledge_base_id = $2 AND id = $3")
                .bind(actor.tenant_id).bind(base_id).bind(article_id).bind(article.title).bind(article.body).bind(article.status)
                .bind(article.source_url).bind(article.published_at).bind(actor.actor_id).execute(&mut **transaction).await?;
        } else {
            sqlx::query(
                r#"INSERT INTO knowledge_articles
                (id, tenant_id, project_id, knowledge_base_id, title, body, status, source_url,
                 version, published_at, created_by, updated_by)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)"#,
            )
            .bind(article_id)
            .bind(actor.tenant_id)
            .bind(project_id)
            .bind(base_id)
            .bind(article.title)
            .bind(article.body)
            .bind(article.status)
            .bind(article.source_url)
            .bind(article.version)
            .bind(article.published_at)
            .bind(actor.actor_id)
            .execute(&mut **transaction)
            .await?;
        }
    }
    if reuse {
        sqlx::query("DELETE FROM knowledge_articles WHERE tenant_id = $1 AND knowledge_base_id = $2 AND NOT (id = ANY($3))")
            .bind(actor.tenant_id).bind(base_id).bind(restored_article_ids).execute(&mut **transaction).await?;
    }
    Ok(base_id)
}
