//! At-most-once queue claiming and bounded, cancellable agent execution.

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use super::schema;
use crate::{
    AppState,
    provider_reply::{ApiExecutionError, ApiExecutionInput, execute_api_run},
};

// Admin tests can execute a saved draft without publishing its API. External
// runs must retain the exact key generation and enabled API/endpoint throughout.
const AUTHORIZED_RELATION: &str = r#"
    SELECT 1 FROM ai_api_settings AS settings
    JOIN ai_profiles AS profile ON profile.id=settings.ai_profile_id
        AND profile.tenant_id=settings.tenant_id AND profile.project_id=settings.project_id
    JOIN projects ON projects.id=profile.project_id AND projects.tenant_id=profile.tenant_id
    JOIN ai_provider_connections AS provider ON provider.id=profile.provider_connection_id
        AND provider.tenant_id=profile.tenant_id
    JOIN ai_api_endpoints AS endpoint ON endpoint.id=run.endpoint_id
        AND endpoint.ai_profile_id=profile.id AND endpoint.tenant_id=profile.tenant_id
    WHERE settings.ai_profile_id=run.ai_profile_id
      AND settings.tenant_id=run.tenant_id AND settings.project_id=run.project_id
      AND profile.status='active' AND projects.status='active' AND provider.status='active'
      AND provider.provider_kind IN ('openai','openai_compatible') AND endpoint.deleted_at IS NULL
      AND (NOT run.cache_refresh OR (endpoint.cache_refresh_seconds IS NOT NULL AND endpoint.version=run.endpoint_version))
      AND (run.key_id IS NULL OR (settings.enabled AND endpoint.enabled AND settings.key_id=run.key_id))
"#;

#[derive(FromRow)]
struct ApiRunJob {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    ai_profile_id: Uuid,
    instructions: String,
    input: Value,
    output_schema: Value,
    timeout_seconds: i64,
    locked_by: Uuid,
}

/// Processes one queued invocation without retrying provider execution.
///
/// # Errors
/// Returns an error when queue state cannot be read or durably updated.
pub async fn process_once(state: &AppState) -> Result<bool> {
    maintain(state).await?;
    super::cache::schedule_due(state).await?;
    let Some(job) = claim(state).await? else {
        return Ok(false);
    };
    let lease = crate::ai_execution::Lease::new(&state.db, job.locked_by);
    let timeout = Duration::from_secs(u64::try_from(job.timeout_seconds)?);
    let result = execute_api_run(
        state,
        ApiExecutionInput {
            run_id: job.id,
            tenant_id: job.tenant_id,
            project_id: job.project_id,
            profile_id: job.ai_profile_id,
            instructions: &job.instructions,
            input: &job.input,
            output_schema: &job.output_schema,
            timeout,
        },
    )
    .await;
    finish(state, &job, result).await?;
    lease.release().await?;
    Ok(true)
}

pub(crate) async fn run_is_authorized(state: &AppState, run_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM ai_api_runs AS run WHERE run.id=$1 AND run.status='processing' AND EXISTS({AUTHORIZED_RELATION}))"))
        .bind(run_id).fetch_one(&state.db).await?)
}

async fn maintain(state: &AppState) -> Result<()> {
    sqlx::query("UPDATE ai_api_runs SET status='failed',error_code=CASE WHEN status='pending' THEN 'queue_timeout' ELSE 'timeout' END,error_message='The API run deadline was exceeded',completed_at=now(),locked_by=NULL WHERE (status='pending' AND queue_deadline_at<=now()) OR (status='processing' AND deadline_at<=now())")
        .execute(&state.db).await?;
    sqlx::query(&format!("UPDATE ai_api_runs AS run SET status='cancelled',error_code='cancelled',error_message='API execution is no longer authorized',completed_at=now() WHERE run.status IN ('pending','processing') AND NOT EXISTS({AUTHORIZED_RELATION})"))
        .execute(&state.db).await?;
    // A cancelled execution keeps its lane until its owner removes the runtime
    // grant. An abandoned owner is bounded by the original execution deadline.
    sqlx::query("UPDATE ai_api_runs SET locked_by=NULL WHERE status='cancelled' AND locked_by IS NOT NULL AND deadline_at<=now()")
        .execute(&state.db).await?;
    // Keep the idempotency/result history for seven days, pruning bounded batches.
    sqlx::query("DELETE FROM ai_api_runs WHERE id IN (SELECT id FROM ai_api_runs WHERE completed_at < now()-interval '7 days' AND status IN ('completed','failed','cancelled') ORDER BY completed_at LIMIT 100)")
        .execute(&state.db).await?;
    Ok(())
}

