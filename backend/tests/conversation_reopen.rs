mod support;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, conversation_maintenance, conversations};

const CONVERSATION_ID: &str = "00000000-0000-4000-8000-000000000006";

async fn reopen(app: &Router, token: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::post(format!("/api/v1/conversations/{CONVERSATION_ID}/reopen"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn manual_reopening_is_scoped_idempotent_and_restarts_inactivity(db: PgPool) {
    sqlx::raw_sql(r#"
        INSERT INTO tenants (id, name) VALUES
            ('00000000-0000-4000-8000-000000000001', 'Reopen tenant'),
            ('00000000-0000-4000-8000-000000000101', 'Other tenant');
        INSERT INTO users (id, email, display_name)
        VALUES ('00000000-0000-4000-8000-000000000010', 'operator@example.test', 'Operator');
        INSERT INTO memberships (id, tenant_id, user_id, role) VALUES
            ('00000000-0000-4000-8000-000000000011', '00000000-0000-4000-8000-000000000001',
             '00000000-0000-4000-8000-000000000010', 'admin'),
            ('00000000-0000-4000-8000-000000000111', '00000000-0000-4000-8000-000000000101',
             '00000000-0000-4000-8000-000000000010', 'admin');
        INSERT INTO projects (id, tenant_id, name, slug)
        VALUES ('00000000-0000-4000-8000-000000000002', '00000000-0000-4000-8000-000000000001',
                'Reopen project', 'reopen'),
               ('00000000-0000-4000-8000-000000000102', '00000000-0000-4000-8000-000000000101',
                'Other project', 'other');
        INSERT INTO inboxes (id, tenant_id, project_id, name)
        VALUES ('00000000-0000-4000-8000-000000000003', '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002', 'Support');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name)
        VALUES ('00000000-0000-4000-8000-000000000004', '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002', '00000000-0000-4000-8000-000000000003',
                '00000000-0000-4000-8000-000000000014', 'widget', 'Website');
        INSERT INTO contacts (id, tenant_id, project_id)
        VALUES ('00000000-0000-4000-8000-000000000005', '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id,
                                   contact_id, status, resolved_at, created_at, last_message_at)
        VALUES ('00000000-0000-4000-8000-000000000006', '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002', '00000000-0000-4000-8000-000000000003',
                '00000000-0000-4000-8000-000000000004', '00000000-0000-4000-8000-000000000005',
                'resolved', now() - interval '2 days', now() - interval '3 days', now() - interval '2 days');
        INSERT INTO conversation_assignments (id, tenant_id, conversation_id, user_id)
        VALUES ('00000000-0000-4000-8000-000000000012', '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000006', '00000000-0000-4000-8000-000000000010');
        INSERT INTO conversation_resolutions (id, tenant_id, project_id, inbox_id,
                                             conversation_id, cycle_number, resolved_at)
        SELECT gen_random_uuid(), tenant_id, project_id, inbox_id, id, cycle,
               now() - interval '2 days' FROM conversations CROSS JOIN generate_series(1, 2) AS cycle;
    "#).execute(&db).await.unwrap();

    support::add_project_admin(
        &db,
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap(),
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000010").unwrap(),
    )
    .await;
    support::add_project_admin(
        &db,
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000101").unwrap(),
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000102").unwrap(),
        uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000010").unwrap(),
    )
    .await;

    for (token, tenant, project, permissions, inbox_scope) in [
        (
            "allowed",
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
            vec!["conversations:close"],
            None,
        ),
        (
            "read-only",
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
            vec!["conversations:read"],
            None,
        ),
        (
            "wrong-inbox",
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
            vec!["conversations:close"],
            Some(Vec::<uuid::Uuid>::new()),
        ),
        (
            "wrong-tenant",
            "00000000-0000-4000-8000-000000000101",
            "00000000-0000-4000-8000-000000000102",
            vec!["conversations:close"],
            None,
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO api_keys (id, tenant_id, actor_user_id, name, token_hash,
                                  permissions, inbox_scope, project_id, role, expires_at)
            VALUES ($1, $2, '00000000-0000-4000-8000-000000000010', $3, $4, $5, $6,
                    $7, 'admin', now() + interval '1 hour')
        "#,
        )
        .bind(uuid::Uuid::now_v7())
        .bind(uuid::Uuid::parse_str(tenant).unwrap())
        .bind(token)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .bind(permissions)
        .bind(inbox_scope)
        .bind(uuid::Uuid::parse_str(project).unwrap())
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

    assert_eq!(reopen(&app, "read-only").await, StatusCode::FORBIDDEN);
    assert_eq!(reopen(&app, "wrong-inbox").await, StatusCode::FORBIDDEN);
    assert_eq!(reopen(&app, "wrong-tenant").await, StatusCode::NOT_FOUND);
    let (first, retry) = tokio::join!(reopen(&app, "allowed"), reopen(&app, "allowed"));
    assert_eq!(
        (first, retry),
        (StatusCode::NO_CONTENT, StatusCode::NO_CONTENT)
    );

    let row = sqlx::query_as::<_, (String, i32, Option<DateTime<Utc>>, bool, i64)>(
        r#"
        SELECT status, reopened_count, resolved_at,
               last_reopened_at > last_message_at AND updated_at = last_reopened_at,
               last_message_sequence FROM conversations
    "#,
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(row, ("open".to_owned(), 1, None, true, 0));
    let cycles = sqlx::query_scalar::<_, bool>(
        "SELECT reopened_at IS NOT NULL FROM conversation_resolutions ORDER BY cycle_number",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(cycles, vec![false, true]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE action = 'conversation.reopened'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM outbox_events WHERE event_type = 'conversation.reopened'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messages")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM conversation_assignments WHERE unassigned_at IS NULL"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        events.try_recv().unwrap().event_type,
        "conversation.reopened"
    );
    assert!(events.try_recv().is_err());
    assert_eq!(
        conversation_maintenance::auto_resolve_inactive(&db)
            .await
            .unwrap(),
        0
    );

    sqlx::query("UPDATE conversations SET last_reopened_at = now() - interval '25 hours'")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        conversation_maintenance::auto_resolve_inactive(&db)
            .await
            .unwrap(),
        1
    );
    sqlx::query("DELETE FROM conversation_resolutions")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(reopen(&app, "allowed").await, StatusCode::NO_CONTENT);
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT reopened_count FROM conversations")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );

    for request_rating in [false, true] {
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/conversations/{CONVERSATION_ID}/resolve"))
                    .header("authorization", "Bearer allowed")
                    .header("content-type", "application/json")
                    .body(Body::from(if request_rating {
                        "{}"
                    } else {
                        r#"{"request_rating":false}"#
                    }))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let (status, saved): (String, bool) = sqlx::query_as(
            "SELECT c.status, r.rating_requested FROM conversations c JOIN conversation_resolutions r ON r.conversation_id=c.id ORDER BY r.cycle_number DESC LIMIT 1"
        ).fetch_one(&db).await.unwrap();
        assert_eq!(status, "resolved");
        assert_eq!(saved, request_rating);
        let data: serde_json::Value = sqlx::query_scalar(
            "SELECT payload FROM outbox_events WHERE event_type='conversation.resolved' ORDER BY created_at DESC LIMIT 1"
        ).fetch_one(&db).await.unwrap();
        assert_eq!(data["data"]["rating_requested"], request_rating);
        assert_eq!(data["data"]["resolution_id"].is_string(), request_rating);
        assert_eq!(reopen(&app, "allowed").await, StatusCode::NO_CONTENT);
    }
}
