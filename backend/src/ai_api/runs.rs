//! Scoped invocation, transactional admission, idempotency, and run history.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use super::{
    ApiError, ApiResult, Endpoint, Run, RunRow, audit, ensure_settings, require_profile, schema,
};
use crate::{AppState, auth::ActorContext, error::AppError};

#[derive(FromRow)]
pub(super) struct InvocationIdentity {
    pub(super) tenant_id: Uuid,
    pub(super) project_id: Uuid,
    pub(super) ai_profile_id: Uuid,
    pub(super) key_id: Option<Uuid>,
    #[sqlx(skip)]
    pub(super) actor_id: Option<Uuid>,
    #[sqlx(skip)]
    pub(super) cache_refresh: bool,
}

async fn authenticate(
    state: &AppState,
    profile: Uuid,
    headers: &HeaderMap,
) -> ApiResult<InvocationIdentity> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| value.starts_with("tzapi_") && value.len() <= 100)
        .ok_or(AppError::Unauthorized)?;
    let identity = sqlx::query_as::<_, InvocationIdentity>("SELECT settings.tenant_id,settings.project_id,settings.ai_profile_id,settings.key_id FROM ai_api_settings AS settings JOIN projects ON projects.id=settings.project_id AND projects.tenant_id=settings.tenant_id WHERE settings.ai_profile_id=$1 AND settings.key_hash=$2 AND projects.status='active'")
        .bind(profile).bind(Sha256::digest(token.as_bytes()).to_vec()).fetch_optional(&state.db).await?.ok_or(AppError::Unauthorized)?;
    Ok(identity)
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WaitQuery {
    wait_seconds: Option<u64>,
}

pub(super) async fn invoke(
    State(state): State<AppState>,
    Path((profile, slug)): Path<(Uuid, String)>,
    Query(query): Query<WaitQuery>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> ApiResult<Response> {
    if query.wait_seconds.is_some_and(|wait| wait > 300) {
        return Err(ApiError::invalid("wait_seconds must be between 0 and 300"));
    }
    let identity = authenticate(&state, profile, &headers).await?;
    let (endpoint, cached): (Uuid, bool) = sqlx::query_as(
        "SELECT id,cache_refresh_seconds IS NOT NULL FROM ai_api_endpoints WHERE ai_profile_id=$1 AND slug=$2 AND deleted_at IS NULL",
    )
    .bind(profile)
    .bind(&slug)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let idempotency = parse_idempotency(&headers)?;
    if cached {
        return super::cache::read(&state, &identity, endpoint, &input)
            .await
            .map(no_store);
    }
    let mut run = enqueue(&state, &identity, endpoint, &input, idempotency).await?;
    // Legacy wait settings cannot turn a public invocation into an async response.
    // The worker owns execution deadlines and records terminal failures.
    while matches!(run.status.as_str(), "pending" | "processing") {
        tokio::time::sleep(Duration::from_millis(200)).await;
        authenticate(&state, profile, &headers).await?;
        run = load_run(&state, &identity, run.id).await?;
    }
    let response = match run.status.as_str() {
        "completed" => (
            StatusCode::OK,
            Json(run.result.ok_or_else(|| {
                AppError::internal(anyhow::anyhow!("completed API run is missing result"))
            })?),
        )
            .into_response(),
        "failed" | "cancelled" => {
            let error = run.error.unwrap_or_else(
                || json!({"code":"execution_failed","message":"API execution failed"}),
            );
            let status = if error["code"] == "timeout" || error["code"] == "queue_timeout" {
                StatusCode::GATEWAY_TIMEOUT
            } else if run.status == "cancelled" {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, Json(json!({"error":error,"run_id":run.id}))).into_response()
        }
        _ => return Err(AppError::internal(anyhow::anyhow!("unexpected API run status")).into()),
    };
    Ok(no_store(response))
}

pub(super) async fn poll(
    State(state): State<AppState>,
    Path((profile, slug, run_id)): Path<(Uuid, String, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let identity = authenticate(&state, profile, &headers).await?;
    let row = sqlx::query_as::<_, RunRow>("SELECT run.* FROM ai_api_runs AS run JOIN ai_api_settings AS settings ON settings.tenant_id=run.tenant_id AND settings.project_id=run.project_id AND settings.ai_profile_id=run.ai_profile_id AND settings.key_id=run.key_id WHERE run.tenant_id=$1 AND run.project_id=$2 AND run.ai_profile_id=$3 AND run.key_id=$4 AND run.id=$5 AND run.endpoint_slug=$6")
        .bind(identity.tenant_id).bind(identity.project_id).bind(profile).bind(identity.key_id).bind(run_id).bind(slug).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    Ok(no_store(Json(Run::from(row)).into_response()))
}

pub(super) fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

fn parse_idempotency(headers: &HeaderMap) -> ApiResult<Option<Uuid>> {
    headers
        .get("idempotency-key")
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<Uuid>().ok())
                .filter(|id| !id.is_nil())
                .ok_or_else(|| ApiError::invalid("Idempotency-Key must be a non-nil UUID"))
        })
        .transpose()
}