async fn claim(state: &AppState) -> Result<Option<ApiRunJob>> {
    let mut tx = state.db.begin().await?;
    let Some(capacity) =
        crate::ai_execution::available(&mut tx, state.config.worker.ai_run_concurrency).await?
    else {
        tx.commit().await?;
        return Ok(None);
    };
    let job = sqlx::query_as::<_, ApiRunJob>(&format!(r#"
        WITH candidate AS (
            SELECT run.id FROM ai_api_runs AS run
            WHERE run.status='pending' AND run.queue_deadline_at>now()
              AND NOT (run.ai_profile_id=ANY($2))
              AND EXISTS({AUTHORIZED_RELATION})
            ORDER BY run.created_at,run.id
            LIMIT 1 FOR UPDATE OF run SKIP LOCKED
        )
        UPDATE ai_api_runs AS run SET status='processing',started_at=now(),
            deadline_at=now()+make_interval(secs=>run.timeout_seconds::double precision),locked_by=$1
        FROM candidate WHERE run.id=candidate.id RETURNING run.*
    "#)).bind(Uuid::now_v7()).bind(&capacity.blocked_profiles).fetch_optional(&mut *tx).await?;
    if let Some(job) = &job {
        crate::ai_execution::reserve(
            &mut tx,
            job.locked_by,
            Some(job.ai_profile_id),
            None,
            job.timeout_seconds + 60,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(job)
}

async fn finish(
    state: &AppState,
    job: &ApiRunJob,
    result: Result<Value, ApiExecutionError>,
) -> Result<()> {
    let result = result.and_then(|value| {
        schema::validate(&value, &job.output_schema)
            .map_err(|_| ApiExecutionError::ResultSchemaInvalid)?;
        Ok(value)
    });
    let (status, result, error_code, error_message) = match result {
        Ok(value) => ("completed", Some(value), None, None),
        Err(error) => (
            if error == ApiExecutionError::Cancelled {
                "cancelled"
            } else {
                "failed"
            },
            None,
            Some(error.code()),
            Some(error.to_string()),
        ),
    };
    let mut tx = state.db.begin().await?;
    // Serialize publication with changes to every authority source. SHARE is
    // compatible with FK KEY SHARE checks used by audit/admission inserts, but
    // blocks status/key updates until this short finalization commits.
    // Match admission's project-before-settings order explicitly.
    sqlx::query("SELECT id FROM projects WHERE tenant_id=$1 AND id=$2 FOR SHARE")
        .bind(job.tenant_id)
        .bind(job.project_id)
        .execute(&mut *tx)
        .await?;
    // Profile deletion takes the profile before cascading into API settings.
    // Explicit separate locks keep that same order independent of query plans.
    let provider: Option<Option<Uuid>> = sqlx::query_scalar("SELECT provider_connection_id FROM ai_profiles WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR SHARE")
        .bind(job.tenant_id).bind(job.project_id).bind(job.ai_profile_id).fetch_optional(&mut *tx).await?;
    if let Some(provider) = provider.flatten() {
        sqlx::query("SELECT id FROM ai_provider_connections WHERE id=$1 FOR SHARE")
            .bind(provider)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("SELECT ai_profile_id FROM ai_api_settings WHERE ai_profile_id=$1 FOR SHARE")
        .bind(job.ai_profile_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT id FROM ai_api_endpoints WHERE id=(SELECT endpoint_id FROM ai_api_runs WHERE id=$1) FOR SHARE")
        .bind(job.id).execute(&mut *tx).await?;
    sqlx::query(&format!(r#"
        WITH run_authorization AS (
            SELECT run.id, EXISTS({AUTHORIZED_RELATION}) AS allowed
            FROM ai_api_runs AS run WHERE run.id=$1
        )
        UPDATE ai_api_runs AS run SET
            status=CASE WHEN run.status='cancelled' THEN run.status WHEN NOT run_authorization.allowed THEN 'cancelled' ELSE $3 END,
            result=CASE WHEN run.status='cancelled' OR NOT run_authorization.allowed THEN NULL ELSE $4 END,
            error_code=CASE WHEN run.status='cancelled' THEN run.error_code WHEN NOT run_authorization.allowed THEN 'cancelled' ELSE $5 END,
            error_message=CASE WHEN run.status='cancelled' THEN run.error_message WHEN NOT run_authorization.allowed THEN 'API execution is no longer authorized' ELSE $6 END,
            completed_at=COALESCE(run.completed_at,now()),locked_by=NULL
        FROM run_authorization WHERE run.id=run_authorization.id AND run.locked_by=$2
          AND run.status IN ('processing','cancelled')
    "#))
        .bind(job.id).bind(job.locked_by).bind(status).bind(result).bind(error_code).bind(error_message).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO ai_api_cache (endpoint_id,endpoint_version,key_id,result,updated_at) SELECT endpoint_id,endpoint_version,key_id,result,completed_at FROM ai_api_runs WHERE id=$1 AND cache_refresh AND status='completed' AND result IS NOT NULL ON CONFLICT (endpoint_id) DO UPDATE SET endpoint_version=EXCLUDED.endpoint_version,key_id=EXCLUDED.key_id,result=EXCLUDED.result,updated_at=EXCLUDED.updated_at WHERE ai_api_cache.updated_at<=EXCLUDED.updated_at")
        .bind(job.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
