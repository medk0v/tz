use std::time::Duration;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{
    AppState, Config, auth, conversations, inbox_routing, operator, operator_presence, projects,
    realtime,
};
use uuid::Uuid;

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

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Token projects'), ('{foreign_tenant}', 'Foreign');
        INSERT INTO projects (id, tenant_id, name, slug, status) VALUES
            ('{source}', '{tenant}', 'Source', 'source', 'active'),
            ('{target}', '{tenant}', 'Target', 'target', 'active'),
            ('{disabled}', '{tenant}', 'Disabled', 'disabled', 'disabled'),
            ('{foreign}', '{foreign_tenant}', 'Foreign', 'foreign', 'active');
        INSERT INTO users (id, email, display_name) VALUES
            ('{global_admin}', 'global@token-projects.test', 'Global admin'),
            ('{project_admin}', 'project@token-projects.test', 'Project admin');
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role) VALUES
            ('{global_membership}', '{tenant}', NULL, '{global_admin}', 'admin'),
            ('{project_membership}', '{tenant}', '{source}', '{project_admin}', 'admin');
        INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id,
            token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at) VALUES
            ('{global_session}', '{tenant}', '{source}', '{global_admin}', '{global_membership}',
             decode('{global_hash:x}', 'hex'), decode('{csrf_hash:x}', 'hex'), now()+interval '1 hour', now()+interval '2 hours'),
            ('{project_session}', '{tenant}', '{source}', '{project_admin}', '{project_membership}',
             decode('{project_hash:x}', 'hex'), decode('{csrf_hash:x}', 'hex'), now()+interval '1 hour', now()+interval '2 hours');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions) VALUES
            ('{tenant}', '{source}', 'support', 'Source support', 'manager',
                ARRAY['projects:read','conversations:read','conversations:reply','visitor_network:read']),
            ('{tenant}', '{target}', 'support', 'Target support', 'operator', ARRAY['projects:read','channels:read']),
            ('{tenant}', '{source}', 'source_only', 'Source only', 'operator', ARRAY['projects:read','conversations:read','conversations:reply']);
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
            ('{source_inbox}', '{tenant}', '{source}', 'Source inbox'),
            ('{source_other_inbox}', '{tenant}', '{source}', 'Other source inbox'),
            ('{target_inbox}', '{tenant}', '{target}', 'Target inbox');
        "#,
        tenant=id(1), source=id(2), target=id(3), disabled=id(4), foreign_tenant=id(5), foreign=id(6),
        global_admin=id(10), project_admin=id(11), global_membership=id(20), project_membership=id(21),
        global_session=id(30), project_session=id(31), source_inbox=id(40), source_other_inbox=id(41), target_inbox=id(42),
        global_hash=Sha256::digest(b"global-admin"), project_hash=Sha256::digest(b"project-admin"), csrf_hash=Sha256::digest(b"token-csrf"),
    )).execute(db).await.unwrap();
    for project_id in [id(2), id(3), id(4)] {
        sqlx::query("INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, is_system, permissions) VALUES ($1, $2, 'admin', 'Admin', 'admin', true, $3)")
            .bind(id(1)).bind(project_id).bind(ADMIN_PERMISSIONS).execute(db).await.unwrap();
    }
    auth::router()
        .merge(projects::router())
        .merge(operator::router())
        .merge(operator_presence::router())
        .merge(conversations::router())
        .merge(realtime::router())
        .with_state(test_state(db).await)
}

async fn test_state(db: &PgPool) -> AppState {
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    state
}

