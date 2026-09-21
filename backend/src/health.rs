//! Liveness and readiness endpoints.

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get,
};
use serde::Serialize;

use crate::AppState;

/// High-level process health.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Up,
    Degraded,
}

/// Health of one dependency.
#[derive(Debug, Serialize)]
pub struct DependencyStatus {
    status: HealthStatus,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Public health response shared with the frontend.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    service: &'static str,
    status: HealthStatus,
    dependencies: BTreeMap<&'static str, DependencyStatus>,
}

#[derive(Clone, Copy)]
struct ServiceName(&'static str);

pub fn router() -> Router<AppState> {
    router_for(ServiceName("tz-api"))
}

pub fn worker_router() -> Router<AppState> {
    router_for(ServiceName("tz-worker"))
}

fn router_for(service: ServiceName) -> Router<AppState> {
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .layer(Extension(service))
}

async fn liveness(
    State(_state): State<AppState>,
    Extension(service): Extension<ServiceName>,
) -> Json<HealthResponse> {
    Json(HealthResponse {
        service: service.0,
        status: HealthStatus::Up,
        dependencies: BTreeMap::new(),
    })
}

async fn readiness(
    State(state): State<AppState>,
    Extension(service): Extension<ServiceName>,
) -> impl IntoResponse {
    let mut dependencies = BTreeMap::new();
    let postgres_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    dependencies.insert(
        "postgres",
        DependencyStatus {
            status: if postgres_ok {
                HealthStatus::Up
            } else {
                HealthStatus::Degraded
            },
            required: true,
            detail: (!postgres_ok).then(|| "connection failed".to_owned()),
        },
    );

    let mut required_dependencies_ok = postgres_ok;
    if let Some(client) = &state.redis {
        let redis_ok = match client.get_multiplexed_async_connection().await {
            Ok(mut connection) => redis::cmd("PING")
                .query_async::<String>(&mut connection)
                .await
                .is_ok(),
            Err(_) => false,
        };
        if state.config.redis.required {
            required_dependencies_ok &= redis_ok;
        }
        dependencies.insert(
            "redis",
            DependencyStatus {
                status: if redis_ok {
                    HealthStatus::Up
                } else {
                    HealthStatus::Degraded
                },
                required: state.config.redis.required,
                detail: (!redis_ok).then(|| "connection failed".to_owned()),
            },
        );
    }

    let status = if required_dependencies_ok {
        HealthStatus::Up
    } else {
        HealthStatus::Degraded
    };
    let http_status = if required_dependencies_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        http_status,
        Json(HealthResponse {
            service: service.0,
            status,
            dependencies,
        }),
    )
}
