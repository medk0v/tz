mod support;

use std::path::PathBuf;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, attachments, config::AttachmentScanMode, conversations};
use uuid::Uuid;

const RUSSIAN_REPLY: &str = "Вы заблокированы из-за спама. Напишите на почту, указанную на сайте.";
const DEFAULT_REPLY: &str =
    "You are blocked because of spam. Contact the email address on our website.";

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

struct AttachmentDirectory(PathBuf);

impl Drop for AttachmentDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn response(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn message(app: &Router, token: &str, client_message_id: Uuid) -> (StatusCode, Value) {
    response(
        app,
        Request::post(format!("/widget/v1/conversations/{}/messages", id(7)))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"client_message_id": client_message_id, "body": "A customer message"})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await
}

async fn upload(app: &Router, token: &str, client_message_id: Uuid) -> (StatusCode, Value) {
    let boundary = "blacklist-attachment-test";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"screen.png\"\r\nContent-Type: image/png\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR");
    body.extend_from_slice(&64_u32.to_be_bytes());
    body.extend_from_slice(&48_u32.to_be_bytes());
    body.extend_from_slice(b"\x00\x00\x00\x00IEND\xaeB\x60\x82");
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    response(
        app,
        Request::post(format!("/widget/v1/conversations/{}/attachments", id(7)))
            .header("authorization", format!("Bearer {token}"))
            .header("idempotency-key", client_message_id.to_string())
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap(),
    )
    .await
}

async fn message_rows(db: &PgPool) -> Vec<(i64, String, String, String)> {
    sqlx::query_as(
        "SELECT sequence, direction, author_kind, body FROM messages WHERE conversation_id = $1 ORDER BY sequence",
    )
    .bind(id(7))
    .fetch_all(db)
    .await
    .unwrap()
}

async fn assert_no_support_work(db: &PgPool) {
    let counts = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
        SELECT
            (SELECT count(*) FROM conversation_routing_cycles WHERE conversation_id = $1),
            (SELECT count(*) FROM conversation_participants
             WHERE conversation_id = $1 AND participant_kind = 'ai'),
            (SELECT count(*) FROM outbox_events WHERE tenant_id = $2
             AND (aggregate_type <> 'realtime' OR event_type <> 'message.created'))
        "#,
    )
    .bind(id(7))
    .bind(id(1))
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0));
}

