use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_settings};
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
async fn backups_restore_content_isolate_shared_bases_and_enforce_scope(db: PgPool) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Backups');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'backup@example.test', 'Manager');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
          ('{project}', '{tenant}', 'Project', 'project'), ('{other_project}', '{tenant}', 'Other', 'other');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
          VALUES ('{tenant}', '{project}', 'manager', 'Manager', 'manager', ARRAY['ai:manage', 'knowledge:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
          VALUES ('{user}', '{tenant}', '{project}', '{user}', 'manager');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, instructions, created_by) VALUES
          ('{profile}', '{tenant}', '{project}', 'Agent', 'Original instructions {article}', '{user}'),
          ('{shared_profile}', '{tenant}', '{project}', 'Shared agent', 'Other instructions', '{user}'),
          ('{foreign_profile}', '{tenant}', '{other_project}', 'Outside project', '', '{user}');
        INSERT INTO knowledge_bases (id, tenant_id, project_id, name, created_by) VALUES
          ('{base}', '{tenant}', '{project}', 'Knowledge', '{user}'),
          ('{foreign_base}', '{tenant}', '{other_project}', 'Foreign knowledge', '{user}');
        INSERT INTO knowledge_articles (id, tenant_id, project_id, knowledge_base_id, title, body, status, source_url, version, published_at, created_by, updated_by) VALUES
          ('{article}', '{tenant}', '{project}', '{base}', 'Published', '**Original**', 'published', 'https://example.test/source', 3, now(), '{user}', '{user}'),
          ('{draft}', '{tenant}', '{project}', '{base}', 'Draft', 'See {article}', 'draft', NULL, 2, NULL, '{user}', '{user}'),
          ('{foreign_article}', '{tenant}', '{other_project}', '{foreign_base}', 'Foreign article', 'Private foreign content', 'published', NULL, 1, now(), '{user}', '{user}');
        UPDATE ai_profiles SET description = 'Original purpose', tool_instructions = 'Read {article}', blacklist_reply_text = 'Original blocked reply', blacklist_reply_match_language = false WHERE id = '{profile}';
        INSERT INTO ai_profile_knowledge_bases (tenant_id, ai_profile_id, knowledge_base_id) VALUES
          ('{tenant}', '{profile}', '{base}'), ('{tenant}', '{shared_profile}', '{base}'),
          ('{tenant}', '{profile}', '{foreign_base}');
    "#,
        tenant = id(1), project = id(2), user = id(3), profile = id(4), base = id(5),
        article = id(6), draft = id(7), shared_profile = id(8), other_project = id(9), foreign_profile = id(10),
        foreign_base = id(11), foreign_article = id(12),
    )).execute(&db).await.unwrap();
    for (token, permissions, scope) in [
        ("manager", vec!["ai:manage", "knowledge:manage"], None),
        ("ai-only", vec!["ai:manage"], None),
        ("knowledge-only", vec!["knowledge:manage"], None),
        (
            "scoped",
            vec!["ai:manage", "knowledge:manage"],
            Some(vec![id(99)]),
        ),
    ] {
        sqlx::query("INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, inbox_scope, role, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'manager', now() + interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(3)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(permissions).bind(scope)
            .execute(&db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = ai_settings::router().with_state(state);
    let path = format!("/api/v1/ai/profiles/{}/backups", id(4));
    let (_, empty) = request(&app, Method::GET, &path, "manager", None).await;
    assert_eq!(empty, json!({"items": []}));
    let (status, created) = request(&app, Method::POST, &path, "manager", None).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (_, listed) = request(&app, Method::GET, &path, "manager", None).await;
    let backup = listed["items"][0]["id"].as_str().unwrap();
    assert_eq!(listed["items"][0]["article_count"], 2);
    assert_eq!(listed["items"][0]["knowledge_base_count"], 1);
    // Legacy backups may retain project sharing settings; restoring keeps the owner.
    sqlx::query("UPDATE ai_profile_backups SET snapshot=jsonb_set(snapshot,'{knowledge_bases,0,visibility}',$2) WHERE id=$1")
        .bind(Uuid::parse_str(backup).unwrap())
        .bind(json!({"project_ids":[id(9)],"department_ids":[]}))
        .execute(&db)
        .await
        .unwrap();
    let restore = format!("{path}/{backup}/restore");
    let delete = format!("{path}/{backup}");
    for (token, expected) in [
        ("ai-only", StatusCode::FORBIDDEN),
        ("knowledge-only", StatusCode::FORBIDDEN),
        ("scoped", StatusCode::NOT_FOUND),
    ] {
        for (method, endpoint) in [
            (Method::GET, &path),
            (Method::POST, &path),
            (Method::POST, &restore),
            (Method::DELETE, &delete),
        ] {
            let (status, _) = request(&app, method, endpoint, token, None).await;
            assert_eq!(status, expected, "{token}");
        }
    }
    for inaccessible in [id(8), id(10), id(99)] {
        let (status, _) = request(
            &app,
            Method::POST,
            &format!("/api/v1/ai/profiles/{inaccessible}/backups/{backup}/restore"),
            "manager",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = request(
            &app,
            Method::DELETE,
            &format!("/api/v1/ai/profiles/{inaccessible}/backups/{backup}"),
            "manager",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    sqlx::query("UPDATE ai_profiles SET description = 'Edited purpose', instructions = 'Edited instructions', blacklist_reply_text = 'Edited blocked reply', blacklist_reply_match_language = true WHERE id = $1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE knowledge_articles SET body = 'Edited shared content' WHERE id = $1")
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    let (status, restored) = request(&app, Method::POST, &restore, "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    let restored_base: Uuid = restored["knowledge_base_ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(restored_base, id(5));
    let restored_visibility: Value =
        sqlx::query_scalar("SELECT visibility FROM knowledge_bases WHERE id=$1")
            .bind(restored_base)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(restored_visibility["project_ids"], json!([id(2)]));
    let restored_article: Uuid = sqlx::query_scalar(
        "SELECT id FROM knowledge_articles WHERE knowledge_base_id = $1 AND title = 'Published'",
    )
    .bind(restored_base)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        restored["instructions"],
        format!("Original instructions {restored_article}")
    );
    assert_eq!(
        restored["tool_instructions"],
        format!("Read {restored_article}")
    );
    assert_eq!(restored["blacklist_reply_text"], "Original blocked reply");
    assert_eq!(restored["description"], "Original purpose");
    assert_eq!(restored["blacklist_reply_match_language"], false);
    let articles: Vec<(String, String, String, i64, Option<String>, bool)> = sqlx::query_as("SELECT title, body, status, version, source_url, published_at IS NOT NULL FROM knowledge_articles WHERE knowledge_base_id = $1 ORDER BY title")
        .bind(restored_base).fetch_all(&db).await.unwrap();
    assert_eq!(
        articles,
        vec![
            (
                "Draft".into(),
                format!("See {restored_article}"),
                "draft".into(),
                2,
                None,
                false
            ),
            (
                "Published".into(),
                "**Original**".into(),
                "published".into(),
                3,
                Some("https://example.test/source".into()),
                true
            ),
        ]
    );
    let shared: (Uuid, String) = sqlx::query_as("SELECT assignment.knowledge_base_id, article.body FROM ai_profile_knowledge_bases assignment JOIN knowledge_articles article ON article.knowledge_base_id = assignment.knowledge_base_id WHERE assignment.ai_profile_id = $1 AND article.id = $2")
        .bind(id(8)).bind(id(6)).fetch_one(&db).await.unwrap();
    assert_eq!(shared, (id(5), "Edited shared content".into()));
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT body FROM knowledge_articles WHERE id = $1")
            .bind(id(7))
            .fetch_one(&db)
            .await
            .unwrap(),
        format!("See {}", id(6))
    );
    let (_, copies) = request(&app, Method::GET, &path, "manager", None).await;
    assert_eq!(copies["items"].as_array().unwrap().len(), 2);
    let safety = copies["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["reason"] == "before_restore")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, rolled_back) = request(
        &app,
        Method::POST,
        &format!("{path}/{safety}/restore"),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rolled_back}");
    assert_eq!(rolled_back["instructions"], "Edited instructions");
    assert_eq!(rolled_back["blacklist_reply_text"], "Edited blocked reply");
    assert_eq!(rolled_back["blacklist_reply_match_language"], true);
    assert_eq!(rolled_back["description"], "Edited purpose");
    // A backup stays usable after the original base and all of its articles are deleted.
    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    let (status, restored_again) = request(&app, Method::POST, &restore, "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{restored_again}");
    let restored_again_base: Uuid = restored_again["knowledge_base_ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let restored_again_article: Uuid = sqlx::query_scalar(
        "SELECT id FROM knowledge_articles WHERE knowledge_base_id = $1 AND title = 'Published'",
    )
    .bind(restored_again_base)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        restored_again["instructions"],
        format!("Original instructions {restored_again_article}")
    );
    // Failure midway through restoration must roll back the safety copy and assignments too.
    let backup_count: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_profile_backups")
        .fetch_one(&db)
        .await
        .unwrap();
    let base_count: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_bases")
        .fetch_one(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE ai_profile_backups SET snapshot = jsonb_set(snapshot, '{knowledge_bases,0,articles,0,status}', '\"invalid\"'::jsonb) WHERE id = $1")
        .bind(backup.parse::<Uuid>().unwrap()).execute(&db).await.unwrap();
    let (status, _) = request(&app, Method::POST, &restore, "manager", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let current_bases: Vec<Uuid> = sqlx::query_scalar(
        "SELECT knowledge_base_id FROM ai_profile_knowledge_bases WHERE ai_profile_id = $1",
    )
    .bind(id(4))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(json!(current_bases), restored_again["knowledge_base_ids"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ai_profile_backups")
            .fetch_one(&db)
            .await
            .unwrap(),
        backup_count
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM knowledge_bases")
            .fetch_one(&db)
            .await
            .unwrap(),
        base_count
    );
    let profile_before_delete: Value =
        sqlx::query_scalar("SELECT to_jsonb(profile) FROM ai_profiles profile WHERE id = $1")
            .bind(id(4))
            .fetch_one(&db)
            .await
            .unwrap();
    let articles_before_delete: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(article) ORDER BY article.id) FROM knowledge_articles article",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    let (status, body) = request(&app, Method::DELETE, &delete, "manager", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, remaining) = request(&app, Method::GET, &path, "manager", None).await;
    assert!(
        remaining["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != backup)
    );
    let (status, _) = request(&app, Method::DELETE, &delete, "manager", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request(&app, Method::POST, &restore, "manager", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        sqlx::query_scalar::<_, Value>(
            "SELECT to_jsonb(profile) FROM ai_profiles profile WHERE id = $1",
        )
        .bind(id(4))
        .fetch_one(&db)
        .await
        .unwrap(),
        profile_before_delete
    );
    assert_eq!(
        sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_agg(to_jsonb(article) ORDER BY article.id) FROM knowledge_articles article",
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        articles_before_delete
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE action = 'ai_profile.backup_deleted' AND resource_id = $1",
        )
        .bind(backup.parse::<Uuid>().unwrap())
        .fetch_one(&db)
        .await
        .unwrap(),
        1
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn backups_restore_existing_knowledge_identity_and_legacy_copies(db: PgPool) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Backups');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'backup@example.test', 'Manager');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Project', 'project');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
          VALUES ('{tenant}', '{project}', 'manager', 'Manager', 'manager', ARRAY['ai:manage', 'knowledge:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
          VALUES ('{user}', '{tenant}', '{project}', '{user}', 'manager');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, instructions, tool_instructions, created_by)
          VALUES ('{profile}', '{tenant}', '{project}', 'Agent', 'Original instructions', 'Original tools', '{user}');
        INSERT INTO knowledge_bases (id, tenant_id, project_id, name, description, created_by)
          VALUES ('{base}', '{tenant}', '{project}', 'Knowledge', 'Original description', '{user}');
        INSERT INTO knowledge_articles (id, tenant_id, project_id, knowledge_base_id, title, body, status, source_url, version, published_at, created_by, updated_by) VALUES
          ('{article}', '{tenant}', '{project}', '{base}', 'Published', '**Original**', 'published', 'https://example.test/source', 3, now(), '{user}', '{user}'),
          ('{draft}', '{tenant}', '{project}', '{base}', 'Draft', 'Unpublished content', 'draft', NULL, 2, NULL, '{user}', '{user}');
        INSERT INTO ai_profile_knowledge_bases (tenant_id, ai_profile_id, knowledge_base_id)
          VALUES ('{tenant}', '{profile}', '{base}');
    "#,
        tenant = id(1), project = id(2), user = id(3), profile = id(4), base = id(5),
        article = id(6), draft = id(7),
    )).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, role, expires_at) VALUES ($1, $2, $3, $4, 'manager', $5, ARRAY['ai:manage', 'knowledge:manage'], 'manager', now() + interval '1 hour')")
        .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(3))
        .bind(Sha256::digest(b"manager").to_vec()).execute(&db).await.unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = ai_settings::router().with_state(state);
    let path = format!("/api/v1/ai/profiles/{}/backups", id(4));
    let (status, created) = request(&app, Method::POST, &path, "manager", None).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (_, listed) = request(&app, Method::GET, &path, "manager", None).await;
    let backup: Uuid = listed["items"][0]["id"].as_str().unwrap().parse().unwrap();
    let restore = format!("{path}/{backup}/restore");
    let mut snapshot: Value =
        sqlx::query_scalar("SELECT snapshot FROM ai_profile_backups WHERE id = $1")
            .bind(backup)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(snapshot["knowledge_bases"][0]["id"], json!(id(5)));
    assert_eq!(
        snapshot["knowledge_bases"][0]["articles"][0]["id"],
        json!(id(6))
    );
    sqlx::raw_sql(&format!(r#"
        UPDATE ai_profiles SET instructions = 'Edited instructions', tool_instructions = 'Edited tools' WHERE id = '{profile}';
        UPDATE knowledge_bases SET name = 'Renamed knowledge', description = 'Edited description' WHERE id = '{base}';
        UPDATE knowledge_articles SET title = 'Renamed article', body = 'Edited content', version = 8 WHERE id = '{article}';
        DELETE FROM knowledge_articles WHERE id = '{draft}';
        INSERT INTO knowledge_articles (id, tenant_id, project_id, knowledge_base_id, title, body, created_by, updated_by)
          VALUES ('{added}', '{tenant}', '{project}', '{base}', 'Added after backup', 'Extra content', '{user}', '{user}');
    "#,
        tenant = id(1), project = id(2), user = id(3), profile = id(4), base = id(5),
        article = id(6), draft = id(7), added = id(8),
    )).execute(&db).await.unwrap();
    let (status, restored) = request(
        &app,
        Method::POST,
        &restore,
        "manager",
        Some(json!({ "create_backup": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["instructions"], "Original instructions");
    assert_eq!(restored["tool_instructions"], "Original tools");
    assert_eq!(restored["knowledge_base_ids"], json!([id(5)]));
    let base: (String, String) =
        sqlx::query_as("SELECT name, description FROM knowledge_bases WHERE id = $1")
            .bind(id(5))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(base, ("Knowledge".into(), "Original description".into()));
    let articles: Vec<(Uuid, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, title, body, status, version FROM knowledge_articles WHERE knowledge_base_id = $1 ORDER BY id",
    )
    .bind(id(5))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        articles,
        vec![
            (
                id(6),
                "Published".into(),
                "**Original**".into(),
                "published".into(),
                9
            ),
            (
                id(7),
                "Draft".into(),
                "Unpublished content".into(),
                "draft".into(),
                2
            ),
        ]
    );
    let (_, copies) = request(&app, Method::GET, &path, "manager", None).await;
    assert_eq!(copies["items"].as_array().unwrap().len(), 1);

    // Omitting the option keeps the default safety copy and reuses the same knowledge.
    let (status, repeated) =
        request(&app, Method::POST, &restore, "manager", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(repeated["knowledge_base_ids"], json!([id(5)]));
    let (_, copies) = request(&app, Method::GET, &path, "manager", None).await;
    assert_eq!(copies["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        copies["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["reason"] == "before_restore")
            .count(),
        1
    );

    // Copies produced before IDs were saved match the unchanged base name and unique titles.
    snapshot.as_object_mut().unwrap().remove("description");
    snapshot
        .as_object_mut()
        .unwrap()
        .remove("blacklist_reply_text");
    snapshot
        .as_object_mut()
        .unwrap()
        .remove("blacklist_reply_match_language");
    let legacy_base = snapshot["knowledge_bases"][0].as_object_mut().unwrap();
    legacy_base.remove("id");
    for article in legacy_base["articles"].as_array_mut().unwrap() {
        article.as_object_mut().unwrap().remove("id");
    }
    sqlx::query("UPDATE ai_profile_backups SET snapshot = $2 WHERE id = $1")
        .bind(backup)
        .bind(snapshot)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE knowledge_articles SET body = 'Edited again', version = 12 WHERE id = $1")
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE ai_profiles SET description = 'Keep current purpose', blacklist_reply_text = 'Keep current blocked reply', blacklist_reply_match_language = false WHERE id = $1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    let (status, legacy_restored) = request(
        &app,
        Method::POST,
        &restore,
        "manager",
        Some(json!({ "create_backup": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{legacy_restored}");
    assert_eq!(
        legacy_restored["blacklist_reply_text"],
        "Keep current blocked reply"
    );
    assert_eq!(legacy_restored["blacklist_reply_match_language"], false);
    assert_eq!(legacy_restored["description"], "Keep current purpose");
    assert_eq!(legacy_restored["knowledge_base_ids"], json!([id(5)]));
    let article: (String, i64) =
        sqlx::query_as("SELECT body, version FROM knowledge_articles WHERE id = $1")
            .bind(id(6))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(article, ("**Original**".into(), 13));
    let article_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knowledge_articles WHERE knowledge_base_id = $1 ORDER BY id",
    )
    .bind(id(5))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(article_ids, vec![id(6), id(7)]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM knowledge_bases")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    let (_, copies) = request(&app, Method::GET, &path, "manager", None).await;
    assert_eq!(copies["items"].as_array().unwrap().len(), 2);

    // A detached base with the legacy name is not an authorized restore target.
    sqlx::query("DELETE FROM ai_profile_knowledge_bases WHERE ai_profile_id = $1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE knowledge_articles SET body = 'Detached content' WHERE id = $1")
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    let (status, isolated) = request(
        &app,
        Method::POST,
        &restore,
        "manager",
        Some(json!({ "create_backup": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{isolated}");
    let isolated_base: Uuid = isolated["knowledge_base_ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(isolated_base, id(5));
    let isolated_article_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knowledge_articles WHERE knowledge_base_id = $1 ORDER BY id",
    )
    .bind(isolated_base)
    .fetch_all(&db)
    .await
    .unwrap();
    let (status, repeated_clone) = request(
        &app,
        Method::POST,
        &restore,
        "manager",
        Some(json!({ "create_backup": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{repeated_clone}");
    assert_eq!(repeated_clone["knowledge_base_ids"], json!([isolated_base]));
    assert_eq!(
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM knowledge_articles WHERE knowledge_base_id = $1 ORDER BY id",
        )
        .bind(isolated_base)
        .fetch_all(&db)
        .await
        .unwrap(),
        isolated_article_ids
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM knowledge_bases")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT body FROM knowledge_articles WHERE id = $1")
            .bind(id(6))
            .fetch_one(&db)
            .await
            .unwrap(),
        "Detached content"
    );

    // Duplicate legacy titles must not collapse two articles or guess which ID a reference means.
    let mut ambiguous: Value =
        sqlx::query_scalar("SELECT snapshot FROM ai_profile_backups WHERE id = $1")
            .bind(backup)
            .fetch_one(&db)
            .await
            .unwrap();
    ambiguous["instructions"] = json!(format!("Read {}", id(6)));
    ambiguous["knowledge_bases"][0]["articles"][1]["title"] = json!("Published");
    ambiguous["knowledge_bases"][0]["articles"][1]["body"] = json!("Second legacy article");
    sqlx::query("UPDATE ai_profile_backups SET snapshot = $2 WHERE id = $1")
        .bind(backup)
        .bind(ambiguous)
        .execute(&db)
        .await
        .unwrap();
    let (status, duplicate_titles) = request(
        &app,
        Method::POST,
        &restore,
        "manager",
        Some(json!({ "create_backup": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{duplicate_titles}");
    assert_eq!(duplicate_titles["instructions"], format!("Read {}", id(6)));
    let duplicates: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, title, body FROM knowledge_articles WHERE knowledge_base_id = $1 ORDER BY body",
    )
    .bind(isolated_base)
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(duplicates.len(), 2);
    assert_eq!(duplicates[0].1, "Published");
    assert_eq!(duplicates[0].2, "**Original**");
    assert_eq!(duplicates[1].1, "Published");
    assert_eq!(duplicates[1].2, "Second legacy article");
    assert!(
        duplicates
            .iter()
            .all(|(id, _, _)| !isolated_article_ids.contains(id))
    );
}
