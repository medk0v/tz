use std::time::Duration;

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
        "INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Sharing'), ('{other_tenant}', 'Other');
         INSERT INTO projects (id, tenant_id, name, slug) VALUES
             ('{project}', '{tenant}', 'First', 'first'), ('{other_project}', '{tenant}', 'Second', 'second'),
             ('{foreign_project}', '{other_tenant}', 'Foreign', 'foreign');
         INSERT INTO notes (id, tenant_id, project_id, parent_id, title, body, is_favorite) VALUES
             ('{ancestor}', '{tenant}', '{project}', NULL, 'Private ancestor', 'Ancestor body', false),
             ('{root}', '{tenant}', '{project}', '{ancestor}', 'Published root', '# Root', true),
             ('{child}', '{tenant}', '{project}', '{root}', 'Child', 'Child body', false),
             ('{outside}', '{tenant}', '{project}', '{ancestor}', 'Private sibling', 'Private body', false),
             ('{other_note}', '{tenant}', '{other_project}', NULL, 'Other project', 'Private body', false),
             ('{foreign_note}', '{other_tenant}', '{foreign_project}', NULL, 'Other tenant', 'Private body', false);",
        tenant=id(1), other_tenant=id(100), project=id(2), other_project=id(3), foreign_project=id(101),
        ancestor=id(20), root=id(21), child=id(22), outside=id(23), other_note=id(24), foreign_note=id(25),
    )).execute(db).await.unwrap();
    for (token, user, tenant, project, permissions) in [
        ("writer", 10, 1, 2, vec!["notes:read", "notes:write"]),
        ("reader", 11, 1, 2, vec!["notes:read"]),
        ("other-project", 12, 1, 3, vec!["notes:read", "notes:write"]),
        (
            "other-tenant",
            13,
            100,
            101,
            vec!["notes:read", "notes:write"],
        ),
    ] {
        sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)")
            .bind(id(user))
            .bind(format!("{token}@note-shares.test"))
            .bind(token)
            .execute(db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions) VALUES ($1, $2, $3, $3, 'operator', $4)")
            .bind(id(tenant)).bind(id(project)).bind(token).bind(&permissions).execute(db).await.unwrap();
        sqlx::query("INSERT INTO memberships (id, tenant_id, project_id, user_id, role) VALUES ($1, $2, $3, $1, $4)")
            .bind(id(user)).bind(id(tenant)).bind(id(project)).bind(token).execute(db).await.unwrap();
        sqlx::query("INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, role, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $5, now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(tenant)).bind(id(project)).bind(id(user)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(&permissions).execute(db).await.unwrap();
    }
    application(db, id(2)).await
}