async fn listed_contact_is_blocked(app: &Router) -> bool {
    let (status, body) = response(
        app,
        Request::get(format!("/api/v1/inboxes/{}/conversations", id(3)))
            .header("authorization", "Bearer operator")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"][0]["id"], json!(id(7)));
    body["items"][0]["contact"]["is_blocked"].as_bool().unwrap()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn widget_blacklist_replies_are_localized_idempotent_and_skip_support_work(db: PgPool) {
    for offset in [0, 100] {
        sqlx::raw_sql(&format!(
            r#"
            INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Blacklist test');
            INSERT INTO projects (id, tenant_id, name, slug)
                VALUES ('{project}', '{tenant}', 'Project', 'project');
            INSERT INTO inboxes (id, tenant_id, project_id, name)
                VALUES ('{inbox}', '{tenant}', '{project}', 'Support');
            INSERT INTO channel_connections
                (id, tenant_id, project_id, inbox_id, public_id, kind, name, status) VALUES
                ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Widget', 'active'),
                ('{other_channel}', '{tenant}', '{project}', '{inbox}', '{other_channel}', 'widget', 'Other widget', 'active');
            INSERT INTO widget_configs
                (channel_connection_id, tenant_id, allowed_origins, attachments_enabled)
                VALUES ('{channel}', '{tenant}', ARRAY['https://example.test'], true);
            INSERT INTO contacts (id, tenant_id, project_id, is_blocked) VALUES
                ('{contact}', '{tenant}', '{project}', true),
                ('{other_contact}', '{tenant}', '{project}', false);
            INSERT INTO conversations
                (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, widget_language)
                VALUES ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}', 'ru-ru');
            "#,
            tenant = id(offset + 1), project = id(offset + 2), inbox = id(offset + 3),
            channel = id(offset + 4), contact = id(offset + 5), conversation = id(offset + 7),
            other_channel = id(offset + 8), other_contact = id(offset + 9),
        ))
        .execute(&db)
        .await
        .unwrap();
    }
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO users (id, email, display_name)
            VALUES ('{operator}', 'operator@example.test', 'Operator');
        INSERT INTO memberships (id, tenant_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{operator}', 'admin');
        INSERT INTO operator_sessions
            (id, tenant_id, project_id, user_id, membership_id, token_hash, csrf_token_hash,
             idle_expires_at, absolute_expires_at)
            VALUES ('{session}', '{tenant}', '{project}', '{operator}', '{membership}',
                    decode(repeat('01', 32), 'hex'), decode(repeat('02', 32), 'hex'),
                    now() + interval '1 hour', now() + interval '2 hours');
        INSERT INTO mobile_push_devices
            (id, installation_id, tenant_id, project_id, user_id, session_id, token, token_hash)
            VALUES ('{device}', '{device}', '{tenant}', '{project}', '{operator}', '{session}',
                    'local-test-push-token', decode(repeat('03', 32), 'hex'));
        INSERT INTO inbox_routing_policies
            (tenant_id, project_id, inbox_id, enabled, telegram_new_message, telegram_chat_ids)
            VALUES ('{tenant}', '{project}', '{inbox}', true, true, ARRAY['42']);
        INSERT INTO ai_provider_connections
            (id, tenant_id, project_id, name, provider_kind, base_url, default_model, status, created_by)
            VALUES ('{provider}', '{tenant}', '{project}', 'Test provider', 'openai_compatible',
                    'https://provider.example.test/v1', 'test-model', 'active', '{operator}');
        INSERT INTO ai_profiles
            (id, tenant_id, project_id, provider_connection_id, name, status, language,
             auto_join_new_conversations, created_by)
            VALUES ('{profile}', '{tenant}', '{project}', '{provider}', 'Support', 'active',
                    'ru, en', true, '{operator}');
        INSERT INTO ai_profile_channel_connections (tenant_id, ai_profile_id, channel_connection_id)
            VALUES ('{tenant}', '{profile}', '{channel}');
        "#,
        tenant = id(1), project = id(2), inbox = id(3), channel = id(4),
        operator = id(10), membership = id(11), provider = id(12), profile = id(13),
        session = id(15), device = id(16),
    ))
    .execute(&db)
    .await
    .unwrap();
    support::add_project_admin(&db, id(1), id(2), id(10)).await;

    sqlx::query("UPDATE channel_connections SET blacklist_reply = $1 WHERE id = $2")
        .bind(json!({
            "default_language": "en",
            "translations": {"ru": RUSSIAN_REPLY, "en": DEFAULT_REPLY}
        }))
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    for (token, offset, channel, contact, language) in [
        ("widget", 0, 4, 5, "ru-ru"),
        ("fallback", 0, 4, 5, "de-de"),
        ("other-contact", 0, 4, 9, "ru"),
        ("other-channel", 0, 8, 5, "ru"),
        ("other-tenant", 100, 104, 105, "ru"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO widget_sessions
                (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id,
                 token_hash, origin, language, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'https://example.test', $8, now() + interval '1 hour')
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(id(offset + 1))
        .bind(id(offset + 2))
        .bind(id(offset + 3))
        .bind(id(channel))
        .bind(id(contact))
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .bind(language)
        .execute(&db)
        .await
        .unwrap();
    }
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, role, expires_at)
        VALUES ($1, $2, $5, $3, 'Operator', $4, ARRAY['conversations:read'], 'admin', now() + interval '1 hour')
        "#,
    )
    .bind(id(14))
    .bind(id(1))
    .bind(id(10))
    .bind(Sha256::digest(b"operator").to_vec())
    .bind(id(2))
    .execute(&db)
    .await
    .unwrap();

    let storage = AttachmentDirectory(
        std::env::temp_dir().join(format!("tzomet-blacklist-{}", Uuid::now_v7())),
    );
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = true;
    config.attachments.storage_path = storage.0.clone();
    config.attachments.scan_mode = AttachmentScanMode::Disabled;
    config.attachments.clamav_address = None;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = conversations::router()
        .merge(attachments::router())
        .with_state(state.clone());

    assert_eq!(
        message(&app, "invalid", id(20)).await.0,
        StatusCode::UNAUTHORIZED
    );
    for token in ["other-contact", "other-channel", "other-tenant"] {
        assert_eq!(
            message(&app, token, id(20)).await.0,
            StatusCode::NOT_FOUND,
            "{token}"
        );
        assert_eq!(
            upload(&app, token, id(21)).await.0,
            StatusCode::NOT_FOUND,
            "{token}"
        );
    }
    assert!(message_rows(&db).await.is_empty());

    let first = message(&app, "widget", id(20)).await;
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(first.1["direction"], "inbound");
    let retry = message(&app, "widget", id(20)).await;
    assert_eq!(retry.0, StatusCode::OK, "{}", retry.1);
    assert_eq!(retry.1["id"], first.1["id"]);
    let rows = message_rows(&db).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1],
        (
            2,
            "outbound".to_owned(),
            "system".to_owned(),
            RUSSIAN_REPLY.to_owned()
        )
    );
    assert_no_support_work(&db).await;
    assert!(listed_contact_is_blocked(&app).await);
    let history = response(
        &app,
        Request::get(format!("/widget/v1/conversations/{}/messages", id(7)))
            .header("authorization", "Bearer widget")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(history.0, StatusCode::OK, "{}", history.1);
    assert_eq!(history.1["items"][1]["author_kind"], "system");
    assert_eq!(history.1["items"][1]["body"], RUSSIAN_REPLY);
    for token in ["other-contact", "other-channel", "other-tenant"] {
        let history = response(
            &app,
            Request::get(format!("/widget/v1/conversations/{}/messages", id(7)))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(history.0, StatusCode::NOT_FOUND, "{token}");
    }

    let second = message(&app, "fallback", id(21)).await;
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    let rows = message_rows(&db).await;
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[3].3, DEFAULT_REPLY);

    let attachment = upload(&app, "fallback", id(22)).await;
    assert_eq!(attachment.0, StatusCode::CREATED, "{}", attachment.1);
    assert_eq!(attachment.1["kind"], "attachment");
    assert_eq!(attachment.1["attachments"].as_array().unwrap().len(), 1);
    let retried_attachment = upload(&app, "fallback", id(22)).await;
    assert_eq!(
        retried_attachment.0,
        StatusCode::CREATED,
        "{}",
        retried_attachment.1
    );
    assert_eq!(retried_attachment.1["id"], attachment.1["id"]);
    let rows = message_rows(&db).await;
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[5].3, DEFAULT_REPLY);
    assert_no_support_work(&db).await;
    assert_eq!(
        std::fs::read_dir(storage.0.join("objects"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_dir(storage.0.join("quarantine"))
            .unwrap()
            .count(),
        0
    );

    sqlx::query("UPDATE contacts SET is_blocked = false WHERE id = $1")
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    let unblocked = message(&app, "widget", id(23)).await;
    assert_eq!(unblocked.0, StatusCode::OK, "{}", unblocked.1);
    assert_eq!(message_rows(&db).await.len(), 7);
    assert!(!listed_contact_is_blocked(&app).await);
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        r#"
        SELECT (SELECT count(*) FROM conversation_routing_cycles WHERE conversation_id = $1),
               (SELECT count(*) FROM outbox_events WHERE tenant_id = $2 AND aggregate_type = 'provider_reply'),
               (SELECT count(*) FROM outbox_events WHERE tenant_id = $2 AND aggregate_type = 'telegram'),
               (SELECT count(*) FROM outbox_events WHERE tenant_id = $2 AND aggregate_type = 'mobile_push')
        "#,
    )
    .bind(id(7)).bind(id(1)).fetch_one(&db).await.unwrap();
    assert_eq!(counts, (1, 1, 1, 1));

    sqlx::query("UPDATE contacts SET is_blocked = true WHERE id = $1")
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE ai_profiles SET blacklist_reply_text = $2, blacklist_reply_match_language = false WHERE id = $1")
        .bind(id(13)).bind("Точный текст агента без перевода.").execute(&db).await.unwrap();
    assert_eq!(message(&app, "fallback", id(24)).await.0, StatusCode::OK);
    let rows = message_rows(&db).await;
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[8].3, "Точный текст агента без перевода.");

    sqlx::query("UPDATE ai_profiles SET blacklist_reply_match_language = true WHERE id = $1")
        .bind(id(13))
        .execute(&db)
        .await
        .unwrap();
    // Translation failure must still send the stored text, without running the regular agent.
    sqlx::query("UPDATE ai_provider_connections SET status = 'disabled' WHERE id = $1")
        .bind(id(12))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(message(&app, "widget", id(25)).await.0, StatusCode::OK);
    assert_eq!(message(&app, "widget", id(25)).await.0, StatusCode::OK);
    assert_eq!(message_rows(&db).await.len(), 10);
    assert!(
        conversations::process_blacklist_reply_once(&state, Uuid::now_v7())
            .await
            .unwrap()
    );
    let rows = message_rows(&db).await;
    assert_eq!(rows.len(), 11);
    assert_eq!(rows[10].3, "Точный текст агента без перевода.");
    assert!(
        !conversations::process_blacklist_reply_once(&state, Uuid::now_v7())
            .await
            .unwrap()
    );
}
