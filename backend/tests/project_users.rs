use std::net::SocketAddr;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, auth, demo, projects};
use uuid::Uuid;

const PASSWORD: &str = "project-user-password";
const ADMIN_PERMISSIONS: &[&str] = &[
    "projects:read",
    "projects:manage",
    "conversations:read",
    "conversations:reply",
    "conversations:close",
    "contacts:read",
    "contacts:manage",
    "channels:read",
    "channels:manage",
    "access_tokens:manage",
    "visitor_network:read",
    "quality:read",
    "quality:read_all",
    "reviews:read",
    "routing:manage",
    "teams:manage",
    "ai:manage",
    "knowledge:manage",
    "reply_templates:manage",
    "processes:read",
    "processes:edit",
    "processes:approve",
    "tasks:own",
    "tasks:manage",
    "tasks:configure",
    "integrations:manage",
    "roles:manage",
    "system:read",
];

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

struct Session {
    cookie: String,
    csrf: String,
}

fn admin() -> Session {
    Session {
        cookie: "tzomet_session=users-admin; tzomet_csrf=users-csrf".to_owned(),
        csrf: "users-csrf".to_owned(),
    }
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Users test'), ('{foreign_tenant}', 'Other tenant');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{project}', '{tenant}', 'Project', 'users'),
            ('{other_project}', '{tenant}', 'Other project', 'other'),
            ('{foreign_project}', '{foreign_tenant}', 'Foreign project', 'foreign');
        INSERT INTO users (id, email, display_name) VALUES ('{admin}', 'admin@users.test', 'Admin');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, is_system, permissions) VALUES
            ('{tenant}', '{project}', 'operator', 'Operator', 'operator', true, ARRAY['projects:read','conversations:read']),
            ('{tenant}', '{project}', 'support', 'Support', 'manager', false, ARRAY['projects:read','contacts:read']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{project}', '{admin}', 'admin');
        INSERT INTO departments (id, tenant_id, project_id, name)
            VALUES ('{department}', '{tenant}', '{project}', 'Support');
        INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id, token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at)
            VALUES ('{session}', '{tenant}', '{project}', '{admin}', '{membership}', decode('{token_hash:x}', 'hex'), decode('{csrf_hash:x}', 'hex'), now()+interval '1 hour', now()+interval '2 hours');
        INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, role, expires_at)
            VALUES ('{token}', '{tenant}', '{project}', '{admin}', 'Admin token', decode('{api_hash:x}', 'hex'), ARRAY['projects:read','projects:manage'], 'admin', now()+interval '1 hour');
        "#,
        tenant=id(1), project=id(2), admin=id(3), membership=id(4), session=id(5), department=id(6),
        other_project=id(7), foreign_tenant=id(8), foreign_project=id(9), token=id(10),
        token_hash=Sha256::digest(b"users-admin"), csrf_hash=Sha256::digest(b"users-csrf"), api_hash=Sha256::digest(b"users-api"),
    )).execute(db).await.unwrap();
    sqlx::query("INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, is_system, permissions) VALUES ($1, $2, 'admin', 'Admin', 'admin', true, $3)")
        .bind(id(1))
        .bind(id(2))
        .bind(ADMIN_PERMISSIONS)
        .execute(db)
        .await
        .unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    projects::router().merge(auth::router()).with_state(state)
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
        .extension(ConnectInfo(
            "203.0.113.1:8080".parse::<SocketAddr>().unwrap(),
        ))
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

fn input(email: &str) -> Value {
    json!({"display_name":" New operator ","email":email,"password":PASSWORD,"role_id":"operator"})
}

async fn create(app: &Router, input: Value) -> (StatusCode, Value) {
    let (status, _, body) = request(
        app,
        Method::POST,
        &format!("/api/v1/projects/{}/users", id(2)),
        Some(&admin()),
        Some(input),
    )
    .await;
    (status, body)
}

