use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(870_000 + n)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header(
                    "cookie",
                    "tzomet_session=integration-visibility-session; tzomet_csrf=integration-visibility-csrf",
                )
                .header("x-csrf-token", "integration-visibility-csrf")
                .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Visibility'),('{foreign}','Other');
        INSERT INTO users (id,email,display_name) VALUES ('{user}','visibility@example.test','Manager');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES
          ('{a}','{tenant}','Project A','visibility-a'), ('{b}','{tenant}','Project B','visibility-b'),
          ('{c}','{foreign}','Foreign project','visibility-c');
        INSERT INTO departments (id,tenant_id,project_id,name) VALUES
          ('{support}','{tenant}','{a}','Support'), ('{sales}','{tenant}','{a}','Sales'),
          ('{ops}','{tenant}','{b}','Operations'), ('{foreign_dept}','{foreign}','{c}','Foreign');
        UPDATE projects SET default_department_id='{support}' WHERE id='{a}';
        UPDATE projects SET default_department_id='{ops}' WHERE id='{b}';
        INSERT INTO inboxes (id,tenant_id,project_id,department_id,name) VALUES
          ('{inbox_a}','{tenant}','{a}','{support}','Support inbox'),
          ('{inbox_sales}','{tenant}','{a}','{sales}','Sales inbox'),
          ('{inbox_b}','{tenant}','{b}','{ops}','Operations inbox');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions)
          SELECT '{tenant}',id,'manager','Manager','manager',ARRAY['projects:read','channels:read','channels:manage','ai:manage','knowledge:manage','reply_templates:manage','integrations:manage']
          FROM projects WHERE tenant_id='{tenant}';
        INSERT INTO memberships (id,tenant_id,user_id,role,permissions,director_access)
          VALUES ('{member}','{tenant}','{user}','manager',ARRAY['projects:read','channels:read','channels:manage','ai:manage','knowledge:manage','reply_templates:manage','integrations:manage'],true);
        INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at)
          VALUES ('{session}','{tenant}','{a}','{user}','{member}',decode('{token:x}','hex'),decode('{csrf:x}','hex'),now()+interval '1 hour',now()+interval '2 hours');
    "#, tenant=id(1), foreign=id(2), user=id(3), a=id(4), b=id(5), c=id(6), support=id(7), sales=id(8), ops=id(9), foreign_dept=id(10), inbox_a=id(11), inbox_sales=id(12), inbox_b=id(13), member=id(14), session=id(15), token=Sha256::digest(b"integration-visibility-session"), csrf=Sha256::digest(b"integration-visibility-csrf")))
        .execute(db).await.unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    tz_backend::app::router(state)
}

