use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, note_databases, notes};
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
        .merge(note_databases::router())
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

const BASE: &str = "/api/v1/notes/databases";

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn record_order_is_atomic_versioned_and_database_delete_cascades_own_content(db: PgPool) {
    let app = fixture(&db).await;
    let database = create_database(&app, "writer", "Ordered").await;
    create_field(&app, &database, "text", json!({})).await;
    let records_path = format!("{}/records", path(&database));
    let mut rows = Vec::new();
    for _ in 0..3 {
        let (status, row) =
            request(&app, Method::POST, &records_path, "writer", Some(json!({}))).await;
        assert_eq!(status, StatusCode::CREATED, "{row}");
        rows.push(row);
    }
    let current = detail(&app, &database).await;
    let ids = rows
        .iter()
        .rev()
        .map(|row| row["id"].clone())
        .collect::<Vec<_>>();
    let order = json!({"ids":ids,"expected_version":current["database"]["version"]});
    let order_path = format!("{records_path}/order");
    assert_eq!(
        request(
            &app,
            Method::PUT,
            &order_path,
            "writer",
            Some(order.clone())
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, Method::PUT, &order_path, "writer", Some(order))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let list = request(&app, Method::GET, &records_path, "reader", None)
        .await
        .1;
    assert_eq!(list["items"][0]["id"], ids[0]);
    assert_eq!(list["items"][0]["version"], 2);
    assert_eq!(list["items"][1]["version"], 1);
    assert_eq!(list["items"][2]["position"], 2);
    let old_first = rows[0]["id"].as_str().unwrap();
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{records_path}/{old_first}?expected_version=1"),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{records_path}/{old_first}?expected_version=2"),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let current = detail(&app, &database).await;
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!(
                "{}?expected_version={}",
                path(&database),
                current["database"]["version"]
            ),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM note_database_records")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM note_database_fields")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
}

