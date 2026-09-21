use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, provider_reply::agent_tests};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn request(
    app: &Router,
    method: Method,
    source: &str,
    token: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    request_path(
        app,
        method,
        &format!(
            "/api/v1/ai/profiles/{}/test-runs/{}/sources/{source}",
            id(4),
            id(6)
        ),
        token,
        input,
    )
    .await
}

async fn request_path(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    builder = if token == "password-session" {
        builder
            .header(
                "cookie",
                "tzomet_session=source-session; tzomet_csrf=source-csrf",
            )
            .header("x-csrf-token", "source-csrf")
    } else {
        builder.header("authorization", format!("Bearer {token}"))
    };
    let response = app
        .clone()
        .oneshot(
            builder
                .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn setup(db: PgPool) -> (Router, Value, Value) {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Source editing');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'editor@example.test', 'Editor');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Project', 'project');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
          VALUES ('{tenant}', '{project}', 'manager', 'Manager', 'manager', ARRAY['ai:manage','knowledge:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
          VALUES ('{user}', '{tenant}', '{project}', '{user}', 'manager');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, instructions, tool_instructions, created_by)
          VALUES ('{profile}', '{tenant}', '{project}', 'Keep this name', 'Current instructions', 'Keep tool instructions', '{user}');
        INSERT INTO ai_test_scenarios (id, tenant_id, project_id, profile_id, scenario)
          VALUES ('{scenario}', '{tenant}', '{project}', '{profile}', '{{}}');
        INSERT INTO knowledge_bases (id, tenant_id, project_id, name, status, created_by)
          VALUES ('{base}', '{tenant}', '{project}', 'Knowledge', 'active', '{user}');
        INSERT INTO knowledge_articles (id, tenant_id, project_id, knowledge_base_id, title, body, status, version, created_by, updated_by, published_at)
          VALUES ('{article}', '{tenant}', '{project}', '{base}', 'Keep title', 'Current article', 'published', 4, '{user}', '{user}', '2026-01-01T00:00:00Z'),
                 ('{unread}', '{tenant}', '{project}', '{base}', 'Unread', 'Must not edit', 'published', 1, '{user}', '{user}', now());
        INSERT INTO ai_profile_knowledge_bases (tenant_id, ai_profile_id, knowledge_base_id)
          VALUES ('{tenant}', '{profile}', '{base}');
        "#,
        tenant = id(1),
        project = id(2),
        user = id(3),
        profile = id(4),
        scenario = id(5),
        base = id(7),
        article = id(8),
        unread = id(9),
    ))
    .execute(&db)
    .await
    .unwrap();
    let snapshot = json!({
        "profile":{"instructions":"Historical instructions","tool_instructions":"Historical tools"},
        "knowledge":[{"article_id":id(8),"version":3,"content":"Historical article"},{"article_id":id(9),"version":1,"content":"Must not edit"}]
    });
    let result = json!({"trace":[{
        "call":{"tool":"read_article","parameters":{"article_id":id(8),"version":3}},
        "response":{"article_id":id(8),"version":3,"content":"Historical article"}
    }]});
    sqlx::query("INSERT INTO ai_test_runs (id,tenant_id,project_id,profile_id,scenario_id,scenario_revision,mode,status,fingerprint,snapshot,result,created_by) VALUES ($1,$2,$3,$4,$5,1,'fixtures','passed','test',$6,$7,$8)")
        .bind(id(6)).bind(id(1)).bind(id(2)).bind(id(4)).bind(id(5)).bind(&snapshot).bind(&result).bind(id(3))
        .execute(&db).await.unwrap();
    for (token, permissions, inbox_scope) in [
        ("editor", vec!["ai:manage", "knowledge:manage"], None),
        ("ai-only", vec!["ai:manage"], None),
        ("knowledge-only", vec!["knowledge:manage"], None),
        (
            "scoped",
            vec!["ai:manage", "knowledge:manage"],
            Some(vec![id(99)]),
        ),
    ] {
        sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,inbox_scope,role,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'manager',now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(3)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(permissions).bind(inbox_scope)
            .execute(&db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = agent_tests::router().with_state(state);
    (app, snapshot, result)
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn shared_agent_runs_keep_historical_knowledge_in_the_execution_project(db: PgPool) {
    let (app, _, _) = setup(db.clone()).await;
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO projects (id,tenant_id,name,slug) VALUES ('{selected}','{tenant}','Selected','selected');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions)
          VALUES ('{tenant}','{selected}','manager','Manager','manager',ARRAY['ai:manage','knowledge:manage']);
        UPDATE memberships SET project_id=NULL,director_access=true WHERE id='{user}';
        INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at)
          VALUES ('{session}','{tenant}','{selected}','{user}','{user}',decode('{token:x}','hex'),decode('{csrf:x}','hex'),now()+interval '1 hour',now()+interval '2 hours');
        "#,
        tenant=id(1), selected=id(20), user=id(3), session=id(21),
        token=Sha256::digest(b"source-session"), csrf=Sha256::digest(b"source-csrf"),
    )).execute(&db).await.unwrap();
    let path = format!("/api/v1/ai/profiles/{}/test-runs", id(4));
    let run_path = format!("{path}/{}", id(6));
    let token = "password-session";
    let (status, body) = request_path(&app, Method::GET, &path, token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"], json!([]));
    for (method, endpoint, input) in [
        (Method::GET, run_path.clone(), None),
        (
            Method::GET,
            format!("{run_path}/sources/article:{}", id(8)),
            None,
        ),
        (Method::POST, format!("{run_path}/recommendations"), None),
        (
            Method::POST,
            format!("{run_path}/review"),
            Some(json!({"step":0,"verdict":"pass","note":"Checked"})),
        ),
    ] {
        let (status, body) = request_path(&app, method, &endpoint, token, input).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{endpoint}: {body}");
    }
    // New runs explicitly record the workspace whose knowledge they used.
    sqlx::query("UPDATE ai_test_runs SET snapshot=$2,result='{}' WHERE id=$1")
        .bind(id(6))
        .bind(json!({"knowledge_project_id":id(20),"knowledge":[],"scenario":{}}))
        .execute(&db)
        .await
        .unwrap();
    let (status, body) = request_path(&app, Method::GET, &run_path, token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["snapshot"]["knowledge"], json!([]));
    let (status, body) = request_path(&app, Method::GET, &path, token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn edits_only_authorized_used_sources_with_atomic_revision_checks(db: PgPool) {
    let (app, snapshot, result) = setup(db.clone()).await;
    let article = format!("article:{}", id(8));

    for (source, token, expected) in [
        ("instructions", "knowledge-only", StatusCode::FORBIDDEN),
        ("instructions", "scoped", StatusCode::NOT_FOUND),
        (article.as_str(), "ai-only", StatusCode::FORBIDDEN),
    ] {
        for method in [Method::GET, Method::PATCH] {
            let input = (method == Method::PATCH)
                .then(|| json!({"text":"Denied", "expected_revision":"4"}));
            assert_eq!(
                request(&app, method, source, token, input).await.0,
                expected
            );
        }
    }
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("article:{}", id(9)),
            "editor",
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let (status, current) = request(&app, Method::GET, "instructions", "editor", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["text"], "Current instructions");
    // Two saves from the same read cannot both replace the source.
    let (first, second) = tokio::join!(
        request(
            &app,
            Method::PATCH,
            "instructions",
            "editor",
            Some(json!({"text":"First change","expected_revision":current["revision"]}))
        ),
        request(
            &app,
            Method::PATCH,
            "instructions",
            "editor",
            Some(json!({"text":"Second change","expected_revision":current["revision"]}))
        ),
    );
    assert!(
        (first.0 == StatusCode::OK && second.0 == StatusCode::CONFLICT)
            || (first.0 == StatusCode::CONFLICT && second.0 == StatusCode::OK)
    );
    let (name, tools): (String, String) =
        sqlx::query_as("SELECT name,tool_instructions FROM ai_profiles WHERE id=$1")
            .bind(id(4))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(name, "Keep this name");
    assert_eq!(tools, "Keep tool instructions");

    let (status, current) = request(&app, Method::GET, &article, "editor", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["text"], "Current article");
    assert_eq!(current["revision"], "4");
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &article,
            "editor",
            Some(json!({"text":"Wrong stale write","expected_revision":"3"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &article,
        "editor",
        Some(json!({"text":"Updated article","expected_revision":"4"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["revision"], "5");
    assert_eq!(saved["title"], "Keep title");
    assert_eq!(saved["status"], "published");
    let publication_unchanged: bool = sqlx::query_scalar("SELECT published_at='2026-01-01T00:00:00Z'::timestamptz FROM knowledge_articles WHERE id=$1")
        .bind(id(8)).fetch_one(&db).await.unwrap();
    assert!(publication_unchanged);
    let (stored_snapshot, stored_result): (Value, Value) =
        sqlx::query_as("SELECT snapshot,result FROM ai_test_runs WHERE id=$1")
            .bind(id(6))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(stored_snapshot, snapshot);
    assert_eq!(stored_result, result);
    let audits: Vec<Value> = sqlx::query_scalar("SELECT metadata FROM audit_log WHERE action IN ('ai_profile.updated','knowledge_article.updated')")
        .fetch_all(&db).await.unwrap();
    assert_eq!(audits.len(), 2);
    assert!(
        audits
            .iter()
            .all(|metadata| metadata["test_run_id"] == id(6).to_string()
                && metadata.get("text").is_none())
    );

    sqlx::query("DELETE FROM ai_profile_knowledge_bases WHERE ai_profile_id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    for method in [Method::GET, Method::PATCH] {
        let input = (method == Method::PATCH)
            .then(|| json!({"text":"Revoked write","expected_revision":"5"}));
        assert_eq!(
            request(&app, method, &article, "editor", input).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn applies_exact_recommendation_batches_atomically_without_touching_other_fields(db: PgPool) {
    let (app, snapshot, result) = setup(db.clone()).await;
    let path = format!(
        "/api/v1/ai/profiles/{}/test-runs/{}/recommendations/apply",
        id(4),
        id(6)
    );
    let current = request(&app, Method::GET, "instructions", "editor", None)
        .await
        .1;
    let changes = json!([
        {"source_key":"instructions","expected_revision":current["revision"],"original_text":"Current","suggested_text":"New"},
        {"source_key":"instructions","expected_revision":current["revision"],"original_text":"instructions","suggested_text":"agent instructions"},
        {"source_key":format!("article:{}", id(8)),"expected_revision":"4","original_text":"Current","suggested_text":"Updated"}
    ]);
    for (token, expected) in [
        ("ai-only", StatusCode::FORBIDDEN),
        ("knowledge-only", StatusCode::FORBIDDEN),
        ("scoped", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            request_path(
                &app,
                Method::POST,
                &path,
                token,
                Some(json!({"changes":changes}))
            )
            .await
            .0,
            expected
        );
    }
    let mut stale = changes.clone();
    stale[2]["expected_revision"] = json!("3");
    let mut missing = changes.clone();
    missing[2]["original_text"] = json!("Missing");
    let mut overlapping = changes.clone();
    overlapping[1]["original_text"] = json!("Current instructions");
    let mut unread = changes.clone();
    unread[2]["source_key"] = json!(format!("article:{}", id(9)));
    for (changes, expected) in [
        (stale, StatusCode::CONFLICT),
        (missing, StatusCode::CONFLICT),
        (overlapping, StatusCode::CONFLICT),
        (unread, StatusCode::NOT_FOUND),
        (json!([]), StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            request_path(
                &app,
                Method::POST,
                &path,
                "editor",
                Some(json!({"changes":changes}))
            )
            .await
            .0,
            expected
        );
    }
    let before: (String, String, i64) = sqlx::query_as(
        "SELECT p.instructions,a.body,a.version FROM ai_profiles p JOIN knowledge_articles a ON a.id=$2 WHERE p.id=$1",
    ).bind(id(4)).bind(id(8)).fetch_one(&db).await.unwrap();
    assert_eq!(
        before,
        ("Current instructions".into(), "Current article".into(), 4)
    );
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(audits, 0);

    // Concurrent application of the same batch can succeed only once.
    let (first, second) = tokio::join!(
        request_path(
            &app,
            Method::POST,
            &path,
            "editor",
            Some(json!({"changes":changes}))
        ),
        request_path(
            &app,
            Method::POST,
            &path,
            "editor",
            Some(json!({"changes":changes}))
        ),
    );
    let saved = if first.0 == StatusCode::OK {
        assert_eq!(second.0, StatusCode::CONFLICT);
        first.1
    } else {
        assert_eq!(first.0, StatusCode::CONFLICT);
        assert_eq!(second.0, StatusCode::OK);
        second.1
    };
    let sources = saved["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0]["source_key"], "instructions");
    assert_eq!(sources[0]["text"], "New agent instructions");
    assert_eq!(sources[1]["source_key"], format!("article:{}", id(8)));
    assert_eq!(sources[1]["text"], "Updated article");
    assert_eq!(sources[1]["revision"], "5");
    assert_eq!(sources[1]["title"], "Keep title");
    assert_eq!(sources[1]["status"], "published");
    let unchanged: bool = sqlx::query_scalar(
        "SELECT p.name='Keep this name' AND p.tool_instructions='Keep tool instructions' AND a.published_at='2026-01-01T00:00:00Z'::timestamptz FROM ai_profiles p JOIN knowledge_articles a ON a.id=$2 WHERE p.id=$1",
    ).bind(id(4)).bind(id(8)).fetch_one(&db).await.unwrap();
    assert!(unchanged);
    let stored: (Value, Value) =
        sqlx::query_as("SELECT snapshot,result FROM ai_test_runs WHERE id=$1")
            .bind(id(6))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(stored, (snapshot, result));
    let audits: Vec<Value> = sqlx::query_scalar("SELECT metadata FROM audit_log ORDER BY id")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[0]["recommendations_applied"], 2);
    assert_eq!(audits[1]["recommendations_applied"], 1);
    assert!(
        audits
            .iter()
            .all(|entry| entry["test_run_id"] == id(6).to_string()
                && entry.get("original_text").is_none()
                && entry.get("suggested_text").is_none())
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn inbox_scope_cannot_modify_an_api_agent_even_when_all_channels_are_visible(db: PgPool) {
    let (app, _, _) = setup(db.clone()).await;
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO inboxes (id, tenant_id, project_id, name)
          VALUES ('{inbox}', '{tenant}', '{project}', 'Visible inbox');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name, status)
          VALUES ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}', 'widget', 'Visible channel', 'active');
        INSERT INTO ai_profile_channel_connections (tenant_id, ai_profile_id, channel_connection_id)
          VALUES ('{tenant}', '{profile}', '{channel}');
        INSERT INTO ai_api_settings (tenant_id, project_id, ai_profile_id, enabled)
          VALUES ('{tenant}', '{project}', '{profile}', true);
        "#,
        tenant = id(1), project = id(2), profile = id(4), inbox = id(99), channel = id(100),
    )).execute(&db).await.unwrap();
    let current = request(&app, Method::GET, "instructions", "editor", None)
        .await
        .1;
    assert_eq!(
        request(&app, Method::GET, "instructions", "scoped", None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "instructions",
            "scoped",
            Some(json!({
                "text":"Unauthorized change","expected_revision":current["revision"]
            }))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request_path(
            &app,
            Method::POST,
            &format!(
                "/api/v1/ai/profiles/{}/test-runs/{}/recommendations/apply",
                id(4),
                id(6)
            ),
            "scoped",
            Some(json!({"changes":[{
                "source_key":"instructions","expected_revision":current["revision"],
                "original_text":"Current","suggested_text":"Unauthorized"
            }]}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let unchanged = request(&app, Method::GET, "instructions", "editor", None)
        .await
        .1;
    assert_eq!(unchanged, current);
}