async fn select(app: &Router, project: Uuid, department: Option<Uuid>) {
    let (status, body) = request(
        app,
        Method::POST,
        "/api/v1/auth/project",
        Some(json!({"project_id":project})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = request(
        app,
        Method::POST,
        "/api/v1/auth/department",
        Some(json!({"department_id":department})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn profile(name: &str) -> Value {
    json!({"name":name,"status":"draft","provider_connection_id":null,"language":"en","max_output_tokens":800,"public_identities":[{"language":"en","display_name":name}]})
}

async fn create(app: &Router, path: &str, input: &Value) -> Value {
    let (status, body) = request(app, Method::POST, path, Some(input.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "POST {path}: {body}");
    body
}

fn integration(name: &str) -> Value {
    json!({"name":name,"key":name,"description":"Lookup","base_url":"https://api.example.test",
        "status":"active","auth_kind":"none","clear_token":false,"ai_profile_ids":[],
        "actions":[{"key":"lookup","name":"Lookup","description":"Read a record","path_template":"/records/{id}"}]})
}

const ENDPOINT: &str = "/api/v1/integrations/apis";

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn integration_scope_controls_listing_mutations_and_cross_project_grants(db: PgPool) {
    let app = fixture(&db).await;
    select(&app, id(4), None).await;
    let global = create(&app, ENDPOINT, &integration("global")).await;
    assert_eq!(
        global["visibility"],
        json!({"project_ids":[],"department_ids":[]})
    );
    let mut scoped_input = integration("scoped");
    scoped_input["visibility"] = json!({"project_ids":[id(4)],"department_ids":[id(7)]});
    let scoped = create(&app, ENDPOINT, &scoped_input).await;
    let scoped_path = format!("{ENDPOINT}/{}", scoped["id"].as_str().unwrap());
    let global_path = format!("{ENDPOINT}/{}", global["id"].as_str().unwrap());

    select(&app, id(4), Some(id(8))).await;
    let (status, list) = request(&app, Method::GET, ENDPOINT, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["id"], global["id"]);
    for method in [Method::PUT, Method::DELETE] {
        let (status, _) = request(&app, method, &scoped_path, Some(scoped_input.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // Ordinary managers cannot request a global integration, even by crafting JSON.
    let mut employee_input = integration("employee");
    employee_input["visibility"] = json!({"project_ids":[],"department_ids":[]});
    let employee = create(&app, ENDPOINT, &employee_input).await;
    assert_eq!(
        employee["visibility"],
        json!({"project_ids":[id(4)],"department_ids":[id(8)]})
    );
    let employee_path = format!("{ENDPOINT}/{}", employee["id"].as_str().unwrap());
    let (status, _) = request(&app, Method::PUT, &employee_path, Some(employee_input)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A visible shared resource keeps its existing scope when edited by an employee.
    let mut global_input = integration("global");
    global_input["description"] = json!("Changed from Sales");
    let (status, updated) =
        request(&app, Method::PUT, &global_path, Some(global_input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["visibility"], global["visibility"]);

    select(&app, id(5), None).await;
    let agent = create(&app, "/api/v1/ai/profiles", &profile("Project B agent")).await;
    global_input["ai_profile_ids"] = json!([agent["id"]]);
    let (status, updated) =
        request(&app, Method::PUT, &global_path, Some(global_input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["ai_profile_ids"], json!([agent["id"]]));
    let owner: Uuid = sqlx::query_scalar("SELECT project_id FROM api_integrations WHERE id=$1")
        .bind(Uuid::parse_str(global["id"].as_str().unwrap()).unwrap())
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(owner, id(4));
    // Duplicate keys from different owners cannot become ambiguous runtime grants.
    let mut duplicate = integration("global");
    duplicate["ai_profile_ids"] = json!([agent["id"]]);
    let (status, _) = request(&app, Method::POST, ENDPOINT, Some(duplicate)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    global_input["visibility"] = json!({"project_ids":[id(5)],"department_ids":[id(9)]});
    let (status, updated) =
        request(&app, Method::PUT, &global_path, Some(global_input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    for visibility in [
        json!({"project_ids":[id(6)],"department_ids":[]}),
        json!({"project_ids":[id(5)],"department_ids":[id(7)]}),
    ] {
        global_input["visibility"] = visibility;
        let (status, _) =
            request(&app, Method::PUT, &global_path, Some(global_input.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    select(&app, id(4), None).await;
    let (status, list) = request(&app, Method::GET, ENDPOINT, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert!(
        !list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == global["id"])
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn department_assigned_employees_can_manage_only_visible_integrations(db: PgPool) {
    let app = fixture(&db).await;
    sqlx::query(
        "UPDATE memberships SET project_id=$2,department_id=$3,director_access=false WHERE id=$1",
    )
    .bind(id(14))
    .bind(id(4))
    .bind(id(7))
    .execute(&db)
    .await
    .unwrap();
    let created = create(&app, ENDPOINT, &integration("support")).await;
    assert_eq!(
        created["visibility"],
        json!({"project_ids":[id(4)],"department_ids":[id(7)]})
    );
    let (status, list) = request(&app, Method::GET, ENDPOINT, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let path = format!("{ENDPOINT}/{}", created["id"].as_str().unwrap());
    let (status, _) = request(&app, Method::PUT, &path, Some(integration("support"))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(&app, Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    sqlx::query("UPDATE project_roles SET permissions=ARRAY['projects:read'] WHERE tenant_id=$1 AND id='manager'")
        .bind(id(1)).execute(&db).await.unwrap();
    let (status, _) = request(&app, Method::GET, ENDPOINT, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
