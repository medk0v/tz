use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, auth};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn app(db: &PgPool) -> Router {
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    auth::router().with_state(state)
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Remembered session test');
        INSERT INTO projects (id, tenant_id, name, slug)
            VALUES ('{project}', '{tenant}', 'Project', 'remembered');
        INSERT INTO users (id, email, display_name, password_hash)
            VALUES ('{user}', 'operator@example.test', 'Operator',
            '$argon2id$v=19$m=65536,t=4,p=1$VlFQbFhQdTdXclRUcnRvUA$1/DI8gntCfvU5r3yAYUC/1bK2qldIX0UEHCxsnSEDaU');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
            VALUES ('{tenant}', '{project}', 'operator', 'Operator', 'operator', ARRAY['conversations:read']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{project}', '{user}', 'operator');
        "#,
        tenant = id(1),
        project = id(2),
        user = id(3),
        membership = id(4),
    ))
    .execute(db)
    .await
    .unwrap();
    app(db).await
}

struct Session {
    id: Uuid,
    cookie: String,
    csrf: String,
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    session: Option<&Session>,
    input: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(session) = session {
        builder = builder
            .header(header::COOKIE, &session.cookie)
            .header("x-csrf-token", &session.csrf);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(input.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 100_000).await.unwrap();
    (
        parts.status,
        parts.headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn login(app: &Router, db: &PgPool, remember_me: Option<bool>) -> Session {
    let mut input = json!({"email": "operator@example.test", "password": "tzomet-local-admin"});
    if let Some(value) = remember_me {
        input["remember_me"] = value.into();
    }
    let (status, headers, body) =
        request(app, Method::POST, "/api/v1/auth/login", None, Some(input)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["actor_id"], json!(id(3)));
    assert_eq!(body["project_id"], json!(id(2)));
    assert_eq!(body["auth_method"], "session");
    let expected_max_age = if remember_me == Some(true) {
        7_776_000
    } else {
        43_200
    };
    let cookies: Vec<&str> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|header| {
            let value = header.to_str().unwrap();
            assert!(value.contains(&format!("Max-Age={expected_max_age};")));
            value.split(';').next().unwrap()
        })
        .collect();
    assert_eq!(cookies.len(), 2);
    let token = cookies
        .iter()
        .find_map(|cookie| cookie.strip_prefix("tzomet_session="))
        .unwrap();
    let csrf = cookies
        .iter()
        .find_map(|cookie| cookie.strip_prefix("tzomet_csrf="))
        .unwrap()
        .to_owned();
    let session_id = sqlx::query_scalar("SELECT id FROM operator_sessions WHERE token_hash = $1")
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .fetch_one(db)
        .await
        .unwrap();
    Session {
        id: session_id,
        cookie: cookies.join("; "),
        csrf,
    }
}

async fn deadlines(db: &PgPool, session: &Session) -> (bool, DateTime<Utc>, DateTime<Utc>) {
    sqlx::query_as(
        "SELECT remember_me, idle_expires_at, absolute_expires_at FROM operator_sessions WHERE id = $1",
    )
    .bind(session.id)
    .fetch_one(db)
    .await
    .unwrap()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn remembered_login_restores_and_renews_with_its_persisted_lifetime(db: PgPool) {
    let initial = fixture(&db).await;
    for remember_me in [None, Some(false), Some(true)] {
        let session = login(&initial, &db, remember_me).await;
        let (remembered, idle, absolute) = deadlines(&db, &session).await;
        assert_eq!(remembered, remember_me.unwrap_or(false));
        let (idle_seconds, absolute_seconds) = if remembered {
            (2_592_000, 7_776_000)
        } else {
            (3_600, 43_200)
        };
        assert!(((idle - Utc::now()).num_seconds() - idle_seconds).abs() <= 5);
        assert!(((absolute - Utc::now()).num_seconds() - absolute_seconds).abs() <= 5);

        // A fresh server/router only has the database and the restored cookies.
        // Renewal must use the persisted policy, not the login request in memory.
        sqlx::query("UPDATE operator_sessions SET idle_expires_at = now() + interval '1 minute' WHERE id = $1")
            .bind(session.id).execute(&db).await.unwrap();
        let restored = app(&db).await;
        let (status, _, actor) =
            request(&restored, Method::GET, "/api/v1/me", Some(&session), None).await;
        assert_eq!(status, StatusCode::OK, "{actor}");
        assert_eq!(actor["actor_id"], json!(id(3)));
        let (_, renewed_idle, same_absolute) = deadlines(&db, &session).await;
        assert!(((renewed_idle - Utc::now()).num_seconds() - idle_seconds).abs() <= 5);
        assert_eq!(same_absolute, absolute);

        sqlx::query("UPDATE operator_sessions SET idle_expires_at = now() + interval '1 minute', absolute_expires_at = now() + interval '2 minutes' WHERE id = $1")
            .bind(session.id).execute(&db).await.unwrap();
        assert_eq!(
            request(&restored, Method::GET, "/api/v1/me", Some(&session), None)
                .await
                .0,
            StatusCode::OK
        );
        let (_, capped_idle, capped_absolute) = deadlines(&db, &session).await;
        assert_eq!(capped_idle, capped_absolute);
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn remembered_sessions_keep_expiration_logout_and_live_access_checks(db: PgPool) {
    let app = fixture(&db).await;
    let session = login(&app, &db, Some(true)).await;
    for (invalidate, restore) in [
        (
            "UPDATE operator_sessions SET idle_expires_at = now() - interval '1 second'",
            "UPDATE operator_sessions SET idle_expires_at = now() + interval '1 day'",
        ),
        (
            "UPDATE operator_sessions SET idle_expires_at = now() - interval '1 second', absolute_expires_at = now() - interval '1 second'",
            "UPDATE operator_sessions SET idle_expires_at = now() + interval '1 day', absolute_expires_at = now() + interval '90 days'",
        ),
        (
            "UPDATE operator_sessions SET revoked_at = now()",
            "UPDATE operator_sessions SET revoked_at = NULL",
        ),
        (
            "UPDATE users SET status = 'disabled'",
            "UPDATE users SET status = 'active'",
        ),
        (
            "UPDATE memberships SET revoked_at = now()",
            "UPDATE memberships SET revoked_at = NULL",
        ),
        (
            "UPDATE projects SET status = 'disabled'",
            "UPDATE projects SET status = 'active'",
        ),
    ] {
        sqlx::query(invalidate).execute(&db).await.unwrap();
        assert_eq!(
            request(&app, Method::GET, "/api/v1/me", Some(&session), None)
                .await
                .0,
            StatusCode::UNAUTHORIZED,
            "{invalidate}"
        );
        sqlx::query(restore).execute(&db).await.unwrap();
        assert_eq!(
            request(&app, Method::GET, "/api/v1/me", Some(&session), None)
                .await
                .0,
            StatusCode::OK
        );
    }

    let other_session = login(&app, &db, None).await;
    let (status, headers, _) = request(
        &app,
        Method::POST,
        "/api/v1/auth/logout",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        headers
            .get_all(header::SET_COOKIE)
            .iter()
            .all(|cookie| { cookie.to_str().unwrap().contains("Max-Age=0;") })
    );
    let revoked: bool =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM operator_sessions WHERE id = $1")
            .bind(session.id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(revoked);
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", Some(&session), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", Some(&other_session), None)
            .await
            .0,
        StatusCode::OK
    );
}
