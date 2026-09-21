use std::net::SocketAddr;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
    routing::post,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, config::ProductConfig, conversations};
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

const TENANT: u128 = 1;
const PROJECT: u128 = 2;
const INBOX: u128 = 3;
const CONVERSATION: u128 = 4;
const ADMIN: u128 = 10;
const OPERATOR: u128 = 11;
const COLLEAGUE: u128 = 12;

struct Session {
    token: &'static str,
    csrf: &'static str,
}

const ADMIN_SESSION: Session = Session {
    token: "takeover-admin",
    csrf: "takeover-admin-csrf",
};
const COLLEAGUE_SESSION: Session = Session {
    token: "takeover-colleague",
    csrf: "takeover-colleague-csrf",
};

async fn fixture(db: &PgPool) {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Takeover tenant');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Support', 'takeover');
        INSERT INTO users (id, email, display_name) VALUES
            ('{admin}', 'admin@takeover.test', 'Darius'),
            ('{operator}', 'alex@takeover.test', 'Alex'),
            ('{colleague}', 'colleague@takeover.test', 'Colleague');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, is_system, permissions)
            VALUES ('{tenant}', '{project}', 'operator', 'Operator', 'operator', true,
                    ARRAY['conversations:read', 'conversations:reply']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role) VALUES
            ('{admin_membership}', '{tenant}', '{project}', '{admin}', 'admin'),
            ('{operator_membership}', '{tenant}', '{project}', '{operator}', 'operator'),
            ('{colleague_membership}', '{tenant}', '{project}', '{colleague}', 'operator');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES ('{inbox}', '{tenant}', '{project}', 'Support');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name)
            VALUES ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel_public}', 'widget', 'Website');
        INSERT INTO contacts (id, tenant_id, project_id) VALUES ('{contact}', '{tenant}', '{project}');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, status)
            VALUES ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}', 'open');
        INSERT INTO conversation_assignments (id, tenant_id, conversation_id, user_id, assigned_by)
            VALUES ('{assignment}', '{tenant}', '{conversation}', '{operator}', '{operator}');
        INSERT INTO conversation_participants (id, tenant_id, conversation_id, participant_kind, user_id)
            VALUES ('{participant}', '{tenant}', '{conversation}', 'operator', '{operator}');
        INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id, token_hash,
                                       csrf_token_hash, idle_expires_at, absolute_expires_at) VALUES
            ('{admin_session}', '{tenant}', '{project}', '{admin}', '{admin_membership}',
             decode('{admin_hash:x}', 'hex'), decode('{admin_csrf_hash:x}', 'hex'),
             now() + interval '1 hour', now() + interval '2 hours'),
            ('{colleague_session}', '{tenant}', '{project}', '{colleague}', '{colleague_membership}',
             decode('{colleague_hash:x}', 'hex'), decode('{colleague_csrf_hash:x}', 'hex'),
             now() + interval '1 hour', now() + interval '2 hours');
        "#,
        tenant = id(TENANT),
        project = id(PROJECT),
        inbox = id(INBOX),
        conversation = id(CONVERSATION),
        admin = id(ADMIN),
        operator = id(OPERATOR),
        colleague = id(COLLEAGUE),
        channel = id(20),
        channel_public = id(21),
        contact = id(22),
        assignment = id(23),
        participant = id(24),
        admin_membership = id(30),
        operator_membership = id(31),
        colleague_membership = id(32),
        admin_session = id(40),
        colleague_session = id(41),
        admin_hash = Sha256::digest(ADMIN_SESSION.token.as_bytes()),
        admin_csrf_hash = Sha256::digest(ADMIN_SESSION.csrf.as_bytes()),
        colleague_hash = Sha256::digest(COLLEAGUE_SESSION.token.as_bytes()),
        colleague_csrf_hash = Sha256::digest(COLLEAGUE_SESSION.csrf.as_bytes()),
    ))
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, is_system, permissions) VALUES ($1, $2, 'admin', 'Admin', 'admin', true, $3)",
    )
    .bind(id(TENANT))
    .bind(id(PROJECT))
    .bind(ADMIN_PERMISSIONS)
    .execute(db)
    .await
    .unwrap();
}

