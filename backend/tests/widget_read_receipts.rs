use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, conversations, outbox, realtime::RealtimeEvent};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn acknowledge(app: &Router, token: &str, message_ids: &[Uuid]) -> StatusCode {
    app.clone()
        .oneshot(
            Request::post(format!("/widget/v1/conversations/{}/read", id(7)))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "message_ids": message_ids }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn widget_receipts_are_scoped_idempotent_and_survive_delayed_delivery(db: PgPool) {
    for offset in [0, 100] {
        sqlx::raw_sql(&format!(r#"
            INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Read receipts');
            INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Test', 'test');
            INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES ('{inbox}', '{tenant}', '{project}', 'Support');
            INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name) VALUES
                ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Website'),
                ('{other_channel}', '{tenant}', '{project}', '{inbox}', '{other_channel}', 'widget', 'Other website');
            INSERT INTO contacts (id, tenant_id, project_id) VALUES
                ('{contact}', '{tenant}', '{project}'), ('{other_contact}', '{tenant}', '{project}');
            INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id)
                VALUES ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}');
        "#,
            tenant=id(offset+1), project=id(offset+2), inbox=id(offset+3), channel=id(offset+4),
            contact=id(offset+5), conversation=id(offset+7), other_channel=id(offset+8), other_contact=id(offset+9),
        )).execute(&db).await.unwrap();
    }
    for (token, offset, channel, contact) in [
        ("allowed", 0, 4, 5),
        ("other-contact", 0, 4, 9),
        ("other-channel", 0, 8, 5),
        ("other-tenant", 100, 104, 105),
    ] {
        sqlx::query(
            r#"
            INSERT INTO widget_sessions (id, tenant_id, project_id, inbox_id, channel_connection_id,
                                         contact_id, token_hash, origin, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'https://example.test', now() + interval '1 hour')
        "#,
        )
        .bind(Uuid::now_v7())
        .bind(id(offset + 1))
        .bind(id(offset + 2))
        .bind(id(offset + 3))
        .bind(id(channel))
        .bind(id(contact))
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .execute(&db)
        .await
        .unwrap();
    }
    for (message_id, direction, author, status, offset) in [
        (20, "outbound", "operator", "queued", 0),
        (21, "outbound", "ai", "sent", 0),
        (22, "inbound", "contact", "queued", 0),
        (23, "internal", "system", "queued", 0),
        (24, "outbound", "operator", "failed", 0),
        (25, "outbound", "operator", "sent", 0),
        (120, "outbound", "operator", "sent", 100),
    ] {
        sqlx::query(
            r#"
            INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id,
                                  sequence, direction, kind, author_kind, status, client_message_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'text', $8, $9, $1)
        "#,
        )
        .bind(id(message_id))
        .bind(id(offset + 1))
        .bind(id(offset + 2))
        .bind(id(offset + 3))
        .bind(id(offset + 7))
        .bind(i64::try_from(message_id).unwrap())
        .bind(direction)
        .bind(author)
        .bind(status)
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
    let app = conversations::router().with_state(state.clone());

    assert_eq!(
        acknowledge(&app, "invalid", &[id(20)]).await,
        StatusCode::UNAUTHORIZED
    );
    for token in ["other-contact", "other-channel", "other-tenant"] {
        assert_eq!(
            acknowledge(&app, token, &[id(20)]).await,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        acknowledge(&app, "allowed", &[]).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        acknowledge(&app, "allowed", &vec![id(20); 101]).await,
        StatusCode::BAD_REQUEST
    );
    let ids = [id(20), id(21), id(22), id(23), id(24), id(120)];
    assert_eq!(
        acknowledge(&app, "allowed", &ids).await,
        StatusCode::NO_CONTENT
    );
    let receipt = events.try_recv().unwrap();
    assert_eq!(receipt.event_type, "message.status_updated");
    assert_eq!(receipt.tenant_id, id(1));
    assert_eq!(receipt.project_id, id(2));
    assert_eq!(receipt.inbox_id, id(3));
    assert_eq!(receipt.contact_id, Some(id(5)));
    assert_eq!(receipt.data["conversation_id"], json!(id(7)));
    assert_eq!(receipt.data["status"], "read");
    let mut read_ids: Vec<Uuid> =
        serde_json::from_value(receipt.data["message_ids"].clone()).unwrap();
    read_ids.sort_unstable();
    assert_eq!(read_ids, [id(20), id(21)]);
    assert_eq!(
        acknowledge(&app, "allowed", &ids).await,
        StatusCode::NO_CONTENT
    );
    assert!(events.try_recv().is_err());
    let statuses = sqlx::query_scalar::<_, String>("SELECT status FROM messages ORDER BY id")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(
        statuses,
        ["read", "read", "queued", "queued", "failed", "sent", "sent"]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox_events")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );

    // The delivery worker may handle the original message after the visitor has read it.
    let created = RealtimeEvent {
        event_id: Uuid::now_v7(),
        event_type: "message.created".to_owned(),
        tenant_id: id(1),
        project_id: id(2),
        inbox_id: id(3),
        contact_id: Some(id(5)),
        aggregate_id: id(20),
        sequence: Some(20),
        occurred_at: Utc::now(),
        data: json!({ "conversation_id": id(7), "direction": "outbound" }),
    };
    sqlx::query(
        r#"
        INSERT INTO outbox_events (id, tenant_id, aggregate_type, aggregate_id, event_type, payload)
        VALUES ($1, $2, 'realtime', $3, 'message.created', $4)
    "#,
    )
    .bind(created.event_id)
    .bind(created.tenant_id)
    .bind(created.aggregate_id)
    .bind(serde_json::to_value(created).unwrap())
    .execute(&db)
    .await
    .unwrap();
    let worker_id = Uuid::now_v7();
    assert!(outbox::process_once(&state, worker_id).await.unwrap());
    assert!(outbox::process_once(&state, worker_id).await.unwrap());
    assert!(!outbox::process_once(&state, worker_id).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM messages WHERE id = $1")
            .bind(id(20))
            .fetch_one(&db)
            .await
            .unwrap(),
        "read"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM outbox_events WHERE status = 'completed'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        2
    );
    let mut final_status = None;
    while let Ok(event) = events.try_recv() {
        if event.event_type == "message.status_updated" && event.aggregate_id == id(20) {
            final_status = Some(event.data["status"].clone());
        }
    }
    assert_eq!(final_status, Some(json!("read")));
}