async fn login(app: &Router, email: &str, password: &str) -> (Session, Value) {
    let (status, headers, body) = request(
        app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({"email":email,"password":password})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let cookies: Vec<_> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().split(';').next().unwrap())
        .collect();
    let csrf = cookies
        .iter()
        .find_map(|cookie| cookie.strip_prefix("tzomet_csrf="))
        .unwrap()
        .to_owned();
    (
        Session {
            cookie: cookies.join("; "),
            csrf,
        },
        body,
    )
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn created_user_can_login_and_permissions_follow_membership_changes(db: PgPool) {
    let app = fixture(&db).await;
    let (status, member) = create(&app, input(" New@Users.Test ")).await;
    assert_eq!(status, StatusCode::CREATED, "{member}");
    assert_eq!(member["email"], "new@users.test");
    assert_eq!(member["display_name"], "New operator");
    assert!(member.get("password").is_none());
    assert!(member.get("password_hash").is_none());
    let stored: (String, String) =
        sqlx::query_as("SELECT status, password_hash FROM users WHERE email='new@users.test'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(stored.0, "active");
    assert!(stored.1.starts_with("$argon2id$"));
    assert_ne!(stored.1, PASSWORD);
    let (session, actor) = login(&app, "new@users.test", PASSWORD).await;
    assert_eq!(actor["actor_id"], member["user_id"]);
    assert_eq!(actor["project_id"], json!(id(2)));
    assert_eq!(
        actor["permissions"],
        json!(["projects:read", "conversations:read"])
    );
    let path = format!(
        "/api/v1/projects/{}/members/{}",
        id(2),
        member["membership_id"].as_str().unwrap()
    );
    let (status, _, updated) = request(
        &app,
        Method::PATCH,
        &path,
        Some(&admin()),
        Some(json!({"role_id":"support","department_id":id(6)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let (status, _, actor) = request(&app, Method::GET, "/api/v1/me", Some(&session), None).await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["role_id"], "support");
    assert_eq!(
        actor["permissions"],
        json!(["projects:read", "contacts:read"])
    );
    assert_eq!(actor["department_id"], json!(id(6)));
    let (status, _, body) = request(&app, Method::DELETE, &path, Some(&admin()), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", Some(&session), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let audit: Value =
        sqlx::query_scalar("SELECT metadata FROM audit_log WHERE action='project.user_created'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(audit["user_id"], member["user_id"]);
    assert!(!audit.to_string().contains(PASSWORD));

    let mut admin_input = input("new-admin@users.test");
    admin_input["role_id"] = json!("admin");
    admin_input["director_access"] = json!(true);
    let (status, member) = create(&app, admin_input).await;
    assert_eq!(status, StatusCode::CREATED, "{member}");
    let (_, actor) = login(&app, "new-admin@users.test", PASSWORD).await;
    assert_eq!(actor["role"], "admin");
    let permissions = actor["permissions"].as_array().unwrap();
    assert_eq!(permissions.len(), ADMIN_PERMISSIONS.len());
    for permission in ADMIN_PERMISSIONS {
        assert!(
            permissions.contains(&json!(permission)),
            "{permission}: {actor}"
        );
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn duplicate_and_concurrent_creates_preserve_existing_accounts(db: PgPool) {
    let app = fixture(&db).await;
    let (first, second) = tokio::join!(
        create(&app, input("RACE@users.test")),
        create(&app, input("race@users.test"))
    );
    assert!(
        [first.0, second.0].contains(&StatusCode::CREATED),
        "{first:?} {second:?}"
    );
    assert!(
        [first.0, second.0].contains(&StatusCode::CONFLICT),
        "{first:?} {second:?}"
    );
    let before: (Uuid, String, String) = sqlx::query_as(
        "SELECT id, display_name, password_hash FROM users WHERE email='race@users.test'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    let mut duplicate = input(" RACE@users.test ");
    duplicate["password"] = json!("replacement-password");
    duplicate["display_name"] = json!("Replaced");
    duplicate["role_id"] = json!("admin");
    assert_eq!(create(&app, duplicate).await.0, StatusCode::CONFLICT);
    let after = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, display_name, password_hash FROM users WHERE email='race@users.test'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(before, after);
    let grants: Vec<String> = sqlx::query_scalar("SELECT role FROM memberships WHERE user_id=$1")
        .bind(before.0)
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(grants, ["operator"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE action='project.user_created'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
    sqlx::query("INSERT INTO users (id,email,display_name,password_hash) VALUES ($1,'external@users.test','External','preserve-external')").bind(id(20)).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO memberships (id,tenant_id,project_id,user_id,role) VALUES ($1,$2,$3,$4,'operator')").bind(id(21)).bind(id(8)).bind(id(9)).bind(id(20)).execute(&db).await.unwrap();
    assert_eq!(
        create(&app, input("external@users.test")).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM memberships WHERE tenant_id=$1 AND user_id=$2"
        )
        .bind(id(1))
        .bind(id(20))
        .fetch_one(&db)
        .await
        .unwrap(),
        0
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn create_rejects_invalid_identity_role_and_department_without_partial_users(db: PgPool) {
    let app = fixture(&db).await;
    for (field, value, expected) in [
        ("password", json!("short"), StatusCode::BAD_REQUEST),
        ("password", json!("я".repeat(65)), StatusCode::BAD_REQUEST),
        ("display_name", json!("   "), StatusCode::BAD_REQUEST),
        ("email", json!("invalid"), StatusCode::BAD_REQUEST),
        ("role_id", json!("missing"), StatusCode::NOT_FOUND),
        ("department_id", json!(id(99)), StatusCode::BAD_REQUEST),
    ] {
        let mut invalid = input("invalid@users.test");
        invalid[field] = value;
        let (status, body) = create(&app, invalid).await;
        assert_eq!(status, expected, "{field}: {body}");
    }
    for (role, director) in [("admin", false), ("operator", true)] {
        let mut invalid = input("invalid@users.test");
        invalid["role_id"] = json!(role);
        invalid["department_id"] = json!(id(6));
        invalid["director_access"] = json!(director);
        assert_eq!(create(&app, invalid).await.0, StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users WHERE email='invalid@users.test'")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    let mut valid = input("scoped@users.test");
    valid["role_id"] = json!("support");
    valid["department_id"] = json!(id(6));
    let (status, body) = create(&app, valid).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["role_id"], "support");
    assert_eq!(body["department_id"], json!(id(6)));
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn user_creation_requires_non_demo_admin_session_for_target_project(db: PgPool) {
    let app = fixture(&db).await;
    let path = format!("/api/v1/projects/{}/users", id(2));
    assert_eq!(
        request(
            &app,
            Method::POST,
            &path,
            None,
            Some(input("denied@users.test"))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(&path)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer users-api")
                .body(Body::from(input("denied@users.test").to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    for project in [id(7), id(9)] {
        let (status, _, body) = request(
            &app,
            Method::POST,
            &format!("/api/v1/projects/{project}/users"),
            Some(&admin()),
            Some(input("denied@users.test")),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    let (demo_session, _) = login(&app, demo::DEMO_EMAIL, demo::DEMO_PASSWORD).await;
    assert_eq!(
        request(
            &app,
            Method::POST,
            &format!("/api/v1/projects/{}/users", demo::DEMO_PROJECT_ID),
            Some(&demo_session),
            Some(input("denied@users.test"))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for role in ["support", "operator"] {
        sqlx::query("UPDATE memberships SET role=$1 WHERE id=$2")
            .bind(role)
            .bind(id(4))
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            create(&app, input("denied@users.test")).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users WHERE email='denied@users.test'")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
}