async fn application(db: &PgPool, fixed_project: Uuid) -> Router {
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    config.product.project_id = Some(fixed_project);
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
    actor: Option<&str>,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(actor) = actor {
        builder = builder.header("authorization", format!("Bearer {actor}"));
    }
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
    if path.starts_with("/api/v1/public/notes") || path.starts_with("/api/v1/notes/shares") {
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        assert_eq!(
            response.headers().get("referrer-policy").unwrap(),
            "no-referrer"
        );
        if status == StatusCode::TOO_MANY_REQUESTS {
            assert_eq!(response.headers().get("retry-after").unwrap(), "60");
        }
    }
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn publish(app: &Router, input: Value) -> Value {
    let (status, body) = request(
        app,
        Method::POST,
        "/api/v1/notes/shares",
        Some("writer"),
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

async fn resolve(
    app: &Router,
    token: &Value,
    note: Option<Uuid>,
    access: Option<&Value>,
) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        "/api/v1/public/notes/resolve",
        None,
        Some(json!({"token":token,"note_id":note,"access_token":access})),
    )
    .await
}

fn edit(token: &Value, note: Uuid, version: i32) -> Value {
    json!({"token":token,"note_id":note,"title":"External title","body":"# Public edit\n\n🙂","icon":"📘","expected_version":version})
}

fn settings(share: &Value) -> Value {
    json!({"theme_palette":share["theme_palette"],"theme_mode":share["theme_mode"],
        "allow_theme_change":share["allow_theme_change"],"include_descendants":share["include_descendants"],
        "included_note_ids":share["included_note_ids"],"can_edit":share["can_edit"],"expected_version":share["version"]})
}

fn share_path(share: &Value) -> String {
    format!("/api/v1/notes/shares/{}", share["id"].as_str().unwrap())
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn appearance_and_selection_updates_apply_to_existing_unlocked_tokens_without_rotating_identity(
    db: PgPool,
) {
    let app = fixture(&db).await;
    let share = publish(&app, json!({"note_id":id(21),"password":"settings password","custom_slug":"theme-docs","can_edit":true})).await;
    let stored: (Vec<u8>, Option<String>) =
        sqlx::query_as("SELECT token_hash,password_hash FROM note_shares WHERE id=$1")
            .bind(Uuid::parse_str(share["id"].as_str().unwrap()).unwrap())
            .fetch_one(&db)
            .await
            .unwrap();
    let (status, unlocked) = request(
        &app,
        Method::POST,
        "/api/v1/public/notes/unlock",
        None,
        Some(json!({"token":share["token"],"password":"settings password"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unlocked}");
    let access = &unlocked["access_token"];
    assert_eq!(
        resolve(&app, &share["token"], Some(id(22)), Some(access))
            .await
            .0,
        StatusCode::OK
    );
    let mut input = settings(&share);
    input["theme_palette"] = json!("forest");
    input["theme_mode"] = json!("dark");
    input["allow_theme_change"] = json!(false);
    input["include_descendants"] = json!(false);
    input["included_note_ids"] = json!([]);
    input["can_edit"] = json!(false);
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &share_path(&share),
        Some("writer"),
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["version"], 2);
    for key in ["note_id", "custom_slug", "has_password", "created_at"] {
        assert_eq!(saved[key], share[key]);
    }
    assert!(saved.get("token").is_none() && saved.get("password_hash").is_none());
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &share_path(&share),
            Some("writer"),
            Some(input)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, public) = resolve(&app, &share["token"], None, Some(access)).await;
    assert_eq!(status, StatusCode::OK, "{public}");
    assert_eq!(public["theme_palette"], "forest");
    assert_eq!(public["theme_mode"], "dark");
    assert_eq!(public["allow_theme_change"], false);
    assert_eq!(public["include_descendants"], false);
    assert_eq!(public["items"].as_array().unwrap().len(), 1);
    assert!(public.get("included_note_ids").is_none());
    assert_eq!(
        resolve(&app, &share["token"], Some(id(22)), Some(access))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut edit_input = edit(&share["token"], id(21), 1);
    edit_input["access_token"] = access.clone();
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(edit_input)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut expanded = settings(&saved);
    expanded["include_descendants"] = json!(true);
    expanded["included_note_ids"] = json!([id(22)]);
    expanded["can_edit"] = json!(true);
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &share_path(&saved),
            Some("writer"),
            Some(expanded)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        resolve(&app, &share["token"], Some(id(22)), Some(access))
            .await
            .0,
        StatusCode::OK
    );
    let unchanged: bool = sqlx::query_scalar("SELECT token_hash=$2 AND password_hash=$3 AND custom_slug='theme-docs' AND note_id=$4 FROM note_shares WHERE id=$1")
        .bind(Uuid::parse_str(share["id"].as_str().unwrap()).unwrap()).bind(stored.0).bind(stored.1).bind(id(21)).fetch_one(&db).await.unwrap();
    assert!(unchanged);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn publication_settings_validate_scope_ancestor_closure_and_complete_versioned_inputs(
    db: PgPool,
) {
    let app = fixture(&db).await;
    sqlx::query("INSERT INTO notes(id,tenant_id,project_id,parent_id,title) VALUES($1,$2,$3,$4,'Grandchild')")
        .bind(id(26)).bind(id(1)).bind(id(2)).bind(id(22)).execute(&db).await.unwrap();
    for (extra, expected) in [
        (json!({"theme_palette":"unknown"}), StatusCode::BAD_REQUEST),
        (json!({"theme_mode":"auto"}), StatusCode::BAD_REQUEST),
        (
            json!({"included_note_ids":[id(22),id(22)]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"included_note_ids":[id(26)]}),
            StatusCode::BAD_REQUEST,
        ),
        (json!({"included_note_ids":[id(23)]}), StatusCode::NOT_FOUND),
        (json!({"included_note_ids":[id(24)]}), StatusCode::NOT_FOUND),
        (json!({"included_note_ids":[id(25)]}), StatusCode::NOT_FOUND),
        (
            json!({"included_note_ids":[null]}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let mut input = extra;
        input["note_id"] = json!(id(21));
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/notes/shares",
            Some("writer"),
            Some(input),
        )
        .await;
        assert_eq!(status, expected, "{body}");
    }
    let too_many = (0..5001).map(|_| Uuid::now_v7()).collect::<Vec<_>>();
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/notes/shares",
            Some("writer"),
            Some(json!({"included_note_ids":too_many}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let share = publish(
        &app,
        json!({"note_id":id(21),"included_note_ids":[id(22),id(26)]}),
    )
    .await;
    assert_eq!(
        resolve(&app, &share["token"], None, None).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    for (actor, expected) in [
        ("reader", StatusCode::FORBIDDEN),
        ("other-project", StatusCode::NOT_FOUND),
        ("other-tenant", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            request(
                &app,
                Method::PATCH,
                &share_path(&share),
                Some(actor),
                Some(settings(&share))
            )
            .await
            .0,
            expected
        );
    }
    let mut missing = settings(&share);
    missing.as_object_mut().unwrap().remove("included_note_ids");
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &share_path(&share),
            Some("writer"),
            Some(missing)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut override_root = settings(&share);
    override_root["note_id"] = json!(id(23));
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &share_path(&share),
            Some("writer"),
            Some(override_root)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut invalid_scope = settings(&share);
    invalid_scope["included_note_ids"] = json!([id(25)]);
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &share_path(&share),
            Some("writer"),
            Some(invalid_scope)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &share_path(&share),
            Some("writer"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &share_path(&share),
            Some("writer"),
            Some(settings(&share))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn legacy_project_and_single_note_defaults_remain_compatible_with_explicit_root_selections(
    db: PgPool,
) {
    let app = fixture(&db).await;
    let legacy = publish(&app, json!({})).await;
    assert_eq!(legacy["version"], 1);
    assert_eq!(legacy["included_note_ids"], Value::Null);
    assert_eq!(legacy["theme_palette"], Value::Null);
    assert_eq!(legacy["theme_mode"], Value::Null);
    assert_eq!(legacy["allow_theme_change"], true);
    assert_eq!(legacy["include_descendants"], true);
    assert_eq!(
        resolve(&app, &legacy["token"], None, None).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let roots = publish(&app, json!({"include_descendants":false})).await;
    let public = resolve(&app, &roots["token"], None, None).await.1;
    assert_eq!(public["items"].as_array().unwrap().len(), 1);
    assert_eq!(public["items"][0]["id"], json!(id(20)));
    let selected = publish(&app, json!({"included_note_ids":[id(20),id(21)]})).await;
    assert_eq!(
        resolve(&app, &selected["token"], None, None).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/notes/shares",
            Some("writer"),
            Some(json!({"included_note_ids":[id(21)]}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let none = publish(&app, json!({"included_note_ids":[]})).await;
    let public = resolve(&app, &none["token"], None, None).await.1;
    assert_eq!(public["items"], json!([]));
    assert_eq!(public["note"], Value::Null);
    let single = publish(&app, json!({"note_id":id(21),"included_note_ids":[]})).await;
    assert_eq!(
        resolve(&app, &single["token"], None, None).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let (status, created) = request(
        &app,
        Method::POST,
        "/api/v1/notes",
        Some("writer"),
        Some(json!({"title":"Future child","parent_id":id(21)})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let note_id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        resolve(&app, &legacy["token"], Some(note_id), None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        resolve(&app, &selected["token"], Some(note_id), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn moving_selected_pages_below_unchecked_ancestors_closes_public_reads_and_edits(db: PgPool) {
    let app = fixture(&db).await;
    sqlx::query("INSERT INTO notes(id,tenant_id,project_id,parent_id,title) VALUES($1,$2,$3,$4,'Selected grandchild'),($5,$2,$3,$6,'Unchecked parent')")
        .bind(id(26)).bind(id(1)).bind(id(2)).bind(id(22)).bind(id(27)).bind(id(21)).execute(&db).await.unwrap();
    let share = publish(
        &app,
        json!({"note_id":id(21),"included_note_ids":[id(22),id(26)],"can_edit":true}),
    )
    .await;
    assert_eq!(
        resolve(&app, &share["token"], Some(id(26)), None).await.0,
        StatusCode::OK
    );
    let move_path = format!("/api/v1/notes/{}/move", id(26));
    assert_eq!(
        request(
            &app,
            Method::POST,
            &move_path,
            Some("writer"),
            Some(json!({"parent_id":id(27),"before_id":null,"expected_version":1}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let public = resolve(&app, &share["token"], None, None).await.1;
    assert_eq!(public["items"].as_array().unwrap().len(), 2);
    assert!(!public.to_string().contains("Unchecked parent"));
    assert!(!public.to_string().contains("Selected grandchild"));
    assert_eq!(
        resolve(&app, &share["token"], Some(id(26)), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(edit(&share["token"], id(26), 2))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            &move_path,
            Some("writer"),
            Some(json!({"parent_id":id(22),"before_id":null,"expected_version":2}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        resolve(&app, &share["token"], Some(id(26)), None).await.0,
        StatusCode::OK
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn changed_visibility_wins_over_an_edit_waiting_on_the_project_lock(db: PgPool) {
    let app = fixture(&db).await;
    let share = publish(&app, json!({"note_id":id(21),"can_edit":true})).await;
    let mut transaction = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM projects WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
        .bind(id(1))
        .bind(id(2))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let application = app.clone();
    let input = edit(&share["token"], id(22), 1);
    let pending = tokio::spawn(async move {
        request(
            &application,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(input),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!pending.is_finished());
    sqlx::query("UPDATE note_shares SET include_descendants=false, included_note_ids='{}',version=version+1 WHERE id=$1")
        .bind(Uuid::parse_str(share["id"].as_str().unwrap()).unwrap()).execute(&mut *transaction).await.unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(pending.await.unwrap().0, StatusCode::NOT_FOUND);
    let body: String = sqlx::query_scalar("SELECT body FROM notes WHERE id=$1")
        .bind(id(22))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(body, "Child body");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn management_supports_multiple_links_and_real_custom_addresses_without_leaking_secrets(
    db: PgPool,
) {
    let app = fixture(&db).await;
    let random = publish(&app, json!({"note_id":id(21)})).await;
    let custom = publish(
        &app,
        json!({"note_id":id(21),"custom_slug":"  Product-Docs  ","can_edit":true}),
    )
    .await;
    assert_eq!(custom["token"], "product-docs");
    assert_eq!(custom["custom_slug"], "product-docs");
    assert!(random["token"].as_str().unwrap().starts_with('_'));
    assert_eq!(random["token"].as_str().unwrap().len(), 44);
    let (status, listed) = request(
        &app,
        Method::GET,
        "/api/v1/notes/shares",
        Some("writer"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("token").is_none() && item.get("password_hash").is_none())
    );
    assert_eq!(
        resolve(&app, &json!("product-docs"), None, None).await.0,
        StatusCode::OK
    );
    for actor in ["writer", "other-project", "other-tenant"] {
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/notes/shares",
            Some(actor),
            Some(json!({"custom_slug":"product-docs"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
    }
    for actor in ["other-project", "other-tenant"] {
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/notes/shares",
            Some(actor),
            Some(json!({"note_id":id(21)})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(
            request(
                &app,
                Method::DELETE,
                &format!("/api/v1/notes/shares/{}", random["id"].as_str().unwrap()),
                Some(actor),
                None
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(&app, Method::GET, "/api/v1/notes/shares", Some(actor), None)
                .await
                .1["items"],
            json!([])
        );
    }
    for (method, path, input) in [
        (Method::GET, "/api/v1/notes/shares".to_owned(), None),
        (
            Method::POST,
            "/api/v1/notes/shares".to_owned(),
            Some(json!({})),
        ),
        (
            Method::DELETE,
            format!("/api/v1/notes/shares/{}", random["id"].as_str().unwrap()),
            None,
        ),
    ] {
        assert_eq!(
            request(&app, method, &path, Some("reader"), input).await.0,
            StatusCode::FORBIDDEN
        );
    }
    for input in [
        json!({"custom_slug":"ab"}),
        json!({"custom_slug":"has space"}),
        json!({"password":"short"}),
        json!({"password":"x".repeat(129)}),
    ] {
        assert_eq!(
            request(
                &app,
                Method::POST,
                "/api/v1/notes/shares",
                Some("writer"),
                Some(input)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let revoke_path = format!("/api/v1/notes/shares/{}", custom["id"].as_str().unwrap());
    assert_eq!(
        request(&app, Method::DELETE, &revoke_path, Some("writer"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, Method::DELETE, &revoke_path, Some("writer"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        resolve(&app, &custom["token"], None, None).await.0,
        StatusCode::NOT_FOUND
    );
    let reused = publish(&app, json!({"custom_slug":"product-docs"})).await;
    assert_ne!(reused["id"], custom["id"]);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn subtree_and_workspace_scopes_are_rechecked_after_reparenting_and_deletion(db: PgPool) {
    let app = fixture(&db).await;
    let subtree = publish(&app, json!({"note_id":id(21),"can_edit":true})).await;
    let (status, visible) = resolve(&app, &subtree["token"], None, None).await;
    assert_eq!(status, StatusCode::OK, "{visible}");
    assert_eq!(visible["root_note_id"], json!(id(21)));
    assert_eq!(visible["items"].as_array().unwrap().len(), 2);
    assert_eq!(visible["note"]["id"], json!(id(21)));
    assert_eq!(visible["note"]["parent_id"], Value::Null);
    assert!(visible["note"].get("is_favorite").is_none());
    for forbidden in [id(20), id(23), id(24), id(25)] {
        assert_eq!(
            resolve(&app, &subtree["token"], Some(forbidden), None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let workspace = publish(&app, json!({"note_id":null})).await;
    let (status, visible) = resolve(&app, &workspace["token"], None, None).await;
    assert_eq!(status, StatusCode::OK, "{visible}");
    assert_eq!(visible["root_note_id"], Value::Null);
    assert_eq!(visible["items"].as_array().unwrap().len(), 4);
    assert_eq!(
        resolve(&app, &workspace["token"], Some(id(24)), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(request(&app,Method::PATCH,&format!("/api/v1/notes/{}",id(22)),Some("writer"),Some(json!({"title":"Child","body":"Child body","icon":"","parent_id":id(23),"is_favorite":false,"expected_version":1}))).await.0,StatusCode::OK);
    assert_eq!(
        resolve(&app, &subtree["token"], Some(id(22)), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(edit(&subtree["token"], id(22), 2))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/v1/notes/{}?expected_version=1", id(21)),
            Some("writer"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        resolve(&app, &subtree["token"], None, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        resolve(&app, &workspace["token"], Some(id(22)), None)
            .await
            .0,
        StatusCode::OK
    );
    sqlx::query("UPDATE projects SET status='disabled' WHERE id=$1")
        .bind(id(2))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        resolve(&app, &workspace["token"], None, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn passwords_unlock_only_their_share_and_expire_or_revoke_independently(db: PgPool) {
    let app = fixture(&db).await;
    let share = publish(
        &app,
        json!({"note_id":id(21),"password":"a private passphrase","can_edit":true}),
    )
    .await;
    assert_eq!(share["has_password"], true);
    let stored: (Vec<u8>, String) =
        sqlx::query_as("SELECT token_hash,password_hash FROM note_shares WHERE id=$1")
            .bind(Uuid::parse_str(share["id"].as_str().unwrap()).unwrap())
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        stored.0,
        Sha256::digest(share["token"].as_str().unwrap().as_bytes()).to_vec()
    );
    assert!(stored.1.starts_with("$argon2id$"));
    assert_ne!(stored.1, "a private passphrase");
    assert_eq!(
        resolve(&app, &share["token"], None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/public/notes/unlock",
            None,
            Some(json!({"token":share["token"],"password":"incorrect"}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, unlocked) = request(
        &app,
        Method::POST,
        "/api/v1/public/notes/unlock",
        None,
        Some(json!({"token":share["token"],"password":"a private passphrase"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unlocked}");
    assert_eq!(
        resolve(&app, &share["token"], None, Some(&unlocked["access_token"]))
            .await
            .0,
        StatusCode::OK
    );
    let other = publish(
        &app,
        json!({"note_id":id(21),"password":"a private passphrase"}),
    )
    .await;
    assert_eq!(
        resolve(&app, &other["token"], None, Some(&unlocked["access_token"]))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let mut input = edit(&share["token"], id(21), 1);
    input["access_token"] = unlocked["access_token"].clone();
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(input)
        )
        .await
        .0,
        StatusCode::OK
    );
    let access_hash =
        Sha256::digest(unlocked["access_token"].as_str().unwrap().as_bytes()).to_vec();
    let hours: f64=sqlx::query_scalar("SELECT extract(epoch FROM expires_at-created_at)::double precision/3600 FROM note_share_access_tokens WHERE token_hash=$1")
        .bind(&access_hash).fetch_one(&db).await.unwrap();
    assert!((hours - 12.0).abs() < 0.01);
    sqlx::query("UPDATE note_share_access_tokens SET created_at=now()-interval '13 hours',expires_at=now()-interval '1 hour' WHERE token_hash=$1")
        .bind(access_hash).execute(&db).await.unwrap();
    assert_eq!(
        resolve(&app, &share["token"], None, Some(&unlocked["access_token"]))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/v1/notes/shares/{}", share["id"].as_str().unwrap()),
            Some("writer"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        resolve(&app, &share["token"], None, Some(&unlocked["access_token"]))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn external_edits_are_versioned_and_cannot_modify_hierarchy_or_favorites(db: PgPool) {
    let app = fixture(&db).await;
    let readonly = publish(&app, json!({"note_id":id(21)})).await;
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(edit(&readonly["token"], id(21), 1))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let share = publish(&app, json!({"note_id":id(21),"can_edit":true})).await;
    let input = edit(&share["token"], id(21), 1);
    let (status, saved) = request(
        &app,
        Method::PATCH,
        "/api/v1/public/notes/page",
        None,
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["version"], 2);
    assert_eq!(saved["body"], input["body"]);
    assert_eq!(saved["parent_id"], Value::Null);
    assert!(saved.get("is_favorite").is_none());
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(input)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    for (field, value) in [("parent_id", Value::Null), ("is_favorite", json!(false))] {
        let mut input = edit(&share["token"], id(21), 2);
        input[field] = value;
        assert_eq!(
            request(
                &app,
                Method::PATCH,
                "/api/v1/public/notes/page",
                None,
                Some(input)
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let stored: (Option<Uuid>, bool) =
        sqlx::query_as("SELECT parent_id,is_favorite FROM notes WHERE id=$1")
            .bind(id(21))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(stored, (Some(id(20)), true));
    let audit: (Option<Uuid>,Value)=sqlx::query_as("SELECT actor_id,metadata FROM audit_log WHERE resource_id=$1 AND action='note.public_updated'")
        .bind(id(21)).fetch_one(&db).await.unwrap();
    assert_eq!(audit.0, None);
    assert_eq!(audit.1, json!({"share_id":share["id"]}));
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/public/notes/missing",
            None,
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::GET,
            "/api/v1/public/notes/resolve",
            None,
            None
        )
        .await
        .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn revocation_wins_over_an_edit_waiting_for_the_project_lock(db: PgPool) {
    let app = fixture(&db).await;
    let share = publish(&app, json!({"note_id":id(21),"can_edit":true})).await;
    let mut transaction = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM projects WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
        .bind(id(1))
        .bind(id(2))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let input = edit(&share["token"], id(21), 1);
    let mut pending = tokio::spawn(async move {
        request(
            &app,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(input),
        )
        .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut pending)
            .await
            .is_err()
    );
    sqlx::query("UPDATE note_shares SET revoked_at=now() WHERE id=$1")
        .bind(Uuid::parse_str(share["id"].as_str().unwrap()).unwrap())
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(pending.await.unwrap().0, StatusCode::NOT_FOUND);
    let body: String = sqlx::query_scalar("SELECT body FROM notes WHERE id=$1")
        .bind(id(21))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(body, "# Root");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn password_attempt_limits_are_persistent_and_recover_after_the_window(db: PgPool) {
    let app = fixture(&db).await;
    let share = publish(
        &app,
        json!({"note_id":id(21),"password":"a private passphrase"}),
    )
    .await;
    let share_id = Uuid::parse_str(share["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE note_shares SET unlock_attempts=9 WHERE id=$1")
        .bind(share_id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/public/notes/unlock",
            None,
            Some(json!({"token":share["token"],"password":"incorrect"}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/public/notes/unlock",
            None,
            Some(json!({"token":share["token"],"password":"a private passphrase"}))
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query(
        "UPDATE note_shares SET unlock_window_start=now()-interval '2 minutes' WHERE id=$1",
    )
    .bind(share_id)
    .execute(&db)
    .await
    .unwrap();
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/public/notes/unlock",
            None,
            Some(json!({"token":share["token"],"password":"a private passphrase"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let attempts: i32 = sqlx::query_scalar("SELECT unlock_attempts FROM note_shares WHERE id=$1")
        .bind(share_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(attempts, 1);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn public_links_are_restricted_to_the_fixed_deployment_project(db: PgPool) {
    let app = fixture(&db).await;
    let foreign = publish(&app, json!({"note_id":id(21),"can_edit":true})).await;
    let (status, allowed) = request(
        &app,
        Method::POST,
        "/api/v1/notes/shares",
        Some("other-project"),
        Some(json!({"note_id":id(24),"can_edit":true})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{allowed}");
    let lite = application(&db, id(3)).await;
    assert_eq!(
        resolve(&lite, &foreign["token"], None, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &lite,
            Method::POST,
            "/api/v1/public/notes/unlock",
            None,
            Some(json!({"token":foreign["token"],"password":"anything"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &lite,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(edit(&foreign["token"], id(21), 1))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, resolved) = resolve(&lite, &allowed["token"], None, None).await;
    assert_eq!(status, StatusCode::OK, "{resolved}");
    assert_eq!(resolved["note"]["id"], json!(id(24)));
    assert_eq!(
        request(
            &lite,
            Method::POST,
            "/api/v1/public/notes/unlock",
            None,
            Some(json!({"token":allowed["token"],"password":"anything"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &lite,
            Method::PATCH,
            "/api/v1/public/notes/page",
            None,
            Some(edit(&allowed["token"], id(24), 1))
        )
        .await
        .0,
        StatusCode::OK
    );
}
