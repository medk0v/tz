use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, note_databases, note_shares, notes};
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
        .merge(note_databases::router())
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

async fn setup_tables(app: &Router) -> (Value, Value, Value, Value, Value) {
    let create = |path: String, body: Value| async move {
        let (status, result) = request(app, Method::POST, &path, Some("writer"), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{result}");
        result
    };
    let visible = create(
        "/api/v1/notes/databases".into(),
        json!({"name":"Published table"}),
    )
    .await;
    let hidden = create(
        "/api/v1/notes/databases".into(),
        json!({"name":"Private table"}),
    )
    .await;
    let db = visible["id"].as_str().unwrap();
    let target = hidden["id"].as_str().unwrap();
    let title = create(
        format!("/api/v1/notes/databases/{db}/fields"),
        json!({"name":"Title","field_type":"text"}),
    )
    .await;
    let relation = create(format!("/api/v1/notes/databases/{db}/fields"), json!({"name":"Private relation","field_type":"relation","relation":{"target_database_id":target,"cardinality":"many_to_many","inverse_name":"Backlinks"}})).await;
    create(format!("/api/v1/notes/databases/{db}/fields"), json!({"name":"Secret count","field_type":"rollup","rollup":{"relation_field_id":relation["id"],"target_field_id":null,"operation":"count"}})).await;
    let target_row = create(
        format!("/api/v1/notes/databases/{target}/records"),
        json!({"content_markdown":"private record body"}),
    )
    .await;
    let row = create(format!("/api/v1/notes/databases/{db}/records"), json!({"values":{title["id"].as_str().unwrap():"Visible record",relation["id"].as_str().unwrap():[target_row["id"]]},"content_markdown":"# Record details"})).await;
    create(format!("/api/v1/notes/databases/{db}/views"), json!({"name":"Private relation filter","config":{"filters":[{"field_id":relation["id"],"operator":"is_empty"}],"sorts":[{"field_id":relation["id"],"direction":"asc"}],"column_order":[relation["id"],title["id"]]}})).await;
    create(
        format!("/api/v1/notes/{}/databases", id(21)),
        json!({"database_id":db}),
    )
    .await;
    create(
        format!("/api/v1/notes/{}/databases", id(23)),
        json!({"database_id":target}),
    )
    .await;
    (visible, hidden, title, relation, row)
}

async fn public(app: &Router, token: &Value, action: Value) -> (StatusCode, Value) {
    let mut body = action;
    body.as_object_mut()
        .unwrap()
        .insert("token".into(), token.clone());
    request(
        app,
        Method::POST,
        "/api/v1/public/notes/databases",
        None,
        Some(body),
    )
    .await
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn updated_page_selection_hides_tables_relations_rollups_and_public_writes_immediately(
    db: PgPool,
) {
    let app = fixture(&db).await;
    let (table, hidden, title, relation, row) = setup_tables(&app).await;
    let share = publish(&app, json!({"can_edit":true})).await;
    let token = &share["token"];
    assert_eq!(
        public(&app, token, json!({"action":"list"})).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        public(
            &app,
            token,
            json!({"action":"detail","database_id":hidden["id"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let input = json!({"theme_palette":"paper","theme_mode":"light","allow_theme_change":false,
        "include_descendants":true,"included_note_ids":[id(20),id(21)],"can_edit":true,"expected_version":1});
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/notes/shares/{}", share["id"].as_str().unwrap()),
        Some("writer"),
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        public(&app, token, json!({"action":"list"})).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for action in [
        json!({"action":"detail","database_id":hidden["id"]}),
        json!({"action":"records","database_id":hidden["id"]}),
        json!({"action":"embeds","note_id":id(23)}),
        json!({"action":"create_record","database_id":hidden["id"],"values":{},"content_markdown":"leak"}),
        json!({"action":"create_field","database_id":hidden["id"],"field":{"name":"Hidden write","field_type":"text"}}),
    ] {
        assert_eq!(public(&app, token, action).await.0, StatusCode::NOT_FOUND);
    }
    let detail = public(
        &app,
        token,
        json!({"action":"detail","database_id":table["id"]}),
    )
    .await
    .1;
    assert_eq!(detail["fields"].as_array().unwrap().len(), 1);
    assert_eq!(detail["fields"][0]["id"], title["id"]);
    assert!(!detail["fields"].to_string().contains("Private relation"));
    assert!(!detail["fields"].to_string().contains("Secret count"));
    let fetched = public(
        &app,
        token,
        json!({"action":"record","database_id":table["id"],"record_id":row["id"]}),
    )
    .await
    .1;
    assert_eq!(fetched["values"].as_object().unwrap().len(), 1);
    let hidden_id = row["values"][relation["id"].as_str().unwrap()][0]
        .as_str()
        .unwrap();
    let searched = public(
        &app,
        token,
        json!({"action":"records","database_id":table["id"],"q":hidden_id}),
    )
    .await
    .1;
    assert_eq!(searched["total"], 0);
    let (status, edited) = public(&app, token, json!({"action":"update_record","database_id":table["id"],"record_id":row["id"],
        "values":{title["id"].as_str().unwrap():"Allowed edit"},"content_markdown":"Public details","expected_version":row["version"]})).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    let internal = request(
        &app,
        Method::GET,
        &format!(
            "/api/v1/notes/databases/{}/records/{}",
            table["id"].as_str().unwrap(),
            row["id"].as_str().unwrap()
        ),
        Some("writer"),
        None,
    )
    .await
    .1;
    assert_eq!(
        internal["values"][relation["id"].as_str().unwrap()],
        row["values"][relation["id"].as_str().unwrap()]
    );
    assert_eq!(request(&app, Method::PATCH, "/api/v1/public/notes/page", None,
        Some(json!({"token":token,"note_id":id(23),"title":"Hidden edit","body":"leak","icon":"","expected_version":1}))).await.0, StatusCode::NOT_FOUND);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn public_tables_hide_private_relations_and_rollups_before_search_and_preserve_them_on_edit(
    db: PgPool,
) {
    let app = fixture(&db).await;
    let (table, hidden, title, relation, row) = setup_tables(&app).await;
    let share = publish(&app, json!({"note_id":id(21),"can_edit":true})).await;
    let token = &share["token"];
    let (status, listed) = public(&app, token, json!({"action":"list"})).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["id"], table["id"]);
    for action in [
        json!({"action":"detail","database_id":hidden["id"]}),
        json!({"action":"embeds","note_id":id(23)}),
    ] {
        assert_eq!(public(&app, token, action).await.0, StatusCode::NOT_FOUND);
    }
    let (status, detail) = public(
        &app,
        token,
        json!({"action":"detail","database_id":table["id"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["fields"].as_array().unwrap().len(), 1);
    assert_eq!(detail["views"][0]["config"]["filters"], json!([]));
    assert_eq!(detail["views"][0]["config"]["sorts"], json!([]));
    assert_eq!(
        detail["views"][0]["config"]["column_order"],
        json!([title["id"]])
    );
    let (status, projected_view) = public(
        &app,
        token,
        json!({"action":"records","database_id":table["id"],"view_id":detail["views"][0]["id"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{projected_view}");
    assert_eq!(projected_view["total"], 1);
    let (status, public_row) = public(
        &app,
        token,
        json!({"action":"record","database_id":table["id"],"record_id":row["id"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{public_row}");
    assert_eq!(public_row["values"].as_object().unwrap().len(), 1);
    assert_eq!(public_row["content_markdown"], "# Record details");
    let (_, searched) = public(&app, token, json!({"action":"records","database_id":table["id"],"q":row["values"][relation["id"].as_str().unwrap()][0]})).await;
    assert_eq!(searched["total"], 0, "{searched}");
    let (status, updated) = public(&app, token, json!({"action":"update_record","database_id":table["id"],"record_id":row["id"],"values":{title["id"].as_str().unwrap():"Changed publicly"},"content_markdown":"# New card","expected_version":row["version"]})).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let (_, private_row) = request(
        &app,
        Method::GET,
        &format!(
            "/api/v1/notes/databases/{}/records/{}",
            table["id"].as_str().unwrap(),
            row["id"].as_str().unwrap()
        ),
        Some("writer"),
        None,
    )
    .await;
    assert_eq!(
        private_row["values"][relation["id"].as_str().unwrap()],
        row["values"][relation["id"].as_str().unwrap()]
    );
    assert_eq!(private_row["content_markdown"], "# New card");
    assert_eq!(public(&app, token, json!({"action":"update_record","database_id":table["id"],"record_id":row["id"],"values":{},"content_markdown":"stale","expected_version":row["version"]})).await.0, StatusCode::CONFLICT);
    assert_eq!(public(&app, token, json!({"action":"update_record","database_id":table["id"],"record_id":row["id"],"values":{relation["id"].as_str().unwrap():[]},"content_markdown":"malicious","expected_version":updated["version"]})).await.0, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn password_readonly_revocation_and_removed_embeds_apply_to_every_public_table_operation(
    db: PgPool,
) {
    let app = fixture(&db).await;
    let (table, hidden, _, hidden_relation, row) = setup_tables(&app).await;
    let share = publish(
        &app,
        json!({"note_id":id(21),"password":"correct-password"}),
    )
    .await;
    let action = json!({"action":"records","database_id":table["id"]});
    assert_eq!(
        public(&app, &share["token"], action.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, unlocked) = request(
        &app,
        Method::POST,
        "/api/v1/public/notes/unlock",
        None,
        Some(json!({"token":share["token"],"password":"correct-password"})),
    )
    .await;
    let mut authorized = action.clone();
    authorized["access_token"] = unlocked["access_token"].clone();
    assert_eq!(
        public(&app, &share["token"], authorized.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(public(&app, &share["token"], json!({"action":"create_field","database_id":table["id"],"field":{"name":"Denied","field_type":"text"},"access_token":unlocked["access_token"]})).await.0, StatusCode::FORBIDDEN);
    assert_eq!(public(&app, &share["token"], json!({"action":"delete_record","database_id":table["id"],"record_id":row["id"],"expected_version":row["version"],"access_token":unlocked["access_token"]})).await.0, StatusCode::FORBIDDEN);
    request(
        &app,
        Method::DELETE,
        &format!("/api/v1/notes/shares/{}", share["id"].as_str().unwrap()),
        Some("writer"),
        None,
    )
    .await;
    assert_eq!(
        public(&app, &share["token"], authorized).await.0,
        StatusCode::NOT_FOUND
    );
    let editable = publish(&app, json!({"note_id":id(21),"can_edit":true})).await;
    let (status, field) = public(&app, &editable["token"], json!({"action":"create_field","database_id":table["id"],"field":{"name":"Public number","field_type":"number"}})).await;
    assert_eq!(status, StatusCode::OK, "{field}");
    let (status, changed_field) = public(&app, &editable["token"], json!({"action":"update_field","database_id":table["id"],"field_id":field["id"],"field":{"name":"Renamed public field","field_type":"number","config":{},"expected_version":field["version"]}})).await;
    assert_eq!(status, StatusCode::OK, "{changed_field}");
    assert_eq!(public(&app, &editable["token"], json!({"action":"create_field","database_id":table["id"],"field":{"name":"Escape","field_type":"relation","relation":{"target_database_id":hidden["id"],"cardinality":"many_to_many","inverse_name":"Leak"}}})).await.0, StatusCode::NOT_FOUND);
    assert_eq!(public(&app, &editable["token"], json!({"action":"delete_field","database_id":table["id"],"field_id":hidden_relation["id"],"expected_version":hidden_relation["version"]})).await.0, StatusCode::NOT_FOUND);
    assert_eq!(public(&app, &share["token"], json!({"action":"create_field","database_id":table["id"],"field":{"name":"Revoked","field_type":"text"},"access_token":unlocked["access_token"]})).await.0, StatusCode::NOT_FOUND);
    assert_eq!(public(&app, &editable["token"], json!({"action":"delete_field","database_id":table["id"],"field_id":field["id"],"expected_version":changed_field["version"]})).await.0, StatusCode::OK);
    let (status, created) = public(&app,&editable["token"],json!({"action":"create_record","database_id":table["id"],"values":{},"content_markdown":"Public-created card"})).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(public(&app,&editable["token"],json!({"action":"delete_record","database_id":table["id"],"record_id":created["id"],"expected_version":created["version"]})).await.0,StatusCode::OK);
    sqlx::query("DELETE FROM note_database_embeds WHERE note_id=$1")
        .bind(id(21))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        public(&app, &editable["token"], action).await.0,
        StatusCode::NOT_FOUND
    );
}
