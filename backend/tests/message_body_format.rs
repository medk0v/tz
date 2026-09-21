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
async fn explicit_markdown_survives_message_reload_and_plain_messages_stay_plain(db: PgPool) {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Message formatting');
        INSERT INTO users (id, email, display_name)
            VALUES ('{operator}', 'operator@example.test', 'Operator');
        INSERT INTO memberships (id, tenant_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{operator}', 'admin');
        INSERT INTO projects (id, tenant_id, name, slug)
            VALUES ('{project}', '{tenant}', 'Formatting', 'formatting');
        INSERT INTO inboxes (id, tenant_id, project_id, name)
            VALUES ('{inbox}', '{tenant}', '{project}', 'Support');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name)
            VALUES ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Website');
        INSERT INTO widget_configs (channel_connection_id, tenant_id, allowed_origins)
            VALUES ('{channel}', '{tenant}', ARRAY['https://example.test']);
        INSERT INTO contacts (id, tenant_id, project_id)
            VALUES ('{contact}', '{tenant}', '{project}');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id,
                                   contact_id, status, last_message_sequence)
            VALUES ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}',
                    '{contact}', 'open', 1);
        INSERT INTO conversation_assignments (id, tenant_id, conversation_id, user_id)
            VALUES ('{assignment}', '{tenant}', '{conversation}', '{operator}');
        INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id,
                              sequence, direction, kind, author_kind, body, client_message_id)
            VALUES ('{legacy_message}', '{tenant}', '{project}', '{inbox}', '{conversation}',
                    1, 'outbound', 'text', 'operator', '**literal text**', '{legacy_message}');
        "#,
        tenant = id(1),
        project = id(2),
        inbox = id(3),
        channel = id(4),
        contact = id(5),
        conversation = id(6),
        operator = id(10),
        membership = id(11),
        assignment = id(12),
        legacy_message = id(20),
    ))
    .execute(&db)
    .await
    .unwrap();
    support::add_project_admin(&db, id(1), id(2), id(10)).await;

    sqlx::query(
        r#"
        INSERT INTO api_keys (id, tenant_id, actor_user_id, name, token_hash, permissions,
                              project_id, role, expires_at)
        VALUES ($1, $2, $3, 'Operator', $4, ARRAY['conversations:reply', 'conversations:read'],
                $5, 'admin', now() + interval '1 hour')
        "#,
    )
    .bind(id(13))
    .bind(id(1))
    .bind(id(10))
    .bind(Sha256::digest(b"operator").to_vec())
    .bind(id(2))
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO widget_sessions (id, tenant_id, project_id, inbox_id, channel_connection_id,
                                     contact_id, token_hash, origin, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'https://example.test', now() + interval '1 hour')
        "#,
    )
    .bind(id(14))
    .bind(id(1))
    .bind(id(2))
    .bind(id(3))
    .bind(id(4))
    .bind(id(5))
    .bind(Sha256::digest(b"widget").to_vec())
    .execute(&db)
    .await
    .unwrap();

    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = conversations::router().with_state(state);
    let operator_path = format!("/api/v1/conversations/{}/messages", id(6));
    let widget_path = format!("/widget/v1/conversations/{}/messages", id(6));
    let markdown = json!({
        "client_message_id": id(21),
        "body": "**Hello**\n\n- First item\n- Second item",
        "body_format": "markdown"
    });

    let (status, sent) = request(
        &app,
        "POST",
        &operator_path,
        "operator",
        Some(markdown.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["body_format"], "markdown");
    assert_eq!(sent["body"], markdown["body"]);
    let (status, repeated) =
        request(&app, "POST", &operator_path, "operator", Some(markdown)).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(repeated["id"], sent["id"]);
    assert_eq!(repeated["body_format"], "markdown");

    for (path, token, client_message_id, body_format) in [
        (&operator_path, "operator", id(22), None),
        (&widget_path, "widget", id(23), Some("markdown")),
    ] {
        let mut body = json!({"client_message_id": client_message_id, "body": "**plain text**"});
        if let Some(format) = body_format {
            body["body_format"] = json!(format);
        }
        let (status, message) = request(&app, "POST", path, token, Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{message}");
        assert_eq!(message["body_format"], "plain");
        assert_eq!(message["body"], "**plain text**");
    }

    for (path, token) in [(&operator_path, "operator"), (&widget_path, "widget")] {
        let (status, history) = request(&app, "GET", path, token, None).await;
        assert_eq!(status, StatusCode::OK, "{history}");
        let items = history["items"].as_array().unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0]["body_format"], "plain");
        assert_eq!(items[0]["body"], "**literal text**");
        assert_eq!(items[1]["body_format"], "markdown");
        assert_eq!(items[1]["body"], sent["body"]);
        assert_eq!(items[2]["body_format"], "plain");
        assert_eq!(items[3]["body_format"], "plain");
    }
}
