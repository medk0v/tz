use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tower::ServiceExt;
use tz_backend::{AppState, Config, knowledge, reply_templates};
use uuid::Uuid;

const GENERATED: &str = "Здравствуйте! Обработка по этому направлению занимает до 30 минут.";

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
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, body)
}

fn names(list: &Value) -> Vec<&str> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|agent| agent["name"].as_str().unwrap())
        .collect()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn agents_draft_templates_and_knowledge_materials_from_a_title(db: PgPool) {
    let captured = Arc::new(RwLock::new(Vec::<Value>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = captured.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/chat/completions",
                post(move |Json(request): Json<Value>| {
                    let requests = requests.clone();
                    async move {
                        requests.write().await.push(request);
                        Json(json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":GENERATED}}]}))
                    }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let base_url = format!("http://{address}/v1");
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Drafts');
        INSERT INTO users (id, email, display_name)
            VALUES ('{user}', 'drafts@example.test', 'Editor');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{project}', '{tenant}', 'Drafts', 'drafts'),
            ('{other_project}', '{tenant}', 'Other project', 'other-project');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
            VALUES ('{tenant}', '{project}', 'operator', 'Operator', 'operator',
                ARRAY['conversations:reply', 'reply_templates:manage', 'knowledge:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{project}', '{user}', 'operator');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
            ('{inbox}', '{tenant}', '{project}', 'Support'),
            ('{other_inbox}', '{tenant}', '{project}', 'Sales');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name) VALUES
            ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Website'),
            ('{other_channel}', '{tenant}', '{project}', '{other_inbox}', '{other_channel}', 'widget', 'Sales website');
        INSERT INTO ai_provider_connections (id, tenant_id, project_id, name, provider_kind, base_url, default_model, status, created_by) VALUES
            ('{provider}', '{tenant}', '{project}', 'Provider', 'openai_compatible', '{base_url}', 'test-model', 'active', '{user}'),
            ('{other_provider}', '{tenant}', '{other_project}', 'Other provider', 'openai_compatible', '{base_url}', 'test-model', 'active', '{user}');
        INSERT INTO ai_profiles (id, tenant_id, project_id, provider_connection_id, name, instructions, status, created_by) VALUES
            ('{support_agent}', '{tenant}', '{project}', '{provider}', 'Support agent', 'Отвечай клиентам Cyber Money вежливо.', 'active', '{user}'),
            ('{sales_agent}', '{tenant}', '{project}', '{provider}', 'Other inbox agent', 'Sell plans.', 'active', '{user}'),
            ('{draft_agent}', '{tenant}', '{project}', '{provider}', 'Unfinished agent', '', 'draft', '{user}'),
            ('{outside_agent}', '{tenant}', '{other_project}', '{other_provider}', 'Outside agent', '', 'active', '{user}');
        INSERT INTO ai_profile_channel_connections (tenant_id, ai_profile_id, channel_connection_id) VALUES
            ('{tenant}', '{support_agent}', '{channel}'),
            ('{tenant}', '{sales_agent}', '{other_channel}'),
            ('{tenant}', '{draft_agent}', '{channel}');
        INSERT INTO knowledge_bases (id, tenant_id, project_id, name, created_by)
            VALUES ('{base}', '{tenant}', '{project}', 'Exchanges', '{user}');
        "#,
        tenant = id(1),
        project = id(2),
        inbox = id(3),
        other_inbox = id(4),
        user = id(5),
        membership = id(6),
        other_project = id(7),
        channel = id(20),
        other_channel = id(21),
        provider = id(30),
        other_provider = id(31),
        support_agent = id(40),
        sales_agent = id(41),
        draft_agent = id(42),
        outside_agent = id(43),
        base = id(50),
    ))
    .execute(&db)
    .await
    .unwrap();
    for (token, permissions, scope) in [
        (
            "manager",
            vec!["reply_templates:manage", "knowledge:manage"],
            vec![id(3)],
        ),
        (
            "wide-manager",
            vec!["reply_templates:manage", "knowledge:manage"],
            vec![id(3), id(4)],
        ),
        ("operator", vec!["conversations:reply"], vec![id(3)]),
    ] {
        sqlx::query(
            r#"
            INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name,
                                  token_hash, permissions, inbox_scope, role, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'operator', now() + interval '1 hour')
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(id(1))
        .bind(id(2))
        .bind(id(5))
        .bind(token)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .bind(permissions)
        .bind(scope)
        .execute(&db)
        .await
        .unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    config.openclaw.base_url = Some(base_url);
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = reply_templates::router()
        .merge(knowledge::router())
        .with_state(state);
    let template_agents = format!("/api/v1/inboxes/{}/reply-template-draft-agents", id(3));
    let template_drafts = format!("/api/v1/inboxes/{}/reply-template-drafts", id(3));
    let article_agents = format!("/api/v1/knowledge-bases/{}/article-draft-agents", id(50));
    let article_drafts = format!("/api/v1/knowledge-bases/{}/article-drafts", id(50));

    // Inbox-scoped credentials see agents whose channels stay inside their Inboxes.
    let (status, agents) = request(&app, Method::GET, &template_agents, "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{agents}");
    assert_eq!(names(&agents), ["Support agent"]);
    assert_eq!(agents["items"][0]["id"], id(40).to_string());
    assert!(agents["items"][0]["avatar_url"].is_null());
    let (status, agents) = request(&app, Method::GET, &template_agents, "wide-manager", None).await;
    assert_eq!(status, StatusCode::OK, "{agents}");
    assert_eq!(names(&agents), ["Other inbox agent", "Support agent"]);
    let (status, agents) = request(&app, Method::GET, &article_agents, "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{agents}");
    assert_eq!(names(&agents), ["Support agent"]);

    let (status, draft) = request(
        &app,
        Method::POST,
        &template_drafts,
        "manager",
        Some(json!({ "ai_profile_id": id(40), "title": "  Обмен ЮСДТ -\n Монеро  " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(
        draft,
        json!({ "body": GENERATED, "body_format": "markdown" })
    );
    {
        let requests = captured.read().await;
        let messages = requests[0]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.starts_with("Отвечай клиентам Cyber Money вежливо."));
        assert!(system.contains("<tzomet_reply_template_draft>"));
        assert!(system.contains("<tzomet_customer_reply_security>"));
        let title: Value = serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(title, json!({ "title": "Обмен ЮСДТ - Монеро" }));
    }

    // An existing draft is continued rather than replaced.
    let (status, draft) = request(
        &app,
        Method::POST,
        &template_drafts,
        "manager",
        Some(json!({ "ai_profile_id": id(40), "title": "Обмен", "draft_body": "Здравствуйте!" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    {
        let requests = captured.read().await;
        let messages = requests[1]["messages"].as_array().unwrap();
        assert_eq!(messages.last().unwrap()["role"], "assistant");
        assert_eq!(messages.last().unwrap()["content"], "Здравствуйте!");
    }

    let (status, draft) = request(
        &app,
        Method::POST,
        &article_drafts,
        "manager",
        Some(json!({ "ai_profile_id": id(40), "title": "Сроки обмена" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["body"], GENERATED);
    {
        let requests = captured.read().await;
        let system = requests[2]["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("<tzomet_knowledge_article_draft>"));
        assert!(!system.contains("<tzomet_customer_reply_security>"));
    }

    // Hidden, unfinished and other-project agents never draft content.
    for (agent, path) in [
        (id(41), &template_drafts),
        (id(42), &template_drafts),
        (id(43), &article_drafts),
        (Uuid::now_v7(), &article_drafts),
    ] {
        let (status, error) = request(
            &app,
            Method::POST,
            path,
            "manager",
            Some(json!({ "ai_profile_id": agent, "title": "Сроки обмена" })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{agent}: {error}");
    }
    for (title, path) in [
        (" \n ".to_owned(), &template_drafts),
        ("Ж".repeat(121), &template_drafts),
        ("Ж".repeat(301), &article_drafts),
    ] {
        let (status, error) = request(
            &app,
            Method::POST,
            path,
            "manager",
            Some(json!({ "ai_profile_id": id(40), "title": title })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    }
    let (status, _) = request(
        &app,
        Method::POST,
        &template_drafts,
        "manager",
        Some(
            json!({ "ai_profile_id": id(40), "title": "Обмен", "draft_body": "Ж".repeat(10_001) }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(captured.read().await.len(), 3);

    for path in [&template_agents, &article_agents] {
        let (status, _) = request(&app, Method::GET, path, "operator", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    let (status, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/inboxes/{}/reply-template-draft-agents", id(4)),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &app,
        Method::GET,
        &format!(
            "/api/v1/knowledge-bases/{}/article-draft-agents",
            Uuid::now_v7()
        ),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    server.abort();
}