async fn app(db: &PgPool) -> Router {
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    config.product = ProductConfig {
        project_id: Some(id(PROJECT)),
    };
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    conversations::router().with_state(state)
}

async fn request(
    app: &Router,
    session: &Session,
    path: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let prefix = "tz";
    let method = if path.ends_with("/conversations") || path.ends_with("/reply-suggestion-agents") {
        Method::GET
    } else {
        Method::POST
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .extension(ConnectInfo(
                    "203.0.113.1:8080".parse::<SocketAddr>().unwrap(),
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::COOKIE,
                    format!(
                        "{prefix}_session={}; {prefix}_csrf={}",
                        session.token, session.csrf
                    ),
                )
                .header("x-csrf-token", session.csrf)
                .body(input.map_or_else(Body::empty, |body| Body::from(body.to_string())))
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

fn conversation_path(action: &str) -> String {
    format!("/api/v1/conversations/{}/{action}", id(CONVERSATION))
}

fn message(body: &str, send_as: &str) -> Value {
    json!({ "client_message_id": Uuid::now_v7(), "body": body, "send_as": send_as })
}

async fn active_operator(db: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "SELECT user_id FROM conversation_assignments WHERE conversation_id = $1 AND user_id IS NOT NULL AND unassigned_at IS NULL",
    )
    .bind(id(CONVERSATION))
    .fetch_one(db)
    .await
    .unwrap()
}

async fn audit_count(db: &PgPool, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = $1")
        .bind(action)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn ai_fixture(db: &PgPool) {
    sqlx::raw_sql(&format!(
        r#"
        UPDATE channel_connections SET status = 'active' WHERE id = '{channel}';
        INSERT INTO ai_provider_connections (
            id, tenant_id, project_id, name, provider_kind, base_url, default_model, created_by
        ) VALUES ('{provider}', '{tenant}', '{project}', 'Test provider', 'openai_compatible',
                  'http://unused.test', 'test-model', '{admin}');
        INSERT INTO ai_profiles (
            id, tenant_id, project_id, provider_connection_id, name, status, language,
            auto_join_new_conversations, created_by
        ) VALUES ('{profile}', '{tenant}', '{project}', '{provider}', 'Support AI', 'active',
                  'en', true, '{admin}');
        INSERT INTO ai_profile_channel_connections (tenant_id, ai_profile_id, channel_connection_id)
            VALUES ('{tenant}', '{profile}', '{channel}');
        INSERT INTO ai_profile_public_identities (tenant_id, ai_profile_id, language, display_name)
            VALUES ('{tenant}', '{profile}', 'en', 'Support AI');
        "#,
        provider = id(50),
        profile = id(51),
        tenant = id(TENANT),
        project = id(PROJECT),
        admin = id(ADMIN),
        channel = id(20),
    ))
    .execute(db)
    .await
    .unwrap();
}

async fn add_timed_message(
    db: &PgPool,
    conversation: Uuid,
    sequence: i64,
    age_minutes: i32,
    author: &str,
    status: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id, sequence,
                              direction, kind, author_kind, body, status, created_at, client_message_id)
        SELECT gen_random_uuid(), tenant_id, project_id, inbox_id, id, $2,
               CASE WHEN $4 = 'contact' THEN 'inbound' ELSE 'outbound' END,
               'text', $4, 'Please help', $5, now() - $3 * interval '1 minute', gen_random_uuid()
        FROM conversations WHERE id = $1
        "#,
    )
    .bind(conversation).bind(sequence).bind(age_minutes).bind(author).bind(status)
    .execute(db).await.unwrap();
    sqlx::query(
        "UPDATE conversations SET last_message_sequence = $2, last_message_at = now() - $3 * interval '1 minute' WHERE id = $1",
    )
    .bind(conversation).bind(sequence).bind(age_minutes)
    .execute(db).await.unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn administrator_generates_ai_reply_drafts_without_joining(db: PgPool) {
    fixture(&db).await;
    ai_fixture(&db).await;
    add_timed_message(&db, id(CONVERSATION), 1, 5, "contact", "delivered").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/chat/completions",
                post(|| async {
                    Json(json!({"choices": [{"finish_reason": "stop", "message": {
                        "role": "assistant", "content": "I will check the payment status."
                    }}]}))
                }),
            ),
        )
        .await
        .unwrap();
    });
    sqlx::query("UPDATE ai_provider_connections SET base_url = $1 WHERE id = $2")
        .bind(&base_url)
        .bind(id(50))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO conversation_participants (id, tenant_id, conversation_id, participant_kind, ai_profile_id) VALUES ($1, $2, $3, 'ai', $4)",
    )
    .bind(Uuid::now_v7())
    .bind(id(TENANT))
    .bind(id(CONVERSATION))
    .bind(id(51))
    .execute(&db)
    .await
    .unwrap();

    {
        let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
        config.pg.migrate_on_start = false;
        config.redis.url = None;
        config.attachments.enabled = false;
        config.openclaw.base_url = Some(base_url.clone());
        config.product = ProductConfig {
            project_id: Some(id(PROJECT)),
        };
        let mut state = AppState::build(config).await.unwrap();
        state.db = db.clone();
        let app = conversations::router().with_state(state);
        let endpoints = [
            (conversation_path("reply-suggestion-agents"), None),
            (
                conversation_path("reply-suggestions"),
                Some(json!({"ai_profile_id": id(51), "draft_body": ""})),
            ),
        ];

        // An AI participant does not bypass another operator's active assignment.
        for (path, input) in &endpoints {
            let (status, body) = request(&app, &ADMIN_SESSION, path, input.clone()).await;
            assert_eq!(status, StatusCode::CONFLICT, "{path}: {body}");
        }
        sqlx::query(
            "UPDATE conversation_assignments SET unassigned_at = now() WHERE conversation_id = $1",
        )
        .bind(id(CONVERSATION))
        .execute(&db)
        .await
        .unwrap();

        // A regular operator must still join before generating replies.
        for (path, input) in &endpoints {
            let (status, body) = request(&app, &COLLEAGUE_SESSION, path, input.clone()).await;
            assert_eq!(status, StatusCode::CONFLICT, "{path}: {body}");
        }
        let (status, agents) = request(&app, &ADMIN_SESSION, &endpoints[0].0, None).await;
        assert_eq!(status, StatusCode::OK, "{agents}");
        assert_eq!(agents["items"][0]["id"], json!(id(51)));
        let (status, draft) = request(
            &app,
            &ADMIN_SESSION,
            &endpoints[1].0,
            endpoints[1].1.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{draft}");
        assert_eq!(draft["body"], "I will check the payment status.");
        assert_eq!(draft["body_format"], "markdown");
        let counts: (i64, i64, i64) = sqlx::query_as(
            r#"SELECT
                (SELECT count(*) FROM messages WHERE conversation_id = $1),
                (SELECT count(*) FROM conversation_assignments WHERE conversation_id = $1 AND unassigned_at IS NULL),
                (SELECT count(*) FROM conversation_participants WHERE conversation_id = $1 AND participant_kind = 'ai' AND left_at IS NULL)
            "#,
        ).bind(id(CONVERSATION)).fetch_one(&db).await.unwrap();
        assert_eq!(
            counts,
            (1, 0, 1),
            "generation must keep the AI connected without joining or sending"
        );

        for change in [
            "UPDATE conversation_participants SET left_at = now() WHERE participant_kind = 'ai'",
            "UPDATE ai_profiles SET status = 'disabled' WHERE id = '00000000-0000-0000-0000-000000000033'",
            "UPDATE conversations SET status = 'resolved'",
        ] {
            sqlx::query(change).execute(&db).await.unwrap();
            for (path, input) in &endpoints {
                let (status, body) = request(&app, &ADMIN_SESSION, path, input.clone()).await;
                assert_eq!(status, StatusCode::CONFLICT, "{change}: {path}: {body}");
            }
            sqlx::raw_sql(
                "UPDATE conversation_participants SET left_at = NULL WHERE participant_kind = 'ai';
                 UPDATE ai_profiles SET status = 'active' WHERE id = '00000000-0000-0000-0000-000000000033';
                 UPDATE conversations SET status = 'open';",
            ).execute(&db).await.unwrap();
        }
        // Assigned operators retain the existing generation path without an active AI.
        sqlx::raw_sql(
            "UPDATE conversation_participants SET left_at = now() WHERE participant_kind = 'ai';
             UPDATE conversation_assignments SET user_id = '00000000-0000-0000-0000-00000000000c', unassigned_at = NULL;",
        ).execute(&db).await.unwrap();
        for (path, input) in &endpoints {
            let (status, body) = request(&app, &COLLEAGUE_SESSION, path, input.clone()).await;
            assert_eq!(status, StatusCode::OK, "{path}: {body}");
        }
        sqlx::query(
            "UPDATE conversation_participants SET left_at = NULL WHERE participant_kind = 'ai'",
        )
        .execute(&db)
        .await
        .unwrap();
    }
    server.abort();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn administrator_returns_another_operators_chat_directly_to_ai(db: PgPool) {
    fixture(&db).await;
    ai_fixture(&db).await;
    let app = app(&db).await;
    add_timed_message(&db, id(CONVERSATION), 1, 5, "contact", "delivered").await;
    let path = conversation_path("return-to-ai");
    let (status, body) = request(&app, &COLLEAGUE_SESSION, &path, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(active_operator(&db).await, id(OPERATOR));

    // With no eligible AI the operator must stay assigned.
    sqlx::query("UPDATE ai_profiles SET status = 'disabled' WHERE id = $1")
        .bind(id(51))
        .execute(&db)
        .await
        .unwrap();
    let (status, body) = request(&app, &ADMIN_SESSION, &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(active_operator(&db).await, id(OPERATOR));
    sqlx::query("UPDATE ai_profiles SET status = 'active' WHERE id = $1")
        .bind(id(51))
        .execute(&db)
        .await
        .unwrap();

    // No previous AI participant is required, and no intermediate takeover occurs.
    let (status, body) = request(&app, &ADMIN_SESSION, &path, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ai_agent"]["display_name"], "Support AI");
    assert_eq!(body["ai_agent"]["active"], true);
    let active_humans: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM conversation_assignments WHERE conversation_id = $1 AND user_id IS NOT NULL AND unassigned_at IS NULL",
    ).bind(id(CONVERSATION)).fetch_one(&db).await.unwrap();
    assert_eq!(active_humans, 0);
    let active_participants: Vec<String> = sqlx::query_scalar(
        "SELECT participant_kind FROM conversation_participants WHERE conversation_id = $1 AND left_at IS NULL",
    ).bind(id(CONVERSATION)).fetch_all(&db).await.unwrap();
    assert_eq!(active_participants, ["ai"]);
    let (actor, metadata): (Uuid, Value) = sqlx::query_as(
        "SELECT actor_id, metadata FROM audit_log WHERE action = 'conversation.returned_to_ai'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(actor, id(ADMIN));
    assert_eq!(metadata["previous_operator_id"], json!(id(OPERATOR)));
    assert_eq!(
        audit_count(&db, "conversation.taken_over_by_admin").await,
        0
    );
    let reply_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE aggregate_type = 'provider_reply' AND status = 'pending'",
    ).fetch_one(&db).await.unwrap();
    assert_eq!(reply_jobs, 1);
    let (status, _) = request(&app, &ADMIN_SESSION, &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn unanswered_operator_timeout_tracks_customer_waiting_and_assignment(db: PgPool) {
    fixture(&db).await;
    ai_fixture(&db).await;
    // 100: due, 101: recently assigned, 102: recent customer message,
    // 103: repeated customer message, 104: answered, 105: new wait after a reply,
    // 106: resolved, 107: reopened, 108: empty, 109: failed operator reply,
    // 110: failed AI placeholder, 111: exactly 30 minutes, 112: attachment.
    for number in 100..=112 {
        sqlx::query(
            r#"
            INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id,
                                       contact_id, status, created_at)
            SELECT $1, tenant_id, project_id, inbox_id, channel_connection_id,
                   contact_id, 'open', now() - interval '2 hours'
            FROM conversations WHERE id = $2
            "#,
        )
        .bind(id(number))
        .bind(id(CONVERSATION))
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO conversation_assignments (id, tenant_id, conversation_id, user_id, assigned_at)
            VALUES (gen_random_uuid(), $1, $2, $3, now() - interval '1 hour')
            "#,
        ).bind(id(TENANT)).bind(id(number)).bind(id(OPERATOR)).execute(&db).await.unwrap();
        sqlx::query(
            r#"
            INSERT INTO conversation_participants (id, tenant_id, conversation_id, participant_kind, user_id)
            VALUES (gen_random_uuid(), $1, $2, 'operator', $3)
            "#,
        ).bind(id(TENANT)).bind(id(number)).bind(id(OPERATOR)).execute(&db).await.unwrap();
        if number != 108 {
            let age = if number == 102 {
                29
            } else if number == 111 {
                30
            } else {
                40
            };
            add_timed_message(&db, id(number), 1, age, "contact", "delivered").await;
        }
    }
    sqlx::query("UPDATE conversation_assignments SET assigned_at = now() - interval '29 minutes' WHERE conversation_id = $1")
        .bind(id(101)).execute(&db).await.unwrap();
    add_timed_message(&db, id(103), 2, 1, "contact", "delivered").await;
    add_timed_message(&db, id(104), 2, 31, "operator", "sent").await;
    add_timed_message(&db, id(105), 2, 35, "operator", "sent").await;
    add_timed_message(&db, id(105), 3, 29, "contact", "delivered").await;
    sqlx::query("UPDATE conversations SET status = 'resolved', resolved_at = now() WHERE id = $1")
        .bind(id(106))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE conversations SET last_reopened_at = now() - interval '5 minutes' WHERE id = $1",
    )
    .bind(id(107))
    .execute(&db)
    .await
    .unwrap();
    add_timed_message(&db, id(109), 2, 1, "operator", "failed").await;
    add_timed_message(&db, id(110), 2, 1, "ai", "sent").await;
    sqlx::query("UPDATE messages SET body = 'No response from OpenClaw.' WHERE conversation_id = $1 AND sequence = 2")
        .bind(id(110)).execute(&db).await.unwrap();
    sqlx::query("UPDATE messages SET kind = 'attachment' WHERE conversation_id = $1")
        .bind(id(112))
        .execute(&db)
        .await
        .unwrap();
    // Reading a waiting chat must not postpone the handoff.
    sqlx::query("INSERT INTO conversation_read_cursors (tenant_id, conversation_id, actor_id, last_read_sequence) VALUES ($1, $2, $3, 1)")
        .bind(id(TENANT)).bind(id(100)).bind(id(OPERATOR)).execute(&db).await.unwrap();

    assert_eq!(
        conversations::return_unanswered_operator_conversations_to_ai(&db)
            .await
            .unwrap(),
        6
    );
    assert_eq!(
        conversations::return_unanswered_operator_conversations_to_ai(&db)
            .await
            .unwrap(),
        0
    );
    let handed_off: Vec<Uuid> = sqlx::query_scalar(
        "SELECT conversation_id FROM conversation_participants WHERE participant_kind = 'ai' AND left_at IS NULL ORDER BY conversation_id",
    ).fetch_all(&db).await.unwrap();
    assert_eq!(handed_off, [100, 103, 109, 110, 111, 112].map(id));
    let active_humans: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM conversation_assignments WHERE user_id IS NOT NULL AND unassigned_at IS NULL",
    ).fetch_one(&db).await.unwrap();
    assert_eq!(active_humans, 8);
    let reply_triggers: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT aggregate_id, (payload->>'triggering_sequence')::bigint FROM outbox_events WHERE aggregate_type = 'provider_reply' AND status = 'pending' ORDER BY aggregate_id",
    ).fetch_all(&db).await.unwrap();
    assert_eq!(
        reply_triggers,
        [
            (id(100), 1),
            (id(103), 2),
            (id(109), 1),
            (id(110), 1),
            (id(111), 1),
            (id(112), 1)
        ]
    );
    assert_eq!(
        audit_count(&db, "conversation.auto_returned_to_ai").await,
        6
    );
    let realtime: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type = 'conversation.ai_joined' AND aggregate_type = 'realtime'",
    ).fetch_one(&db).await.unwrap();
    assert_eq!(realtime, 6);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn automatic_handoff_waits_for_eligible_ai_and_skips_locked_conversations(db: PgPool) {
    fixture(&db).await;
    ai_fixture(&db).await;
    sqlx::query("UPDATE conversation_assignments SET assigned_at = now() - interval '1 hour'")
        .execute(&db)
        .await
        .unwrap();
    // A worker catching up after downtime must hand off before the 24-hour cleanup.
    add_timed_message(&db, id(CONVERSATION), 1, 25 * 60, "contact", "delivered").await;
    for (table, column, unavailable, available) in [
        ("ai_profiles", "status", "disabled", "active"),
        ("ai_profiles", "language", "ru", "en"),
        ("ai_provider_connections", "status", "disabled", "active"),
        ("channel_connections", "status", "disabled", "active"),
        ("inboxes", "status", "disabled", "active"),
    ] {
        let query = format!("UPDATE {table} SET {column} = $1");
        sqlx::query(&query)
            .bind(unavailable)
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            conversations::return_unanswered_operator_conversations_to_ai(&db)
                .await
                .unwrap(),
            0
        );
        assert_eq!(active_operator(&db).await, id(OPERATOR));
        sqlx::query(&query)
            .bind(available)
            .execute(&db)
            .await
            .unwrap();
    }
    let mut transaction = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM conversations WHERE id = $1 FOR UPDATE")
        .bind(id(CONVERSATION))
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(
        conversations::return_unanswered_operator_conversations_to_ai(&db)
            .await
            .unwrap(),
        0
    );
    transaction.commit().await.unwrap();
    let (first, second) = tokio::join!(
        conversations::return_unanswered_operator_conversations_to_ai(&db),
        conversations::return_unanswered_operator_conversations_to_ai(&db),
    );
    assert_eq!(first.unwrap() + second.unwrap(), 1);
    assert_eq!(
        audit_count(&db, "conversation.auto_returned_to_ai").await,
        1
    );
    assert_eq!(
        tz_backend::conversation_maintenance::auto_resolve_inactive(&db)
            .await
            .unwrap(),
        0,
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn administrators_write_for_the_assigned_operator_or_join_in_their_place(db: PgPool) {
    fixture(&db).await;
    let app = app(&db).await;

    // Other operators keep the conversation locked.
    for (path, input, expected) in [
        (conversation_path("take-over"), None, StatusCode::FORBIDDEN),
        (
            conversation_path("messages"),
            Some(message("Not mine", "assigned_operator")),
            StatusCode::FORBIDDEN,
        ),
        (conversation_path("join"), None, StatusCode::CONFLICT),
    ] {
        let (status, body) = request(&app, &COLLEAGUE_SESSION, &path, input).await;
        assert_eq!(status, expected, "{path}: {body}");
    }

    // The administrator writes as Alex, who keeps the conversation.
    let (status, sent) = request(
        &app,
        &ADMIN_SESSION,
        &conversation_path("messages"),
        Some(message(
            "Alex asked me to confirm the refund.",
            "assigned_operator",
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["author_kind"], "operator");
    let author: Uuid = sqlx::query_scalar("SELECT author_id FROM messages WHERE id = $1")
        .bind(sent["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(author, id(OPERATOR));
    assert_eq!(active_operator(&db).await, id(OPERATOR));
    let audit: (Uuid, Value) = sqlx::query_as(
        "SELECT actor_id, metadata FROM audit_log WHERE action = 'conversation.operator_message.sent_by_admin'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(audit.0, id(ADMIN));
    assert_eq!(audit.1["operator_id"], json!(id(OPERATOR)));
    assert_eq!(audit.1["message_id"], sent["id"]);

    // A plain join still refuses to replace another operator.
    let (status, body) = request(&app, &ADMIN_SESSION, &conversation_path("join"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // Taking over replaces Alex, like joining replaces an AI agent, and is idempotent.
    for _ in 0..2 {
        let (status, joined) =
            request(&app, &ADMIN_SESSION, &conversation_path("take-over"), None).await;
        assert_eq!(status, StatusCode::OK, "{joined}");
        assert_eq!(joined["operator"]["display_name"], "Darius");
    }
    assert_eq!(active_operator(&db).await, id(ADMIN));
    let participants: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT user_id, left_at IS NULL FROM conversation_participants WHERE conversation_id = $1 AND participant_kind = 'operator' ORDER BY joined_at, id",
    )
    .bind(id(CONVERSATION))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(participants, vec![(id(OPERATOR), false), (id(ADMIN), true)]);
    let takeover: (Uuid, Value) = sqlx::query_as(
        "SELECT actor_id, metadata FROM audit_log WHERE action = 'conversation.taken_over_by_admin'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(takeover.0, id(ADMIN));
    assert_eq!(takeover.1["previous_operator_id"], json!(id(OPERATOR)));
    let joined_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type = 'conversation.operator_joined' AND aggregate_id = $1",
    )
    .bind(id(CONVERSATION))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(joined_events, 1);

    let (status, list) = request(
        &app,
        &ADMIN_SESSION,
        &format!("/api/v1/inboxes/{}/conversations", id(INBOX)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["items"][0]["assigned_to_me"], true);
    assert_eq!(list["items"][0]["operator"]["display_name"], "Darius");

    // Alex can no longer reply; the administrator's own replies need no audit trail.
    let (status, sent) = request(
        &app,
        &ADMIN_SESSION,
        &conversation_path("messages"),
        Some(message("I will take it from here.", "operator")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(
        audit_count(&db, "conversation.operator_message.sent_by_admin").await,
        1
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn administrators_also_write_for_or_replace_the_assigned_operator(db: PgPool) {
    fixture(&db).await;
    let app = app(&db).await;

    let (status, body) = request(
        &app,
        &COLLEAGUE_SESSION,
        &conversation_path("take-over"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, sent) = request(
        &app,
        &ADMIN_SESSION,
        &conversation_path("messages"),
        Some(message("Written for Alex", "assigned_operator")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["author_kind"], "operator");
    assert_eq!(active_operator(&db).await, id(OPERATOR));

    let (status, joined) =
        request(&app, &ADMIN_SESSION, &conversation_path("take-over"), None).await;
    assert_eq!(status, StatusCode::OK, "{joined}");
    assert_eq!(active_operator(&db).await, id(ADMIN));
    assert_eq!(
        audit_count(&db, "conversation.operator_message.sent_by_admin").await,
        1
    );
    assert_eq!(
        audit_count(&db, "conversation.taken_over_by_admin").await,
        1
    );
}
