//! Project-wide administration of API definitions and invocation credentials.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    ApiError, ApiResult, Endpoint, EndpointInput, audit, cancel_runs, ensure_settings,
    require_profile,
};
use crate::{AppState, auth::ActorContext, error::AppError};

pub(super) async fn settings(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let settings: Option<(bool, Option<String>)> = sqlx::query_as("SELECT enabled,key_prefix FROM ai_api_settings WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3")
        .bind(actor.tenant_id).bind(project).bind(profile).fetch_optional(&state.db).await?;
    let (enabled, prefix) = settings.unwrap_or((false, None));
    let endpoints = sqlx::query_as::<_, Endpoint>("SELECT * FROM ai_api_endpoints WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3 AND deleted_at IS NULL ORDER BY created_at,id")
        .bind(actor.tenant_id).bind(project).bind(profile).fetch_all(&state.db).await?;
    Ok(Json(
        json!({"enabled":enabled,"key_configured":prefix.is_some(),"key_prefix":prefix,
        "can_manage_key":actor.require_session_admin().is_ok(),"endpoints":endpoints}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SettingsInput {
    enabled: bool,
}

pub(super) async fn update_settings(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
    Json(input): Json<SettingsInput>,
) -> ApiResult<Json<Value>> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let mut tx = state.db.begin().await?;
    ensure_settings(&mut tx, actor.tenant_id, project, profile).await?;
    sqlx::query("UPDATE ai_api_settings SET enabled=$2,updated_at=now() WHERE ai_profile_id=$1")
        .bind(profile)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
    if !input.enabled {
        cancel_runs(&mut tx, profile, None, "api_disabled").await?;
    }
    audit(&mut tx, &actor, project, profile, "ai_api.settings_updated").await?;
    tx.commit().await?;
    settings(State(state), actor, Path(profile)).await
}

pub(super) async fn rotate_key(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let project = require_profile(&state.db, &actor, profile).await?;
    actor.require_session_admin()?;
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let key = format!("tzapi_{}", URL_SAFE_NO_PAD.encode(bytes));
    let prefix: String = key.chars().take(14).collect();
    let mut tx = state.db.begin().await?;
    ensure_settings(&mut tx, actor.tenant_id, project, profile).await?;
    sqlx::query("UPDATE ai_api_settings SET key_id=$2,key_hash=$3,key_prefix=$4,key_created_by=$5,key_created_at=now(),key_last_used_at=NULL,updated_at=now() WHERE ai_profile_id=$1")
        .bind(profile).bind(Uuid::now_v7()).bind(Sha256::digest(key.as_bytes()).to_vec()).bind(&prefix).bind(actor.actor_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_api_endpoints SET cache_next_refresh_at=now() WHERE ai_profile_id=$1 AND cache_refresh_seconds IS NOT NULL AND deleted_at IS NULL")
        .bind(profile).execute(&mut *tx).await?;
    cancel_runs(&mut tx, profile, None, "key_rotated").await?;
    audit(&mut tx, &actor, project, profile, "ai_api.key_rotated").await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((headers, Json(json!({"key":key,"key_prefix":prefix}))))
}

pub(super) async fn revoke_key(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let project = require_profile(&state.db, &actor, profile).await?;
    actor.require_session_admin()?;
    let mut tx = state.db.begin().await?;
    ensure_settings(&mut tx, actor.tenant_id, project, profile).await?;
    sqlx::query("UPDATE ai_api_settings SET key_id=NULL,key_hash=NULL,key_prefix=NULL,key_created_by=NULL,key_created_at=NULL,key_last_used_at=NULL,updated_at=now() WHERE ai_profile_id=$1")
        .bind(profile).execute(&mut *tx).await?;
    cancel_runs(&mut tx, profile, None, "key_revoked").await?;
    audit(&mut tx, &actor, project, profile, "ai_api.key_revoked").await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn create_endpoint(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
    Json(input): Json<EndpointInput>,
) -> ApiResult<(StatusCode, Json<Endpoint>)> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let (input, input_schema, output_schema) = input.normalize()?;
    let mut tx = state.db.begin().await?;
    ensure_settings(&mut tx, actor.tenant_id, project, profile).await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_api_endpoints WHERE ai_profile_id=$1 AND deleted_at IS NULL",
    )
    .bind(profile)
    .fetch_one(&mut *tx)
    .await?;
    if count >= 32 {
        return Err(ApiError::invalid(
            "an agent may have at most 32 API endpoints",
        ));
    }
    let endpoint = sqlx::query_as::<_, Endpoint>("INSERT INTO ai_api_endpoints (id,tenant_id,project_id,ai_profile_id,name,slug,instructions,input_example,output_example,input_schema,output_schema,enabled,timeout_seconds,cache_refresh_seconds,response_wait_seconds) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING *")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project).bind(profile).bind(input.name).bind(input.slug).bind(input.instructions).bind(input.input_example).bind(input.output_example).bind(input_schema).bind(output_schema).bind(input.enabled).bind(input.timeout_seconds).bind(input.cache_refresh_seconds).bind(input.response_wait_seconds).fetch_one(&mut *tx).await?;
    audit(&mut tx, &actor, project, profile, "ai_api.endpoint_created").await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(endpoint)))
}

pub(super) async fn update_endpoint(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile, id)): Path<(Uuid, Uuid)>,
    Json(input): Json<EndpointInput>,
) -> ApiResult<Json<Endpoint>> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let (input, input_schema, output_schema) = input.normalize()?;
    let mut tx = state.db.begin().await?;
    ensure_settings(&mut tx, actor.tenant_id, project, profile).await?;
    let endpoint = sqlx::query_as::<_, Endpoint>("UPDATE ai_api_endpoints SET name=$3,slug=$4,instructions=$5,input_example=$6,output_example=$7,input_schema=$8,output_schema=$9,enabled=$10,timeout_seconds=$11,cache_refresh_seconds=$12,response_wait_seconds=$13,cache_next_refresh_at=now(),version=version+1,updated_at=now() WHERE ai_profile_id=$1 AND id=$2 AND deleted_at IS NULL RETURNING *")
        .bind(profile).bind(id).bind(input.name).bind(input.slug).bind(input.instructions).bind(input.input_example).bind(input.output_example).bind(input_schema).bind(output_schema).bind(input.enabled).bind(input.timeout_seconds).bind(input.cache_refresh_seconds).bind(input.response_wait_seconds).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    sqlx::query("DELETE FROM ai_api_cache WHERE endpoint_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if !input.enabled {
        cancel_runs(&mut tx, profile, Some(id), "endpoint_disabled").await?;
    }
    audit(&mut tx, &actor, project, profile, "ai_api.endpoint_updated").await?;
    tx.commit().await?;
    Ok(Json(endpoint))
}

pub(super) async fn delete_endpoint(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let mut tx = state.db.begin().await?;
    ensure_settings(&mut tx, actor.tenant_id, project, profile).await?;
    let count = sqlx::query("UPDATE ai_api_endpoints SET enabled=false,deleted_at=now(),updated_at=now() WHERE ai_profile_id=$1 AND id=$2 AND deleted_at IS NULL").bind(profile).bind(id).execute(&mut *tx).await?.rows_affected();
    if count == 0 {
        return Err(AppError::NotFound.into());
    }
    cancel_runs(&mut tx, profile, Some(id), "endpoint_deleted").await?;
    audit(&mut tx, &actor, project, profile, "ai_api.endpoint_deleted").await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
