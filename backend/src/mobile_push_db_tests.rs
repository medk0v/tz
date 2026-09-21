use std::sync::Arc;

use axum::{body::Body, http::Request, routing::post};
use sqlx::PgPool;
use tower::ServiceExt;

use super::*;
use crate::{Config, auth::hash_token};

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn test_state(db: PgPool) -> AppState {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Mobile push');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'push@example.test', 'Operator');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{project}', '{tenant}', 'Project', 'project'), ('{other_project}', '{tenant}', 'Other', 'other');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions) VALUES
            ('{tenant}', '{project}', 'operator', 'Operator', 'operator', ARRAY['conversations:read']),
            ('{tenant}', '{other_project}', 'operator', 'Operator', 'operator', ARRAY['conversations:read']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{project}', '{user}', 'operator');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
            ('{inbox}', '{tenant}', '{project}', 'Support'), ('{other_inbox}', '{tenant}', '{project}', 'Other');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name, status)
            VALUES ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Widget', 'active');
        INSERT INTO contacts (id, tenant_id, project_id) VALUES ('{contact}', '{tenant}', '{project}');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, status, last_message_sequence)
            VALUES ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}', 'open', 2);
    "#, tenant=id(1), project=id(2), inbox=id(3), other_inbox=id(4), user=id(5), channel=id(6), contact=id(7), membership=id(8), conversation=id(9), other_project=id(20)))
        .execute(&db).await.unwrap();
    for (session, token) in [(id(10), "session-one"), (id(11), "session-two")] {
        sqlx::query("INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id, token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, now() + interval '1 hour', now() + interval '2 hours')")
            .bind(session).bind(id(1)).bind(id(2)).bind(id(5)).bind(id(8)).bind(hash_token(token)).bind(hash_token("csrf"))
            .execute(&db).await.unwrap();
    }
    sqlx::query("INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, role, expires_at) VALUES ($1, $2, $3, $4, 'Test', $5, ARRAY['conversations:read'], 'operator', now() + interval '1 hour')")
        .bind(id(12)).bind(id(1)).bind(id(2)).bind(id(5)).bind(hash_token("api-token")).execute(&db).await.unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db;
    state.mobile_push = Some(Arc::new(mock_client("http://127.0.0.1:1".to_owned())));
    state
}

fn mock_client(endpoint: String) -> FcmClient {
    FcmClient {
        http: reqwest::Client::new(),
        endpoint,
        client_email: String::new(),
        key: EncodingKey::from_secret(b"unused-local-test-key"),
        key_id: None,
        access_token: Mutex::new(Some(CachedToken {
            value: "fake-local-token".to_owned(),
            expires_at: Instant::now() + Duration::from_secs(3600),
        })),
    }
}

