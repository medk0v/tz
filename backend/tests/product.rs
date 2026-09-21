use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, PgPool};
use tower::ServiceExt;
use tz_backend::{
    AppState, Config, auth, bootstrap::create_admin, config::ProductConfig, projects,
};
use uuid::Uuid;

fn config(db: &PgPool) -> Config {
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.product = ProductConfig {
        project_id: Some(Uuid::now_v7()),
    };
    config.pg.url = db.connect_options().to_url_lossy().to_string();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    config
}

fn credentials_path() -> PathBuf {
    std::env::temp_dir().join(format!("tz-bootstrap-test-{}", Uuid::now_v7()))
}

async fn app(db: &PgPool, config: Config) -> Router {
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    auth::router()
        .merge(projects::router())
        .merge(tz_backend::resource_visibility::router())
        .merge(tz_backend::ai_settings::router())
        .merge(tz_backend::ai_tasks::router())
        .with_state(state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    headers: &HeaderMap,
    input: Option<Value>,
) -> (StatusCode, Value, HeaderMap) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    req.headers_mut().unwrap().extend(headers.clone());
    let response = app
        .clone()
        .oneshot(
            req.body(input.map_or_else(Body::empty, |v| Body::from(v.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 100_000).await.unwrap();
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        parts.headers,
    )
}

async fn login(app: &Router, password: &str) -> (Value, HeaderMap) {
    let (status, actor, headers) = request(
        app,
        Method::POST,
        "/api/v1/auth/login",
        &HeaderMap::new(),
        Some(json!({"email":"admin@example.test","password":password})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    let cookies: Vec<_> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap())
        .collect();
    assert!(cookies.iter().any(|v| v.starts_with("tz_session=")));
    let csrf = cookies
        .iter()
        .find_map(|v| v.strip_prefix("tz_csrf="))
        .expect("login must issue the CSRF cookie");
    let mut auth = HeaderMap::new();
    auth.insert(header::COOKIE, cookies.join("; ").parse().unwrap());
    auth.insert("x-csrf-token", csrf.parse().unwrap());
    (actor, auth)
}

async fn bootstrap(db: &PgPool) -> (Config, String, PathBuf) {
    let config = config(db);
    let path = credentials_path();
    assert!(
        create_admin(&config, "admin@example.test", &path)
            .await
            .unwrap()
    );
    let credentials = fs::read_to_string(&path).unwrap();
    let password = credentials
        .lines()
        .find_map(|line| line.strip_prefix("PASSWORD="))
        .unwrap()
        .to_owned();
    (config, password, path)
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn bootstrap_preserves_demo_and_never_overwrites_credentials(db: PgPool) {
    let (config, _, path) = bootstrap(&db).await;
    let original = fs::read(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        !create_admin(&config, "admin@example.test", &path)
            .await
            .unwrap()
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM projects), (SELECT count(*) FROM project_roles WHERE project_id=$1)")
        .bind(config.product.project_id).fetch_one(&db).await.unwrap();
    assert_eq!(counts, (2, 3));
    assert!(
        create_admin(&config, "different@example.test", &credentials_path())
            .await
            .is_err()
    );
    fs::remove_file(path).unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn bootstrap_rolls_back_when_credentials_file_exists(db: PgPool) {
    let config = config(&db);
    let path = credentials_path();
    fs::write(&path, "keep this file").unwrap();
    assert!(
        create_admin(&config, "admin@example.test", &path)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE NOT is_demo")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep this file");
    fs::remove_file(path).unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn login_and_project_routes_keep_the_internal_workspace(db: PgPool) {
    let (config, password, path) = bootstrap(&db).await;
    let fixed = config.product.project_id.unwrap();
    let app = app(&db, config).await;
    let (actor, headers) = login(&app, &password).await;
    assert_eq!(actor["project_id"], json!(fixed));
    assert!(actor["department_id"].is_string());
    let (status, list, _) = request(&app, Method::GET, "/api/v1/projects", &headers, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["id"], json!(fixed));
    for (method, route, body) in [
        (
            Method::POST,
            "/api/v1/projects".to_owned(),
            json!({"name":"Extra", "slug":"extra", "inbox_name":"Extra"}),
        ),
        (
            Method::POST,
            "/api/v1/auth/project".to_owned(),
            json!({"project_id":Uuid::now_v7()}),
        ),
        (
            Method::PATCH,
            format!("/api/v1/projects/{fixed}"),
            json!({"status":"disabled"}),
        ),
        (
            Method::PATCH,
            format!("/api/v1/projects/{}", Uuid::now_v7()),
            json!({"director_enabled":false}),
        ),
    ] {
        let (status, value, _) = request(&app, method, &route, &headers, Some(body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{route}: {value}");
    }
    let (status, value, _) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/projects/{fixed}"),
        &headers,
        Some(json!({"director_enabled":false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let mut other_header = headers.clone();
    other_header.insert(
        "x-tz-project-id",
        Uuid::now_v7().to_string().parse().unwrap(),
    );
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", &other_header, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            Method::GET,
            "/api/v1/auth/demo",
            &HeaderMap::new(),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/auth/login",
            &HeaderMap::new(),
            Some(json!({"email":tz_backend::demo::DEMO_EMAIL,"password":"demo"}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    fs::remove_file(path).unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn speech_connections_and_agent_voice_are_not_offered(db: PgPool) {
    let (config, password, path) = bootstrap(&db).await;
    let app = app(&db, config).await;
    let (_, headers) = login(&app, &password).await;
    let provider = |model_type: &str| {
        json!({"name": format!("{model_type} model"), "provider_kind": "openai_compatible",
            "model_type": model_type, "base_url": "https://models.example.test/v1",
            "default_model": "model", "status": "active"})
    };
    for model_type in ["speech_to_text", "text_to_speech"] {
        let (status, body, _) = request(
            &app,
            Method::POST,
            "/api/v1/ai/providers",
            &headers,
            Some(provider(model_type)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (status, chat, _) = request(
        &app,
        Method::POST,
        "/api/v1/ai/providers",
        &headers,
        Some(provider("chat")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{chat}");
    assert_eq!(chat["model_type"], "chat");
    let (status, body, _) = request(
        &app,
        Method::POST,
        "/api/v1/ai/profiles",
        &headers,
        Some(
            json!({"name":"Voice agent","status":"draft","language":"ru","max_output_tokens":800,
            "public_identities":[{"language":"ru","display_name":"Агент"}],
            "voice":{"speech_to_text_connection_id": chat["id"]}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    fs::remove_file(path).unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn the_task_calendar_is_not_offered(db: PgPool) {
    let (config, password, path) = bootstrap(&db).await;
    let app = app(&db, config).await;
    let (_, headers) = login(&app, &password).await;
    let (status, body, _) = request(
        &app,
        Method::GET,
        "/api/v1/ai/task-calendar?from=2026-09-01T00:00:00Z&to=2026-09-30T00:00:00Z",
        &headers,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body, _) = request(&app, Method::GET, "/api/v1/ai/tasks", &headers, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    fs::remove_file(path).unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn a_task_duration_is_not_offered(db: PgPool) {
    let (config, password, path) = bootstrap(&db).await;
    let app = app(&db, config).await;
    let (_, headers) = login(&app, &password).await;
    let base = json!({"text": "Draft the report", "schedule": {"kind": "manual"}, "agent_ids": []});
    let with = |extra: Value| {
        let mut body = base.clone();
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };

    for extra in [
        json!({"starts_at": "2026-10-01T07:00:00Z", "due_at": "2026-10-01T09:00:00Z"}),
        json!({"all_day": true}),
        json!({"time_zone": "Europe/Istanbul"}),
        json!({"reminder_minutes": 30}),
    ] {
        let (status, body, _) = request(
            &app,
            Method::POST,
            "/api/v1/ai/tasks",
            &headers,
            Some(with(extra)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    // Explicit nulls must not trip the gate: a client may send fields it never sets.
    let (plain, plain_body, _) = request(
        &app,
        Method::POST,
        "/api/v1/ai/tasks",
        &headers,
        Some(base.clone()),
    )
    .await;
    let (nulled, nulled_body, _) = request(
        &app,
        Method::POST,
        "/api/v1/ai/tasks",
        &headers,
        Some(with(
            json!({"starts_at": null, "due_at": null, "all_day": false, "time_zone": null}),
        )),
    )
    .await;
    assert_eq!(nulled, plain, "{nulled_body} differs from {plain_body}");

    // The bell is standard-edition only, so its routes are not there to poll.
    for (method, route) in [
        (Method::GET, "/api/v1/task-reminders"),
        (Method::POST, "/api/v1/task-reminders/read"),
    ] {
        let body = (method == Method::POST).then(|| json!({"ids": []}));
        let (status, body, _) = request(&app, method, route, &headers, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{route}: {body}");
    }
    fs::remove_file(path).unwrap();
}
