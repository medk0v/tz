use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{
    AppState, Config,
    error::AppError,
    note_databases::{self, fields, model::*, records, relations, views},
};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn fixture(db: &PgPool) -> Scope {
    sqlx::raw_sql(&format!(
        "INSERT INTO tenants(id,name) VALUES('{tenant}','Tables');
         INSERT INTO projects(id,tenant_id,name,slug) VALUES('{project}','{tenant}','Main','main'),('{other}','{tenant}','Other','other');
         INSERT INTO note_databases(id,tenant_id,project_id,name) VALUES('{source}','{tenant}','{project}','Source'),('{target}','{tenant}','{project}','Target'),('{foreign}','{tenant}','{other}','Other project');
         INSERT INTO note_database_records(id,tenant_id,project_id,database_id,position) VALUES('{a}','{tenant}','{project}','{source}',0),('{b}','{tenant}','{project}','{source}',1),('{x}','{tenant}','{project}','{target}',0),('{y}','{tenant}','{project}','{target}',1),('{outside}','{tenant}','{other}','{foreign}',0);",
         tenant=id(1),project=id(2),other=id(3),source=id(10),target=id(11),foreign=id(12),a=id(20),b=id(21),x=id(22),y=id(23),outside=id(24)
    )).execute(db).await.unwrap();
    Scope {
        tenant_id: id(1),
        project_id: id(2),
    }
}

async fn create_field(db: &PgPool, scope: &Scope, database: Uuid, value: Value) -> Field {
    let mut tx = db.begin().await.unwrap();
    note_databases::lock_project(&mut tx, scope).await.unwrap();
    let field = fields::create(
        &mut tx,
        scope,
        database,
        serde_json::from_value(value).unwrap(),
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    field
}

async fn projected(db: &PgPool, scope: &Scope, database: Uuid) -> Vec<Record> {
    let mut tx = db.begin().await.unwrap();
    note_databases::lock_project(&mut tx, scope).await.unwrap();
    let fields = fields::list(&mut tx, scope, database).await.unwrap();
    let mut records=sqlx::query_as("SELECT id,database_id,position,version,field_values AS values,NULL::text AS content_markdown,created_at,updated_at FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 ORDER BY position,id")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database).fetch_all(&mut *tx).await.unwrap();
    relations::project_records(&mut tx, scope, database, &fields, &mut records)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    records
}

