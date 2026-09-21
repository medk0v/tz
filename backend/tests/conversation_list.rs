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

async fn list(app: &Router, query: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/inboxes/{}/conversations?{query}", id(3)))
                .header("authorization", "Bearer operator")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn return_to_ai(app: &Router, conversation_id: Uuid) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/v1/conversations/{conversation_id}/return-to-ai"
            ))
            .header("authorization", "Bearer operator")
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn inbox_summaries_and_filters_preserve_reply_and_contact_scope(db: PgPool) {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Inbox summaries');
        INSERT INTO users (id, email, display_name)
            VALUES ('{operator}', 'operator@example.test', 'Operator');
        INSERT INTO memberships (id, tenant_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{operator}', 'admin');
        INSERT INTO projects (id, tenant_id, name, slug)
            VALUES ('{project}', '{tenant}', 'Summaries', 'summaries');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
            ('{inbox}', '{tenant}', '{project}', 'Support'),
            ('{other_inbox}', '{tenant}', '{project}', 'Other inbox');
        INSERT INTO channel_connections (
            id, tenant_id, project_id, inbox_id, public_id, kind, name, status
        ) VALUES
            ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Website', 'active'),
            ('{other_channel}', '{tenant}', '{project}', '{other_inbox}', '{other_channel}', 'widget', 'Other website', 'active');
        INSERT INTO widget_configs (
            channel_connection_id, tenant_id, default_language, allowed_origins
        ) VALUES ('{channel}', '{tenant}', 'ru-ru', ARRAY['https://example.test']);
        INSERT INTO contacts (id, tenant_id, project_id) VALUES
            ('{contact}', '{tenant}', '{project}'),
            ('{other_contact}', '{tenant}', '{project}');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id,
                                   contact_id, status, widget_language, last_message_sequence, updated_at) VALUES
            ('{current}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}', 'open', 'ru-ru', 6, now()),
            ('{resolved}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}', 'resolved', 'ru', 1, now() - interval '1 day'),
            ('{other}', '{tenant}', '{project}', '{inbox}', '{channel}', '{other_contact}', 'open', 'ru', 1, now() - interval '2 days'),
            ('{outside}', '{tenant}', '{project}', '{other_inbox}', '{other_channel}', '{contact}', 'open', 'ru', 1, now()),
            ('{empty}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}', 'open', 'ru', 0, now());
        INSERT INTO ai_provider_connections (
            id, tenant_id, project_id, name, provider_kind, base_url,
            default_model, status, created_by
        ) VALUES (
            '{ai_provider}', '{tenant}', '{project}', 'OpenClaw', 'openai_compatible',
            'http://openclaw.test', 'test-model', 'active', '{operator}'
        );
        INSERT INTO ai_profiles (
            id, tenant_id, project_id, provider_connection_id, name, status,
            language, created_by
        ) VALUES (
            '{ai_profile}', '{tenant}', '{project}', '{ai_provider}',
            'Оператор виджета', 'active', 'ru, en', '{operator}'
        );
        INSERT INTO ai_profile_channel_connections (
            tenant_id, ai_profile_id, channel_connection_id
        ) VALUES ('{tenant}', '{ai_profile}', '{channel}');
        INSERT INTO ai_profile_public_identities (tenant_id, ai_profile_id, language, display_name) VALUES
            ('{tenant}', '{ai_profile}', 'ru', 'Екатерина'),
            ('{tenant}', '{ai_profile}', 'en', 'Sophie');
        INSERT INTO ai_profile_public_identity_avatars (
            tenant_id, ai_profile_id, language, public_id, media_type, content,
            sha256, width, height, created_by
        ) VALUES (
            '{tenant}', '{ai_profile}', 'ru', '{ai_avatar}', 'image/png',
            decode('00', 'hex'), decode(repeat('00', 32), 'hex'), 1, 1, '{operator}'
        );
        INSERT INTO conversation_participants (
            id, tenant_id, conversation_id, participant_kind, ai_profile_id, joined_at
        ) VALUES (
            '{ai_participant}', '{tenant}', '{current}', 'ai', '{ai_profile}',
            '2026-09-01 09:59:00Z'
        );
        INSERT INTO conversation_assignments (id, tenant_id, conversation_id, user_id)
            VALUES ('{assignment}', '{tenant}', '{current}', '{operator}');
        INSERT INTO conversation_read_cursors (tenant_id, conversation_id, actor_id, last_read_sequence)
            VALUES ('{tenant}', '{current}', '{operator}', 6);
        INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id,
                              sequence, direction, kind, author_kind, body, body_format,
                              status, created_at, client_message_id)
        SELECT gen_random_uuid(), conversation.tenant_id, conversation.project_id,
               conversation.inbox_id, conversation.id, message.sequence, message.direction,
               message.kind, message.author_kind, message.body, message.body_format,
               message.status, message.created_at::timestamptz, gen_random_uuid()
        FROM conversations AS conversation
        CROSS JOIN (VALUES
            (1, 'inbound', 'text', 'contact', 'Initial question', 'plain', 'delivered', '2026-09-01 10:00:00Z'),
            (2, 'outbound', 'text', 'operator', 'First reply', 'plain', 'sent', '2026-09-01 10:01:00Z'),
            (3, 'inbound', 'text', 'contact', 'Follow-up question', 'plain', 'delivered', '2026-09-01 10:02:00Z'),
            (4, 'inbound', 'attachment', 'contact', '', 'plain', 'delivered', '2026-09-01 10:03:00Z'),
            (5, 'outbound', 'text', 'operator', repeat('Ж', 350), 'markdown', 'failed', '2026-09-01 10:04:00Z'),
            (6, 'internal', 'system', 'system', 'Internal update', 'plain', 'sent', '2026-09-01 10:05:00Z')
        ) AS message(sequence, direction, kind, author_kind, body, body_format, status, created_at)
        WHERE conversation.id = '{current}';
        INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id,
                              sequence, direction, kind, author_kind, body, client_message_id)
        SELECT gen_random_uuid(), tenant_id, project_id, inbox_id, id, 1,
               'inbound', 'text', 'contact', 'Question', gen_random_uuid()
        FROM conversations WHERE id IN ('{resolved}', '{other}', '{outside}');
        "#,
        tenant = id(1), project = id(2), inbox = id(3), channel = id(4), contact = id(5),
        current = id(6), resolved = id(7), other = id(8), outside = id(9),
        operator = id(10), membership = id(11), assignment = id(12), empty = id(16),
        ai_profile = id(17), ai_participant = id(18), ai_avatar = id(19), ai_provider = id(20),
        other_inbox = id(103), other_channel = id(104), other_contact = id(105),
    ))
    .execute(&db)
    .await
    .unwrap();
    support::add_project_admin(&db, id(1), id(2), id(10)).await;

    sqlx::query(
        r#"
        INSERT INTO api_keys (id, tenant_id, actor_user_id, name, token_hash, permissions,
                              inbox_scope, project_id, role, expires_at)
        VALUES ($1, $2, $3, 'Operator', $4,
                ARRAY['conversations:read', 'conversations:reply'], $5, $6,
                'admin', now() + interval '1 hour')
        "#,
    )
    .bind(id(13))
    .bind(id(1))
    .bind(id(10))
    .bind(Sha256::digest(b"operator").to_vec())
    .bind(vec![id(3)])
    .bind(id(2))
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

    let first_page = list(&app, "limit=1").await;
    assert_eq!(first_page["has_more"], true);
    let current = &first_page["items"][0];
    assert_eq!(current["id"], json!(id(6)));
    assert_eq!(current["unread_customer_messages"], 0);
    assert_eq!(current["awaiting_reply_since"], "2026-09-01T10:02:00Z");
    assert_eq!(current["last_message"]["body_format"], "markdown");
    assert_eq!(current["last_message"]["direction"], "outbound");
    assert_eq!(current["last_message"]["body"], "Ж".repeat(280));
    assert_eq!(current["contact_conversation_count"], 2);
    assert_eq!(current["ai_agent"]["display_name"], "Екатерина");
    assert_eq!(
        current["ai_agent"]["avatar_url"],
        format!("/public/v1/avatars/{}", id(19))
    );

    let needing_reply = list(&app, "needs_reply=true").await;
    assert_eq!(needing_reply["items"].as_array().unwrap().len(), 2);
    assert_eq!(needing_reply["has_more"], false);
    let mine = list(&app, "mine=true&status=active").await;
    assert_eq!(mine["items"].as_array().unwrap().len(), 1);
    assert_eq!(mine["items"][0]["id"], json!(id(6)));
    let resolved = list(&app, "status=resolved").await;
    assert_eq!(resolved["items"].as_array().unwrap().len(), 1);
    assert!(resolved["items"][0]["awaiting_reply_since"].is_null());

    let history = list(&app, &format!("contact_id={}", id(5))).await;
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    let included = list(
        &app,
        &format!(
            "limit=1&contact_id={}&include_conversation_id={}",
            id(5),
            id(7)
        ),
    )
    .await;
    assert_eq!(included["items"][0]["id"], json!(id(7)));
    let wrong_contact = list(
        &app,
        &format!("contact_id={}&include_conversation_id={}", id(105), id(7)),
    )
    .await;
    assert_eq!(wrong_contact["items"].as_array().unwrap().len(), 1);
    assert_eq!(wrong_contact["items"][0]["id"], json!(id(8)));
    let wrong_channel = list(&app, &format!("channel_id={}", id(104))).await;
    assert!(wrong_channel["items"].as_array().unwrap().is_empty());
    let empty = list(&app, &format!("include_conversation_id={}", id(16))).await;
    assert_eq!(empty["items"][0]["id"], json!(id(16)));
    assert!(empty["items"][0]["last_message"].is_null());

    for (index, placeholder) in [
        "No response from OpenClaw.",
        "⚠️ \u{00a0}AGENT\tcouldn’t\u{202f}generate a response: quota",
        " \nASSISTANT    FAILED TO GENERATE A RESPONSE.",
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            r#"
            INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id,
                                  sequence, direction, kind, author_kind, body, status, client_message_id)
            SELECT gen_random_uuid(), tenant_id, project_id, inbox_id, id, $2,
                   'outbound', 'text', 'ai', $3, 'sent', gen_random_uuid()
            FROM conversations WHERE id = $1
            "#,
        )
        .bind(id(6))
        .bind(7 + i64::try_from(index).unwrap())
        .bind(placeholder)
        .execute(&db)
        .await
        .unwrap();
        let unanswered = list(&app, "mine=true&needs_reply=true").await;
        assert_eq!(unanswered["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            unanswered["items"][0]["awaiting_reply_since"],
            "2026-09-01T10:02:00Z"
        );
        assert_eq!(
            unanswered["items"][0]["last_message"]["body"],
            "Ж".repeat(280)
        );
    }

    sqlx::query(
        "UPDATE messages SET status = 'queued' WHERE conversation_id = $1 AND sequence = 5",
    )
    .bind(id(6))
    .execute(&db)
    .await
    .unwrap();
    let answered = list(&app, "mine=true&needs_reply=true").await;
    assert!(answered["items"].as_array().unwrap().is_empty());

    sqlx::query("UPDATE conversation_participants SET left_at = now() WHERE id = $1")
        .bind(id(18))
        .execute(&db)
        .await
        .unwrap();
    let returned = return_to_ai(&app, id(6)).await;
    assert_eq!(returned["ai_agent"]["display_name"], "Екатерина");
    assert_eq!(
        returned["ai_agent"]["avatar_url"],
        format!("/public/v1/avatars/{}", id(19))
    );
}
