use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_settings, job_positions, projects};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','KPI test'), ('{foreign_tenant}','Foreign');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES
            ('{project}','{tenant}','Project','kpi'), ('{other_project}','{tenant}','Other','other'),
            ('{foreign_project}','{foreign_tenant}','Foreign','foreign');
        INSERT INTO departments (id,tenant_id,project_id,name) VALUES
            ('{support}','{tenant}','{project}','Support'), ('{sales}','{tenant}','{project}','Sales'),
            ('{other_department}','{tenant}','{other_project}','Other');
        INSERT INTO users (id,email,display_name) VALUES
            ('{manager}','manager@kpi.test','Manager'), ('{employee}','employee@kpi.test','Employee'),
            ('{colleague}','colleague@kpi.test','Colleague'), ('{other_user}','other@kpi.test','Other');
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role,permissions,department_id) VALUES
            ('{manager_membership}','{tenant}','{project}','{manager}','manager',ARRAY['projects:read','teams:manage','ai:manage'],NULL),
            ('{employee_membership}','{tenant}','{project}','{employee}','operator',ARRAY['projects:read','teams:manage'],'{support}'),
            ('{colleague_membership}','{tenant}','{project}','{colleague}','operator',ARRAY['projects:read'],'{support}'),
            ('{other_membership}','{tenant}','{other_project}','{other_user}','operator',ARRAY['projects:read'],NULL);
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,created_by,visibility) VALUES
            ('{agent}','{tenant}','{project}','Support AI','{manager}','{{"project_ids":[],"department_ids":["{support}"]}}'),
            ('{foreign_agent}','{tenant}','{other_project}','Other AI','{manager}','{{"project_ids":[],"department_ids":[]}}');
        INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,role,expires_at) VALUES
            ('{token}','{tenant}','{project}','{manager}','Token',decode('{token_hash:x}','hex'),ARRAY['projects:read','teams:manage','ai:manage'],'manager',now()+interval '1 hour');
    "#,
        tenant=id(1),project=id(2),manager=id(3),manager_membership=id(4),employee=id(5),employee_membership=id(6),
        support=id(7),sales=id(8),other_project=id(9),other_department=id(10),foreign_tenant=id(11),foreign_project=id(12),
        colleague=id(13),colleague_membership=id(14),other_user=id(15),other_membership=id(16),agent=id(17),foreign_agent=id(18),token=id(19),
        token_hash=Sha256::digest(b"kpi-token"),
    )).execute(db).await.unwrap();
    sqlx::raw_sql(
        r#"INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions)
        SELECT tenant_id,id,'manager','Manager','manager',ARRAY['projects:read','teams:manage','ai:manage'] FROM projects
        ON CONFLICT DO NOTHING;
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions)
        SELECT tenant_id,id,'operator','Operator','operator',ARRAY['projects:read','teams:manage'] FROM projects
        ON CONFLICT DO NOTHING;
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,is_system,permissions)
        SELECT tenant_id,id,'admin','Admin','admin',true,ARRAY[
          'projects:read','projects:manage','conversations:read','conversations:reply','conversations:close',
          'contacts:read','contacts:manage','channels:read','channels:manage','access_tokens:manage',
          'visitor_network:read','quality:read','quality:read_all','reviews:read','routing:manage',
          'teams:manage','ai:manage','knowledge:manage', 'notes:read', 'notes:write','reply_templates:manage','integrations:manage',
          'roles:manage','system:read','processes:read','processes:edit','processes:approve','tasks:own','tasks:manage','tasks:configure'
        ] FROM projects ON CONFLICT DO NOTHING;"#,
    ).execute(db).await.unwrap();
    for (session_id, user_id, membership_id, token) in
        [(20, 3, 4, "manager"), (21, 5, 6, "employee")]
    {
        sqlx::query("INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,now()+interval '1 hour',now()+interval '2 hours')")
            .bind(id(session_id)).bind(id(1)).bind(id(2)).bind(id(user_id)).bind(id(membership_id))
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(Sha256::digest(b"kpi-csrf").to_vec()).execute(db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    job_positions::router()
        .merge(projects::router())
        .merge(ai_settings::router())
        .with_state(state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    identity: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if identity == "token" {
        builder = builder.header(header::AUTHORIZATION, "Bearer kpi-token");
    } else {
        builder = builder
            .header(
                header::COOKIE,
                format!("tzomet_session={identity}; tzomet_csrf=kpi-csrf"),
            )
            .header("x-csrf-token", "kpi-csrf");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn metric() -> Value {
    json!({"name":"First response time","formula":"Total wait / answered conversations","data_source":"Conversation history","target":"Under 5 minutes","evaluation_period":"Monthly","data_owner":"Support lead"})
}

fn position_input(department_id: Option<Uuid>) -> Value {
    json!({"name":"Support manager","description":"Customer support","instructions":"Answer and resolve customer requests","department_id":department_id,"kpi_goal":"Improve response time and quality","draft_kpis":[metric()]})
}

fn positions_path() -> String {
    format!("/api/v1/projects/{}/positions", id(2))
}

async fn create(app: &Router, input: Value) -> Value {
    let (status, body) =
        request(app, Method::POST, &positions_path(), "manager", Some(input)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn manager_can_approve_and_edit_drafts_without_changing_approved_values(db: PgPool) {
    let app = fixture(&db).await;
    let created = create(&app, position_input(Some(id(7)))).await;
    assert_eq!(created["revision"], 1);
    assert_eq!(created["approved_kpis"], json!([]));
    let path = format!("{}/{}", positions_path(), created["id"].as_str().unwrap());
    let (status, approved) = request(
        &app,
        Method::POST,
        &format!("{path}/approve-kpis"),
        "manager",
        Some(json!({"revision":1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["approved_kpis"], json!([metric()]));
    assert_eq!(approved["approved_by"], id(3).to_string());
    assert!(approved["approved_at"].is_string());

    let mut edited = position_input(Some(id(7)));
    edited["revision"] = json!(2);
    edited["draft_kpis"][0]["target"] = json!("Under 3 minutes");
    let (status, updated) =
        request(&app, Method::PATCH, &path, "manager", Some(edited.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["revision"], 3);
    assert_eq!(updated["approved_kpis"], approved["approved_kpis"]);
    let (status, _) = request(&app, Method::PATCH, &path, "manager", Some(edited)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = request(
        &app,
        Method::POST,
        &format!("{path}/approve-kpis"),
        "manager",
        Some(json!({"revision":2})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action='job_position.kpis_approved'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(audit_count, 1);
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn approval_requires_targets_and_data_owners_and_bounds_drafts(db: PgPool) {
    let app = fixture(&db).await;
    let mut input = position_input(None);
    input["draft_kpis"][0]["target"] = json!("");
    let created = create(&app, input).await;
    let path = format!(
        "{}/{}/approve-kpis",
        positions_path(),
        created["id"].as_str().unwrap()
    );
    let (status, _) = request(
        &app,
        Method::POST,
        &path,
        "manager",
        Some(json!({"revision":1})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let mut input = position_input(None);
    input["draft_kpis"][0]["data_owner"] = json!(" ");
    let created = create(&app, input).await;
    let path = format!(
        "{}/{}/approve-kpis",
        positions_path(),
        created["id"].as_str().unwrap()
    );
    let (status, _) = request(
        &app,
        Method::POST,
        &path,
        "manager",
        Some(json!({"revision":1})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let mut input = position_input(None);
    input["draft_kpis"] = json!(vec![metric(); 21]);
    let (status, _) = request(
        &app,
        Method::POST,
        &positions_path(),
        "manager",
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn scoped_employees_see_own_account_and_only_approved_department_kpis(db: PgPool) {
    let app = fixture(&db).await;
    let support = create(&app, position_input(Some(id(7)))).await;
    create(&app, position_input(Some(id(8)))).await;
    create(&app, position_input(None)).await;
    let (status, positions) = request(&app, Method::GET, &positions_path(), "employee", None).await;
    assert_eq!(status, StatusCode::OK, "{positions}");
    assert_eq!(positions["items"].as_array().unwrap().len(), 2);
    assert!(
        positions["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|position| position["draft_kpis"] == json!([]) && position["kpi_goal"] == "")
    );
    let (status, team) = request(
        &app,
        Method::GET,
        &format!("/api/v1/projects/{}/team", id(2)),
        "employee",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{team}");
    assert_eq!(team["can_manage"], false);
    assert_eq!(team["members"].as_array().unwrap().len(), 1);
    assert_eq!(team["members"][0]["user_id"], id(5).to_string());
    assert_eq!(team["agents"], json!([]));
    let (status, _) = request(
        &app,
        Method::POST,
        &positions_path(),
        "employee",
        Some(position_input(None)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let path = format!(
        "{}/{}/approve-kpis",
        positions_path(),
        support["id"].as_str().unwrap()
    );
    let (status, _) = request(
        &app,
        Method::POST,
        &path,
        "employee",
        Some(json!({"revision":1})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn assignments_preserve_access_and_validate_project_and_department(db: PgPool) {
    let app = fixture(&db).await;
    let support = create(&app, position_input(Some(id(7)))).await;
    let sales = create(&app, position_input(Some(id(8)))).await;
    let member_path = format!("/api/v1/projects/{}/team/members/{}/position", id(2), id(5));
    let agent_path = format!("/api/v1/projects/{}/team/agents/{}/position", id(2), id(17));
    for path in [&member_path, &agent_path] {
        let (status, body) = request(
            &app,
            Method::PUT,
            path,
            "manager",
            Some(json!({"position_id":sales["id"]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let (status, body) = request(
            &app,
            Method::PUT,
            path,
            "manager",
            Some(json!({"position_id":support["id"]})),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    let membership: (String, Option<Uuid>, Vec<String>) =
        sqlx::query_as("SELECT role,department_id,permissions FROM memberships WHERE id=$1")
            .bind(id(6))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        membership,
        (
            "operator".to_owned(),
            Some(id(7)),
            vec!["projects:read".to_owned(), "teams:manage".to_owned()]
        )
    );
    let (status, team) = request(
        &app,
        Method::GET,
        &format!("/api/v1/projects/{}/team", id(2)),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{team}");
    assert_eq!(team["can_manage"], true);
    let agents = team["agents"].as_array().unwrap();
    let assigned_agent = agents
        .iter()
        .find(|agent| agent["id"] == id(17).to_string())
        .unwrap();
    assert_eq!(assigned_agent["position_id"], support["id"]);
    assert!(!agents.iter().any(|agent| agent["id"] == id(18).to_string()));

    let mut changed_department = position_input(Some(id(8)));
    changed_department["revision"] = json!(1);
    let (status, _) = request(
        &app,
        Method::PATCH,
        &format!("{}/{}", positions_path(), support["id"].as_str().unwrap()),
        "manager",
        Some(changed_department),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    for (kind, target) in [("members", id(15)), ("agents", id(18))] {
        let path = format!("/api/v1/projects/{}/team/{kind}/{target}/position", id(2));
        let (status, _) = request(
            &app,
            Method::PUT,
            &path,
            "manager",
            Some(json!({"position_id":support["id"]})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    sqlx::query("UPDATE memberships SET revoked_at=now() WHERE id=$1")
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    let (status, _) = request(
        &app,
        Method::PUT,
        &member_path,
        "manager",
        Some(json!({"position_id":support["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request(
        &app,
        Method::PUT,
        &agent_path,
        "manager",
        Some(json!({"position_id":null})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn current_project_foreign_references_and_access_tokens_are_rejected(db: PgPool) {
    let app = fixture(&db).await;
    sqlx::query("INSERT INTO job_positions (id,tenant_id,project_id,name) VALUES ($1,$2,$3,'Foreign position')")
        .bind(id(999)).bind(id(1)).bind(id(9)).execute(&db).await.unwrap();
    for project in [id(9), id(12)] {
        let path = format!("/api/v1/projects/{project}/positions");
        let (status, _) = request(&app, Method::GET, &path, "manager", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (status, _) = request(
        &app,
        Method::POST,
        &positions_path(),
        "manager",
        Some(position_input(Some(id(10)))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(&app, Method::GET, &positions_path(), "token", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &app,
        Method::POST,
        &positions_path(),
        "token",
        Some(position_input(None)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &app,
        Method::PUT,
        &format!("/api/v1/projects/{}/team/members/{}/position", id(2), id(5)),
        "manager",
        Some(json!({"position_id":id(999)})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let foreign_reference = sqlx::query("UPDATE memberships SET position_id=$1 WHERE id=$2")
        .bind(id(999))
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap_err();
    assert_eq!(
        foreign_reference
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("23503")
    );
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn generation_requires_ai_permission_in_addition_to_team_management(db: PgPool) {
    let app = fixture(&db).await;
    let position = create(&app, position_input(None)).await;
    sqlx::query(
        "UPDATE project_roles SET permissions=ARRAY['projects:read','teams:manage'] WHERE project_id=$1 AND id='manager'",
    )
    .bind(id(2))
    .execute(&db)
    .await
    .unwrap();
    let (status, _) = request(
        &app,
        Method::POST,
        &format!(
            "{}/{}/generate-kpis",
            positions_path(),
            position["id"].as_str().unwrap()
        ),
        "manager",
        Some(json!({"provider_connection_id":id(500),"goal":"Improve quality","locale":"en"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn scope_changes_clear_assignments_but_noop_updates_preserve_them(db: PgPool) {
    let app = fixture(&db).await;
    let position = create(&app, position_input(Some(id(7)))).await;
    let position_id: Uuid = position["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE memberships SET role='admin' WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions) VALUES ($1,$2,'operator','Operator','operator',ARRAY['projects:read']) ON CONFLICT DO NOTHING")
        .bind(id(1)).bind(id(2)).execute(&db).await.unwrap();
    sqlx::query("UPDATE memberships SET position_id=$2 WHERE id=$1")
        .bind(id(6))
        .bind(position_id)
        .execute(&db)
        .await
        .unwrap();
    let path = format!("/api/v1/projects/{}/members/{}", id(2), id(6));
    for (department, expected) in [(id(7), Some(position_id)), (id(8), None)] {
        let (status, body) = request(
            &app,
            Method::PATCH,
            &path,
            "manager",
            Some(json!({"department_id":department})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let actual: Option<Uuid> =
            sqlx::query_scalar("SELECT position_id FROM memberships WHERE id=$1")
                .bind(id(6))
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(actual, expected);
    }
    sqlx::query("UPDATE ai_profiles SET position_id=$2 WHERE id=$1")
        .bind(id(17))
        .bind(position_id)
        .execute(&db)
        .await
        .unwrap();
    for (department, expected) in [(id(7), Some(position_id)), (id(8), None)] {
        let (status, body) = request(&app, Method::PATCH, &format!("/api/v1/ai/profiles/{}",id(17)), "manager", Some(json!({
            "name":"Support AI", "status":"draft", "instructions":"Support customers", "language":"en", "max_output_tokens":1024,
            "public_identities":[{"language":"en","display_name":"Support"}], "visibility":{"project_ids":[],"department_ids":[department]}
        }))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let actual: Option<Uuid> =
            sqlx::query_scalar("SELECT position_id FROM ai_profiles WHERE id=$1")
                .bind(id(17))
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(actual, expected);
    }
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn position_department_change_respects_organization_seats(db: PgPool) {
    let app = fixture(&db).await;
    let position = create(&app, position_input(None)).await;
    let position_id: Uuid = position["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("INSERT INTO company_positions (id,tenant_id,project_id,title,department_id,job_position_id,occupant_kind) VALUES ($1,$2,$3,'Support seat',$4,$5,'vacant')")
        .bind(id(100)).bind(id(1)).bind(id(2)).bind(id(7)).bind(position_id).execute(&db).await.unwrap();
    let mut input = position_input(Some(id(8)));
    input["revision"] = json!(1);
    let (status, _) = request(
        &app,
        Method::PATCH,
        &format!("{}/{position_id}", positions_path()),
        "manager",
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    input["department_id"] = json!(id(7));
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("{}/{position_id}", positions_path()),
        "manager",
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn team_assignments_respect_occupied_organization_seats(db: PgPool) {
    let app = fixture(&db).await;
    let original = create(&app, position_input(Some(id(7)))).await;
    let replacement = create(&app, position_input(Some(id(7)))).await;
    let position_id: Uuid = original["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("INSERT INTO company_positions (id,tenant_id,project_id,title,department_id,job_position_id,occupant_kind,user_id,ai_profile_id) VALUES ($1,$2,$3,'Human seat',$4,$5,'human',$6,NULL), ($7,$2,$3,'Agent seat',$4,$5,'agent',NULL,$8)")
        .bind(id(100)).bind(id(1)).bind(id(2)).bind(id(7)).bind(position_id).bind(id(5)).bind(id(101)).bind(id(17)).execute(&db).await.unwrap();
    for (kind, occupant) in [("members", id(5)), ("agents", id(17))] {
        let path = format!("/api/v1/projects/{}/team/{kind}/{occupant}/position", id(2));
        for (position, expected) in [
            (replacement["id"].clone(), StatusCode::CONFLICT),
            (original["id"].clone(), StatusCode::NO_CONTENT),
            (Value::Null, StatusCode::NO_CONTENT),
        ] {
            let (status, body) = request(
                &app,
                Method::PUT,
                &path,
                "manager",
                Some(json!({"position_id":position})),
            )
            .await;
            assert_eq!(status, expected, "{body}");
        }
    }
}

#[sqlx::test]
#[ignore = "requires PostgreSQL with CREATEDB"]
async fn restoring_access_clears_stale_position_assignments(db: PgPool) {
    let app = fixture(&db).await;
    let position = create(&app, position_input(Some(id(7)))).await;
    let position_id: Uuid = position["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE memberships SET role='admin' WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE memberships SET position_id=$2 WHERE id=$1")
        .bind(id(6))
        .bind(position_id)
        .execute(&db)
        .await
        .unwrap();
    let members_path = format!("/api/v1/projects/{}/members", id(2));
    let grant = json!({"email":"employee@kpi.test","role_id":"operator","department_id":id(7)});
    let (status, body) = request(
        &app,
        Method::POST,
        &members_path,
        "manager",
        Some(grant.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unchanged: Option<Uuid> =
        sqlx::query_scalar("SELECT position_id FROM memberships WHERE id=$1")
            .bind(id(6))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(unchanged, Some(position_id));
    let (status, body) = request(
        &app,
        Method::DELETE,
        &format!("{members_path}/{}", id(6)),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let mut moved = position_input(Some(id(8)));
    moved["revision"] = json!(1);
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("{}/{position_id}", positions_path()),
        "manager",
        Some(moved),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = request(&app, Method::POST, &members_path, "manager", Some(grant)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let restored: (Option<Uuid>, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT position_id,department_id,revoked_at IS NULL FROM memberships WHERE id=$1",
    )
    .bind(id(6))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(restored, (None, Some(id(7)), true));
}