async fn create_database(app: &Router, token: &str, name: &str) -> Value {
    let (status, body) = request(app, Method::POST, BASE, token, Some(json!({"name": name}))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

fn path(database: &Value) -> String {
    format!("{BASE}/{}", database["id"].as_str().unwrap())
}

async fn create_field(app: &Router, database: &Value, field_type: &str, config: Value) -> Value {
    let (status, body) = request(
        app,
        Method::POST,
        &format!("{}/fields", path(database)),
        "writer",
        Some(json!({"name":field_type,"field_type":field_type,"config":config})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

async fn detail(app: &Router, database: &Value) -> Value {
    let (status, body) = request(app, Method::GET, &path(database), "writer", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn database_crud_is_project_scoped_permission_checked_and_versioned(db: PgPool) {
    let app = fixture(&db).await;
    let database = create_database(&app, "writer", "  Customers  ").await;
    assert_eq!(database["name"], "Customers");
    create_database(&app, "other-project", "Other project").await;
    create_database(&app, "other-tenant", "Other tenant").await;
    let (status, list) = request(&app, Method::GET, BASE, "reader", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let payload =
        json!({"name":"Renamed","description":"Description","icon":"📋","expected_version":1});
    for token in ["other-project", "other-tenant"] {
        for (method, suffix, body) in [
            (Method::GET, "", None),
            (Method::PATCH, "", Some(payload.clone())),
            (Method::DELETE, "?expected_version=1", None),
            (Method::GET, "/records", None),
            (Method::GET, "/fields", None),
        ] {
            let (status, body) = request(
                &app,
                method,
                &format!("{}{suffix}", path(&database)),
                token,
                body,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{token}: {body}");
        }
    }
    for token in ["reader", "denied"] {
        let (status, body) = request(
            &app,
            Method::PATCH,
            &path(&database),
            token,
            Some(payload.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    assert_eq!(
        request(&app, Method::GET, BASE, "denied", None).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &path(&database),
        "writer",
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["version"], 2);
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &path(&database),
            "writer",
            Some(payload)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{}?expected_version=1", path(&database)),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{}?expected_version=2", path(&database)),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn scalar_fields_validate_records_and_concurrent_edits_keep_one_winner(db: PgPool) {
    let app = fixture(&db).await;
    let database = create_database(&app, "writer", "Typed records").await;
    let option = id(900).to_string();
    let mut values = serde_json::Map::new();
    let mut fields = Vec::new();
    for (kind, good, bad) in [
        ("text", json!("Customer"), json!(5)),
        ("long_text", json!("Multiline\ntext"), json!(false)),
        ("number", json!(12.5), json!("12.5")),
        (
            "date",
            json!("2026-09-21T12:30:00+03:00"),
            json!("2026-02-30"),
        ),
        ("boolean", json!(true), json!("true")),
        ("single_select", json!(option), json!("missing")),
        ("multi_select", json!([option]), json!([option, option])),
        (
            "url",
            json!("https://example.com/page"),
            json!("javascript:alert(1)"),
        ),
        ("email", json!("user@example.com"), json!("wrong")),
    ] {
        let config = if kind.ends_with("select") {
            json!({"options":[{"id":option,"label":"Active","color":"green"}]})
        } else {
            json!({})
        };
        let field = create_field(&app, &database, kind, config).await;
        let key = field["id"].as_str().unwrap().to_owned();
        let (status, body) = request(
            &app,
            Method::POST,
            &format!("{}/records", path(&database)),
            "writer",
            Some(json!({"values":{key.clone():bad}})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{kind}: {body}");
        values.insert(key, good);
        fields.push(field);
    }
    let record_path = format!("{}/records", path(&database));
    let (status, row) = request(
        &app,
        Method::POST,
        &record_path,
        "writer",
        Some(json!({"values":values,"content_markdown":"# Customer details\n\nSaved content"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{row}");
    assert_eq!(row["values"], json!(values));
    assert_eq!(
        row["content_markdown"],
        "# Customer details\n\nSaved content"
    );
    let row_path = format!("{record_path}/{}", row["id"].as_str().unwrap());
    assert_eq!(
        request(&app, Method::GET, &row_path, "reader", None)
            .await
            .1,
        row
    );
    assert_eq!(
        request(&app, Method::GET, &row_path, "other-project", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let list = request(&app, Method::GET, &record_path, "reader", None)
        .await
        .1;
    assert_eq!(list["total"], 1);
    assert!(list["items"][0].get("content_markdown").is_none());
    let payload = json!({"values":values,"content_markdown":"edited","expected_version":1});
    let (first, second) = tokio::join!(
        request(
            &app,
            Method::PATCH,
            &row_path,
            "writer",
            Some(payload.clone())
        ),
        request(&app, Method::PATCH, &row_path, "writer", Some(payload)),
    );
    let statuses = [first.0, second.0];
    assert!(statuses.contains(&StatusCode::OK), "{first:?} {second:?}");
    assert!(
        statuses.contains(&StatusCode::CONFLICT),
        "{first:?} {second:?}"
    );
    let invalid = json!({"values":{id(999).to_string():"unknown"}});
    assert_eq!(
        request(&app, Method::POST, &record_path, "writer", Some(invalid))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let number = fields
        .iter()
        .find(|field| field["field_type"] == "number")
        .unwrap();
    assert_eq!(
        request(
            &app,
            Method::POST,
            &record_path,
            "writer",
            Some(json!({"values":{number["id"].as_str().unwrap():9_007_199_254_740_992_u64}}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let text = &fields[0];
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!(
            "{}/fields/{}",
            path(&database),
            text["id"].as_str().unwrap()
        ),
        "writer",
        Some(json!({"name":"Converted","field_type":"number","config":{},"expected_version":1})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let current = detail(&app, &database).await;
    let ids = current["fields"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .map(|field| field["id"].clone())
        .collect::<Vec<_>>();
    let order = json!({"ids":ids,"expected_version":current["database"]["version"]});
    assert_eq!(
        request(
            &app,
            Method::PUT,
            &format!("{}/fields/order", path(&database)),
            "writer",
            Some(order.clone())
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            Method::PUT,
            &format!("{}/fields/order", path(&database)),
            "writer",
            Some(order)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let current = detail(&app, &database).await;
    assert_eq!(current["fields"][0]["id"], ids[0]);
    let first_field = &current["fields"][0];
    let removed_key = first_field["id"].as_str().unwrap();
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!(
                "{}/fields/{removed_key}?expected_version={}",
                path(&database),
                first_field["version"]
            ),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let after = request(&app, Method::GET, &row_path, "reader", None)
        .await
        .1;
    assert!(after["values"].get(removed_key).is_none());
    assert_eq!(after["version"], 3);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn embeds_validate_scope_reorder_without_ties_and_invalidate_note_versions(db: PgPool) {
    let app = fixture(&db).await;
    let database = create_database(&app, "writer", "Embedded").await;
    let foreign = create_database(&app, "other-project", "Private").await;
    let (status, note) = request(
        &app,
        Method::POST,
        "/api/v1/notes",
        "writer",
        Some(json!({"title":"Page"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let note_path = format!("/api/v1/notes/{}", note["id"].as_str().unwrap());
    let embeds_path = format!("{note_path}/databases");
    assert_eq!(
        request(
            &app,
            Method::POST,
            &embeds_path,
            "writer",
            Some(json!({"database_id":foreign["id"]}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            &embeds_path,
            "reader",
            Some(json!({"database_id":database["id"]}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut created = Vec::new();
    for _ in 0..3 {
        let (status, embed) = request(
            &app,
            Method::POST,
            &embeds_path,
            "writer",
            Some(json!({"database_id":database["id"]})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{embed}");
        created.push(embed);
    }
    let third_path = format!("{embeds_path}/{}", created[2]["id"].as_str().unwrap());
    let (status, moved) = request(
        &app,
        Method::PATCH,
        &third_path,
        "writer",
        Some(
            json!({"database_id":database["id"],"view_id":null,"position":0,"expected_version":1}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    let list = request(&app, Method::GET, &embeds_path, "reader", None)
        .await
        .1;
    let items = list["items"].as_array().unwrap();
    assert_eq!(
        items
            .iter()
            .map(|value| value["position"].clone())
            .collect::<Vec<_>>(),
        vec![json!(0), json!(1), json!(2)]
    );
    assert_eq!(items[0]["id"], created[2]["id"]);
    assert_eq!(items[1]["id"], created[0]["id"]);
    assert_eq!(items[1]["version"], 2);
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!(
                "{embeds_path}/{}?expected_version=1",
                created[0]["id"].as_str().unwrap()
            ),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, Method::GET, &note_path, "reader", None)
            .await
            .1["version"],
        5
    );
    let current = detail(&app, &database).await;
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!(
                "{}?expected_version={}",
                path(&database),
                current["database"]["version"]
            ),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{third_path}?expected_version=2"),
            "writer",
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let list = request(&app, Method::GET, &embeds_path, "reader", None)
        .await
        .1;
    assert_eq!(list["items"][0]["position"], 0);
    assert_eq!(list["items"][1]["position"], 1);
    assert_eq!(
        request(&app, Method::GET, &embeds_path, "other-tenant", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn bounded_records_fields_and_pagination_are_enforced(db: PgPool) {
    let app = fixture(&db).await;
    let database = create_database(&app, "writer", "Capacity").await;
    let database_id: Uuid = serde_json::from_value(database["id"].clone()).unwrap();
    sqlx::query("INSERT INTO note_database_fields(id,tenant_id,project_id,database_id,name,field_type,position) SELECT gen_random_uuid(),$1,$2,$3,'Field ' || n,'text',n-1 FROM generate_series(1,100) n")
        .bind(id(1)).bind(id(2)).bind(database_id).execute(&db).await.unwrap();
    let (status, body) = request(
        &app,
        Method::POST,
        &format!("{}/fields", path(&database)),
        "writer",
        Some(json!({"name":"Overflow","field_type":"text"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    sqlx::query("INSERT INTO note_database_records(id,tenant_id,project_id,database_id,position) SELECT gen_random_uuid(),$1,$2,$3,n-1 FROM generate_series(1,5000) n")
        .bind(id(1)).bind(id(2)).bind(database_id).execute(&db).await.unwrap();
    let records_path = format!("{}/records", path(&database));
    assert_eq!(
        request(&app, Method::POST, &records_path, "writer", Some(json!({})))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, body) = request(
        &app,
        Method::GET,
        &format!("{records_path}?page=10&per_page=500"),
        "reader",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 5000);
    assert_eq!(body["items"].as_array().unwrap().len(), 500);
    assert_eq!(body["items"][0]["position"], 4500);
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("{records_path}?page=0"),
            "reader",
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let current = detail(&app, &database).await;
    let duplicate = current["fields"][0]["id"].clone();
    assert_eq!(request(&app, Method::PUT, &format!("{}/fields/order",path(&database)), "writer", Some(json!({"ids":[duplicate,duplicate],"expected_version":current["database"]["version"]}))).await.0, StatusCode::CONFLICT);
}
