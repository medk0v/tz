use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_settings};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn profile_blacklist_settings_preserve_legacy_updates_and_management_scope(db: PgPool) {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Blacklist settings');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'manager@example.test', 'Manager');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{project}', '{tenant}', 'Project', 'project'),
            ('{other_project}', '{tenant}', 'Other', 'other');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
            VALUES ('{tenant}', '{project}', 'manager', 'Manager', 'manager', ARRAY['ai:manage', 'knowledge:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{user}', '{tenant}', '{project}', '{user}', 'manager');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, created_by)
            VALUES ('{foreign_profile}', '{tenant}', '{other_project}', 'Foreign', '{user}');
        "#,
        tenant = id(1),
        project = id(2),
        user = id(3),
        other_project = id(9),
        foreign_profile = id(10),
    ))
    .execute(&db)
    .await
    .unwrap();
    for (token, permissions, scope) in [
        ("manager", vec!["ai:manage"], None),
        ("unprivileged", vec!["knowledge:manage"], None),
        ("scoped", vec!["ai:manage"], Some(vec![id(99)])),
    ] {
        sqlx::query("INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, inbox_scope, role, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'manager', now() + interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(3)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(permissions).bind(scope)
            .execute(&db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = ai_settings::router().with_state(state);
    let legacy_input = json!({
        "name": "Support", "status": "draft", "language": "en", "max_output_tokens": 800,
        "public_identities": [{"language": "en", "display_name": "Support"}]
    });
    let (status, created) = request(
        &app,
        Method::POST,
        "/api/v1/ai/profiles",
        "manager",
        Some(legacy_input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["blacklist_reply_text"], "");
    assert_eq!(created["blacklist_reply_match_language"], true);
    let profile_id = created["id"].as_str().unwrap();
    let path = format!("/api/v1/ai/profiles/{profile_id}");
    let mut input = legacy_input.clone();
    input["blacklist_reply_text"] = json!("  Blocked for spam. Contact the website email.  ");
    input["blacklist_reply_match_language"] = json!(false);
    let (status, updated) =
        request(&app, Method::PATCH, &path, "manager", Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        updated["blacklist_reply_text"],
        "Blocked for spam. Contact the website email."
    );
    assert_eq!(updated["blacklist_reply_match_language"], false);
    let (status, preserved) =
        request(&app, Method::PATCH, &path, "manager", Some(legacy_input)).await;
    assert_eq!(status, StatusCode::OK, "{preserved}");
    assert_eq!(
        preserved["blacklist_reply_text"],
        updated["blacklist_reply_text"]
    );
    assert_eq!(preserved["blacklist_reply_match_language"], false);
    for (token, target, expected) in [
        ("unprivileged", path.clone(), StatusCode::FORBIDDEN),
        ("scoped", path.clone(), StatusCode::NOT_FOUND),
        (
            "manager",
            format!("/api/v1/ai/profiles/{}", id(10)),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (status, body) =
            request(&app, Method::PATCH, &target, token, Some(input.clone())).await;
        assert_eq!(status, expected, "{token}: {body}");
    }
    input["blacklist_reply_text"] = json!("x".repeat(4_001));
    let (status, body) = request(&app, Method::PATCH, &path, "manager", Some(input.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, listed) = request(&app, Method::GET, "/api/v1/ai/profiles", "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let custom_profiles: Vec<_> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|profile| profile["preset_key"].is_null())
        .collect();
    assert_eq!(custom_profiles.len(), 1);
    assert_eq!(custom_profiles[0]["id"], profile_id);
    assert_eq!(
        custom_profiles[0]["blacklist_reply_text"],
        updated["blacklist_reply_text"]
    );
    assert_eq!(custom_profiles[0]["blacklist_reply_match_language"], false);
    input["blacklist_reply_text"] = json!("");
    input["blacklist_reply_match_language"] = json!(true);
    let (status, cleared) = request(&app, Method::PATCH, &path, "manager", Some(input)).await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["blacklist_reply_text"], "");
    assert_eq!(cleared["blacklist_reply_match_language"], true);
}
