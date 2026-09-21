//! Fixed-input snapshots refreshed by the durable API worker, never by readers.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use super::{
    ApiError, ApiResult,
    runs::{InvocationIdentity, enqueue},
};
use crate::{AppState, error::AppError};

// Shared by readers and scheduling: an unpublished or unavailable agent must
// not expose a previous snapshot or continue consuming provider resources.
const AVAILABLE: &str = r#"
    FROM ai_api_endpoints AS endpoint
    JOIN ai_api_settings AS settings ON settings.ai_profile_id=endpoint.ai_profile_id
      AND settings.tenant_id=endpoint.tenant_id AND settings.project_id=endpoint.project_id
    JOIN ai_profiles AS profile ON profile.id=endpoint.ai_profile_id
      AND profile.tenant_id=endpoint.tenant_id AND profile.project_id=endpoint.project_id
    JOIN projects ON projects.id=endpoint.project_id AND projects.tenant_id=endpoint.tenant_id
    JOIN ai_provider_connections AS provider ON provider.id=profile.provider_connection_id
      AND provider.tenant_id=profile.tenant_id
    WHERE endpoint.deleted_at IS NULL AND endpoint.enabled AND settings.enabled
      AND settings.key_id IS NOT NULL AND endpoint.cache_refresh_seconds IS NOT NULL
      AND profile.status='active' AND projects.status='active' AND provider.status='active'
      AND provider.provider_kind IN ('openai','openai_compatible')
"#;

#[derive(FromRow)]
struct Snapshot {
    input_example: Value,
    result: Option<Value>,
    updated_at: Option<DateTime<Utc>>,
    stale: Option<bool>,
}

pub(super) async fn read(
    state: &AppState,
    identity: &InvocationIdentity,
    endpoint: Uuid,
    input: &Value,
) -> ApiResult<Response> {
    let snapshot = sqlx::query_as::<_, Snapshot>(&format!(r#"
        SELECT endpoint.input_example,
          (SELECT result FROM ai_api_cache WHERE endpoint_id=endpoint.id AND endpoint_version=endpoint.version AND key_id=settings.key_id) AS result,
          (SELECT updated_at FROM ai_api_cache WHERE endpoint_id=endpoint.id AND endpoint_version=endpoint.version AND key_id=settings.key_id) AS updated_at,
          (SELECT updated_at + make_interval(secs=>endpoint.cache_refresh_seconds::double precision) < now() FROM ai_api_cache WHERE endpoint_id=endpoint.id AND endpoint_version=endpoint.version AND key_id=settings.key_id) AS stale
        {AVAILABLE} AND endpoint.id=$1 AND settings.key_id=$2
          AND endpoint.tenant_id=$3 AND endpoint.project_id=$4 AND endpoint.ai_profile_id=$5
    "#)).bind(endpoint).bind(identity.key_id).bind(identity.tenant_id).bind(identity.project_id).bind(identity.ai_profile_id)
        .fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    if input != &snapshot.input_example {
        return Err(ApiError::invalid(
            "Cached methods require the saved input_example exactly",
        ));
    }
    let Some(result) = snapshot.result else {
        return Ok((StatusCode::SERVICE_UNAVAILABLE, [("retry-after", "5")], Json(json!({
            "error":{"code":"cache_warming","message":"No successful background refresh is available yet"}
        }))).into_response());
    };
    Ok(
        Json(json!({"result":result,"updated_at":snapshot.updated_at,"stale":snapshot.stale}))
            .into_response(),
    )
}

#[derive(FromRow)]
struct Due {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    ai_profile_id: Uuid,
    key_id: Uuid,
    input_example: Value,
}

pub(super) async fn schedule_due(state: &AppState) -> anyhow::Result<()> {
    let due = sqlx::query_as::<_, Due>(&format!("SELECT endpoint.id,endpoint.tenant_id,endpoint.project_id,endpoint.ai_profile_id,settings.key_id,endpoint.input_example {AVAILABLE} AND endpoint.cache_next_refresh_at<=now() AND NOT EXISTS(SELECT 1 FROM ai_api_runs WHERE endpoint_id=endpoint.id AND cache_refresh AND status IN ('pending','processing')) ORDER BY endpoint.cache_next_refresh_at,endpoint.id LIMIT 16"))
        .fetch_all(&state.db).await?;
    for endpoint in due {
        let identity = InvocationIdentity {
            tenant_id: endpoint.tenant_id,
            project_id: endpoint.project_id,
            ai_profile_id: endpoint.ai_profile_id,
            key_id: Some(endpoint.key_id),
            actor_id: None,
            cache_refresh: true,
        };
        match enqueue(state, &identity, endpoint.id, &endpoint.input_example, None).await {
            Ok(_)
            | Err(
                ApiError::Problem(StatusCode::CONFLICT | StatusCode::TOO_MANY_REQUESTS, _, _)
                | ApiError::App(AppError::NotFound | AppError::Forbidden | AppError::Unauthorized),
            ) => {}
            Err(error) => anyhow::bail!("cache refresh scheduling failed: {error:?}"),
        }
    }
    Ok(())
}