async fn replace(
    db: &PgPool,
    scope: &Scope,
    database: Uuid,
    record: Uuid,
    value: Value,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    note_databases::lock_project(&mut tx, scope).await?;
    let fields = fields::list(&mut tx, scope, database).await?;
    let mut value = value;
    relations::replace_record_relations(&mut tx, scope, &fields, record, &mut value).await?;
    tx.commit().await?;
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn canonical_edges_inverse_cardinality_and_cross_project_rejection(db: PgPool) {
    let scope = fixture(&db).await;
    let relation=create_field(&db,&scope,id(10),json!({"name":"Targets","field_type":"relation","relation":{"target_database_id":id(11),"cardinality":"one_to_many","inverse_name":"Source"}})).await;
    let inverse = relation.inverse_field_id.unwrap();
    replace(
        &db,
        &scope,
        id(10),
        id(20),
        json!({relation.id.to_string():[id(22),id(23)]}),
    )
    .await
    .unwrap();
    let target = projected(&db, &scope, id(11)).await;
    assert_eq!(target[0].values[inverse.to_string()], json!([id(20)]));
    assert_eq!(target[1].values[inverse.to_string()], json!([id(20)]));
    assert_eq!(target[0].version, 2);
    assert!(matches!(
        replace(
            &db,
            &scope,
            id(10),
            id(21),
            json!({relation.id.to_string():[id(22)]})
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(
        projected(&db, &scope, id(10)).await[0].values[relation.id.to_string()],
        json!([id(22), id(23)])
    );
    assert!(matches!(
        replace(
            &db,
            &scope,
            id(11),
            id(22),
            json!({inverse.to_string():[id(20),id(21)]})
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        replace(
            &db,
            &scope,
            id(10),
            id(20),
            json!({relation.id.to_string():[id(24)]})
        )
        .await,
        Err(AppError::NotFound)
    ));
    replace(
        &db,
        &scope,
        id(11),
        id(22),
        json!({inverse.to_string():[id(21)]}),
    )
    .await
    .unwrap();
    let source = projected(&db, &scope, id(10)).await;
    assert_eq!(source[0].values[relation.id.to_string()], json!([id(23)]));
    assert_eq!(source[1].values[relation.id.to_string()], json!([id(22)]));
    assert_eq!(source[0].version, 2);
    assert_eq!(source[1].version, 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM note_record_relations")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn rollups_compute_live_values_and_dependent_fields_cannot_break_them(db: PgPool) {
    let scope = fixture(&db).await;
    let number = create_field(
        &db,
        &scope,
        id(11),
        json!({"name":"Amount","field_type":"number"}),
    )
    .await;
    let relation=create_field(&db,&scope,id(10),json!({"name":"Targets","field_type":"relation","relation":{"target_database_id":id(11),"cardinality":"many_to_many","inverse_name":"Sources"}})).await;
    sqlx::query("UPDATE note_database_records SET field_values=jsonb_build_object($1::text,CASE id WHEN $2 THEN 5 ELSE 9 END) WHERE database_id=$3")
        .bind(number.id.to_string()).bind(id(22)).bind(id(11)).execute(&db).await.unwrap();
    replace(
        &db,
        &scope,
        id(10),
        id(20),
        json!({relation.id.to_string():[id(22),id(23)]}),
    )
    .await
    .unwrap();
    let sum=create_field(&db,&scope,id(10),json!({"name":"Total","field_type":"rollup","rollup":{"relation_field_id":relation.id,"target_field_id":number.id,"operation":"sum"}})).await;
    let count=create_field(&db,&scope,id(10),json!({"name":"Count","field_type":"rollup","rollup":{"relation_field_id":relation.id,"operation":"count"}})).await;
    let first = projected(&db, &scope, id(10)).await;
    assert_eq!(first[0].values[sum.id.to_string()], json!(14.0));
    assert_eq!(first[0].values[count.id.to_string()], json!(2));
    assert!(matches!(
        replace(&db, &scope, id(10), id(20), json!({sum.id.to_string():999})).await,
        Err(AppError::BadRequest(_))
    ));
    let mut tx = db.begin().await.unwrap();
    assert!(matches!(
        relations::delete_field(&mut tx, &scope, &relation).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        relations::validate_field_update(&mut tx, &scope, &number, "text").await,
        Err(AppError::Conflict(_))
    ));
    tx.rollback().await.unwrap();
    sqlx::query(
        "UPDATE note_database_records SET field_values=jsonb_build_object($1::text,20) WHERE id=$2",
    )
    .bind(number.id.to_string())
    .bind(id(22))
    .execute(&db)
    .await
    .unwrap();
    assert_eq!(
        projected(&db, &scope, id(10)).await[0].values[sum.id.to_string()],
        json!(29.0)
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn one_to_one_and_many_to_many_preserve_inverse_and_layout_invariants(db: PgPool) {
    let scope = fixture(&db).await;
    let one = create_field(&db, &scope, id(10), json!({"name":"One","field_type":"relation","relation":{"target_database_id":id(11),"cardinality":"one_to_one","inverse_name":"Inverse"}})).await;
    replace(
        &db,
        &scope,
        id(10),
        id(20),
        json!({one.id.to_string():[id(22)]}),
    )
    .await
    .unwrap();
    assert!(matches!(
        replace(
            &db,
            &scope,
            id(10),
            id(20),
            json!({one.id.to_string():[id(22),id(23)]})
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        replace(
            &db,
            &scope,
            id(10),
            id(21),
            json!({one.id.to_string():[id(22)]})
        )
        .await,
        Err(AppError::Conflict(_))
    ));
    sqlx::query("INSERT INTO note_database_views(id,tenant_id,project_id,database_id,name,position,config) VALUES($1,$2,$3,$4,'Layout',0,$5)")
        .bind(id(90)).bind(scope.tenant_id).bind(scope.project_id).bind(id(11)).bind(json!({"hidden_fields":[one.inverse_field_id]})).execute(&db).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    note_databases::lock_project(&mut tx, &scope).await.unwrap();
    fields::delete(&mut tx, &scope, id(10), one.id, 1, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM note_database_fields")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    let config: Value = sqlx::query_scalar("SELECT config FROM note_database_views WHERE id=$1")
        .bind(id(90))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(config["hidden_fields"], json!([]));
    let many = create_field(&db,&scope,id(10),json!({"name":"Many","field_type":"relation","relation":{"target_database_id":id(11),"cardinality":"many_to_many","inverse_name":"Inverse many"}})).await;
    replace(
        &db,
        &scope,
        id(10),
        id(20),
        json!({many.id.to_string():[id(22)]}),
    )
    .await
    .unwrap();
    replace(
        &db,
        &scope,
        id(10),
        id(21),
        json!({many.id.to_string():[id(22)]}),
    )
    .await
    .unwrap();
    assert_eq!(
        projected(&db, &scope, id(11)).await[0].values[many.inverse_field_id.unwrap().to_string()],
        json!([id(20), id(21)])
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn private_overflowing_rollup_does_not_block_projected_reads_or_writes(db: PgPool) {
    let scope = fixture(&db).await;
    let title = create_field(
        &db,
        &scope,
        id(10),
        json!({"name":"Title","field_type":"text"}),
    )
    .await;
    let amount = create_field(
        &db,
        &scope,
        id(11),
        json!({"name":"Private amount","field_type":"number"}),
    )
    .await;
    let relation=create_field(&db,&scope,id(10),json!({"name":"Private relation","field_type":"relation","relation":{"target_database_id":id(11),"cardinality":"many_to_many","inverse_name":"Inverse"}})).await;
    replace(
        &db,
        &scope,
        id(10),
        id(20),
        json!({relation.id.to_string():[id(22),id(23)]}),
    )
    .await
    .unwrap();
    create_field(&db,&scope,id(10),json!({"name":"Private sum","field_type":"rollup","rollup":{"relation_field_id":relation.id,"target_field_id":amount.id,"operation":"sum"}})).await;
    sqlx::query("UPDATE note_database_records SET field_values=jsonb_build_object($1::text,9007199254740991::bigint) WHERE database_id=$2")
        .bind(amount.id.to_string()).bind(id(11)).execute(&db).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    note_databases::lock_project(&mut tx, &scope).await.unwrap();
    assert!(matches!(
        records::get(&mut tx, &scope, id(10), id(20)).await,
        Err(AppError::Conflict(_))
    ));
    let allowed = std::collections::HashSet::from([title.id]);
    let public = records::get_projected(&mut tx, &scope, id(10), id(20), Some(&allowed))
        .await
        .unwrap();
    assert!(public.values.as_object().unwrap().is_empty());
    let saved=records::update_projected(&mut tx,&scope,id(10),id(20),UpdateRecord {
        values:json!({title.id.to_string():"Public change",relation.id.to_string():[id(22),id(23)]}),
        content_markdown:"Public body".into(),expected_version:public.version,
    },None,Some(&allowed)).await.unwrap();
    assert_eq!(saved.values, json!({title.id.to_string():"Public change"}));
    assert_eq!(saved.version, public.version + 1);
    records::delete(&mut tx, &scope, id(10), id(20), saved.version, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn application(db: &PgPool) -> Router {
    for (token, user, project, permissions) in [
        ("view-writer", 30, 2, vec!["notes:read", "notes:write"]),
        ("view-reader", 31, 2, vec!["notes:read"]),
        ("view-outsider", 32, 3, vec!["notes:read", "notes:write"]),
    ] {
        sqlx::query("INSERT INTO users(id,email,display_name) VALUES($1,$2,$2)")
            .bind(id(user))
            .bind(format!("{token}@notes.test"))
            .execute(db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO project_roles(tenant_id,project_id,id,name,base_role,permissions) VALUES($1,$2,$3,$3,'operator',$4)").bind(id(1)).bind(id(project)).bind(token).bind(&permissions).execute(db).await.unwrap();
        sqlx::query(
            "INSERT INTO memberships(id,tenant_id,project_id,user_id,role) VALUES($1,$2,$3,$1,$4)",
        )
        .bind(id(user))
        .bind(id(1))
        .bind(id(project))
        .bind(token)
        .execute(db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO api_keys(id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,role,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$5,now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(project)).bind(id(user)).bind(token).bind(Sha256::digest(token.as_bytes()).to_vec()).bind(&permissions).execute(db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    views::router().with_state(state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn saved_views_enforce_scope_versions_and_database_layout_references(db: PgPool) {
    let scope = fixture(&db).await;
    let number = create_field(
        &db,
        &scope,
        id(10),
        json!({"name":"Number","field_type":"number"}),
    )
    .await;
    let app = application(&db).await;
    let base = format!("/api/v1/notes/databases/{}/views", id(10));
    let input = json!({"name":"Large","config":{"filters":[{"field_id":number.id,"operator":"greater_than","value":10}],"column_widths":{number.id.to_string():120}}});
    assert_eq!(
        request(
            &app,
            Method::POST,
            &base,
            "view-reader",
            Some(input.clone())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            &base,
            "view-outsider",
            Some(input.clone())
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, created) = request(&app, Method::POST, &base, "view-writer", Some(input)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let path = format!("{base}/{}", created["id"].as_str().unwrap());
    assert_eq!(
        request(&app, Method::GET, &path, "view-outsider", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let update = json!({"name":"Changed","config":{},"expected_version":1});
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &path,
            "view-writer",
            Some(update.clone())
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, Method::PATCH, &path, "view-writer", Some(update))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(request(&app,Method::PATCH,&path,"view-writer",Some(json!({"name":"Foreign field","expected_version":2,"config":{"hidden_fields":[id(999)]}}))).await.0,StatusCode::BAD_REQUEST);
    let version: i32 = sqlx::query_scalar("SELECT version FROM note_databases WHERE id=$1")
        .bind(id(10))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            Method::PUT,
            &format!("{base}/order"),
            "view-writer",
            Some(json!({"ids":[created["id"]],"expected_version":version-1}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, ordered) = request(
        &app,
        Method::PUT,
        &format!("{base}/order"),
        "view-writer",
        Some(json!({"ids":[created["id"]],"expected_version":version})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ordered}");
    assert_eq!(ordered["version"], json!(version + 1));
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{path}?expected_version=1"),
            "view-writer",
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
            &format!("{path}?expected_version=2"),
            "view-writer",
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}
