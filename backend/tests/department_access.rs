use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(80_000 + value)
}
fn hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}

async fn fixture(db: PgPool) -> (Router, AppState) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Departments');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'department@example.test', 'Operator');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Departments', 'departments');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions) VALUES
          ('{tenant}', '{project}', 'operator', 'Operator', 'operator', ARRAY['projects:read','conversations:read','conversations:reply','contacts:read','contacts:manage','channels:read','channels:manage']);
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions, is_system)
          VALUES ('{tenant}', '{project}', 'admin', 'Admin', 'admin', ARRAY[
            'projects:read','projects:manage','conversations:read','conversations:reply','conversations:close',
            'contacts:read','contacts:manage','channels:read','channels:manage','access_tokens:manage',
            'visitor_network:read','quality:read','quality:read_all','reviews:read','routing:manage','teams:manage',
            'ai:manage','knowledge:manage', 'notes:read', 'notes:write','reply_templates:manage','processes:read','processes:edit','processes:approve','tasks:own','tasks:manage','tasks:configure','integrations:manage','roles:manage','system:read'
          ],true);
        INSERT INTO departments (id, tenant_id, project_id, name) VALUES
          ('{support}', '{tenant}', '{project}', 'Support'), ('{sales}', '{tenant}', '{project}', 'Sales');
        UPDATE projects SET default_department_id = '{sales}' WHERE id = '{project}';
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role, department_id)
          VALUES ('{member}', '{tenant}', '{project}', '{user}', 'operator', '{support}');
        INSERT INTO inboxes (id, tenant_id, project_id, department_id, name) VALUES
          ('{inbox}', '{tenant}', '{project}', '{support}', 'Support'),
          ('{other_inbox}', '{tenant}', '{project}', '{sales}', 'Sales');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name, status) VALUES
          ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Support widget', 'active'),
          ('{other_channel}', '{tenant}', '{project}', '{other_inbox}', '{other_channel}', 'widget', 'Sales widget', 'active');
        INSERT INTO contacts (id, tenant_id, project_id, display_name) VALUES
          ('{contact}', '{tenant}', '{project}', 'Support contact'), ('{other_contact}', '{tenant}', '{project}', 'Sales contact');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id) VALUES
          ('{conversation}', '{tenant}', '{project}', '{inbox}', '{channel}', '{contact}'),
          ('{other_conversation}', '{tenant}', '{project}', '{other_inbox}', '{other_channel}', '{other_contact}');
    "#, tenant=id(1), project=id(2), user=id(3), member=id(4), support=id(5), sales=id(6), inbox=id(7), other_inbox=id(8), channel=id(9), other_channel=id(10), contact=id(11), other_contact=id(12), conversation=id(13), other_conversation=id(14)))
        .execute(&db).await.unwrap();
    sqlx::query("INSERT INTO messages (id,tenant_id,project_id,inbox_id,conversation_id,sequence,direction,kind,author_kind,author_id,body,status) SELECT gen_random_uuid(),tenant_id,project_id,inbox_id,id,1,'inbound','text','contact',contact_id,'Test request','delivered' FROM conversations WHERE tenant_id=$1")
        .bind(id(1)).execute(&db).await.unwrap();
    sqlx::query("UPDATE conversations SET last_message_sequence=1 WHERE tenant_id=$1")
        .bind(id(1))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,now()+interval '1 hour',now()+interval '2 hours')")
        .bind(id(15)).bind(id(1)).bind(id(2)).bind(id(3)).bind(id(4)).bind(hash("department-session")).bind(hash("csrf"))
        .execute(&db).await.unwrap();
    sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,role,inbox_scope,expires_at) VALUES ($1,$2,$3,$4,'Department test',$5,ARRAY['conversations:read','channels:read','contacts:read'],'operator',$6,now()+interval '1 hour')")
        .bind(id(16)).bind(id(1)).bind(id(2)).bind(id(3)).bind(hash("department-token")).bind(vec![id(7),id(8)])
        .execute(&db).await.unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db;
    let app = tz_backend::app::router(state.clone());
    (app, state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    input: Option<Value>,
    token: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    builder = if token {
        builder.header(header::AUTHORIZATION, "Bearer department-token")
    } else {
        builder
            .header(
                header::COOKIE,
                "tzomet_session=department-session; tzomet_csrf=csrf",
            )
            .header("x-csrf-token", "csrf")
    };
    let response = app
        .clone()
        .oneshot(
            builder
                .body(input.map_or_else(Body::empty, |body| Body::from(body.to_string())))
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
async fn assigned_department_filters_resources_and_rejects_direct_cross_department_access(
    db: PgPool,
) {
    let (app, _) = fixture(db).await;
    let (status, actor) = request(&app, Method::GET, "/api/v1/me", None, false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(actor["department_id"], id(5).to_string());
    assert_eq!(actor["department_restricted"], true);
    assert_eq!(actor["can_access_director"], false);
    for (path, expected) in [
        ("/api/v1/inboxes", id(7)),
        ("/api/v1/channels", id(9)),
        ("/api/v1/contacts", id(11)),
    ] {
        let (status, body) = request(&app, Method::GET, path, None, false).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let items = body["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "{path}: {body}");
        assert_eq!(items[0]["id"], expected.to_string());
    }
    let (status, conversations) = request(
        &app,
        Method::GET,
        &format!("/api/v1/inboxes/{}/conversations", id(7)),
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{conversations}");
    assert_eq!(conversations["items"][0]["id"], id(13).to_string());
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("/api/v1/inboxes/{}/conversations", id(8)),
            None,
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for department_id in [Some(id(6)), None] {
        assert_eq!(
            request(
                &app,
                Method::POST,
                "/api/v1/auth/department",
                Some(json!({"department_id":department_id})),
                false
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("/api/v1/conversations/{}/messages", id(14)),
            None,
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/v1/channels/{}", id(10)),
            None,
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/realtime-tickets",
            Some(json!({"inbox_id":id(8)})),
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn unrestricted_admin_switches_department_and_director_persistently(db: PgPool) {
    let (app, _) = fixture(db.clone()).await;
    sqlx::query("UPDATE memberships SET department_id=NULL,role='admin' WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    let (status, actor) = request(&app, Method::GET, "/api/v1/me", None, false).await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["department_id"], id(6).to_string());
    assert_eq!(actor["can_access_director"], true);
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/auth/department",
            Some(json!({})),
            false
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for department_id in [Some(id(5)), None, Some(id(6))] {
        let (status, actor) = request(
            &app,
            Method::POST,
            "/api/v1/auth/department",
            Some(json!({"department_id":department_id})),
            false,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{actor}");
        assert_eq!(actor["is_director"], department_id.is_none());
        let (_, persisted) = request(&app, Method::GET, "/api/v1/me", None, false).await;
        assert_eq!(persisted["department_id"], json!(department_id));
        assert_eq!(persisted["is_director"], department_id.is_none());
        let (_, inboxes) = request(&app, Method::GET, "/api/v1/inboxes", None, false).await;
        assert_eq!(
            inboxes["items"].as_array().unwrap().len(),
            if department_id.is_none() { 2 } else { 1 }
        );
        assert_eq!(
            request(&app, Method::GET, "/api/v1/roles", None, false)
                .await
                .0,
            StatusCode::OK
        );
    }
    request(
        &app,
        Method::POST,
        "/api/v1/auth/department",
        Some(json!({"department_id":null})),
        false,
    )
    .await;
    let (status, actor) = request(
        &app,
        Method::POST,
        "/api/v1/auth/project",
        Some(json!({"project_id":id(2)})),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(actor["department_id"].is_null());
    assert_eq!(actor["is_director"], true);
    sqlx::query("UPDATE projects SET director_enabled=false WHERE id=$1")
        .bind(id(2))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/auth/department",
            Some(json!({"department_id":null})),
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn tokens_intersect_live_department_assignment_permissions_and_revocation(db: PgPool) {
    let (app, _) = fixture(db.clone()).await;
    for (department, inbox) in [(id(5), id(7)), (id(6), id(8))] {
        sqlx::query("UPDATE memberships SET department_id=$2 WHERE id=$1")
            .bind(id(4))
            .bind(department)
            .execute(&db)
            .await
            .unwrap();
        let (status, actor) = request(&app, Method::GET, "/api/v1/me", None, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(actor["inbox_scope"], json!([inbox]));
        assert_eq!(
            request(
                &app,
                Method::POST,
                "/api/v1/auth/department",
                Some(json!({"department_id":department})),
                true
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE api_keys SET inbox_scope=$2 WHERE id=$1")
        .bind(id(16))
        .bind(vec![id(7)])
        .execute(&db)
        .await
        .unwrap();
    let (status, actor) = request(&app, Method::GET, "/api/v1/me", None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        actor["inbox_scope"],
        json!([]),
        "department assignment cannot expand the token's explicit inbox grant"
    );
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("/api/v1/conversations/{}/messages", id(14)),
            None,
            true
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE project_roles SET permissions=ARRAY['conversations:read'] WHERE project_id=$1 AND id='operator'").bind(id(2)).execute(&db).await.unwrap();
    assert_eq!(
        request(&app, Method::GET, "/api/v1/channels", None, true)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE memberships SET revoked_at=now() WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", None, true).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", None, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

async fn websocket_upgrade(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    ticket: &str,
) -> reqwest::Response {
    client
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

async fn frame(stream: &mut reqwest::Upgraded) -> u8 {
    use tokio::io::AsyncReadExt;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let first = stream.read_u8().await.unwrap();
        let second = stream.read_u8().await.unwrap();
        let length = match second & 0x7f {
            126 => usize::from(stream.read_u16().await.unwrap()),
            127 => usize::try_from(stream.read_u64().await.unwrap()).unwrap(),
            value => usize::from(value),
        };
        assert!(length < 16_384);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).await.unwrap();
        first & 0x0f
    })
    .await
    .unwrap()
}

async fn assert_department_scope_change_closes_socket(db: PgPool, select_department: bool) {
    let (app, state) = fixture(db.clone()).await;
    if select_department {
        sqlx::query("UPDATE memberships SET department_id=NULL WHERE id=$1")
            .bind(id(4))
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("UPDATE projects SET default_department_id=$2 WHERE id=$1")
            .bind(id(2))
            .bind(id(5))
            .execute(&db)
            .await
            .unwrap();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, server_app).await.unwrap() });
    let mut tickets = Vec::new();
    for _ in 0..2 {
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/realtime-tickets",
            Some(json!({"inbox_id":id(7)})),
            false,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        tickets.push(body["ticket"].as_str().unwrap().to_owned());
    }
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = websocket_upgrade(&client, address, &tickets[0]).await;
    assert_eq!(response.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);
    let mut stream = response.upgrade().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.realtime.receiver_count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let event = tz_backend::realtime::RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: id(1),
        project_id: id(2),
        inbox_id: id(7),
        contact_id: Some(id(11)),
        event_type: "draft.updated".to_owned(),
        aggregate_id: id(13),
        sequence: None,
        occurred_at: chrono::Utc::now(),
        data: json!({"body":"Support draft"}),
    };
    state.publish(&event).await.unwrap();
    assert_eq!(frame(&mut stream).await, 1);
    if select_department {
        assert_eq!(
            request(
                &app,
                Method::POST,
                "/api/v1/auth/department",
                Some(json!({"department_id":id(6)})),
                false
            )
            .await
            .0,
            StatusCode::OK
        );
    } else {
        sqlx::query("UPDATE memberships SET department_id=$2 WHERE id=$1")
            .bind(id(4))
            .bind(id(6))
            .execute(&db)
            .await
            .unwrap();
    }
    state.publish(&event).await.unwrap();
    assert_eq!(
        frame(&mut stream).await,
        8,
        "old scope must close before another department event"
    );
    assert_eq!(
        websocket_upgrade(&client, address, &tickets[1])
            .await
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    server.abort();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn reassigned_membership_closes_live_socket_and_rejects_previously_issued_ticket(db: PgPool) {
    assert_department_scope_change_closes_socket(db, false).await;
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn same_session_department_switch_closes_live_socket_and_rejects_previously_issued_ticket(
    db: PgPool,
) {
    assert_department_scope_change_closes_socket(db, true).await;
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn presence_does_not_advertise_an_operator_in_another_department(db: PgPool) {
    let (app, state) = fixture(db.clone()).await;
    assert_eq!(
        request(&app, Method::POST, "/api/v1/operator-presence", None, false)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        tz_backend::operator_presence::inbox_has_online_operator(&state.db, id(1), id(2), id(7))
            .await
            .unwrap()
    );
    assert!(
        !tz_backend::operator_presence::inbox_has_online_operator(&state.db, id(1), id(2), id(8))
            .await
            .unwrap()
    );
    sqlx::query("UPDATE memberships SET department_id=$2 WHERE id=$1")
        .bind(id(4))
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    assert!(
        !tz_backend::operator_presence::inbox_has_online_operator(&state.db, id(1), id(2), id(7))
            .await
            .unwrap()
    );
    assert!(
        tz_backend::operator_presence::inbox_has_online_operator(&state.db, id(1), id(2), id(8))
            .await
            .unwrap()
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn queue_assignment_rechecks_department_of_a_legacy_queue_member(db: PgPool) {
    let (app, state) = fixture(db.clone()).await;
    sqlx::query("UPDATE project_roles SET permissions=array_append(permissions,'routing:manage') WHERE project_id=$1 AND id='operator'").bind(id(2)).execute(&db).await.unwrap();
    sqlx::raw_sql(&format!(r#"
        INSERT INTO users (id,email,display_name) VALUES ('{sales_user}','sales-operator@example.test','Sales operator');
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role,department_id)
          VALUES ('{sales_member}','{tenant}','{project}','{sales_user}','operator','{sales}');
        INSERT INTO inbox_routing_queues (id,tenant_id,project_id,inbox_id,name)
          VALUES ('{queue}','{tenant}','{project}','{inbox}','Sales queue');
        INSERT INTO inbox_routing_policies (tenant_id,project_id,inbox_id,enabled,assignment_strategy,default_queue_id)
          VALUES ('{tenant}','{project}','{inbox}',true,'least_active','{queue}');
        INSERT INTO inbox_routing_queue_members (tenant_id,project_id,inbox_id,queue_id,user_id)
          VALUES ('{tenant}','{project}','{inbox}','{queue}','{user}');
        UPDATE operator_sessions SET presence_last_seen_at=now() WHERE id='{session}';
    "#,tenant=id(1),project=id(2),inbox=id(8),queue=id(20),user=id(3),session=id(15),sales_user=id(21),sales_member=id(22),sales=id(6))).execute(&db).await.unwrap();
    let (status, envelope) = request(
        &app,
        Method::GET,
        &format!("/api/v1/inboxes/{}/routing", id(7)),
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{envelope}");
    let options = envelope["operator_options"].as_array().unwrap();
    assert_eq!(
        options.len(),
        1,
        "other department operators are not eligible"
    );
    assert_eq!(options[0]["id"], id(3).to_string());
    let context = tz_backend::inbox_routing::InboundContext {
        tenant_id: id(1),
        project_id: id(2),
        inbox_id: id(8),
        conversation_id: id(14),
        channel_connection_id: id(10),
        widget_language: None,
    };
    let mut transaction = db.begin().await.unwrap();
    let decision =
        tz_backend::inbox_routing::on_inbound_message(&state, &mut transaction, &context)
            .await
            .unwrap();
    assert!(
        !decision.operator_assigned,
        "a legacy queue membership cannot override department access"
    );
    transaction.commit().await.unwrap();
    sqlx::query("UPDATE memberships SET department_id=$2 WHERE id=$1")
        .bind(id(4))
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    let mut transaction = db.begin().await.unwrap();
    let decision =
        tz_backend::inbox_routing::on_inbound_message(&state, &mut transaction, &context)
            .await
            .unwrap();
    assert!(
        decision.operator_assigned,
        "the same online operator becomes eligible in the assigned department"
    );
    transaction.commit().await.unwrap();
    // Moving the inbox must also release an owner whose membership did not change.
    sqlx::query("UPDATE inboxes SET department_id=$2 WHERE id=$1")
        .bind(id(8))
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    for _ in 0..2 {
        let mut transaction = db.begin().await.unwrap();
        let decision =
            tz_backend::inbox_routing::on_inbound_message(&state, &mut transaction, &context)
                .await
                .unwrap();
        assert!(
            !decision.operator_assigned,
            "moving an inbox cannot preserve an inaccessible owner"
        );
        transaction.commit().await.unwrap();
    }
    let active_assignments: i64=sqlx::query_scalar("SELECT COUNT(*) FROM conversation_assignments WHERE tenant_id=$1 AND conversation_id=$2 AND unassigned_at IS NULL")
        .bind(id(1)).bind(id(14)).fetch_one(&db).await.unwrap();
    assert_eq!(active_assignments, 0);
    let active_participants: i64=sqlx::query_scalar("SELECT COUNT(*) FROM conversation_participants WHERE tenant_id=$1 AND conversation_id=$2 AND participant_kind='operator' AND left_at IS NULL")
        .bind(id(1)).bind(id(14)).fetch_one(&db).await.unwrap();
    assert_eq!(active_participants, 0);
    let departure_events: i64=sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE tenant_id=$1 AND aggregate_id=$2 AND event_type='conversation.operator_left'")
        .bind(id(1)).bind(id(14)).fetch_one(&db).await.unwrap();
    assert_eq!(
        departure_events, 1,
        "runtime cleanup emits a single departure event"
    );
}

async fn request_as(
    app: &Router,
    method: Method,
    path: &str,
    input: Option<Value>,
    session: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::COOKIE,
                    format!("tzomet_session={session}; tzomet_csrf=csrf"),
                )
                .header("x-csrf-token", "csrf")
                .body(input.map_or_else(Body::empty, |body| Body::from(body.to_string())))
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
async fn project_default_workspace_only_covers_members_who_never_chose_one(db: PgPool) {
    let (app, _) = fixture(db.clone()).await;
    sqlx::query("UPDATE memberships SET department_id=NULL,role='admin' WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE projects SET default_director_workspace=true WHERE id=$1")
        .bind(id(2))
        .execute(&db)
        .await
        .unwrap();
    // Nothing chosen yet, so the project default opens the control center.
    let (status, actor) = request(&app, Method::GET, "/api/v1/me", None, false).await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["is_director"], true);
    assert!(actor["department_id"].is_null());
    let (status, actor) = request(
        &app,
        Method::POST,
        "/api/v1/auth/department",
        Some(json!({"department_id":id(5)})),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["department_id"], id(5).to_string());
    // A later login keeps the member's own choice instead of the project default.
    sqlx::query("INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,now()+interval '1 hour',now()+interval '2 hours')")
        .bind(id(17)).bind(id(1)).bind(id(2)).bind(id(3)).bind(id(4)).bind(hash("next-session")).bind(hash("csrf"))
        .execute(&db).await.unwrap();
    let (status, actor) = request_as(&app, Method::GET, "/api/v1/me", None, "next-session").await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["department_id"], id(5).to_string());
    assert_eq!(actor["is_director"], false);
    request(
        &app,
        Method::POST,
        "/api/v1/auth/department",
        Some(json!({"department_id":null})),
        false,
    )
    .await;
    let (_, actor) = request_as(&app, Method::GET, "/api/v1/me", None, "next-session").await;
    assert_eq!(actor["is_director"], true);
    assert!(actor["department_id"].is_null());
    // A department the member is pinned to still wins over both.
    sqlx::query("UPDATE memberships SET department_id=$2,role='operator' WHERE id=$1")
        .bind(id(4))
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    let (_, actor) = request_as(&app, Method::GET, "/api/v1/me", None, "next-session").await;
    assert_eq!(actor["department_id"], id(6).to_string());
    assert_eq!(actor["is_director"], false);
}
