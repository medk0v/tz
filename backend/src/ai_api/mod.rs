//! Agent-owned API definitions, scoped keys, and durable JSON invocations.

mod cache;
mod management;
mod runs;
mod schema;
mod worker;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError};
pub use worker::process_once;
pub(crate) use worker::run_is_authorized;

type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
enum ApiError {
    App(AppError),
    Problem(StatusCode, &'static str, String),
}

impl ApiError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            message.into(),
        )
    }
}

impl From<AppError> for ApiError {
    fn from(value: AppError) -> Self {
        Self::App(value)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(value: sqlx::Error) -> Self {
        if value
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            return Self::Problem(
                StatusCode::CONFLICT,
                "conflict",
                "This API slug already exists".into(),
            );
        }
        Self::App(AppError::Database(value))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::App(error) => error.into_response(),
            Self::Problem(status, code, message) => (
                status,
                Json(json!({"error":{
                    "code":code,"message":message,"request_id":Uuid::now_v7()
                }})),
            )
                .into_response(),
        }
    }
}

/// Routes for API administration and scoped public invocation.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/profiles/{profile_id}/api",
            get(management::settings).patch(management::update_settings),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/api/key",
            post(management::rotate_key).delete(management::revoke_key),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/api/endpoints",
            post(management::create_endpoint),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/api/endpoints/{endpoint_id}",
            axum::routing::patch(management::update_endpoint).delete(management::delete_endpoint),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/api/endpoints/{endpoint_id}/test",
            post(runs::test_endpoint),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/api/runs",
            get(runs::list_runs),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/api/runs/{run_id}",
            get(runs::get_run),
        )
        .route("/{profile_id}/api/{slug}", post(runs::invoke))
        .route("/{profile_id}/api/{slug}/runs/{run_id}", get(runs::poll))
        .layer(DefaultBodyLimit::max(256 * 1024))
}

#[derive(Debug, FromRow, Serialize)]
struct Endpoint {
    id: Uuid,
    name: String,
    slug: String,
    instructions: String,
    input_example: Value,
    output_example: Value,
    input_schema: Value,
    output_schema: Value,
    enabled: bool,
    timeout_seconds: i64,
    cache_refresh_seconds: Option<i64>,
    response_wait_seconds: i64,
    version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointInput {
    name: String,
    slug: String,
    instructions: String,
    input_example: Value,
    output_example: Value,
    enabled: bool,
    #[serde(default = "default_timeout")]
    timeout_seconds: i64,
    #[serde(default)]
    cache_refresh_seconds: Option<i64>,
    #[serde(default = "default_response_wait")]
    response_wait_seconds: i64,
}

const fn default_timeout() -> i64 {
    180
}

const fn default_response_wait() -> i64 {
    20
}

impl EndpointInput {
    fn normalize(mut self) -> ApiResult<(Self, Value, Value)> {
        self.name = self.name.trim().to_owned();
        self.slug = self.slug.trim().to_owned();
        self.instructions = self.instructions.trim().to_owned();
        if !(1..=100).contains(&self.name.chars().count()) {
            return Err(ApiError::invalid("name must contain 1 to 100 characters"));
        }
        if self.slug.is_empty()
            || self.slug.len() > 64
            || !self.slug.as_bytes()[0].is_ascii_lowercase()
            || !self.slug.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
            })
        {
            return Err(ApiError::invalid(
                "slug must start with a lowercase letter and contain at most 64 lowercase letters, digits, underscores or hyphens",
            ));
        }
        if !(1..=50_000).contains(&self.instructions.chars().count()) {
            return Err(ApiError::invalid(
                "instructions must contain 1 to 50000 characters",
            ));
        }
        if !(10..=300).contains(&self.timeout_seconds) {
            return Err(ApiError::invalid(
                "timeout_seconds must be between 10 and 300",
            ));
        }
        if !(0..=300).contains(&self.response_wait_seconds) {
            return Err(ApiError::invalid(
                "response_wait_seconds must be between 0 and 300",
            ));
        }
        if self
            .cache_refresh_seconds
            .is_some_and(|seconds| !(60..=86_400).contains(&seconds))
        {
            return Err(ApiError::invalid(
                "cache_refresh_seconds must be between 60 and 86400",
            ));
        }
        let input_schema = schema::infer(&self.input_example)
            .map_err(|error| ApiError::invalid(format!("input_example: {error}")))?;
        let output_schema = schema::infer(&self.output_example)
            .map_err(|error| ApiError::invalid(format!("output_example: {error}")))?;
        Ok((self, input_schema, output_schema))
    }
}

#[derive(FromRow)]
struct RunRow {
    id: Uuid,
    endpoint_id: Uuid,
    endpoint_name: String,
    status: String,
    input: Value,
    result: Option<Value>,
    error_code: Option<String>,
    error_message: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct Run {
    id: Uuid,
    endpoint_id: Uuid,
    endpoint_name: String,
    status: String,
    input: Value,
    result: Option<Value>,
    error: Option<Value>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
}

impl From<RunRow> for Run {
    fn from(row: RunRow) -> Self {
        Self {
            id: row.id,
            endpoint_id: row.endpoint_id,
            endpoint_name: row.endpoint_name,
            status: row.status,
            input: row.input,
            result: row.result,
            error: row
                .error_code
                .map(|code| json!({"code":code,"message":row.error_message.unwrap_or_default()})),
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
        }
    }
}

async fn require_profile(db: &PgPool, actor: &ActorContext, profile_id: Uuid) -> ApiResult<Uuid> {
    actor.require("ai:manage")?;
    if actor.has_restricted_inbox_scope() {
        return Err(AppError::Forbidden.into());
    }
    Ok(crate::resource_visibility::owner_project(
        db,
        actor,
        crate::resource_visibility::Resource::Profile,
        profile_id,
    )
    .await?)
}

async fn ensure_settings(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    project: Uuid,
    profile: Uuid,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO ai_api_settings (tenant_id,project_id,ai_profile_id) VALUES ($1,$2,$3) ON CONFLICT (ai_profile_id) DO NOTHING")
        .bind(tenant).bind(project).bind(profile).execute(&mut **tx).await?;
    sqlx::query("SELECT ai_profile_id FROM ai_api_settings WHERE ai_profile_id=$1 FOR UPDATE")
        .bind(profile)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project: Uuid,
    profile: Uuid,
    action: &str,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO audit_log (id,tenant_id,project_id,actor_id,action,resource_kind,resource_id,metadata) VALUES ($1,$2,$3,$4,$5,'ai_profile',$6,'{}'::jsonb)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project).bind(actor.actor_id).bind(action).bind(profile).execute(&mut **tx).await?;
    Ok(())
}

async fn cancel_runs(
    tx: &mut Transaction<'_, Postgres>,
    profile: Uuid,
    endpoint: Option<Uuid>,
    code: &str,
) -> ApiResult<()> {
    sqlx::query("UPDATE ai_api_runs SET status='cancelled',error_code=$3,error_message='API access was disabled or changed',completed_at=now() WHERE ai_profile_id=$1 AND ($2::uuid IS NULL OR endpoint_id=$2) AND status IN ('pending','processing')")
        .bind(profile).bind(endpoint).bind(code).execute(&mut **tx).await?;
    Ok(())
}
