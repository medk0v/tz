use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, note_shares, notes};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(
        "INSERT INTO tenants (id, name) VALUES ('{}', 'Notes'), ('{}', 'Other tenant');
         INSERT INTO projects (id, tenant_id, name, slug) VALUES
             ('{}', '{}', 'First', 'first'), ('{}', '{}', 'Second', 'second'),
             ('{}', '{}', 'Other tenant', 'other');",
        id(1),
        id(100),
        id(2),
        id(1),
        id(3),
        id(1),
        id(101),
        id(100),
    ))
    .execute(db)
    .await
    .unwrap();

    for (token, user_id, tenant_id, project_id, permissions) in [
        ("writer", 10, 1, 2, vec!["notes:read", "notes:write"]),
        ("reader", 11, 1, 2, vec!["notes:read"]),
        ("denied", 12, 1, 2, vec![]),
        ("other-project", 13, 1, 3, vec!["notes:read", "notes:write"]),
        (
            "other-tenant",
            14,
            100,
            101,
            vec!["notes:read", "notes:write"],
        ),
    ] {
        sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)")
            .bind(id(user_id))
            .bind(format!("{token}@notes.test"))
            .bind(token)
            .execute(db)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
             VALUES ($1, $2, $3, $3, 'operator', $4)",
        )
        .bind(id(tenant_id))
        .bind(id(project_id))
        .bind(token)
        .bind(&permissions)
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
             VALUES ($1, $2, $3, $1, $4)",
        )
        .bind(id(user_id))
        .bind(id(tenant_id))
        .bind(id(project_id))
        .bind(token)
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name,
                 token_hash, permissions, role, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $5, now() + interval '1 hour')",
        )
        .bind(Uuid::now_v7())
        .bind(id(tenant_id))
        .bind(id(project_id))
        .bind(id(user_id))
        .bind(token)
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .bind(&permissions)
        .execute(db)
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
    notes::router()
        .merge(note_shares::router())
        .with_state(state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn create(app: &Router, token: &str, title: &str, parent_id: Option<&str>) -> Value {
    let (status, body) = request(
        app,
        Method::POST,
        "/api/v1/notes",
        token,
        Some(json!({"title": title, "parent_id": parent_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

fn path(note: &Value) -> String {
    format!("/api/v1/notes/{}", note["id"].as_str().unwrap())
}

fn update(note: &Value, parent_id: Option<&str>) -> Value {
    json!({
        "title": note["title"], "body": note["body"], "parent_id": parent_id,
        "icon": note["icon"], "is_favorite": note["is_favorite"],
        "expected_version": note["version"]
    })
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn notes_are_shared_within_project_but_isolated_across_projects_and_tenants(db: PgPool) {
    let app = fixture(&db).await;
    let note = create(&app, "writer", "  Shared page  ", None).await;
    assert_eq!(note["title"], "Shared page");
    assert_eq!(note["body"], "");
    assert_eq!(note["version"], 1);
    assert_eq!(note["parent_id"], Value::Null);
    let other = create(&app, "other-project", "Project two", None).await;
    create(&app, "other-tenant", "Tenant two", None).await;

    let (status, list) = request(&app, Method::GET, "/api/v1/notes", "reader", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["id"], note["id"]);
    assert!(list["items"][0].get("body").is_none());
    assert_eq!(
        request(&app, Method::GET, &path(&note), "reader", None).await,
        (StatusCode::OK, note.clone())
    );

    for token in ["other-project", "other-tenant"] {
        for method in [Method::GET, Method::PATCH, Method::DELETE] {
            let request_path = if method == Method::DELETE {
                format!("{}?expected_version=1", path(&note))
            } else {
                path(&note)
            };
            let input = (method == Method::PATCH).then(|| update(&note, None));
            let (status, body) = request(&app, method, &request_path, token, input).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{token}: {body}");
        }
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/notes",
            token,
            Some(json!({"title": "Foreign child", "parent_id": note["id"]})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    let (status, body) = request(
        &app,
        Method::PATCH,
        &path(&note),
        "writer",
        Some(update(&note, other["id"].as_str())),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn role_permissions_control_reads_and_writes(db: PgPool) {
    let app = fixture(&db).await;
    let note = create(&app, "writer", "Permission test", None).await;
    for token in ["reader", "denied"] {
        for (method, request_path, input) in [
            (
                Method::POST,
                "/api/v1/notes".to_owned(),
                Some(json!({"title": "Forbidden"})),
            ),
            (Method::PATCH, path(&note), Some(update(&note, None))),
            (
                Method::DELETE,
                format!("{}?expected_version=1", path(&note)),
                None,
            ),
        ] {
            let (status, body) = request(&app, method, &request_path, token, input).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{token}: {body}");
        }
    }
    for request_path in ["/api/v1/notes".to_owned(), path(&note)] {
        let (status, body) = request(&app, Method::GET, &request_path, "denied", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn a_tenant_session_must_select_a_project(db: PgPool) {
    let app = fixture(&db).await;
    sqlx::raw_sql(&format!(
        "INSERT INTO users (id, email, display_name) VALUES ('{user}', 'tenant@notes.test', 'Tenant');
         INSERT INTO memberships (id, tenant_id, user_id, role, permissions)
             VALUES ('{membership}', '{tenant}', '{user}', 'admin', ARRAY['notes:read', 'notes:write']);
         INSERT INTO operator_sessions (id, tenant_id, user_id, membership_id, token_hash,
             csrf_token_hash, idle_expires_at, absolute_expires_at)
             VALUES ('{session}', '{tenant}', '{user}', '{membership}', decode('{token:x}', 'hex'),
             decode('{csrf:x}', 'hex'), now() + interval '1 hour', now() + interval '2 hours');",
        tenant = id(1),
        user = id(50),
        membership = id(51),
        session = id(52),
        token = Sha256::digest(b"notes-tenant-session"),
        csrf = Sha256::digest(b"notes-csrf"),
    ))
    .execute(&db)
    .await
    .unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/notes")
                .header("cookie", "tzomet_session=notes-tenant-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 10_000).await.unwrap()).unwrap();
    assert_eq!(body["error"]["message"], "select a project first");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn stale_writes_and_deletes_preserve_the_latest_saved_content(db: PgPool) {
    let app = fixture(&db).await;
    let note = create(&app, "writer", "Versioned", None).await;
    let mut input = update(&note, None);
    input["body"] = json!("# Content\n\n- [x] Finished\n\n🙂");
    input["icon"] = json!("📘");
    input["is_favorite"] = json!(true);
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &path(&note),
        "writer",
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["version"], 2);
    assert_eq!(saved["body"], input["body"]);
    assert_eq!(saved["icon"], "📘");
    assert_eq!(saved["is_favorite"], true);
    assert_eq!(saved["created_at"], note["created_at"]);
    input["body"] = json!("Stale content");
    assert_eq!(
        request(&app, Method::PATCH, &path(&note), "writer", Some(input))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{}?expected_version=1", path(&note)),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, Method::GET, &path(&note), "writer", None).await,
        (StatusCode::OK, saved)
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{}?expected_version=2", path(&note)),
            "writer",
            None
        )
        .await,
        (StatusCode::NO_CONTENT, Value::Null)
    );
    assert_eq!(
        request(&app, Method::GET, &path(&note), "writer", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let events = sqlx::query_scalar::<_, String>(
        "SELECT action FROM audit_log WHERE tenant_id = $1 AND project_id = $2
         AND resource_kind = 'note' AND resource_id = $3 ORDER BY occurred_at, id",
    )
    .bind(id(1))
    .bind(id(2))
    .bind(Uuid::parse_str(note["id"].as_str().unwrap()).unwrap())
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(events, ["note.created", "note.updated", "note.deleted"]);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn hierarchy_rejects_cycles_and_requires_moving_children_before_deletion(db: PgPool) {
    let app = fixture(&db).await;
    let parent = create(&app, "writer", "Parent", None).await;
    let child = create(&app, "writer", "Child", parent["id"].as_str()).await;
    let grandchild = create(&app, "writer", "Grandchild", child["id"].as_str()).await;
    for parent_id in [parent["id"].as_str(), grandchild["id"].as_str()] {
        let (status, body) = request(
            &app,
            Method::PATCH,
            &path(&parent),
            "writer",
            Some(update(&parent, parent_id)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let delete_path = format!("{}?expected_version=1", path(&parent));
    assert_eq!(
        request(&app, Method::DELETE, &delete_path, "writer", None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, moved) = request(
        &app,
        Method::PATCH,
        &path(&child),
        "writer",
        Some(update(&child, None)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["parent_id"], Value::Null);
    assert_eq!(
        request(&app, Method::DELETE, &delete_path, "writer", None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, Method::GET, &path(&grandchild), "writer", None)
            .await
            .1["parent_id"],
        child["id"]
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn simultaneous_opposite_moves_cannot_create_a_cycle(db: PgPool) {
    let app = fixture(&db).await;
    let first = create(&app, "writer", "First", None).await;
    let second = create(&app, "writer", "Second", None).await;
    let first_path = path(&first);
    let second_path = path(&second);
    let (left, right) = tokio::join!(
        request(
            &app,
            Method::PATCH,
            &first_path,
            "writer",
            Some(update(&first, second["id"].as_str()))
        ),
        request(
            &app,
            Method::PATCH,
            &second_path,
            "writer",
            Some(update(&second, first["id"].as_str()))
        )
    );
    assert!(
        (left.0 == StatusCode::OK && right.0 == StatusCode::BAD_REQUEST)
            || (right.0 == StatusCode::OK && left.0 == StatusCode::BAD_REQUEST),
        "{left:?}, {right:?}"
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn simultaneous_saves_allow_only_one_writer_for_the_same_version(db: PgPool) {
    let app = fixture(&db).await;
    let note = create(&app, "writer", "Concurrent", None).await;
    let note_path = path(&note);
    let mut first = update(&note, None);
    first["body"] = json!("First writer");
    let mut second = update(&note, None);
    second["body"] = json!("Second writer");
    let (left, right) = tokio::join!(
        request(&app, Method::PATCH, &note_path, "writer", Some(first)),
        request(&app, Method::PATCH, &note_path, "writer", Some(second))
    );
    let winner = if left.0 == StatusCode::OK {
        assert_eq!(right.0, StatusCode::CONFLICT, "{right:?}");
        left.1
    } else {
        assert_eq!(left.0, StatusCode::CONFLICT, "{left:?}");
        assert_eq!(right.0, StatusCode::OK, "{right:?}");
        right.1
    };
    assert_eq!(winner["version"], 2);
    assert_eq!(
        request(&app, Method::GET, &note_path, "writer", None).await,
        (StatusCode::OK, winner)
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn parent_deletion_cannot_race_child_creation(db: PgPool) {
    let app = fixture(&db).await;
    let parent = create(&app, "writer", "Parent", None).await;
    let delete_path = format!("{}?expected_version=1", path(&parent));
    let (deleted, created) = tokio::join!(
        request(&app, Method::DELETE, &delete_path, "writer", None),
        request(
            &app,
            Method::POST,
            "/api/v1/notes",
            "writer",
            Some(json!({"title": "Child", "parent_id": parent["id"]}))
        )
    );
    assert!(
        (deleted.0 == StatusCode::NO_CONTENT && created.0 == StatusCode::NOT_FOUND)
            || (deleted.0 == StatusCode::CONFLICT && created.0 == StatusCode::CREATED),
        "{deleted:?}, {created:?}"
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn invalid_content_and_incomplete_updates_do_not_mutate_notes(db: PgPool) {
    let app = fixture(&db).await;
    for input in [
        json!({"title": "  "}),
        json!({"title": "x".repeat(201)}),
        json!({"title": "Title", "icon": "x".repeat(33)}),
        json!({"title": "Title", "body": "a".repeat(200_001)}),
        json!({"title": "Title", "body": "invalid\0content"}),
    ] {
        let (status, body) =
            request(&app, Method::POST, "/api/v1/notes", "writer", Some(input)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let note = create(&app, "writer", "Valid", None).await;
    let mut input = update(&note, None);
    input.as_object_mut().unwrap().remove("parent_id");
    assert_eq!(
        request(&app, Method::PATCH, &path(&note), "writer", Some(input))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(&app, Method::DELETE, &path(&note), "writer", None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, Method::GET, &path(&note), "writer", None).await,
        (StatusCode::OK, note)
    );
}

async fn move_page(
    app: &Router,
    token: &str,
    note: &Value,
    parent: Option<&str>,
    before: Option<&str>,
) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        &format!("{}/move", path(note)),
        token,
        Some(json!({"parent_id":parent,"before_id":before,"expected_version":note["version"]})),
    )
    .await
}

fn child_ids(list: &Value, parent: Option<&str>) -> Vec<Value> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|note| note["parent_id"].as_str() == parent)
        .map(|note| note["id"].clone())
        .collect()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn moving_notes_persists_root_and_nested_order_without_replacing_content(db: PgPool) {
    let app = fixture(&db).await;
    let first = create(&app, "writer", "Zulu", None).await;
    let second = create(&app, "writer", "Alpha", None).await;
    let third = create(&app, "writer", "Middle", None).await;
    assert_eq!(first["sort_order"], 0);
    assert_eq!(second["sort_order"], 1);
    let mut content = update(&third, None);
    content["body"] = json!("# Keep this body\n\nSaved **content**");
    content["icon"] = json!("📘");
    content["is_favorite"] = json!(true);
    let (status, third) =
        request(&app, Method::PATCH, &path(&third), "writer", Some(content)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(third["sort_order"], 2);
    let (status, moved) = move_page(&app, "writer", &third, None, first["id"].as_str()).await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(
        child_ids(&moved, None),
        vec![
            third["id"].clone(),
            first["id"].clone(),
            second["id"].clone()
        ]
    );
    assert_eq!(moved["note"]["version"], 3);
    for property in ["title", "body", "icon", "is_favorite", "created_at"] {
        assert_eq!(moved["note"][property], third[property]);
    }
    for summary in moved["items"].as_array().unwrap() {
        assert!(summary.get("body").is_none());
        if summary["id"] == first["id"] {
            assert_eq!(summary["version"], first["version"]);
            assert_eq!(summary["updated_at"], first["updated_at"]);
        }
    }
    let reloaded = request(&app, Method::GET, "/api/v1/notes", "reader", None)
        .await
        .1;
    assert_eq!(reloaded["items"], moved["items"]);
    assert_eq!(
        request(&app, Method::GET, &path(&third), "reader", None)
            .await
            .1,
        moved["note"]
    );
    let parent = create(&app, "writer", "Parent", None).await;
    let existing = create(&app, "writer", "Existing child", parent["id"].as_str()).await;
    let (status, nested) = move_page(
        &app,
        "writer",
        &first,
        parent["id"].as_str(),
        existing["id"].as_str(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{nested}");
    assert_eq!(
        child_ids(&nested, parent["id"].as_str()),
        vec![first["id"].clone(), existing["id"].clone()]
    );
    let appended = create(&app, "writer", "New child", parent["id"].as_str()).await;
    assert_eq!(appended["sort_order"], 2);
    let (status, patched) = request(
        &app,
        Method::PATCH,
        &path(&second),
        "writer",
        Some(update(&second, parent["id"].as_str())),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["sort_order"], 3);
    let (status, restored) = move_page(&app, "writer", &nested["note"], None, None).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(
        child_ids(&restored, None),
        vec![
            third["id"].clone(),
            parent["id"].clone(),
            first["id"].clone()
        ]
    );
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='note.moved' AND tenant_id=$1 AND project_id=$2")
        .bind(id(1)).bind(id(2)).fetch_one(&db).await.unwrap();
    assert_eq!(audits, 3);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn moves_reject_foreign_targets_invalid_anchors_cycles_and_stale_versions(db: PgPool) {
    let app = fixture(&db).await;
    let parent = create(&app, "writer", "Parent", None).await;
    let child = create(&app, "writer", "Child", parent["id"].as_str()).await;
    let other = create(&app, "other-project", "Other project", None).await;
    let foreign = create(&app, "other-tenant", "Other tenant", None).await;
    for note in [&other, &foreign] {
        assert_eq!(
            move_page(&app, "writer", note, None, None).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            move_page(&app, "writer", &parent, note["id"].as_str(), None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            move_page(&app, "writer", &parent, None, note["id"].as_str())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    for token in ["reader", "denied"] {
        assert_eq!(
            move_page(&app, token, &parent, None, None).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        move_page(&app, "writer", &parent, child["id"].as_str(), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        move_page(&app, "writer", &parent, parent["id"].as_str(), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        move_page(&app, "writer", &parent, None, parent["id"].as_str())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        move_page(&app, "writer", &parent, None, child["id"].as_str())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app, Method::GET, &path(&parent), "reader", None)
            .await
            .1,
        parent
    );
    let (status, saved) = move_page(&app, "writer", &child, None, parent["id"].as_str()).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        move_page(&app, "writer", &child, parent["id"].as_str(), None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, Method::GET, &path(&child), "reader", None)
            .await
            .1,
        saved["note"]
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn concurrent_moves_keep_one_source_version_and_cannot_create_cycles(db: PgPool) {
    let app = fixture(&db).await;
    let parent = create(&app, "writer", "Parent", None).await;
    let sibling = create(&app, "writer", "Sibling", None).await;
    let (status, source) = request(
        &app,
        Method::POST,
        "/api/v1/notes",
        "writer",
        Some(json!({"title":"Source","body":"Concurrent body stays intact"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (left, right) = tokio::join!(
        move_page(&app, "writer", &source, parent["id"].as_str(), None),
        move_page(&app, "writer", &source, None, sibling["id"].as_str()),
    );
    let winner = if left.0 == StatusCode::OK {
        assert_eq!(right.0, StatusCode::CONFLICT, "{right:?}");
        left.1
    } else {
        assert_eq!(left.0, StatusCode::CONFLICT, "{left:?}");
        assert_eq!(right.0, StatusCode::OK, "{right:?}");
        right.1
    };
    assert_eq!(winner["note"]["body"], source["body"]);
    assert_eq!(winner["note"]["version"], 2);
    assert_eq!(
        request(&app, Method::GET, &path(&source), "reader", None)
            .await
            .1,
        winner["note"]
    );
    let (left, right) = tokio::join!(
        move_page(&app, "writer", &parent, sibling["id"].as_str(), None),
        move_page(&app, "writer", &sibling, parent["id"].as_str(), None),
    );
    assert!(
        (left.0 == StatusCode::OK && right.0 == StatusCode::BAD_REQUEST)
            || (right.0 == StatusCode::OK && left.0 == StatusCode::BAD_REQUEST),
        "{left:?}, {right:?}"
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn ordering_migration_backfills_each_parent_alphabetically_without_touching_content(
    db: PgPool,
) {
    let app = fixture(&db).await;
    let zulu = create(&app, "writer", "Zulu", None).await;
    let alpha = create(&app, "writer", "alpha", None).await;
    let zed = create(&app, "writer", "Zed", zulu["id"].as_str()).await;
    let beta = create(&app, "writer", "beta", zulu["id"].as_str()).await;
    sqlx::query("ALTER TABLE notes DROP COLUMN sort_order")
        .execute(&db)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0138_note_order.sql"))
        .execute(&db)
        .await
        .unwrap();
    let list = request(&app, Method::GET, "/api/v1/notes", "reader", None)
        .await
        .1;
    assert_eq!(
        child_ids(&list, None),
        vec![alpha["id"].clone(), zulu["id"].clone()]
    );
    assert_eq!(
        child_ids(&list, zulu["id"].as_str()),
        vec![beta["id"].clone(), zed["id"].clone()]
    );
    let current = request(&app, Method::GET, &path(&zulu), "reader", None)
        .await
        .1;
    for field in ["body", "title", "version", "created_at", "updated_at"] {
        assert_eq!(current[field], zulu[field]);
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn public_notes_follow_saved_order_and_moving_outside_closes_subtree_access(db: PgPool) {
    let app = fixture(&db).await;
    let parent = create(&app, "writer", "Public parent", None).await;
    let first = create(&app, "writer", "First", parent["id"].as_str()).await;
    let second = create(&app, "writer", "Second", parent["id"].as_str()).await;
    let (status, share) = request(
        &app,
        Method::POST,
        "/api/v1/notes/shares",
        "writer",
        Some(json!({"note_id":parent["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{share}");
    let (status, moved) = move_page(
        &app,
        "writer",
        &second,
        parent["id"].as_str(),
        first["id"].as_str(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    let resolve = json!({"token":share["token"]});
    let (status, public) = request(
        &app,
        Method::POST,
        "/api/v1/public/notes/resolve",
        "",
        Some(resolve.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{public}");
    assert_eq!(
        child_ids(&public, parent["id"].as_str()),
        vec![second["id"].clone(), first["id"].clone()]
    );
    assert!(
        public["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|note| note["sort_order"].is_i64())
    );
    assert_eq!(
        move_page(&app, "writer", &moved["note"], None, None)
            .await
            .0,
        StatusCode::OK
    );
    let public = request(
        &app,
        Method::POST,
        "/api/v1/public/notes/resolve",
        "",
        Some(resolve),
    )
    .await
    .1;
    assert_eq!(
        child_ids(&public, parent["id"].as_str()),
        vec![first["id"].clone()]
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/public/notes/resolve",
            "",
            Some(json!({"token":share["token"],"note_id":second["id"]}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