async fn request(
    state: &AppState,
    method: &str,
    session: &str,
    csrf: bool,
    project: Uuid,
    token: &str,
) -> StatusCode {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/api/v1/mobile-push/devices/{}", id(30)))
        .header("content-type", "application/json");
    if session == "api-token" {
        request = request.header("authorization", "Bearer api-token");
    } else {
        request = request.header(
            "cookie",
            format!("tzomet_session={session}; tzomet_csrf=csrf"),
        );
    }
    if csrf {
        request = request.header("X-CSRF-Token", "csrf");
    }
    router()
        .with_state(state.clone())
        .oneshot(
            request
                .body(Body::from(
                    json!({"project_id": project, "token": token}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn registration_id(db: &PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM mobile_push_devices WHERE installation_id = $1")
        .bind(id(30))
        .fetch_one(db)
        .await
        .unwrap()
}

fn event() -> RealtimeEvent {
    RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: id(1),
        project_id: id(2),
        inbox_id: id(3),
        contact_id: Some(id(7)),
        event_type: "message.created".to_owned(),
        aggregate_id: id(40),
        sequence: Some(2),
        occurred_at: Utc::now(),
        data: json!({"direction": "inbound", "conversation_id": id(9)}),
    }
}

fn push_payload() -> PushPayload {
    PushPayload {
        event_id: Uuid::now_v7(),
        tenant_id: id(1),
        project_id: id(2),
        inbox_id: id(3),
        conversation_id: id(9),
        sequence: Some(2),
        occurred_at: Utc::now(),
        kind: PushKind::NewMessage,
    }
}

#[sqlx::test]
#[ignore = "requires an isolated PostgreSQL role with CREATEDB"]
async fn mobile_push_registration_enforces_auth_scope_and_generation(db: PgPool) {
    let mut state = test_state(db.clone()).await;
    let (mock, server) = start_mock(&mut state).await;
    for (session, csrf, project, expected) in [
        ("api-token", false, id(20), StatusCode::FORBIDDEN),
        ("session-one", false, id(2), StatusCode::FORBIDDEN),
        ("session-one", true, id(20), StatusCode::FORBIDDEN),
        ("missing", true, id(2), StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            request(&state, "PUT", session, csrf, project, "fcm-one").await,
            expected
        );
    }
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let original = registration_id(&db).await;
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    assert_eq!(registration_id(&db).await, original);
    assert_eq!(
        request(&state, "DELETE", "session-two", true, id(2), "").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        registration_id(&db).await,
        original,
        "another session cannot delete this installation"
    );
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-two").await,
        StatusCode::OK
    );
    let rotated = registration_id(&db).await;
    assert_ne!(rotated, original);
    deliver(&state, original, push_payload()).await.unwrap(); // old generation skips HTTP
    // A project switch must also select a membership valid for that project.
    sqlx::query("INSERT INTO memberships (id, tenant_id, project_id, user_id, role) VALUES ($1, $2, $3, $4, 'operator')")
        .bind(id(21)).bind(id(1)).bind(id(20)).bind(id(5))
        .execute(&db).await.unwrap();
    sqlx::query("UPDATE operator_sessions SET project_id = $1, membership_id = $3 WHERE id = $2")
        .bind(id(20))
        .bind(id(10))
        .bind(id(21))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-two").await,
        StatusCode::FORBIDDEN
    );
    let scoped_registration = router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/mobile-push/devices/{}", id(30)))
                .header("content-type", "application/json")
                .header("cookie", "tzomet_session=session-one; tzomet_csrf=csrf")
                .header("X-CSRF-Token", "csrf")
                .header("X-Tzomet-Project-Id", id(2).to_string())
                .body(Body::from(
                    json!({"project_id": id(2), "token": "fcm-two"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(scoped_registration.status(), StatusCode::OK);
    assert_eq!(registration_id(&db).await, rotated);
    // A registration remains bound to its own project when another window
    // changes the session's legacy default project.
    let mut transaction = db.begin().await.unwrap();
    enqueue_realtime(&mut transaction, &event()).await.unwrap();
    transaction.commit().await.unwrap();
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE aggregate_type = 'mobile_push'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(queued, 1);
    crate::outbox::process_mobile_push_once(&state, id(70))
        .await
        .unwrap();
    assert_eq!(mock.requests.lock().await.len(), 1);
    sqlx::query("UPDATE operator_sessions SET project_id = $1, membership_id = $3, revoked_at = now() WHERE id = $2")
        .bind(id(2))
        .bind(id(10))
        .bind(id(8))
        .execute(&db)
        .await
        .unwrap();
    deliver(&state, rotated, push_payload()).await.unwrap();
    assert_eq!(mock.requests.lock().await.len(), 1);
    server.abort();
}

#[sqlx::test]
#[ignore = "requires an isolated PostgreSQL role with CREATEDB"]
async fn mobile_push_access_token_registration_is_scoped_and_supersedes_credentials(db: PgPool) {
    let state = test_state(db.clone()).await;
    let config_status = || async {
        router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/mobile-push/config")
                    .header("authorization", "Bearer api-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    };
    assert_eq!(config_status().await, StatusCode::OK);
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let session_generation = registration_id(&db).await;
    assert_eq!(
        request(&state, "PUT", "api-token", false, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let token_generation = registration_id(&db).await;
    assert_ne!(session_generation, token_generation);
    let columns: (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT session_id, api_key_id FROM mobile_push_devices WHERE installation_id = $1",
    )
    .bind(id(30))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(columns, (None, Some(id(12))));
    deliver(&state, session_generation, push_payload())
        .await
        .unwrap();
    assert_eq!(
        request(&state, "DELETE", "session-one", true, id(2), "").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(registration_id(&db).await, token_generation);
    assert_eq!(
        request(&state, "PUT", "api-token", false, id(2), "fcm-one").await,
        StatusCode::OK
    );
    assert_eq!(registration_id(&db).await, token_generation);
    for (deny, restore) in [
        (
            "UPDATE api_keys SET permissions = '{}'",
            "UPDATE api_keys SET permissions = ARRAY['conversations:read']",
        ),
        // Preserve valid tenant-level membership so this specifically exercises
        // the mobile endpoint's project gate, not authentication failure.
        // Only move fixture credentials; migrations also seed a demo membership.
        (
            "UPDATE api_keys SET project_id = NULL WHERE id = '00000000-0000-0000-0000-00000000000c'; UPDATE memberships SET project_id = NULL WHERE id = '00000000-0000-0000-0000-000000000008'",
            "UPDATE api_keys SET project_id = '00000000-0000-0000-0000-000000000002' WHERE id = '00000000-0000-0000-0000-00000000000c'; UPDATE memberships SET project_id = '00000000-0000-0000-0000-000000000002' WHERE id = '00000000-0000-0000-0000-000000000008'",
        ),
    ] {
        sqlx::raw_sql(deny).execute(&db).await.unwrap();
        assert_eq!(config_status().await, StatusCode::FORBIDDEN);
        assert_eq!(
            request(&state, "PUT", "api-token", false, id(2), "fcm-one").await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&state, "DELETE", "api-token", false, id(2), "").await,
            StatusCode::FORBIDDEN
        );
        sqlx::raw_sql(restore).execute(&db).await.unwrap();
    }
    assert_eq!(
        request(&state, "PUT", "session-two", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let new_session_generation = registration_id(&db).await;
    assert_ne!(token_generation, new_session_generation);
    deliver(&state, token_generation, push_payload())
        .await
        .unwrap();
    assert_eq!(
        request(&state, "DELETE", "api-token", false, id(2), "").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(registration_id(&db).await, new_session_generation);
    assert_eq!(
        request(&state, "PUT", "api-token", false, id(2), "fcm-one").await,
        StatusCode::OK
    );
    assert_eq!(
        request(&state, "DELETE", "api-token", false, id(2), "").await,
        StatusCode::NO_CONTENT
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mobile_push_devices")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
#[ignore = "requires an isolated PostgreSQL role with CREATEDB"]
async fn mobile_push_access_token_delivery_rechecks_live_scope_and_expiration(db: PgPool) {
    let mut state = test_state(db.clone()).await;
    let (mock, server) = start_mock(&mut state).await;
    assert_eq!(
        request(&state, "PUT", "api-token", false, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let device = registration_id(&db).await;
    let mut transaction = db.begin().await.unwrap();
    let source = event();
    enqueue_realtime(&mut transaction, &source).await.unwrap();
    enqueue_realtime(&mut transaction, &source).await.unwrap();
    transaction.commit().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE aggregate_type = 'mobile_push'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "token registrations enqueue once per source event"
    );
    crate::outbox::process_mobile_push_once(&state, id(70))
        .await
        .unwrap();
    assert_eq!(mock.requests.lock().await.len(), 1);
    let before: (DateTime<Utc>, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT expires_at, last_used_at FROM api_keys WHERE id = $1")
            .bind(id(12))
            .fetch_one(&db)
            .await
            .unwrap();
    deliver(&state, device, push_payload()).await.unwrap();
    let after: (DateTime<Utc>, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT expires_at, last_used_at FROM api_keys WHERE id = $1")
            .bind(id(12))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        before, after,
        "delivery cannot refresh or record use of the access token"
    );
    for (deny, restore) in [
        (
            "UPDATE api_keys SET revoked_at = now()",
            "UPDATE api_keys SET revoked_at = NULL",
        ),
        (
            "UPDATE api_keys SET expires_at = now() - interval '1 second'",
            "UPDATE api_keys SET expires_at = now() + interval '1 hour'",
        ),
        (
            "UPDATE api_keys SET permissions = '{}'",
            "UPDATE api_keys SET permissions = ARRAY['conversations:read']",
        ),
        (
            "UPDATE api_keys SET inbox_scope = ARRAY['00000000-0000-0000-0000-000000000004']::uuid[]",
            "UPDATE api_keys SET inbox_scope = NULL",
        ),
        (
            "UPDATE api_keys SET inbox_scope = '{}'",
            "UPDATE api_keys SET inbox_scope = NULL",
        ),
        (
            "UPDATE api_keys SET project_id = NULL WHERE id = '00000000-0000-0000-0000-00000000000c'; UPDATE memberships SET project_id = NULL WHERE id = '00000000-0000-0000-0000-000000000008'",
            "UPDATE api_keys SET project_id = '00000000-0000-0000-0000-000000000002' WHERE id = '00000000-0000-0000-0000-00000000000c'; UPDATE memberships SET project_id = '00000000-0000-0000-0000-000000000002' WHERE id = '00000000-0000-0000-0000-000000000008'",
        ),
        (
            "UPDATE memberships SET revoked_at = now()",
            "UPDATE memberships SET revoked_at = NULL",
        ),
        (
            "UPDATE users SET status = 'disabled'",
            "UPDATE users SET status = 'active'",
        ),
        (
            "UPDATE projects SET status = 'disabled'",
            "UPDATE projects SET status = 'active'",
        ),
        (
            "UPDATE conversations SET status = 'resolved'",
            "UPDATE conversations SET status = 'open'",
        ),
    ] {
        sqlx::raw_sql(deny).execute(&db).await.unwrap();
        deliver(&state, device, push_payload()).await.unwrap();
        assert_eq!(
            mock.requests.lock().await.len(),
            2,
            "unexpected token delivery after {deny}"
        );
        sqlx::raw_sql(restore).execute(&db).await.unwrap();
    }
    sqlx::query("INSERT INTO conversation_read_cursors (tenant_id, conversation_id, actor_id, last_read_sequence, read_at) VALUES ($1, $2, $3, 2, now())").bind(id(1)).bind(id(9)).bind(id(5)).execute(&db).await.unwrap();
    deliver(&state, device, push_payload()).await.unwrap();
    assert_eq!(mock.requests.lock().await.len(), 2);
    sqlx::query("UPDATE api_keys SET revoked_at = now()")
        .execute(&db)
        .await
        .unwrap();
    let mut transaction = db.begin().await.unwrap();
    enqueue_realtime(&mut transaction, &event()).await.unwrap();
    transaction.commit().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE aggregate_type = 'mobile_push'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(count, 1, "revoked keys cannot enqueue more jobs");
    sqlx::query("DELETE FROM api_keys WHERE id = $1")
        .bind(id(12))
        .execute(&db)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mobile_push_devices")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0, "deleting a token cascades to its registration");
    server.abort();
}

#[sqlx::test]
#[ignore = "requires an isolated PostgreSQL role with CREATEDB"]
async fn mobile_push_old_token_failure_cannot_remove_a_new_session_registration(db: PgPool) {
    let mut state = test_state(db.clone()).await;
    let (mock, server) = start_mock(&mut state).await;
    assert_eq!(
        request(&state, "PUT", "api-token", false, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let old_generation = registration_id(&db).await;
    let response_gate = Arc::new(tokio::sync::Notify::new());
    *mock.response_gate.lock().await = Some(response_gate.clone());
    *mock.status.lock().await = StatusCode::NOT_FOUND;
    let delivery_state = state.clone();
    let delivery = tokio::spawn(async move {
        deliver(&delivery_state, old_generation, push_payload())
            .await
            .unwrap();
    });
    tokio::time::timeout(Duration::from_secs(5), mock.request_seen.notified())
        .await
        .unwrap();
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let new_generation = registration_id(&db).await;
    assert_ne!(old_generation, new_generation);
    response_gate.notify_one();
    tokio::time::timeout(Duration::from_secs(5), delivery)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(registration_id(&db).await, new_generation);
    server.abort();
}

#[derive(Clone)]
struct MockProvider {
    requests: Arc<Mutex<Vec<Value>>>,
    status: Arc<Mutex<StatusCode>>,
    request_seen: Arc<tokio::sync::Notify>,
    response_gate: Arc<Mutex<Option<Arc<tokio::sync::Notify>>>>,
}

async fn mock_provider(
    State(mock): State<MockProvider>,
    Json(body): Json<Value>,
) -> (StatusCode, [(String, String); 1], Json<Value>) {
    mock.requests.lock().await.push(body);
    mock.request_seen.notify_one();
    let response_gate = mock.response_gate.lock().await.clone();
    if let Some(gate) = response_gate {
        gate.notified().await;
    }
    let status = *mock.status.lock().await;
    let body = if status == StatusCode::NOT_FOUND {
        json!({"error": {"details": [{"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "UNREGISTERED"}]}})
    } else {
        json!({"name": "local-test"})
    };
    (
        status,
        [("Retry-After".to_owned(), "120".to_owned())],
        Json(body),
    )
}

async fn start_mock(state: &mut AppState) -> (MockProvider, tokio::task::JoinHandle<()>) {
    let mock = MockProvider {
        requests: Arc::default(),
        status: Arc::new(Mutex::new(StatusCode::OK)),
        request_seen: Arc::default(),
        response_gate: Arc::default(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    state.mobile_push = Some(Arc::new(mock_client(format!(
        "http://{}/send",
        listener.local_addr().unwrap()
    ))));
    let app = Router::new()
        .route("/send", post(mock_provider))
        .with_state(mock.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (mock, server)
}

#[sqlx::test]
#[ignore = "requires an isolated PostgreSQL role with CREATEDB"]
async fn mobile_push_delivery_rechecks_permissions_scope_read_and_session(db: PgPool) {
    let mut state = test_state(db.clone()).await;
    let (mock, server) = start_mock(&mut state).await;
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let device = registration_id(&db).await;
    let before: DateTime<Utc> =
        sqlx::query_scalar("SELECT idle_expires_at FROM operator_sessions WHERE id = $1")
            .bind(id(10))
            .fetch_one(&db)
            .await
            .unwrap();
    deliver(&state, device, push_payload()).await.unwrap();
    assert_eq!(mock.requests.lock().await.len(), 1);
    let after: DateTime<Utc> =
        sqlx::query_scalar("SELECT idle_expires_at FROM operator_sessions WHERE id = $1")
            .bind(id(10))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(before, after, "delivery cannot extend session lifetime");
    for (deny, restore) in [
        (
            "UPDATE operator_sessions SET revoked_at = now()",
            "UPDATE operator_sessions SET revoked_at = NULL",
        ),
        (
            "UPDATE operator_sessions SET idle_expires_at = now() - interval '1 second'",
            "UPDATE operator_sessions SET idle_expires_at = now() + interval '1 hour'",
        ),
        (
            "UPDATE users SET status = 'disabled'",
            "UPDATE users SET status = 'active'",
        ),
        (
            "UPDATE memberships SET revoked_at = now()",
            "UPDATE memberships SET revoked_at = NULL",
        ),
        (
            "UPDATE project_roles SET permissions = '{}'",
            "UPDATE project_roles SET permissions = ARRAY['conversations:read']",
        ),
        (
            "UPDATE projects SET status = 'disabled'",
            "UPDATE projects SET status = 'active'",
        ),
        (
            "UPDATE inboxes SET status = 'disabled'",
            "UPDATE inboxes SET status = 'active'",
        ),
        (
            "UPDATE channel_connections SET status = 'disabled'",
            "UPDATE channel_connections SET status = 'active'",
        ),
        (
            "UPDATE conversations SET status = 'resolved'",
            "UPDATE conversations SET status = 'open'",
        ),
        (
            "UPDATE conversations SET operator_unread_baseline_sequence = 2",
            "UPDATE conversations SET operator_unread_baseline_sequence = 0",
        ),
    ] {
        sqlx::query(deny).execute(&db).await.unwrap();
        deliver(&state, device, push_payload()).await.unwrap();
        assert_eq!(
            mock.requests.lock().await.len(),
            1,
            "unexpected send after {deny}"
        );
        sqlx::query(restore).execute(&db).await.unwrap();
    }
    let mut moved = push_payload();
    moved.inbox_id = id(4);
    deliver(&state, device, moved).await.unwrap();
    let mut foreign = push_payload();
    foreign.tenant_id = id(99);
    deliver(&state, device, foreign).await.unwrap();
    let mut old = push_payload();
    old.occurred_at -= chrono::Duration::hours(2);
    deliver(&state, device, old).await.unwrap();
    sqlx::query("INSERT INTO conversation_read_cursors (tenant_id, conversation_id, actor_id, last_read_sequence, read_at) VALUES ($1, $2, $3, 2, now())").bind(id(1)).bind(id(9)).bind(id(5)).execute(&db).await.unwrap();
    deliver(&state, device, push_payload()).await.unwrap();
    assert_eq!(mock.requests.lock().await.len(), 1);
    let sent = &mock.requests.lock().await[0];
    assert_eq!(sent["message"]["data"]["actor_id"], id(5).to_string());
    assert!(sent["message"].get("notification").is_none());
    server.abort();
}

#[sqlx::test]
#[ignore = "requires an isolated PostgreSQL role with CREATEDB"]
async fn mobile_push_outbox_is_atomic_deduplicated_retryable_and_cleans_invalid_tokens(db: PgPool) {
    let mut state = test_state(db.clone()).await;
    let (mock, server) = start_mock(&mut state).await;
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let source = event();
    let mut transaction = db.begin().await.unwrap();
    crate::conversations::insert_outbox(&mut transaction, &source)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut transaction = db.begin().await.unwrap();
    crate::conversations::insert_outbox(&mut transaction, &source)
        .await
        .unwrap();
    enqueue_realtime(&mut transaction, &source).await.unwrap();
    transaction.commit().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE aggregate_type = 'mobile_push'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(count, 1);
    sqlx::query("UPDATE outbox_events SET status = 'completed' WHERE aggregate_type = 'realtime'")
        .execute(&db)
        .await
        .unwrap();
    *mock.status.lock().await = StatusCode::TOO_MANY_REQUESTS;
    assert!(
        !crate::outbox::process_once(&state, id(69)).await.unwrap(),
        "core queue must ignore mobile jobs"
    );
    assert!(mock.requests.lock().await.is_empty());
    crate::outbox::process_mobile_push_once(&state, id(70))
        .await
        .unwrap();
    let (status, attempts, wait): (String, i32, f64) = sqlx::query_as("SELECT status, attempts, EXTRACT(EPOCH FROM available_at - now())::float8 FROM outbox_events WHERE aggregate_type = 'mobile_push'").fetch_one(&db).await.unwrap();
    assert_eq!((status.as_str(), attempts), ("pending", 1));
    assert!(wait >= 115.0);
    assert_eq!(mock.requests.lock().await.len(), 1);
    // Simulate a crashed worker; only the independent mobile job is reclaimed.
    sqlx::query("UPDATE outbox_events SET status = 'processing', available_at = now(), locked_at = now() - interval '3 minutes', locked_by = $1 WHERE aggregate_type = 'mobile_push'").bind(id(71)).execute(&db).await.unwrap();
    *mock.status.lock().await = StatusCode::NOT_FOUND;
    crate::outbox::process_mobile_push_once(&state, id(72))
        .await
        .unwrap();
    let (status, attempts): (String, i32) = sqlx::query_as(
        "SELECT status, attempts FROM outbox_events WHERE aggregate_type = 'mobile_push'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!((status.as_str(), attempts), ("completed", 2));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mobile_push_devices")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(mock.requests.lock().await.len(), 2);
    server.abort();
}

#[sqlx::test]
#[ignore = "requires an isolated PostgreSQL role with CREATEDB"]
async fn mobile_push_slow_provider_does_not_block_core_delivery(db: PgPool) {
    let mut state = test_state(db.clone()).await;
    let (mock, server) = start_mock(&mut state).await;
    assert_eq!(
        request(&state, "PUT", "session-one", true, id(2), "fcm-one").await,
        StatusCode::OK
    );
    let mut transaction = db.begin().await.unwrap();
    enqueue(&mut transaction, &push_payload()).await.unwrap();
    let mut core = event();
    core.event_type = "contact.updated".to_owned();
    crate::conversations::insert_outbox(&mut transaction, &core)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let response_gate = Arc::new(tokio::sync::Notify::new());
    *mock.response_gate.lock().await = Some(response_gate.clone());
    let mobile_state = state.clone();
    let mobile = tokio::spawn(async move {
        crate::outbox::process_mobile_push_once(&mobile_state, id(73))
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), mock.request_seen.notified())
        .await
        .unwrap();
    assert!(
        !mobile.is_finished(),
        "provider response remains deliberately blocked"
    );
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            crate::outbox::process_once(&state, id(74))
        )
        .await
        .unwrap()
        .unwrap()
    );
    let status: String = sqlx::query_scalar("SELECT status FROM outbox_events WHERE id = $1")
        .bind(core.event_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "completed");
    assert!(!mobile.is_finished());
    response_gate.notify_one();
    assert!(mobile.await.unwrap());
    server.abort();
}