enum Credential<'a> {
    Session(&'a str),
    Token(&'a str),
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    credential: Credential<'_>,
    selected_project: Option<&str>,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    builder = match credential {
        Credential::Session(token) => builder
            .header(
                header::COOKIE,
                format!("tzomet_session={token}; tzomet_csrf=token-csrf"),
            )
            .header("x-csrf-token", "token-csrf"),
        Credential::Token(token) => {
            builder.header(header::AUTHORIZATION, format!("Bearer {token}"))
        }
    };
    if let Some(project_id) = selected_project {
        builder = builder.header("X-Tzomet-Project-Id", project_id);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 100_000).await.unwrap();
    (
        parts.status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn token_input(scope: &str, role: &str) -> Value {
    json!({"name":"Workspace token","role_id":role,"expires_in_days":7,"project_scope":scope})
}

async fn create(app: &Router, admin: &str, input: Value) -> Value {
    let (status, body) = request(
        app,
        Method::POST,
        "/api/v1/access-tokens",
        Credential::Session(admin),
        None,
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn actor(app: &Router, secret: &str, project_id: Option<Uuid>) -> (StatusCode, Value) {
    let project_id = project_id.map(|value| value.to_string());
    request(
        app,
        Method::GET,
        "/api/v1/me",
        Credential::Token(secret),
        project_id.as_deref(),
        None,
    )
    .await
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn fixed_tokens_reject_scope_changes_and_invalid_project_headers(db: PgPool) {
    let app = fixture(&db).await;
    let token = create(&app, "project-admin", token_input("project", "support")).await;
    let secret = token["secret"].as_str().unwrap();
    assert_eq!(token["project_scope"], "project");
    assert_eq!(token["project_id"], json!(id(2)));
    assert_eq!(token["role_project_id"], json!(id(2)));
    let (status, current) = actor(&app, secret, None).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["project_id"], json!(id(2)));
    assert_eq!(current["access_token_project_scope"], "project");
    assert_eq!(actor(&app, secret, Some(id(2))).await.0, StatusCode::OK);
    for project in [id(3), id(4), id(6), id(999)] {
        let (status, body) = actor(&app, secret, Some(project)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{project}: {body}");
    }
    let (status, body) = request(
        &app,
        Method::GET,
        "/api/v1/me",
        Credential::Token(secret),
        Some("not-a-uuid"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, projects) = request(
        &app,
        Method::GET,
        "/api/v1/projects",
        Credential::Token(secret),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{projects}");
    assert_eq!(projects["items"].as_array().unwrap().len(), 1);
    assert_eq!(projects["items"][0]["id"], json!(id(2)));
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn all_project_tokens_keep_live_source_role_instead_of_target_role(db: PgPool) {
    let app = fixture(&db).await;
    let token = create(&app, "global-admin", token_input("all", "support")).await;
    let secret = token["secret"].as_str().unwrap();
    assert_eq!(token["project_scope"], "all");
    assert!(token["project_id"].is_null());
    assert_eq!(token["role_project_id"], json!(id(2)));
    assert_eq!(
        actor(&app, secret, None).await.1["project_id"],
        json!(id(2))
    );
    let (status, selected) = actor(&app, secret, Some(id(3))).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    assert_eq!(selected["project_id"], json!(id(3)));
    assert_eq!(selected["access_token_project_scope"], "all");
    assert_eq!(selected["role"], "manager");
    assert_eq!(selected["role_name"], "Source support");
    assert!(
        selected["permissions"]
            .as_array()
            .unwrap()
            .contains(&json!("conversations:reply"))
    );
    assert!(
        !selected["permissions"]
            .as_array()
            .unwrap()
            .contains(&json!("channels:read"))
    );
    let target = id(3).to_string();
    let (status, inboxes) = request(
        &app,
        Method::GET,
        "/api/v1/inboxes",
        Credential::Token(secret),
        Some(&target),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inboxes}");
    assert_eq!(inboxes["items"].as_array().unwrap().len(), 1);
    assert_eq!(inboxes["items"][0]["id"], json!(id(42)));
    let (status, projects) = request(
        &app,
        Method::GET,
        "/api/v1/projects",
        Credential::Token(secret),
        Some(&target),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{projects}");
    let ids: Vec<_> = projects["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].clone())
        .collect();
    assert_eq!(ids.len(), 2, "{projects}");
    assert!(ids.contains(&json!(id(2))) && ids.contains(&json!(id(3))));
    for project in [id(4), id(6), id(999)] {
        assert_eq!(
            actor(&app, secret, Some(project)).await.0,
            StatusCode::FORBIDDEN
        );
    }

    let (status, body) = request(&app, Method::PATCH, "/api/v1/roles/support", Credential::Session("global-admin"), None,
        Some(json!({"name":"Updated source","base_role":"operator","permissions":["projects:read","contacts:read"]}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, selected) = actor(&app, secret, Some(id(3))).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    assert_eq!(selected["role"], "operator");
    assert_eq!(
        selected["permissions"],
        json!(["projects:read", "contacts:read"])
    );
    assert_eq!(
        request(
            &app,
            Method::GET,
            "/api/v1/inboxes",
            Credential::Token(secret),
            Some(&target),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/auth/project",
        Credential::Session("global-admin"),
        None,
        Some(json!({"project_id":id(3)})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = request(&app, Method::PATCH, "/api/v1/roles/support", Credential::Session("global-admin"), None,
        Some(json!({"name":"Changed target","base_role":"manager","permissions":["projects:read","ai:manage"]}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, selected) = actor(&app, secret, Some(id(3))).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    assert_eq!(
        selected["permissions"],
        json!(["projects:read", "contacts:read"])
    );
    let mut input = token_input("all", "source_only");
    input["project_id"] = json!(id(2));
    let source_only = create(&app, "global-admin", input).await;
    assert_eq!(
        actor(&app, source_only["secret"].as_str().unwrap(), Some(id(3)))
            .await
            .0,
        StatusCode::OK
    );
    let (status, inboxes) = request(
        &app,
        Method::GET,
        "/api/v1/inboxes",
        Credential::Token(source_only["secret"].as_str().unwrap()),
        Some(&target),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inboxes}");
    assert_eq!(inboxes["items"].as_array().unwrap().len(), 1);
    assert_eq!(inboxes["items"][0]["id"], json!(id(42)));
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn only_tenant_admins_can_issue_or_manage_all_project_tokens(db: PgPool) {
    let app = fixture(&db).await;
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/access-tokens",
        Credential::Session("project-admin"),
        None,
        Some(token_input("all", "support")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/access-tokens",
        Credential::Session("global-admin"),
        None,
        Some(token_input("all", "admin")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM api_keys WHERE tenant_id=$1")
            .bind(id(1))
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    let all = create(&app, "global-admin", token_input("all", "support")).await;
    for (admin, can_create, expected_count) in
        [("global-admin", true, 1), ("project-admin", false, 0)]
    {
        let (status, list) = request(
            &app,
            Method::GET,
            "/api/v1/access-tokens",
            Credential::Session(admin),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["can_create_all_projects"], can_create);
        assert_eq!(list["items"].as_array().unwrap().len(), expected_count);
    }
    let token_id = all["id"].as_str().unwrap();
    for (method, path) in [
        (
            Method::POST,
            format!("/api/v1/access-tokens/{token_id}/revoke"),
        ),
        (Method::DELETE, format!("/api/v1/access-tokens/{token_id}")),
    ] {
        let (status, body) = request(
            &app,
            method,
            &path,
            Credential::Session("project-admin"),
            None,
            None,
        )
        .await;
        assert!(
            matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
            "{body}"
        );
    }
    let secret = all["secret"].as_str().unwrap();
    assert_eq!(actor(&app, secret, Some(id(3))).await.0, StatusCode::OK);
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/access-tokens",
            Credential::Token(secret),
            None,
            Some(token_input("all", "support"))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let role_path = format!("/api/v1/roles?project_id={}", id(3));
    let (status, roles) = request(
        &app,
        Method::GET,
        &role_path,
        Credential::Session("global-admin"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{roles}");
    assert!(
        roles["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["name"] == "Target support")
    );
    assert!(
        request(
            &app,
            Method::GET,
            &role_path,
            Credential::Session("project-admin"),
            None,
            None
        )
        .await
        .0
        .is_client_error()
    );
    let mut target_input = token_input("project", "support");
    target_input["project_id"] = json!(id(3));
    let target_token = create(&app, "global-admin", target_input.clone()).await;
    assert_eq!(target_token["project_id"], json!(id(3)));
    assert_eq!(target_token["role_name"], "Target support");
    assert!(
        request(
            &app,
            Method::POST,
            "/api/v1/access-tokens",
            Credential::Session("project-admin"),
            None,
            Some(target_input)
        )
        .await
        .0
        .is_client_error()
    );
}

async fn websocket_upgrade(address: std::net::SocketAddr, ticket: &str) -> reqwest::Response {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .get(format!("http://{address}/ws"))
        .query(&[("ticket", ticket)])
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn realtime_tickets_preserve_selected_project_and_recheck_revocation(db: PgPool) {
    let app = fixture(&db).await;
    let token = create(&app, "global-admin", token_input("all", "support")).await;
    let secret = token["secret"].as_str().unwrap();
    let target = id(3).to_string();
    let mut tickets = Vec::new();
    for _ in 0..2 {
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/realtime-tickets",
            Credential::Token(secret),
            Some(&target),
            Some(json!({"inbox_id":id(42)})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        tickets.push(body["ticket"].as_str().unwrap().to_owned());
    }
    let scopes: Vec<(Option<Uuid>, Option<Vec<Uuid>>)> =
        sqlx::query_as("SELECT project_id,inbox_scope FROM realtime_tickets WHERE tenant_id=$1")
            .bind(id(1))
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(scopes, vec![(Some(id(3)), Some(vec![id(42)])); 2]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let serving_app = app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, serving_app).await.unwrap() });
    let connected = websocket_upgrade(address, &tickets[0]).await;
    assert_eq!(connected.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);
    drop(connected);
    let revoke = format!(
        "/api/v1/access-tokens/{}/revoke",
        token["id"].as_str().unwrap()
    );
    let (status, body) = request(
        &app,
        Method::POST,
        &revoke,
        Credential::Session("global-admin"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(
        websocket_upgrade(address, &tickets[1]).await.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        actor(&app, secret, Some(id(3))).await.0,
        StatusCode::UNAUTHORIZED
    );
    server.abort();
    let _ = server.await;
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn legacy_tokens_keep_their_original_inbox_restriction(db: PgPool) {
    let app = fixture(&db).await;
    let token = create(&app, "project-admin", token_input("project", "support")).await;
    let token_id = Uuid::parse_str(token["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE api_keys SET role_project_id=NULL,inbox_scope=$2 WHERE id=$1")
        .bind(token_id)
        .bind(vec![id(40)])
        .execute(&db)
        .await
        .unwrap();
    let secret = token["secret"].as_str().unwrap();
    let (status, current) = actor(&app, secret, None).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["project_id"], json!(id(2)));
    assert_eq!(current["inbox_scope"], json!([id(40)]));
    let (status, inboxes) = request(
        &app,
        Method::GET,
        "/api/v1/inboxes",
        Credential::Token(secret),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inboxes}");
    assert_eq!(inboxes["items"].as_array().unwrap().len(), 1);
    assert_eq!(inboxes["items"][0]["id"], json!(id(40)));
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/realtime-tickets",
        Credential::Token(secret),
        None,
        Some(json!({"inbox_id":id(41)})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn all_project_presence_uses_live_source_permissions_and_source_status(db: PgPool) {
    let app = fixture(&db).await;
    let token = create(&app, "global-admin", token_input("all", "source_only")).await;
    let target = id(3).to_string();
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/operator-presence",
        Credential::Token(token["secret"].as_str().unwrap()),
        Some(&target),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(
        operator_presence::inbox_has_online_operator(&db, id(1), id(3), id(42))
            .await
            .unwrap()
    );
    sqlx::query("UPDATE project_roles SET permissions=ARRAY['projects:read','conversations:read'] WHERE tenant_id=$1 AND project_id=$2 AND id='source_only'")
        .bind(id(1)).bind(id(2)).execute(&db).await.unwrap();
    assert!(
        !operator_presence::inbox_has_online_operator(&db, id(1), id(3), id(42))
            .await
            .unwrap()
    );
    sqlx::query("UPDATE project_roles SET permissions=ARRAY['projects:read','conversations:read','conversations:reply'] WHERE tenant_id=$1 AND project_id=$2 AND id='source_only'")
        .bind(id(1)).bind(id(2)).execute(&db).await.unwrap();
    assert!(
        operator_presence::inbox_has_online_operator(&db, id(1), id(3), id(42))
            .await
            .unwrap()
    );
    sqlx::query("UPDATE projects SET status='disabled' WHERE id=$1")
        .bind(id(2))
        .execute(&db)
        .await
        .unwrap();
    assert!(
        !operator_presence::inbox_has_online_operator(&db, id(1), id(3), id(42))
            .await
            .unwrap()
    );
    assert_eq!(
        actor(&app, token["secret"].as_str().unwrap(), Some(id(3)))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

async fn route_inbound(db: &PgPool, state: &AppState) -> inbox_routing::InboundDecision {
    let mut transaction = db.begin().await.unwrap();
    let decision = inbox_routing::on_inbound_message(
        state,
        &mut transaction,
        &inbox_routing::InboundContext {
            tenant_id: id(1),
            project_id: id(3),
            inbox_id: id(42),
            conversation_id: id(52),
            channel_connection_id: id(50),
            widget_language: None,
        },
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    decision
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn joined_all_project_operator_survives_routing_until_source_access_is_lost(db: PgPool) {
    let app = fixture(&db).await;
    let state = test_state(&db).await;
    let token = create(&app, "global-admin", token_input("all", "source_only")).await;
    let secret = token["secret"].as_str().unwrap();
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name, status)
            VALUES ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Target widget', 'active');
        INSERT INTO contacts (id, tenant_id, project_id) VALUES ('{contact}', '{tenant}', '{project}');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, status, last_message_sequence)
            VALUES ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}', 'open', 1);
        INSERT INTO inbox_routing_policies (tenant_id, project_id, inbox_id, enabled)
            VALUES ('{tenant}', '{project}', '{inbox}', true);
        "#,
        tenant=id(1), project=id(3), inbox=id(42), channel=id(50), contact=id(51), conversation=id(52),
    )).execute(&db).await.unwrap();
    let path = format!("/api/v1/conversations/{}/join", id(52));
    let target = id(3).to_string();
    for invalidate_source in [false, true] {
        let (status, body) = request(
            &app,
            Method::POST,
            &path,
            Credential::Token(secret),
            Some(&target),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let routed = route_inbound(&db, &state).await;
        assert!(routed.routing_configured && routed.operator_assigned && !routed.allow_ai);
        if invalidate_source {
            sqlx::query("UPDATE projects SET status='disabled' WHERE id=$1")
                .bind(id(2))
                .execute(&db)
                .await
                .unwrap();
        } else {
            sqlx::query("UPDATE project_roles SET permissions=ARRAY['projects:read','conversations:read'] WHERE tenant_id=$1 AND project_id=$2 AND id='source_only'")
                .bind(id(1)).bind(id(2)).execute(&db).await.unwrap();
        }
        let routed = route_inbound(&db, &state).await;
        assert!(!routed.operator_assigned && routed.allow_ai, "{routed:?}");
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversation_assignments WHERE conversation_id=$1 AND unassigned_at IS NULL")
            .bind(id(52)).fetch_one(&db).await.unwrap();
        assert_eq!(active, 0);
        if !invalidate_source {
            sqlx::query("UPDATE project_roles SET permissions=ARRAY['projects:read','conversations:read','conversations:reply'] WHERE tenant_id=$1 AND project_id=$2 AND id='source_only'")
                .bind(id(1)).bind(id(2)).execute(&db).await.unwrap();
        }
    }
}
