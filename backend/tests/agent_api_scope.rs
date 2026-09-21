use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_api, ai_settings, knowledge};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn api_dependencies_require_project_scope_even_for_visible_inbox_agents(db: PgPool) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','API dependency scope');
        INSERT INTO users (id,email,display_name) VALUES ('{user}','scope@example.test','Manager');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES ('{project}','{tenant}','Project','api-scope');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions)
          VALUES ('{tenant}','{project}','manager','Manager','manager',ARRAY['ai:manage','knowledge:manage']);
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role)
          VALUES ('{user}','{tenant}','{project}','{user}','manager');
        INSERT INTO inboxes (id,tenant_id,project_id,name) VALUES ('{inbox}','{tenant}','{project}','Visible inbox');
        INSERT INTO channel_connections (id,tenant_id,project_id,inbox_id,public_id,kind,name)
          VALUES ('{channel}','{tenant}','{project}','{inbox}','{channel}','widget','Widget');
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,instructions,created_by)
          VALUES ('{profile}','{tenant}','{project}','Agent','Original','{user}');
        INSERT INTO ai_profile_channel_connections (tenant_id,ai_profile_id,channel_connection_id)
          VALUES ('{tenant}','{profile}','{channel}');
        INSERT INTO knowledge_bases (id,tenant_id,project_id,name,created_by)
          VALUES ('{base}','{tenant}','{project}','Knowledge','{user}');
        INSERT INTO ai_profile_knowledge_bases (tenant_id,ai_profile_id,knowledge_base_id)
          VALUES ('{tenant}','{profile}','{base}');
    "#,tenant=id(1),project=id(2),user=id(3),profile=id(4),inbox=id(5),base=id(6),channel=id(7)))
        .execute(&db).await.unwrap();
    for (token, scope) in [("full", None), ("scoped", Some(vec![id(5)]))] {
        sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,inbox_scope,role,expires_at) VALUES ($1,$2,$3,$4,$5,$6,ARRAY['ai:manage','knowledge:manage'],$7,'manager',now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(3)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(scope).execute(&db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db;
    let app = ai_api::router()
        .merge(ai_settings::router())
        .merge(knowledge::router())
        .with_state(state);
    let profile_path = format!("/api/v1/ai/profiles/{}", id(4));
    let base_path = format!("/api/v1/knowledge-bases/{}", id(6));
    let profile = json!({
        "name":"Agent","status":"draft","provider_connection_id":null,"model":null,
        "instructions":"Updated","language":"en","max_output_tokens":512,
        "knowledge_base_ids":[id(6)],"channel_ids":[id(7)],
        "public_identities":[{"language":"en","display_name":"Agent"}]
    });
    let base = json!({"name":"Knowledge","description":"Updated","status":"active"});
    for (path, body) in [(&profile_path, &profile), (&base_path, &base)] {
        let (status, response) =
            request(&app, Method::PATCH, path, "scoped", Some(body.clone())).await;
        assert_eq!(status, StatusCode::OK, "{response}");
    }
    let endpoints_path = format!("{profile_path}/api/endpoints");
    let (status, endpoint) = request(
        &app,
        Method::POST,
        &endpoints_path,
        "full",
        Some(json!({
            "name":"Check","slug":"check","instructions":"Check the supplied address",
            "input_example":{"address":"example"},"output_example":{"empty":true},
            "enabled":false,"timeout_seconds":180
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{endpoint}");
    for (path, body) in [(&profile_path, &profile), (&base_path, &base)] {
        let (status, response) =
            request(&app, Method::PATCH, path, "scoped", Some(body.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{response}");
        let (status, response) =
            request(&app, Method::PATCH, path, "full", Some(body.clone())).await;
        assert_eq!(status, StatusCode::OK, "{response}");
    }
    let (status, _) = request(&app, Method::DELETE, &profile_path, "scoped", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let endpoint_path = format!("{endpoints_path}/{}", endpoint["id"].as_str().unwrap());
    let (status, _) = request(&app, Method::DELETE, &endpoint_path, "full", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, response) = request(&app, Method::PATCH, &base_path, "scoped", Some(base)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
}
