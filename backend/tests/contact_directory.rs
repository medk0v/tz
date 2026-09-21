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
use tz_backend::{AppState, Config, operator};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn list(app: &Router, query: &str, token: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/contacts?{query}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn periods_statistics_and_network_details_preserve_contact_scope(db: PgPool) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Directory');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'directory@example.test', 'Operator');
        INSERT INTO memberships (id, tenant_id, user_id, role) VALUES ('{membership}', '{tenant}', '{user}', 'admin');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Directory', 'directory');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
          ('{inbox}', '{tenant}', '{project}', 'Visible'), ('{other_inbox}', '{tenant}', '{project}', 'Hidden');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name, status) VALUES
          ('{widget}', '{tenant}', '{project}', '{inbox}', '{widget}', 'widget', 'Website', 'active'),
          ('{telegram}', '{tenant}', '{project}', '{inbox}', '{telegram}', 'telegram_bot', 'Telegram', 'active'),
          ('{other_widget}', '{tenant}', '{project}', '{other_inbox}', '{other_widget}', 'widget', 'Hidden website', 'active');
        INSERT INTO contacts (id, tenant_id, project_id, display_name, created_at) VALUES
          ('{returning}', '{tenant}', '{project}', 'Returning', now() - interval '10 days'),
          ('{new}', '{tenant}', '{project}', 'New', now() - interval '1 hour'),
          ('{bot}', '{tenant}', '{project}', 'Telegram', now() - interval '30 minutes'),
          ('{old}', '{tenant}', '{project}', 'Old', now() - interval '40 days'),
          ('{hidden}', '{tenant}', '{project}', 'Hidden', now()),
          ('{visitor}', '{tenant}', '{project}', 'No conversation', now());
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, created_at) VALUES
          ('{returning_conversation}', '{tenant}', '{project}', '{inbox}', '{widget}', '{returning}', now() - interval '10 days'),
          ('{new_conversation}', '{tenant}', '{project}', '{inbox}', '{widget}', '{new}', now() - interval '1 hour'),
          ('{bot_conversation}', '{tenant}', '{project}', '{inbox}', '{telegram}', '{bot}', now() - interval '30 minutes'),
          ('{old_conversation}', '{tenant}', '{project}', '{inbox}', '{widget}', '{old}', now() - interval '40 days'),
          ('{hidden_conversation}', '{tenant}', '{project}', '{other_inbox}', '{other_widget}', '{hidden}', now()),
          ('{outside_conversation}', '{tenant}', '{project}', '{other_inbox}', '{telegram}', '{old}', now());
        INSERT INTO messages (id, tenant_id, project_id, inbox_id, conversation_id, sequence, direction, kind, author_kind, body, created_at, client_message_id)
        VALUES
          (gen_random_uuid(), '{tenant}', '{project}', '{inbox}', '{returning_conversation}', 1, 'inbound', 'text', 'contact', 'Historical', now() - interval '3 days', gen_random_uuid()),
          (gen_random_uuid(), '{tenant}', '{project}', '{inbox}', '{returning_conversation}', 2, 'inbound', 'text', 'contact', 'Recent', now() - interval '2 hours', gen_random_uuid());
        INSERT INTO widget_sessions (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, token_hash, origin, expires_at, created_at, presence_last_seen_at, client_ip, client_ip_source, user_agent, client_context, visitor_data_expires_at) VALUES
          (gen_random_uuid(), '{tenant}', '{project}', '{inbox}', '{widget}', '{returning}', decode(repeat('01',32),'hex'), 'https://example.test', now() + interval '1 day', now() - interval '10 days', now() - interval '1 hour', '203.0.113.5', 'peer', 'Safari', '{{"locale":{{"timezone":"Europe/Berlin"}}}}', now() + interval '1 day'),
          (gen_random_uuid(), '{tenant}', '{project}', '{inbox}', '{widget}', '{new}', decode(repeat('02',32),'hex'), 'https://example.test', now() + interval '1 day', now() - interval '1 hour', now() - interval '45 minutes', '203.0.113.5', 'peer', 'Safari', '{{"locale":{{"timezone":"Europe/Berlin"}}}}', now() + interval '1 day'),
          (gen_random_uuid(), '{tenant}', '{project}', '{other_inbox}', '{other_widget}', '{returning}', decode(repeat('03',32),'hex'), 'https://hidden.test', now() + interval '1 day', now(), now(), '198.51.100.8', 'peer', 'Hidden browser', '{{"locale":{{"timezone":"Asia/Tokyo"}}}}', now() + interval '1 day');
    "#,
      tenant=id(1), project=id(2), inbox=id(3), widget=id(4), returning=id(5), new=id(6), bot=id(7),
      old=id(8), hidden=id(9), user=id(10), membership=id(11), telegram=id(12), visitor=id(13),
      other_inbox=id(103), other_widget=id(104), returning_conversation=id(20), new_conversation=id(21),
      bot_conversation=id(22), old_conversation=id(23), hidden_conversation=id(24), outside_conversation=id(25),
    )).execute(&db).await.unwrap();
    support::add_project_admin(&db, id(1), id(2), id(10)).await;

    for (token, permissions) in [
        ("network", vec!["contacts:read", "visitor_network:read"]),
        ("limited", vec!["contacts:read"]),
    ] {
        sqlx::query("INSERT INTO api_keys (id, tenant_id, actor_user_id, name, token_hash, permissions, inbox_scope, project_id, role, expires_at) VALUES ($1,$2,$3,'Directory',$4,$5,$6,$7,'admin',now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(10)).bind(Sha256::digest(token.as_bytes()).to_vec())
            .bind(permissions).bind(vec![id(3)]).bind(id(2)).execute(&db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = operator::router().with_state(state);

    let (status, first) = list(&app, "per_page=1", "network").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["total"], 3);
    assert_eq!(first["total_pages"], 3);
    assert_eq!(first["items"][0]["id"], json!(id(7)));
    assert_eq!(first["items"][0]["channel_kinds"], json!(["telegram_bot"]));
    let stats = &first["statistics"];
    assert_eq!(stats["contacts"], 3);
    assert_eq!(stats["new_contacts"], 2);
    assert_eq!(stats["returning_contacts"], 1);
    assert_eq!(stats["conversations"], 3);
    assert_eq!(stats["widget_sessions"], 2);
    assert_eq!(stats["widget_contacts"], 2);
    assert_eq!(stats["telegram_contacts"], 1);

    let (_, second) = list(&app, "per_page=1&page=2", "network").await;
    assert_eq!(second["items"][0]["id"], json!(id(6)));
    let (_, found) = list(&app, "search=Returning", "network").await;
    assert_eq!(found["total"], 1);
    assert_eq!(found["statistics"]["contacts"], 1);
    assert_eq!(found["items"][0]["client_ip"], "203.0.113.5");
    assert_eq!(found["items"][0]["browser_timezone"], "Europe/Berlin");
    assert_eq!(found["items"][0]["channel_kinds"], json!(["widget"]));

    let (_, limited) = list(&app, "search=Returning", "limited").await;
    assert_eq!(limited["total"], 1);
    for field in [
        "client_ip",
        "geo_ip",
        "browser_timezone",
        "user_agent_details",
    ] {
        assert_eq!(limited["items"][0][field], Value::Null);
    }
    let (_, private_search) = list(&app, "search=203.0.113.5", "limited").await;
    assert_eq!(private_search["total"], 0);

    let now = chrono::Utc::now();
    let historical = format!(
        "period=custom&from={}&to={}",
        (now - chrono::Duration::days(4)).format("%Y-%m-%dT%H:%M:%SZ"),
        (now - chrono::Duration::days(2)).format("%Y-%m-%dT%H:%M:%SZ")
    );
    let (_, history) = list(&app, &historical, "network").await;
    assert_eq!(history["total"], 1);
    assert_eq!(history["items"][0]["id"], json!(id(5)));
    assert_eq!(history["statistics"]["returning_contacts"], 1);
    assert_eq!(history["statistics"]["conversations"], 1);
    for period in ["week", "month"] {
        let (_, result) = list(&app, &format!("period={period}"), "network").await;
        assert_eq!(result["total"], 3);
    }
    let (_, all) = list(&app, "period=all", "network").await;
    assert_eq!(all["total"], 4);
    assert_eq!(all["statistics"]["new_contacts"], 4);
    assert_eq!(all["statistics"]["returning_contacts"], 0);
    for query in [
        "period=custom",
        "period=custom&from=2026-09-05T00:00:00Z&to=2026-09-04T00:00:00Z",
    ] {
        let (status, _) = list(&app, query, "network").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
