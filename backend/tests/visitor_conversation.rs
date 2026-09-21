mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, conversations};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn request(
    app: &Router,
    method: &str,
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn starting_a_visitor_conversation_is_scoped_and_reuses_the_widget_chat(db: PgPool) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Visitor test'), ('{other_tenant}', 'Other tenant');
        INSERT INTO users (id, email, display_name) VALUES
            ('{operator}', 'operator@example.test', 'Operator'),
            ('{other_operator}', 'other@example.test', 'Other operator');
        INSERT INTO memberships (id, tenant_id, user_id, role) VALUES
            ('{membership}', '{tenant}', '{operator}', 'admin'),
            ('{other_membership}', '{other_tenant}', '{operator}', 'admin');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{project}', '{tenant}', 'Test', 'test'),
            ('{other_project}', '{tenant}', 'Other project', 'other'),
            ('{foreign_project}', '{other_tenant}', 'Foreign project', 'foreign');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
            VALUES ('{tenant}', '{other_project}', 'operator', 'Operator', 'operator',
                ARRAY['visitor_network:read', 'conversations:read', 'conversations:reply']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{project_membership}', '{tenant}', '{other_project}', '{operator}', 'operator');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES ('{inbox}', '{tenant}', '{project}', 'Support');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name, status)
            VALUES ('{channel}', '{tenant}', '{project}', '{inbox}', '{public}', 'widget', 'Website', 'active');
        INSERT INTO widget_configs (channel_connection_id, tenant_id, allowed_origins)
            VALUES ('{channel}', '{tenant}', ARRAY['https://example.test']);
        INSERT INTO contacts (id, tenant_id, project_id) VALUES ('{contact}', '{tenant}', '{project}');
    "#,
        tenant=id(1), project=id(2), inbox=id(3), channel=id(4), contact=id(5),
        operator=id(10), membership=id(11), public=id(14), other_operator=id(20),
        other_tenant=id(101), other_membership=id(111), other_project=id(999), project_membership=id(112), foreign_project=id(102),
    )).execute(&db).await.unwrap();
    sqlx::query(r#"
        INSERT INTO widget_sessions (id, tenant_id, project_id, inbox_id, channel_connection_id,
            contact_id, token_hash, origin, expires_at, presence_last_seen_at, language)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'https://example.test', now() + interval '1 hour', now(), 'ru')
    "#).bind(id(6)).bind(id(1)).bind(id(2)).bind(id(3)).bind(id(4)).bind(id(5))
        .bind(Sha256::digest(b"widget").to_vec()).execute(&db).await.unwrap();

    support::add_project_admin(&db, id(1), id(2), id(10)).await;
    support::add_project_admin(&db, id(101), id(102), id(10)).await;

    for (token, tenant, permissions, inbox_scope, project_scope) in [
        (
            "allowed",
            id(1),
            vec![
                "visitor_network:read",
                "conversations:read",
                "conversations:reply",
            ],
            None,
            Some(id(2)),
        ),
        (
            "read-only",
            id(1),
            vec!["visitor_network:read", "conversations:read"],
            None,
            Some(id(2)),
        ),
        (
            "no-visitor-access",
            id(1),
            vec!["conversations:read", "conversations:reply"],
            None,
            Some(id(2)),
        ),
        (
            "wrong-inbox",
            id(1),
            vec![
                "visitor_network:read",
                "conversations:read",
                "conversations:reply",
            ],
            Some(Vec::<Uuid>::new()),
            Some(id(2)),
        ),
        (
            "wrong-project",
            id(1),
            vec![
                "visitor_network:read",
                "conversations:read",
                "conversations:reply",
            ],
            None,
            Some(id(999)),
        ),
        (
            "wrong-tenant",
            id(101),
            vec![
                "visitor_network:read",
                "conversations:read",
                "conversations:reply",
            ],
            None,
            Some(id(102)),
        ),
        (
            "no-project",
            id(1),
            vec![
                "visitor_network:read",
                "conversations:read",
                "conversations:reply",
            ],
            None,
            None,
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO api_keys (id, tenant_id, actor_user_id, name, token_hash, permissions,
                inbox_scope, project_id, role, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now() + interval '1 hour')
        "#,
        )
        .bind(Uuid::now_v7())
        .bind(tenant)
        .bind(id(10))
        .bind(token)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .bind(permissions)
        .bind(inbox_scope)
        .bind(project_scope)
        .bind(if project_scope == Some(id(999)) {
            "operator"
        } else {
            "admin"
        })
        .execute(&db)
        .await
        .unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let mut events = state.realtime.subscribe();
    let app = conversations::router().with_state(state);
    let start_path = format!("/api/v1/visitor-sessions/{}/conversation", id(6));

    for token in [
        "read-only",
        "no-visitor-access",
        "wrong-inbox",
        "wrong-project",
        "no-project",
    ] {
        assert_eq!(
            request(&app, "POST", &start_path, token, None).await.0,
            StatusCode::FORBIDDEN,
            "{token}"
        );
    }
    assert_eq!(
        request(&app, "POST", &start_path, "wrong-tenant", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (first, retry, widget) = tokio::join!(
        request(&app, "POST", &start_path, "allowed", None),
        request(&app, "POST", &start_path, "allowed", None),
        request(
            &app,
            "POST",
            "/widget/v1/conversations",
            "widget",
            Some(json!({}))
        ),
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(retry.0, StatusCode::OK, "{}", retry.1);
    assert_eq!(widget.0, StatusCode::OK, "{}", widget.1);
    assert_eq!(first.1["id"], retry.1["id"]);
    assert_eq!(first.1["id"], widget.1["id"]);
    let conversation_id = first.1["id"].as_str().unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conversations")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM outbox_events WHERE event_type = 'conversation.created'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id FROM conversation_assignments WHERE unassigned_at IS NULL"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        id(10)
    );

    let list_path = format!("/api/v1/inboxes/{}/conversations", id(3));
    assert_eq!(
        request(&app, "GET", &list_path, "allowed", None).await.1["items"],
        json!([])
    );
    let focused_path = format!("{list_path}?include_conversation_id={conversation_id}&limit=1");
    let listed = request(&app, "GET", &focused_path, "allowed", None).await;
    assert_eq!(listed.0, StatusCode::OK, "{}", listed.1);
    assert_eq!(listed.1["items"][0]["id"], conversation_id);
    assert_eq!(listed.1["items"][0]["assigned_to_me"], true);
    assert_eq!(listed.1["items"][0]["last_message_sequence"], 0);
    assert_eq!(
        request(&app, "GET", &focused_path, "wrong-inbox", None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", &focused_path, "wrong-tenant", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    let message_path = format!("/api/v1/conversations/{conversation_id}/messages");
    let sent = request(
        &app,
        "POST",
        &message_path,
        "allowed",
        Some(json!({
            "client_message_id": Uuid::now_v7(), "body": "Hello from support"
        })),
    )
    .await;
    assert_eq!(sent.0, StatusCode::OK, "{}", sent.1);
    let history_path = format!("/widget/v1/conversations/{conversation_id}/messages");
    let history = request(&app, "GET", &history_path, "widget", None).await;
    assert_eq!(history.0, StatusCode::OK, "{}", history.1);
    assert_eq!(history.1["items"][0]["body"], "Hello from support");
    let mut outbound_event_seen = false;
    while let Ok(event) = events.try_recv() {
        if event.event_type == "message.created" && event.data["direction"] == "outbound" {
            assert_eq!(event.contact_id, Some(id(5)));
            assert_eq!(event.data["conversation_id"], conversation_id);
            outbound_event_seen = true;
        }
    }
    assert!(outbound_event_seen);

    sqlx::query("UPDATE conversation_assignments SET user_id = $1 WHERE unassigned_at IS NULL")
        .bind(id(20))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "POST", &start_path, "allowed", None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT user_id FROM conversation_assignments WHERE unassigned_at IS NULL"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        id(20)
    );

    for change in [
        "presence_last_seen_at = now() - interval '2 minutes'",
        "presence_last_seen_at = now(), expires_at = now() - interval '1 minute'",
        "expires_at = now() + interval '1 hour', revoked_at = now()",
    ] {
        sqlx::query(&format!("UPDATE widget_sessions SET {change}"))
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            request(&app, "POST", &start_path, "allowed", None).await.0,
            StatusCode::NOT_FOUND
        );
    }
}
