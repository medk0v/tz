use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, auth, departments, projects};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Departments test');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Original', 'original');
        INSERT INTO users (id, email, display_name) VALUES
            ('{admin}', 'admin@departments.test', 'Admin'), ('{operator}', 'operator@departments.test', 'Operator');
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role, permissions) VALUES
            ('{admin_member}', '{tenant}', NULL, '{admin}', 'admin', ARRAY['projects:read','projects:manage']),
            ('{operator_member}', '{tenant}', '{project}', '{operator}', 'operator', ARRAY['projects:read','conversations:read']);
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions) VALUES
            ('{tenant}', '{project}', 'operator', 'Operator', 'operator', ARRAY['projects:read','conversations:read']);
        INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id, token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at) VALUES
            ('{admin_session}', '{tenant}', NULL, '{admin}', '{admin_member}', decode('{admin_hash:x}', 'hex'), decode('{csrf_hash:x}', 'hex'), now()+interval '1 hour', now()+interval '2 hours'),
            ('{operator_session}', '{tenant}', '{project}', '{operator}', '{operator_member}', decode('{operator_hash:x}', 'hex'), decode('{csrf_hash:x}', 'hex'), now()+interval '1 hour', now()+interval '2 hours');
        "#,
        tenant=id(1),project=id(2),admin=id(3),operator=id(4),admin_member=id(5),operator_member=id(6),admin_session=id(7),operator_session=id(8),
        admin_hash=Sha256::digest(b"department-admin"),operator_hash=Sha256::digest(b"department-operator"),csrf_hash=Sha256::digest(b"department-csrf"),
    )).execute(db).await.unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    projects::router()
        .merge(departments::router())
        .merge(auth::router())
        .with_state(state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    operator: bool,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let token = if operator {
        "department-operator"
    } else {
        "department-admin"
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header(
                    "cookie",
                    format!("tzomet_session={token}; tzomet_csrf=department-csrf"),
                )
                .header("x-csrf-token", "department-csrf")
                .body(input.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn create(app: &Router, slug: &str) -> Uuid {
    let (status, body) = request(app, Method::POST, "/api/v1/projects", false, Some(json!({
        "name":"Business", "slug":slug, "inbox_name":"Customer support",
        "departments":[
            {"name":"Support","sidebar_items":["conversations","contacts"],"default_page":"conversations"},
            {"name":"Sales","sidebar_items":["contacts","conversations"],"default_page":"contacts","show_default_channels":false}
        ], "default_department_index":1, "director_enabled":true
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["inbox_count"], 2);
    Uuid::parse_str(body["id"].as_str().unwrap()).unwrap()
}

async fn list(app: &Router, project_id: Uuid, operator: bool) -> Value {
    let (status, body) = request(
        app,
        Method::GET,
        &format!("/api/v1/projects/{project_id}/departments"),
        operator,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn control_center_menu_is_saved_validated_and_used_as_start_page(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "director-menu").await;
    let menu = json!({"sidebar_items":["tasks","channels","director"],"default_page":"tasks","show_default_channels":false});
    let path = format!("/api/v1/projects/{project_id}");
    let (status, result) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"director_menu":menu})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["director_menu"], menu);
    let departments = list(&app, project_id, false).await;
    assert_eq!(departments["director_menu"], menu);
    assert_eq!(departments["items"][0]["default_page"], "conversations");
    request(
        &app,
        Method::POST,
        "/api/v1/auth/project",
        false,
        Some(json!({"project_id":project_id})),
    )
    .await;
    let (status, actor) = request(
        &app,
        Method::POST,
        "/api/v1/auth/department",
        false,
        Some(json!({"department_id":null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["is_director"], true);
    assert_eq!(actor["director_default_page"], "tasks");
    for (items, page) in [
        (json!([]), "director"),
        (json!(["tasks", "tasks"]), "tasks"),
        (json!(["roles"]), "roles"),
        (json!(["tasks"]), "director"),
    ] {
        let (status, error) = request(&app, Method::PATCH, &path, false, Some(json!({"director_menu":{"sidebar_items":items,"default_page":page,"show_default_channels":true}}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    }
    assert_eq!(list(&app, project_id, false).await["director_menu"], menu);
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &path,
            true,
            Some(json!({"director_menu":menu}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn delete_department_preserves_conversations_and_repairs_workspace(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "delete-departments").await;
    let departments = list(&app, project_id, false).await;
    let support = departments["items"][0]["id"].as_str().unwrap();
    let sales = departments["items"][1]["id"].as_str().unwrap();
    let inbox = departments["items"][1]["inbox_ids"][0].as_str().unwrap();
    sqlx::raw_sql(&format!(r#"
        INSERT INTO contacts (id,tenant_id,project_id) VALUES ('{contact}','{tenant}','{project_id}');
        INSERT INTO channel_connections (id,tenant_id,project_id,inbox_id,public_id,kind,name)
            VALUES ('{channel}','{tenant}','{project_id}','{inbox}','{channel}','widget','Site');
        INSERT INTO conversations (id,tenant_id,project_id,inbox_id,channel_connection_id,contact_id,status,last_message_sequence)
            VALUES ('{conversation}','{tenant}','{project_id}','{inbox}','{channel}','{contact}','open',1);
    "#, tenant=id(1),contact=id(100),channel=id(101),conversation=id(102))).execute(&db).await.unwrap();
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/auth/project",
            false,
            Some(json!({"project_id":project_id}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/auth/department",
            false,
            Some(json!({"department_id":sales}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let path = format!("/api/v1/projects/{project_id}/departments/{sales}");
    let (status, result) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["items"].as_array().unwrap().len(), 1);
    assert_eq!(result["default_department_id"], support);
    assert_eq!(result["inbox_options"].as_array().unwrap().len(), 2);
    let (status, actor) = request(&app, Method::GET, "/api/v1/me", false, None).await;
    assert_eq!(status, StatusCode::OK, "{actor}");
    assert_eq!(actor["department_id"], support);
    let (_, result) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/projects/{project_id}"),
        false,
        Some(json!({"director_enabled":false})),
    )
    .await;
    assert_eq!(result["director_enabled"], false);
    let path = format!("/api/v1/projects/{project_id}/departments/{support}");
    assert_eq!(
        request(&app, Method::DELETE, &path, false, None).await.0,
        StatusCode::CONFLICT
    );
    request(
        &app,
        Method::PATCH,
        &format!("/api/v1/projects/{project_id}"),
        false,
        Some(json!({"director_enabled":true})),
    )
    .await;
    let (status, result) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["items"], json!([]));
    assert!(result["default_department_id"].is_null());
    assert_eq!(result["default_director_workspace"], true);
    let (_, actor) = request(&app, Method::GET, "/api/v1/me", false, None).await;
    assert_eq!(actor["is_director"], true);
    assert!(actor["department_id"].is_null());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversations WHERE project_id=$1")
            .bind(project_id)
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM inboxes WHERE project_id=$1 AND department_id IS NULL"
        )
        .bind(project_id)
        .fetch_one(&db)
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_log WHERE project_id=$1 AND action='department.deleted'"
        )
        .bind(project_id)
        .fetch_one(&db)
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        request(&app, Method::DELETE, &path, false, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn department_deletion_rejects_linked_records_and_unauthorized_callers(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "linked-department").await;
    let departments = list(&app, project_id, false).await;
    let department_id = Uuid::parse_str(departments["items"][0]["id"].as_str().unwrap()).unwrap();
    let inbox = departments["items"][0]["inbox_ids"][0].as_str().unwrap();
    let path = format!("/api/v1/projects/{project_id}/departments/{department_id}");
    assert_eq!(
        request(&app, Method::DELETE, &path, true, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("/api/v1/projects/{}/departments/{department_id}", id(2)),
            false,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO memberships (id,tenant_id,project_id,user_id,role,department_id) VALUES ($1,$2,$3,$4,'operator',$5)")
        .bind(id(200)).bind(id(1)).bind(project_id).bind(id(4)).bind(department_id).execute(&db).await.unwrap();
    let (status, error) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(
        error["error"]["message"],
        "reassign department members before deleting the department"
    );
    sqlx::query("UPDATE memberships SET revoked_at=now() WHERE id=$1")
        .bind(id(200))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO job_positions (id,tenant_id,project_id,department_id,name) VALUES ($1,$2,$3,$4,'Specialist')")
        .bind(id(201)).bind(id(1)).bind(project_id).bind(department_id).execute(&db).await.unwrap();
    let (status, error) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(
        error["error"]["message"],
        "reassign department positions before deleting the department"
    );
    sqlx::query("DELETE FROM job_positions WHERE id=$1")
        .bind(id(201))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO channel_connections (id,tenant_id,project_id,inbox_id,public_id,kind,name,visibility) VALUES ($1,$2,$3,$4,$1,'widget','Site',$5)")
        .bind(id(202)).bind(id(1)).bind(project_id).bind(Uuid::parse_str(inbox).unwrap())
        .bind(json!({"project_ids":[project_id],"department_ids":[department_id]})).execute(&db).await.unwrap();
    let (status, error) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(
        error["error"]["message"],
        "reassign department resource visibility before deleting the department"
    );
    sqlx::query("UPDATE channel_connections SET visibility='{\"project_ids\":[],\"department_ids\":[]}'::jsonb WHERE id=$1").bind(id(202)).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO company_governance (tenant_id,project_id,revision,content,updated_by) VALUES ($1,$2,1,$3,$4)")
        .bind(id(1)).bind(project_id).bind(json!({"teams":[{"department_id":department_id}]})).bind(id(3)).execute(&db).await.unwrap();
    let (status, error) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(
        error["error"]["message"],
        "reassign department process access and working teams before deleting the department"
    );
    assert_eq!(
        list(&app, project_id, false).await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    sqlx::query("DELETE FROM company_governance WHERE project_id=$1")
        .bind(project_id)
        .execute(&db)
        .await
        .unwrap();
    let (status, result) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT revoked_at IS NOT NULL AND department_id IS NULL FROM memberships WHERE id=$1"
        )
        .bind(id(200))
        .fetch_one(&db)
        .await
        .unwrap()
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn project_without_departments_uses_control_center_and_preserves_inbox(db: PgPool) {
    let app = fixture(&db).await;
    for kind in ["project", "company"] {
        let (status, project) = request(&app, Method::POST, "/api/v1/projects", false, Some(json!({
            "name":"Control center only", "slug":format!("center-{kind}"), "inbox_name":"Main inbox",
            "project_kind":kind, "departments":[], "director_enabled":true
        }))).await;
        assert_eq!(status, StatusCode::CREATED, "{project}");
        assert_eq!(project["inbox_count"], 1);
        assert!(project["default_department_id"].is_null());
        assert_eq!(project["default_director_workspace"], true);
        let project_id = Uuid::parse_str(project["id"].as_str().unwrap()).unwrap();
        let departments = list(&app, project_id, false).await;
        assert_eq!(departments["items"], json!([]));
        assert_eq!(departments["can_access_director"], true);
        assert_eq!(departments["inbox_options"].as_array().unwrap().len(), 1);
        let (status, actor) = request(
            &app,
            Method::POST,
            "/api/v1/auth/project",
            false,
            Some(json!({"project_id":project_id})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{actor}");
        assert_eq!(actor["is_director"], true);
        assert!(actor["department_id"].is_null());
        let (status, overview) = request(
            &app,
            Method::GET,
            &format!("/api/v1/projects/{project_id}/overview"),
            false,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{overview}");
        assert_eq!(overview["departments"], json!([]));
        assert_eq!(overview["totals"]["inbox_count"], 1);
        let (status, error) = request(
            &app,
            Method::PATCH,
            &format!("/api/v1/projects/{project_id}"),
            false,
            Some(json!({"director_enabled":false})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        // Even an older project default cannot leave a permitted member without a workspace.
        sqlx::query("UPDATE projects SET default_director_workspace=false WHERE id=$1")
            .bind(project_id)
            .execute(&db)
            .await
            .unwrap();
        let (status, actor) = request(
            &app,
            Method::POST,
            "/api/v1/auth/project",
            false,
            Some(json!({"project_id":project_id})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{actor}");
        assert_eq!(actor["is_director"], true);
        let (status, department) = request(
            &app,
            Method::POST,
            &format!("/api/v1/projects/{project_id}/departments"),
            false,
            Some(json!({"name":"Operations"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{department}");
        let (status, updated) = request(
            &app,
            Method::PATCH,
            &format!("/api/v1/projects/{project_id}"),
            false,
            Some(json!({"director_enabled":false})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{updated}");
    }
    for extra in [
        json!({"director_enabled":false}),
        json!({"default_department_index":0}),
    ] {
        let mut input = json!({"name":"Invalid", "slug":"invalid-empty", "inbox_name":"Main", "departments":[]});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (status, error) =
            request(&app, Method::POST, "/api/v1/projects", false, Some(input)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn project_creation_is_atomic_and_default_department_is_valid(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "business").await;
    let body = list(&app, project_id, false).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["items"][0]["show_default_channels"], true);
    assert_eq!(body["items"][1]["show_default_channels"], false);
    assert_eq!(body["default_department_id"], body["items"][1]["id"]);
    assert_eq!(body["can_access_director"], true);
    assert_eq!(body["inbox_options"].as_array().unwrap().len(), 2);
    let (status, invalid) = request(
        &app,
        Method::POST,
        "/api/v1/projects",
        false,
        Some(json!({
            "name":"Invalid", "slug":"invalid", "inbox_name":"Support",
            "departments":[{"name":"Repeated"},{"name":"Repeated"}]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{invalid}");
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM projects WHERE tenant_id=$1 AND slug='invalid')"
        )
        .bind(id(1))
        .fetch_one(&db)
        .await
        .unwrap()
    );
    sqlx::raw_sql(&format!(r#"
        INSERT INTO contacts (id, tenant_id, project_id) VALUES ('{contact}', '{tenant}', '{project_id}');
        INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id, kind, name) VALUES
            ('{support_channel}', '{tenant}', '{project_id}', '{support_inbox}', '{support_channel}', 'widget', 'Support site'),
            ('{sales_channel}', '{tenant}', '{project_id}', '{sales_inbox}', '{sales_channel}', 'widget', 'Sales site');
        INSERT INTO conversations (id, tenant_id, project_id, inbox_id, channel_connection_id, contact_id, status, last_message_sequence) VALUES
            ('{open_support}', '{tenant}', '{project_id}', '{support_inbox}', '{support_channel}', '{contact}', 'open', 1),
            ('{resolved_support}', '{tenant}', '{project_id}', '{support_inbox}', '{support_channel}', '{contact}', 'resolved', 2),
            ('{open_sales}', '{tenant}', '{project_id}', '{sales_inbox}', '{sales_channel}', '{contact}', 'waiting_customer', 1),
            ('{empty_support}', '{tenant}', '{project_id}', '{support_inbox}', '{support_channel}', '{contact}', 'new', 0);
    "#,
        tenant=id(1),contact=id(100),support_channel=id(101),sales_channel=id(102),
        support_inbox=body["items"][0]["inbox_ids"][0].as_str().unwrap(),
        sales_inbox=body["items"][1]["inbox_ids"][0].as_str().unwrap(),
        open_support=id(110),resolved_support=id(111),open_sales=id(112),empty_support=id(113),
    )).execute(&db).await.unwrap();
    let (status, overview) = request(
        &app,
        Method::GET,
        &format!("/api/v1/projects/{project_id}/overview"),
        false,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(overview["totals"]["inbox_count"], 2);
    assert_eq!(overview["totals"]["member_count"], 1);
    assert_eq!(overview["totals"]["open_conversations"], 2);
    assert_eq!(overview["totals"]["conversation_count"], 3);
    assert_eq!(overview["departments"][0]["conversation_count"], 2);
    assert_eq!(overview["departments"][0]["open_conversations"], 1);
    assert_eq!(overview["departments"][1]["conversation_count"], 1);
    assert_eq!(overview["departments"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn department_default_channels_setting_is_saved_and_preserved(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "channel-settings").await;
    let departments_path = format!("/api/v1/projects/{project_id}/departments");
    let (status, created) = request(&app, Method::POST, &departments_path, false, Some(json!({
        "name":"Operations", "sidebar_items":["channels"], "default_page":"channels", "show_default_channels":false
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["show_default_channels"], false);
    let department_id = created["id"].as_str().unwrap();
    let path = format!("{departments_path}/{department_id}");
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"name":"Back office"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["show_default_channels"], false);
    let body = list(&app, project_id, false).await;
    let saved = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == department_id)
        .unwrap();
    assert_eq!(saved["show_default_channels"], false);
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"show_default_channels":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["show_default_channels"], true);
    assert_eq!(updated["inbox_ids"], created["inbox_ids"]);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn department_configuration_rejects_foreign_inboxes_and_orphans(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "business").await;
    let other_project = create(&app, "other").await;
    let body = list(&app, project_id, false).await;
    let other = list(&app, other_project, false).await;
    let department_id = body["items"][0]["id"].as_str().unwrap();
    let path = format!("/api/v1/projects/{project_id}/departments/{department_id}");
    for input in [
        json!({"sidebar_items":["contacts"],"default_page":"conversations"}),
        json!({"sidebar_items":["contacts","contacts"],"default_page":"contacts"}),
        json!({"inbox_ids":[]}),
        json!({"inbox_ids":[body["items"][0]["inbox_ids"][0],other["items"][0]["inbox_ids"][0]]}),
    ] {
        let (status, result) = request(&app, Method::PATCH, &path, false, Some(input)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{result}");
    }
    let (status, result) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/projects/{project_id}"),
        false,
        Some(json!({"default_department_id":other["items"][0]["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{result}");
    let (status, result) = request(&app, Method::PATCH, &path, false, Some(json!({"name":"Help desk","sidebar_items":["contacts","conversations"],"default_page":"contacts"}))).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["name"], "Help desk");
    assert_eq!(result["default_page"], "contacts");
    assert_eq!(result["inbox_ids"], body["items"][0]["inbox_ids"]);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn department_membership_preserves_scope_and_blocks_director_access(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "business").await;
    let other_project = create(&app, "other").await;
    let body = list(&app, project_id, false).await;
    let other = list(&app, other_project, false).await;
    let department_id = &body["items"][0]["id"];
    let path = format!("/api/v1/projects/{project_id}/members");
    for input in [
        json!({"email":"operator@departments.test","role_id":"operator","department_id":other["items"][0]["id"]}),
        json!({"email":"operator@departments.test","role_id":"admin","department_id":department_id}),
        json!({"email":"operator@departments.test","role_id":"operator","department_id":department_id,"director_access":true}),
    ] {
        let (status, result) = request(&app, Method::POST, &path, false, Some(input)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{result}");
    }
    let (status, granted) = request(&app, Method::POST, &path, false, Some(json!({"email":"operator@departments.test","role_id":"operator","department_id":department_id}))).await;
    assert_eq!(status, StatusCode::CREATED, "{granted}");
    let membership_id = Uuid::parse_str(granted["membership_id"].as_str().unwrap()).unwrap();
    let member_path = format!("{path}/{membership_id}");
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &member_path,
        false,
        Some(json!({"role_id":"manager"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(&updated["department_id"], department_id);
    sqlx::query("UPDATE operator_sessions SET project_id=$1,membership_id=$2 WHERE id=$3")
        .bind(project_id)
        .bind(membership_id)
        .bind(id(8))
        .execute(&db)
        .await
        .unwrap();
    let restricted = list(&app, project_id, true).await;
    assert_eq!(restricted["items"].as_array().unwrap().len(), 1);
    assert_eq!(&restricted["default_department_id"], department_id);
    assert_eq!(restricted["can_access_director"], false);
    let (status, result) = request(
        &app,
        Method::GET,
        &format!("/api/v1/projects/{project_id}/overview"),
        true,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{result}");
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &member_path,
        false,
        Some(json!({"department_id":null,"director_access":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert!(updated["department_id"].is_null());
    assert_eq!(updated["director_access"], true);
    let (status, result) = request(
        &app,
        Method::GET,
        &format!("/api/v1/projects/{project_id}/overview"),
        true,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn narrower_bearer_scope_hides_other_department_inboxes(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "business").await;
    let body = list(&app, project_id, false).await;
    let department_id = Uuid::parse_str(body["items"][0]["id"].as_str().unwrap()).unwrap();
    let allowed_inbox =
        Uuid::parse_str(body["items"][0]["inbox_ids"][0].as_str().unwrap()).unwrap();
    let (status, granted) = request(
        &app,
        Method::POST,
        &format!("/api/v1/projects/{project_id}/members"),
        false,
        Some(json!({
            "email":"operator@departments.test", "role_id":"operator", "department_id":department_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{granted}");
    sqlx::query("INSERT INTO inboxes (id,tenant_id,project_id,department_id,name) VALUES ($1,$2,$3,$4,'Other Support inbox')")
        .bind(id(300)).bind(id(1)).bind(project_id).bind(department_id).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,role,inbox_scope,expires_at) VALUES ($1,$2,$3,$4,'Narrow department token',$5,ARRAY['projects:read'],'operator',$6,now()+interval '1 hour')")
        .bind(id(301)).bind(id(1)).bind(project_id).bind(id(4)).bind(Sha256::digest(b"narrow-department").to_vec()).bind(vec![allowed_inbox]).execute(&db).await.unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/projects/{project_id}/departments"))
                .header("authorization", "Bearer narrow-department")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["inbox_ids"], json!([allowed_inbox]));
    assert_eq!(body["inbox_options"].as_array().unwrap().len(), 1);
    assert_eq!(body["inbox_options"][0]["id"], json!(allowed_inbox));
    assert_eq!(body["can_access_director"], false);
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/projects")
                .header("authorization", "Bearer narrow-department")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["inbox_count"], 1);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn restricting_member_releases_only_assignments_outside_the_department(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "business").await;
    let body = list(&app, project_id, false).await;
    let support_department = &body["items"][0]["id"];
    let (status, member) = request(
        &app,
        Method::POST,
        &format!("/api/v1/projects/{project_id}/members"),
        false,
        Some(json!({
            "email":"operator@departments.test", "role_id":"operator"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{member}");
    sqlx::raw_sql(&format!(r#"
        INSERT INTO contacts (id,tenant_id,project_id) VALUES ('{contact}','{tenant}','{project_id}');
        INSERT INTO channel_connections (id,tenant_id,project_id,inbox_id,public_id,kind,name) VALUES
            ('{support_channel}','{tenant}','{project_id}','{support_inbox}','{support_channel}','widget','Support site'),
            ('{sales_channel}','{tenant}','{project_id}','{sales_inbox}','{sales_channel}','widget','Sales site');
        INSERT INTO conversations (id,tenant_id,project_id,inbox_id,channel_connection_id,contact_id,status,last_message_sequence) VALUES
            ('{support_conversation}','{tenant}','{project_id}','{support_inbox}','{support_channel}','{contact}','open',1),
            ('{sales_conversation}','{tenant}','{project_id}','{sales_inbox}','{sales_channel}','{contact}','open',1);
        INSERT INTO conversation_assignments (id,tenant_id,conversation_id,user_id) VALUES
            ('{support_assignment}','{tenant}','{support_conversation}','{operator}'),
            ('{sales_assignment}','{tenant}','{sales_conversation}','{operator}');
        INSERT INTO conversation_participants (id,tenant_id,conversation_id,participant_kind,user_id) VALUES
            ('{support_participant}','{tenant}','{support_conversation}','operator','{operator}'),
            ('{sales_participant}','{tenant}','{sales_conversation}','operator','{operator}');
    "#,
        tenant=id(1),operator=id(4),contact=id(400),support_channel=id(401),sales_channel=id(402),
        support_conversation=id(410),sales_conversation=id(411),support_assignment=id(420),sales_assignment=id(421),support_participant=id(430),sales_participant=id(431),
        support_inbox=body["items"][0]["inbox_ids"][0].as_str().unwrap(),sales_inbox=body["items"][1]["inbox_ids"][0].as_str().unwrap(),
    )).execute(&db).await.unwrap();
    let membership_id = member["membership_id"].as_str().unwrap();
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/projects/{project_id}/members/{membership_id}"),
        false,
        Some(json!({"department_id":support_department})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let assignments: Vec<(Uuid, bool)> = sqlx::query_as("SELECT conversation_id,unassigned_at IS NULL FROM conversation_assignments WHERE tenant_id=$1 AND user_id=$2 ORDER BY conversation_id")
        .bind(id(1)).bind(id(4)).fetch_all(&db).await.unwrap();
    assert_eq!(assignments, vec![(id(410), true), (id(411), false)]);
    let participants: Vec<(Uuid, bool)> = sqlx::query_as("SELECT conversation_id,left_at IS NULL FROM conversation_participants WHERE tenant_id=$1 AND user_id=$2 ORDER BY conversation_id")
        .bind(id(1)).bind(id(4)).fetch_all(&db).await.unwrap();
    assert_eq!(participants, vec![(id(410), true), (id(411), false)]);
    let events: Vec<Uuid> = sqlx::query_scalar("SELECT aggregate_id FROM outbox_events WHERE tenant_id=$1 AND event_type='conversation.operator_left'")
        .bind(id(1)).fetch_all(&db).await.unwrap();
    assert_eq!(events, vec![id(411)]);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn control_center_default_workspace_requires_an_enabled_control_center(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "business").await;
    let path = format!("/api/v1/projects/{project_id}");
    let (status, result) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"default_director_workspace":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["default_director_workspace"], true);
    assert_eq!(
        list(&app, project_id, false).await["default_director_workspace"],
        true
    );
    // Retiring the control center leaves nobody landing in it.
    let (status, result) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"director_enabled":false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["director_enabled"], false);
    assert_eq!(result["default_director_workspace"], false);
    for input in [
        json!({"default_director_workspace":true}),
        json!({"default_director_workspace":true, "director_enabled":false}),
    ] {
        let (status, rejected) = request(&app, Method::PATCH, &path, false, Some(input)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
    }
    let (status, result) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"default_director_workspace":true, "director_enabled":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["default_director_workspace"], true);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn sidebar_categories_round_trip_preserve_patch_and_validate_access(db: PgPool) {
    let app = fixture(&db).await;
    let project_id = create(&app, "categories").await;
    let categories = json!([
        {"id":"work","name":"Работа","items":["conversations"]},
        {"id":"custom:favorites","name":"Избранное","items":["contacts","channels"]},
        {"id":"administration","name":"Настройки","items":["projects","roles"]},
        {"id":"empty","name":"Позже","items":[]}
    ]);
    let root = format!("/api/v1/projects/{project_id}/departments");
    let (status, created) = request(&app, Method::POST, &root, false, Some(json!({
        "name":"Operations", "sidebar_items":["conversations","contacts"], "default_page":"contacts", "sidebar_categories":categories
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["sidebar_categories"], categories);
    let path = format!("{root}/{}", created["id"].as_str().unwrap());
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"name":"Renamed"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["sidebar_categories"], categories);
    let listed = list(&app, project_id, false).await;
    assert_eq!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == created["id"])
            .unwrap()["sidebar_categories"],
        categories
    );
    for invalid in [
        json!([{"id":"x","name":" ","items":[]}]),
        json!([{"id":"x","name":"X","items":["unknown"]}]),
        json!([{"id":"x","name":"X","items":["director"]}]),
        json!([{"id":"x","name":"X","items":["contacts"]},{"id":"y","name":"Y","items":["contacts"]}]),
        json!([{"id":"x","name":"X","items":[]},{"id":"x","name":"Y","items":[]}]),
    ] {
        let (status, error) = request(
            &app,
            Method::PATCH,
            &path,
            false,
            Some(json!({"sidebar_categories":invalid})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    }
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &path,
        false,
        Some(json!({"sidebar_categories":[]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["sidebar_categories"], json!([]));
    let operator_path = format!("/api/v1/projects/{}/departments", id(2));
    let (status, _) = request(
        &app,
        Method::POST,
        &operator_path,
        true,
        Some(json!({"name":"Unauthorized", "sidebar_categories":categories})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let menu = json!({"sidebar_items":["director","contacts"],"default_page":"director","show_default_channels":true,"sidebar_categories":[{"id":"main","name":"Overview","items":["director","contacts"]}]});
    let project_path = format!("/api/v1/projects/{project_id}");
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &project_path,
        false,
        Some(json!({"director_menu":menu})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(list(&app, project_id, false).await["director_menu"], menu);
    let (status, _) = request(
        &app,
        Method::PATCH,
        &project_path,
        false,
        Some(json!({"name":"Updated project"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list(&app, project_id, false).await["director_menu"], menu);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn project_creation_saves_department_categories(db: PgPool) {
    let app = fixture(&db).await;
    let categories = json!([{"id":"work","name":"My work","items":["contacts"]}]);
    let (status, created) = request(&app, Method::POST, "/api/v1/projects", false, Some(json!({
        "name":"Categories", "slug":"project-categories", "inbox_name":"Support",
        "departments":[{"name":"Support","sidebar_items":["contacts"],"default_page":"contacts","sidebar_categories":categories}],
        "default_department_index":0,"director_enabled":true
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let project_id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        list(&app, project_id, false).await["items"][0]["sidebar_categories"],
        categories
    );
}