pub(super) async fn test_endpoint(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile, endpoint)): Path<(Uuid, Uuid)>,
    Json(input): Json<Value>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let identity = InvocationIdentity {
        tenant_id: actor.tenant_id,
        project_id: project,
        ai_profile_id: profile,
        key_id: None,
        actor_id: Some(actor.actor_id),
        cache_refresh: false,
    };
    let run = enqueue(&state, &identity, endpoint, &input, None).await?;
    let mut tx = state.db.begin().await?;
    audit(&mut tx, &actor, project, profile, "ai_api.test_requested").await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(run)))
}

pub(super) async fn list_runs(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let rows = sqlx::query_as::<_, RunRow>("SELECT * FROM ai_api_runs WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3 ORDER BY created_at DESC,id DESC LIMIT 100")
        .bind(actor.tenant_id).bind(project).bind(profile).fetch_all(&state.db).await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(Run::from).collect::<Vec<_>>()}),
    ))
}

pub(super) async fn get_run(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile, run_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Run>> {
    let project = require_profile(&state.db, &actor, profile).await?;
    let row = sqlx::query_as::<_, RunRow>("SELECT * FROM ai_api_runs WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3 AND id=$4")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(run_id).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    Ok(Json(row.into()))
}

async fn load_run(state: &AppState, identity: &InvocationIdentity, id: Uuid) -> ApiResult<Run> {
    let row = sqlx::query_as::<_, RunRow>("SELECT run.* FROM ai_api_runs AS run JOIN ai_api_settings AS settings ON settings.tenant_id=run.tenant_id AND settings.project_id=run.project_id AND settings.ai_profile_id=run.ai_profile_id WHERE run.tenant_id=$1 AND run.project_id=$2 AND run.ai_profile_id=$3 AND run.id=$4 AND run.key_id IS NOT DISTINCT FROM $5 AND (run.key_id IS NULL OR settings.key_id=run.key_id)")
        .bind(identity.tenant_id).bind(identity.project_id).bind(identity.ai_profile_id).bind(id).bind(identity.key_id).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    Ok(row.into())
}

pub(super) async fn enqueue(
    state: &AppState,
    identity: &InvocationIdentity,
    endpoint_id: Uuid,
    input: &Value,
    idempotency: Option<Uuid>,
) -> ApiResult<Run> {
    let mut tx = state.db.begin().await?;
    // Serialize admission per project so queue/rate bounds also hold across agents.
    let active: Option<bool> = sqlx::query_scalar(
        "SELECT status='active' FROM projects WHERE tenant_id=$1 AND id=$2 FOR NO KEY UPDATE",
    )
    .bind(identity.tenant_id)
    .bind(identity.project_id)
    .fetch_optional(&mut *tx)
    .await?;
    if active != Some(true) {
        return Err(AppError::Forbidden.into());
    }
    ensure_settings(
        &mut tx,
        identity.tenant_id,
        identity.project_id,
        identity.ai_profile_id,
    )
    .await?;
    let (enabled, current_key): (bool, Option<Uuid>) =
        sqlx::query_as("SELECT enabled,key_id FROM ai_api_settings WHERE ai_profile_id=$1")
            .bind(identity.ai_profile_id)
            .fetch_one(&mut *tx)
            .await?;
    if identity.key_id.is_some() && current_key != identity.key_id {
        return Err(AppError::Unauthorized.into());
    }
    let endpoint = sqlx::query_as::<_, Endpoint>("SELECT * FROM ai_api_endpoints WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3 AND id=$4 AND deleted_at IS NULL FOR SHARE")
        .bind(identity.tenant_id).bind(identity.project_id).bind(identity.ai_profile_id).bind(endpoint_id).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    if identity.key_id.is_some() && (!enabled || !endpoint.enabled) {
        return Err(AppError::NotFound.into());
    }
    let available: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_profiles AS profile JOIN ai_provider_connections AS provider ON provider.id=profile.provider_connection_id AND provider.tenant_id=profile.tenant_id WHERE profile.id=$1 AND profile.tenant_id=$2 AND profile.project_id=$3 AND profile.status='active' AND provider.status='active' AND provider.provider_kind IN ('openai','openai_compatible'))")
        .bind(identity.ai_profile_id).bind(identity.tenant_id).bind(identity.project_id).fetch_one(&mut *tx).await?;
    if !available {
        return Err(ApiError::Problem(
            StatusCode::CONFLICT,
            "agent_unavailable",
            "The agent or its provider is not active".into(),
        ));
    }
    if identity.cache_refresh {
        let due: bool = sqlx::query_scalar("SELECT cache_refresh_seconds IS NOT NULL AND cache_next_refresh_at<=now() AND NOT EXISTS(SELECT 1 FROM ai_api_runs WHERE endpoint_id=$1 AND cache_refresh AND status IN ('pending','processing')) FROM ai_api_endpoints WHERE id=$1")
            .bind(endpoint_id).fetch_one(&mut *tx).await?;
        if !due || input != &endpoint.input_example {
            return Err(ApiError::Problem(
                StatusCode::CONFLICT,
                "cache_refresh_not_due",
                "Cache refresh is not due".into(),
            ));
        }
    } else if identity.key_id.is_some() && endpoint.cache_refresh_seconds.is_some() {
        return Err(ApiError::Problem(
            StatusCode::CONFLICT,
            "method_changed",
            "Method settings changed; retry the request".into(),
        ));
    }
    let input_hash = Sha256::digest(input.to_string().as_bytes()).to_vec();
    if let Some(idempotency) = idempotency {
        let prior: Option<(Uuid,Uuid,Vec<u8>)> = sqlx::query_as("SELECT id,endpoint_id,input_hash FROM ai_api_runs WHERE ai_profile_id=$1 AND key_id=$2 AND idempotency_key=$3")
            .bind(identity.ai_profile_id).bind(identity.key_id).bind(idempotency).fetch_optional(&mut *tx).await?;
        if let Some((id, prior_endpoint, prior_hash)) = prior {
            if prior_endpoint != endpoint_id || prior_hash != input_hash {
                return Err(ApiError::Problem(
                    StatusCode::CONFLICT,
                    "idempotency_conflict",
                    "Idempotency-Key was already used for a different request".into(),
                ));
            }
            tx.commit().await?;
            return load_run(state, identity, id).await;
        }
    }
    schema::validate(input, &endpoint.input_schema).map_err(ApiError::invalid)?;
    let (queued, recent): (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM ai_api_runs WHERE tenant_id=$1 AND project_id=$2 AND status IN ('pending','processing')),(SELECT count(*) FROM ai_api_runs WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3 AND created_at>now()-interval '1 minute')")
        .bind(identity.tenant_id).bind(identity.project_id).bind(identity.ai_profile_id).fetch_one(&mut *tx).await?;
    if queued >= 100 || recent >= 30 {
        return Err(ApiError::Problem(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "API capacity reached; retry later".into(),
        ));
    }
    let run = sqlx::query_as::<_, RunRow>("INSERT INTO ai_api_runs (id,tenant_id,project_id,ai_profile_id,endpoint_id,endpoint_name,endpoint_slug,endpoint_version,key_id,actor_id,instructions,input_schema,output_schema,input,timeout_seconds,idempotency_key,input_hash,cache_refresh) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18) RETURNING *")
        .bind(Uuid::now_v7()).bind(identity.tenant_id).bind(identity.project_id).bind(identity.ai_profile_id).bind(endpoint.id).bind(endpoint.name).bind(endpoint.slug).bind(endpoint.version).bind(identity.key_id).bind(identity.actor_id).bind(endpoint.instructions).bind(endpoint.input_schema).bind(endpoint.output_schema).bind(input).bind(endpoint.timeout_seconds).bind(idempotency).bind(input_hash).bind(identity.cache_refresh).fetch_one(&mut *tx).await?;
    if identity.cache_refresh {
        sqlx::query("UPDATE ai_api_endpoints SET cache_next_refresh_at=now()+make_interval(secs=>cache_refresh_seconds::double precision) WHERE id=$1")
            .bind(endpoint_id).execute(&mut *tx).await?;
    }
    if identity.key_id.is_some() {
        sqlx::query("UPDATE ai_api_settings SET key_last_used_at=now() WHERE ai_profile_id=$1")
            .bind(identity.ai_profile_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(run.into())
}

#[cfg(test)]
mod tests {
    use super::parse_idempotency;
    use axum::http::{HeaderMap, HeaderValue};
    #[test]
    fn idempotency_requires_a_non_nil_uuid() {
        let mut headers = HeaderMap::new();
        assert!(parse_idempotency(&headers).unwrap().is_none());
        for input in ["", "not-a-uuid", "00000000-0000-0000-0000-000000000000"] {
            headers.insert("idempotency-key", HeaderValue::from_str(input).unwrap());
            assert!(parse_idempotency(&headers).is_err());
        }
    }
}
