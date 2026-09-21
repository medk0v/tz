use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, operator};
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
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn setup(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Blacklist'), ('{foreign_tenant}', 'Foreign');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'blacklist@example.test', 'Operator');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{project}', '{tenant}', 'Blacklist', 'blacklist'),
            ('{other_project}', '{tenant}', 'Other', 'other'),
            ('{foreign_project}', '{foreign_tenant}', 'Foreign', 'foreign');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
        VALUES ('{tenant}', '{project}', 'operator', 'Operator', 'operator',
            ARRAY['contacts:read', 'contacts:manage', 'channels:read', 'channels:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
        VALUES ('{membership}', '{tenant}', '{project}', '{user}', 'operator');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
            ('{inbox}', '{tenant}', '{project}', 'Visible'),
            ('{hidden_inbox}', '{tenant}', '{project}', 'Hidden'),
            ('{other_inbox}', '{tenant}', '{other_project}', 'Other'),
            ('{foreign_inbox}', '{foreign_tenant}', '{foreign_project}', 'Foreign');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name, status) VALUES
            ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Widget', 'active'),
            ('{hidden_channel}', '{tenant}', '{project}', '{hidden_inbox}', '{hidden_channel}', 'telegram_bot', 'Telegram', 'active'),
            ('{other_channel}', '{tenant}', '{other_project}', '{other_inbox}', '{other_channel}', 'widget', 'Other', 'active'),
            ('{foreign_channel}', '{foreign_tenant}', '{foreign_project}', '{foreign_inbox}', '{foreign_channel}', 'widget', 'Foreign', 'active');
        INSERT INTO contacts (id, tenant_id, project_id, display_name) VALUES
            ('{contact}', '{tenant}', '{project}', 'Visible'),
            ('{shared_contact}', '{tenant}', '{project}', 'Shared'),
            ('{hidden_contact}', '{tenant}', '{project}', 'Hidden'),
            ('{other_contact}', '{tenant}', '{other_project}', 'Other'),
            ('{foreign_contact}', '{foreign_tenant}', '{foreign_project}', 'Foreign'),
            ('{orphan_contact}', '{tenant}', '{project}', 'Orphan');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id) VALUES
            ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}'),
            ('{shared_conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{shared_contact}'),
            ('{hidden_conversation}', '{tenant}', '{project}', '{hidden_inbox}', '{hidden_channel}', '{hidden_contact}');
        INSERT INTO widget_sessions (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, token_hash, origin, expires_at)
        VALUES (gen_random_uuid(), '{tenant}', '{project}', '{hidden_inbox}', '{hidden_channel}', '{shared_contact}',
            decode(repeat('01', 32), 'hex'), 'https://example.test', now() + interval '1 day');
        INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id, sequence, direction, kind, author_kind, body, client_message_id)
        VALUES ('{message}', '{tenant}', '{project}', '{inbox}', '{conversation}', 1, 'inbound', 'text', 'contact', 'Spam', gen_random_uuid());
        INSERT INTO outbox_events (id, tenant_id, aggregate_type, aggregate_id, event_type, payload, status)
        VALUES ('{outbox}', '{tenant}', 'provider_reply', '{conversation}', 'reply', '{{}}', 'pending'),
               ('{job}', '{tenant}', 'provider_reply', '{conversation}', 'reply', '{{}}', 'processing');
    "#,
        tenant=id(1), project=id(2), inbox=id(3), hidden_inbox=id(4), user=id(5), membership=id(6),
        other_project=id(7), other_inbox=id(8), foreign_tenant=id(11), foreign_project=id(12), foreign_inbox=id(13),
        channel=id(20), hidden_channel=id(21), other_channel=id(22), foreign_channel=id(23),
        contact=id(30), shared_contact=id(31), hidden_contact=id(32), other_contact=id(33), foreign_contact=id(34), orphan_contact=id(35),
        conversation=id(40), shared_conversation=id(41), hidden_conversation=id(42), message=id(50), job=id(60), outbox=id(61),
    )).execute(db).await.unwrap();
    for (token, permissions, inbox_scope) in [
        (
            "manager",
            vec![
                "contacts:read",
                "contacts:manage",
                "channels:read",
                "channels:manage",
            ],
            vec![id(3), id(4)],
        ),
        (
            "scoped",
            vec![
                "contacts:read",
                "contacts:manage",
                "channels:read",
                "channels:manage",
            ],
            vec![id(3)],
        ),
        (
            "reader",
            vec!["contacts:read", "channels:read"],
            vec![id(3)],
        ),
    ] {
        sqlx::query("INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, inbox_scope, role, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'operator',now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(5)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(permissions).bind(inbox_scope)
            .execute(db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    operator::router().with_state(state)
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn contact_block_enforces_all_scopes_cancels_ai_and_audits_state_changes(db: PgPool) {
    let app = setup(&db).await;
    for (contact, token, expected) in [
        (30, "reader", StatusCode::FORBIDDEN),
        (31, "scoped", StatusCode::FORBIDDEN),
        (32, "scoped", StatusCode::FORBIDDEN),
        (33, "manager", StatusCode::FORBIDDEN),
        (34, "manager", StatusCode::NOT_FOUND),
        (35, "scoped", StatusCode::FORBIDDEN),
    ] {
        let (status, body) = request(
            &app,
            Method::PATCH,
            &format!("/api/v1/contacts/{}/block", id(contact)),
            token,
            Some(json!({"blocked": true})),
        )
        .await;
        assert_eq!(status, expected, "{body}");
    }
    let path = format!("/api/v1/contacts/{}/block", id(30));
    for blocked in [true, true, false] {
        let (status, body) = request(
            &app,
            Method::PATCH,
            &path,
            "scoped",
            Some(json!({"blocked": blocked})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body, json!({"id": id(30), "is_blocked": blocked}));
        let (_, directory) = request(
            &app,
            Method::GET,
            "/api/v1/contacts?period=all&search=Visible",
            "scoped",
            None,
        )
        .await;
        assert_eq!(directory["items"][0]["is_blocked"], blocked, "{directory}");
    }
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_log WHERE resource_id = $1 ORDER BY occurred_at, id",
    )
    .bind(id(30))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(actions, ["contact.blocked", "contact.unblocked"]);
    let job_status: String = sqlx::query_scalar("SELECT status FROM outbox_events WHERE id = $1")
        .bind(id(60))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(job_status, "completed");
    let outbox_status: String =
        sqlx::query_scalar("SELECT status FROM outbox_events WHERE id = $1")
            .bind(id(61))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(outbox_status, "completed");
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/contacts/{}/block", id(31)),
        "manager",
        Some(json!({"blocked":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn channel_blacklist_validates_localization_and_preserves_channel_scope(db: PgPool) {
    let app = setup(&db).await;
    let input = json!({"default_language":" RU ","translations":{"RU":"  Свяжитесь по почте.  ","en":"Contact us by email."}});
    for (channel, token, expected) in [
        (20, "reader", StatusCode::FORBIDDEN),
        (21, "scoped", StatusCode::FORBIDDEN),
        (22, "manager", StatusCode::FORBIDDEN),
        (23, "manager", StatusCode::NOT_FOUND),
    ] {
        let (status, body) = request(
            &app,
            Method::PATCH,
            &format!("/api/v1/channels/{}/blacklist", id(channel)),
            token,
            Some(input.clone()),
        )
        .await;
        assert_eq!(status, expected, "{body}");
    }
    let path = format!("/api/v1/channels/{}/blacklist", id(20));
    for invalid in [
        json!({"default_language":"en","translations":{"ru":"Текст"}}),
        json!({"default_language":"en","translations":{"en":" "}}),
        json!({"default_language":"en","translations":{"en":"x".repeat(4001)}}),
    ] {
        let (status, body) = request(&app, Method::PATCH, &path, "scoped", Some(invalid)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (status, body) = request(&app, Method::PATCH, &path, "scoped", Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["blacklist_reply"],
        json!({"default_language":"ru","translations":{"ru":"Свяжитесь по почте.","en":"Contact us by email."}})
    );
    let (_, listed) = request(&app, Method::GET, "/api/v1/channels", "scoped", None).await;
    assert_eq!(
        listed["items"][0]["blacklist_reply"],
        body["blacklist_reply"]
    );
    let audit: Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_log WHERE action = 'channel.blacklist.updated'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(audit["languages"], json!(["en", "ru"]));
    assert_eq!(audit["default_language"], "ru");
    assert!(audit.get("translations").is_none());
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/channels/{}/blacklist", id(21)),
        "manager",
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "telegram_bot");
}
