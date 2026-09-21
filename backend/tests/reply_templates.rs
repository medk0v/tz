use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, operator, reply_templates};
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
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn templates_preserve_markdown_and_enforce_crud_permissions_and_inbox_scope(db: PgPool) {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES
            ('{tenant}', 'Templates'), ('{other_tenant}', 'Other tenant');
        INSERT INTO users (id, email, display_name)
            VALUES ('{user}', 'templates@example.test', 'Operator');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{project}', '{tenant}', 'Templates', 'templates'),
            ('{other_project}', '{tenant}', 'Other project', 'other-project'),
            ('{foreign_project}', '{other_tenant}', 'Foreign project', 'foreign-project');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
            VALUES ('{tenant}', '{project}', 'operator', 'Operator', 'operator',
                ARRAY['conversations:read', 'conversations:reply', 'reply_templates:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{project}', '{user}', 'operator');
        INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
            ('{inbox}', '{tenant}', '{project}', 'Support'),
            ('{other_inbox}', '{tenant}', '{project}', 'Other inbox'),
            ('{outside_project_inbox}', '{tenant}', '{other_project}', 'Outside project'),
            ('{foreign_inbox}', '{other_tenant}', '{foreign_project}', 'Foreign inbox');
        "#,
        tenant = id(1),
        project = id(2),
        inbox = id(3),
        other_inbox = id(4),
        user = id(5),
        membership = id(6),
        other_project = id(7),
        outside_project_inbox = id(8),
        other_tenant = id(11),
        foreign_project = id(12),
        foreign_inbox = id(13),
    ))
    .execute(&db)
    .await
    .unwrap();
    for (token, permissions, scope) in [
        (
            "manager",
            vec!["reply_templates:manage"],
            vec![id(3), id(4)],
        ),
        ("operator", vec!["conversations:reply"], vec![id(3)]),
        (
            "scoped-manager",
            vec!["reply_templates:manage"],
            vec![id(3)],
        ),
        ("reader", vec!["conversations:read"], vec![id(3)]),
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
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = reply_templates::router()
        .merge(operator::router())
        .with_state(state);
    let inbox_path = format!("/api/v1/inboxes/{}/reply-templates", id(3));
    let other_path = format!("/api/v1/inboxes/{}/reply-templates", id(4));
    let body = "**Проверяю заявку.**\n\n- Оплата\n- Выплата\n\n[Поддержка](https://example.test)";
    let input = json!({ "title": "  Проверка\n оплаты ", "body": format!("\n{body}\n") });

    let (status, inboxes) = request(&app, Method::GET, "/api/v1/inboxes", "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{inboxes}");
    assert_eq!(inboxes["items"].as_array().unwrap().len(), 2);

    for token in ["manager", "operator"] {
        let (status, empty) = request(&app, Method::GET, &inbox_path, token, None).await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty, json!({ "items": [] }));
    }
    let (status, _) = request(&app, Method::GET, &inbox_path, "reader", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &app,
        Method::POST,
        &inbox_path,
        "operator",
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, created) = request(
        &app,
        Method::POST,
        &inbox_path,
        "manager",
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["title"], "Проверка оплаты");
    assert_eq!(created["body"], body);
    assert_eq!(created["body_format"], "markdown");
    assert_eq!(created["inbox_id"], json!(id(3)));
    let template_id = created["id"].as_str().unwrap();
    let template_path = format!("{inbox_path}/{template_id}");
    let wrong_inbox_template_path = format!("{other_path}/{template_id}");

    let (_, listed) = request(&app, Method::GET, &inbox_path, "operator", None).await;
    assert_eq!(listed["items"], json!([created]));
    let (_, other) = request(&app, Method::GET, &other_path, "manager", None).await;
    assert_eq!(other["items"], json!([]));

    for method in [Method::PATCH, Method::DELETE] {
        let (status, _) = request(
            &app,
            method.clone(),
            &template_path,
            "operator",
            Some(input.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = request(
            &app,
            method,
            &wrong_inbox_template_path,
            "manager",
            Some(input.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    for (inbox, token, expected) in [
        (id(4), "scoped-manager", StatusCode::FORBIDDEN),
        (id(8), "manager", StatusCode::FORBIDDEN),
        (id(13), "manager", StatusCode::NOT_FOUND),
    ] {
        let base_path = format!("/api/v1/inboxes/{inbox}/reply-templates");
        for method in [Method::GET, Method::POST, Method::PATCH, Method::DELETE] {
            let path = if method == Method::PATCH || method == Method::DELETE {
                format!("{base_path}/{template_id}")
            } else {
                base_path.clone()
            };
            let (status, _) = request(&app, method, &path, token, Some(input.clone())).await;
            assert_eq!(status, expected);
        }
    }

    let (status, _) = request(
        &app,
        Method::PATCH,
        &template_path,
        "manager",
        Some(json!({ "title": "Invalid", "body": " \n\t" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, still_unchanged) = request(&app, Method::GET, &inbox_path, "operator", None).await;
    assert_eq!(still_unchanged["items"][0]["body"], body);

    let (status, updated) = request(
        &app,
        Method::PATCH,
        &template_path,
        "manager",
        Some(json!({ "title": "Готово", "body": "**Оплата получена.**" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["body"], "**Оплата получена.**");
    assert_eq!(updated["id"], template_id);
    assert_eq!(updated["created_at"], created["created_at"]);
    let (status, _) = request(&app, Method::DELETE, &template_path, "manager", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = request(&app, Method::DELETE, &template_path, "manager", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, empty) = request(&app, Method::GET, &inbox_path, "operator", None).await;
    assert_eq!(empty["items"], json!([]));
    let audit = sqlx::query_as::<_, (String, Value)>(
        "SELECT action, metadata FROM audit_log WHERE resource_id = $1 ORDER BY occurred_at, id",
    )
    .bind(Uuid::parse_str(template_id).unwrap())
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        audit
            .iter()
            .map(|(action, _)| action.as_str())
            .collect::<Vec<_>>(),
        [
            "reply_template.created",
            "reply_template.updated",
            "reply_template.deleted"
        ]
    );
    assert!(
        audit
            .iter()
            .all(|(_, metadata)| *metadata == json!({ "inbox_id": id(3) }))
    );
}
