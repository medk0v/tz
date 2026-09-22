use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use tz_backend::{AppState, Config, config::ProductConfig};
use uuid::Uuid;

async fn get(app: Router, path: &str) -> String {
    let response = app
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn public_health_and_api_titles_stay_neutral() {
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.product = ProductConfig {
        edition: None,
        project_id: Some(Uuid::now_v7()),
    };
    config.server.allowed_origins = vec!["https://example.com".into()];
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let state = AppState::build(config).await.unwrap();
    let app = tz_backend::app::router(state.clone());
    let api: serde_json::Value =
        serde_json::from_str(&get(app.clone(), "/health/live").await).unwrap();
    assert_eq!(api["service"], "tz-api");
    let worker: serde_json::Value = serde_json::from_str(
        &get(
            tz_backend::health::worker_router().with_state(state),
            "/health/live",
        )
        .await,
    )
    .unwrap();
    assert_eq!(worker["service"], "tz-worker");

    let schema = get(app.clone(), "/openapi.yaml").await;
    assert!(!schema.to_lowercase().contains("tzomet"));

    let preflight = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/v1/me")
                .header("origin", "https://example.com")
                .header("access-control-request-method", "GET")
                .header("access-control-request-headers", "x-tz-project-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let allowed = preflight
        .headers()
        .get("access-control-allow-headers")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        allowed
            .split(',')
            .any(|name| name.trim() == "x-tz-project-id")
    );
    assert!(
        !allowed
            .split(',')
            .any(|name| name.trim() == "x-tzomet-project-id")
    );
}
