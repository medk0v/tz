use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_settings, auth, company_structure};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

fn path(project: Uuid) -> String {
    format!("/api/v1/projects/{project}/company-structure")
}

fn draft(title: &str) -> Value {
    json!({
        "title":title, "department_id":null, "reports_to_position_id":null,
        "job_position_id":null,
        "is_department_head":false, "occupant_kind":"vacant", "occupant_name":null,
        "user_id":null, "ai_profile_id":null,
    })
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Company test'), ('{foreign_tenant}','Foreign');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES
            ('{company}','{tenant}','Company','company'), ('{other}','{tenant}','Other company','other'),
            ('{ordinary}','{tenant}','Ordinary','ordinary'), ('{foreign}','{foreign_tenant}','Foreign','foreign');
        INSERT INTO departments (id,tenant_id,project_id,name) VALUES
            ('{department}','{tenant}','{company}','Support'), ('{other_department}','{tenant}','{other}','Sales'),
            ('{foreign_department}','{foreign_tenant}','{foreign}','Foreign');
        INSERT INTO users (id,email,display_name) VALUES
            ('{admin}','admin@company.test','Admin'), ('{operator}','operator@company.test','Operator'),
            ('{project_admin}','project@company.test','Project admin'), ('{foreign_user}','foreign@company.test','Foreign user'),
            ('{outside}','outside@company.test','Other company user'), ('{disabled}','disabled@company.test','Disabled'),
            ('{revoked}','revoked@company.test','Revoked'), ('{member}','member@company.test','Member'),
            ('{director}','director@company.test','Director');
        UPDATE users SET status='disabled' WHERE id='{disabled}';
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role,permissions,department_id,director_access) VALUES
            ('{admin_membership}','{tenant}',NULL,'{admin}','admin',ARRAY['projects:read','projects:manage','ai:manage'],NULL,false),
            ('{operator_membership}','{tenant}','{company}','{operator}','operator',ARRAY['projects:read'],'{department}',false),
            ('{project_membership}','{tenant}','{company}','{project_admin}','admin',ARRAY['projects:read','projects:manage','ai:manage'],NULL,false),
            ('{foreign_membership}','{foreign_tenant}','{foreign}','{foreign_user}','admin',ARRAY['projects:read','projects:manage'],NULL,false),
            ('{outside_membership}','{tenant}','{other}','{outside}','operator',ARRAY['projects:read'],NULL,false),
            ('{disabled_membership}','{tenant}','{company}','{disabled}','operator',ARRAY['projects:read'],NULL,false),
            ('{revoked_membership}','{tenant}','{company}','{revoked}','operator',ARRAY['projects:read'],NULL,false),
            ('{member_membership}','{tenant}','{company}','{member}','operator',ARRAY['projects:read'],NULL,false),
            ('{director_membership}','{tenant}','{company}','{director}','manager',ARRAY['projects:read'],NULL,true);
        UPDATE memberships SET revoked_at=now() WHERE id='{revoked_membership}';
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,created_by,status) VALUES
            ('{agent}','{tenant}','{company}','Support agent','{admin}','draft'),
            ('{other_agent}','{tenant}','{other}','Other agent','{admin}','draft'),
            ('{disabled_agent}','{tenant}','{company}','Disabled agent','{admin}','disabled'),
            ('{foreign_agent}','{foreign_tenant}','{foreign}','Foreign agent','{foreign_user}','draft');
        INSERT INTO job_positions (id,tenant_id,project_id,department_id,name) VALUES
            ('{shared_job}','{tenant}','{company}',NULL,'Shared role'),
            ('{department_job}','{tenant}','{company}','{department}','Support role'),
            ('{other_job}','{tenant}','{other}',NULL,'Other role'),
            ('{second_job}','{tenant}','{company}',NULL,'Second shared role'),
            ('{foreign_job}','{foreign_tenant}','{foreign}',NULL,'Foreign role');
        "#,
        tenant=id(1), company=id(2), other=id(3), ordinary=id(4), foreign_tenant=id(90), foreign=id(91),
        department=id(20), other_department=id(21), foreign_department=id(92),
        admin=id(10), operator=id(11), project_admin=id(12), foreign_user=id(13), outside=id(14), disabled=id(15), revoked=id(16), member=id(17), director=id(18),
        admin_membership=id(100), operator_membership=id(101), project_membership=id(102), foreign_membership=id(103), outside_membership=id(104), disabled_membership=id(105), revoked_membership=id(106), member_membership=id(107), director_membership=id(108),
        agent=id(30), other_agent=id(31), disabled_agent=id(32), foreign_agent=id(93),
        shared_job=id(40), department_job=id(41), other_job=id(42), second_job=id(43), foreign_job=id(94),
    )).execute(db).await.unwrap();
    for project in [id(2), id(3), id(91)] {
        sqlx::query("UPDATE projects SET project_kind='company', company_profile=$2 WHERE id=$1")
            .bind(project)
            .bind(json!({"description":"","industry":"","country":"","website":"","employee_count":null,"products_services":"","goals":""}))
            .execute(db).await.unwrap();
    }
    sqlx::raw_sql(
        r#"INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,is_system,permissions)
           SELECT tenant_id,id,'admin','Admin','admin',true,ARRAY[
               'projects:read','projects:manage','conversations:read','conversations:reply','conversations:close',
               'contacts:read','contacts:manage','channels:read','channels:manage','access_tokens:manage',
               'visitor_network:read','quality:read','quality:read_all','reviews:read','routing:manage',
               'teams:manage','ai:manage','knowledge:manage', 'notes:read', 'notes:write','reply_templates:manage','integrations:manage',
               'roles:manage','system:read','processes:read','processes:edit','processes:approve','tasks:own','tasks:manage','tasks:configure'
           ] FROM projects;
           INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,is_system,permissions)
           SELECT project.tenant_id,project.id,role.id,role.name,role.id,false,ARRAY['projects:read']
           FROM projects AS project CROSS JOIN (VALUES ('operator','Operator'),('manager','Manager')) AS role(id,name);"#,
    ).execute(db).await.unwrap();
    for (number, token) in [
        (10, "admin"),
        (11, "operator"),
        (12, "project-admin"),
        (13, "foreign"),
        (18, "director"),
    ] {
        sqlx::query(
            r#"INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,now()+interval '1 hour',now()+interval '2 hours')"#,
        ).bind(id(200 + number)).bind(if number==13 {id(90)} else {id(1)})
            .bind(if number==13 {id(91)} else {id(2)}).bind(id(number)).bind(id(90 + number))
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(Sha256::digest(b"company-csrf").to_vec())
            .execute(db).await.unwrap();
    }
    sqlx::query(
        r#"INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,role,token_hash,permissions,expires_at)
           VALUES ($1,$2,$3,$4,'Admin token','admin',$5,ARRAY['projects:read','projects:manage'],now()+interval '1 hour')"#,
    ).bind(id(300)).bind(id(1)).bind(id(2)).bind(id(12)).bind(Sha256::digest(b"company-admin-token").to_vec())
        .execute(db).await.unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    company_structure::router()
        .merge(auth::router())
        .merge(ai_settings::router())
        .with_state(state)
}

async fn request(
    app: &Router,
    method: Method,
    url: &str,
    token: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(url)
        .header("content-type", "application/json");
    builder = if token == "company-admin-token" {
        builder.header("authorization", format!("Bearer {token}"))
    } else {
        builder
            .header(
                "cookie",
                format!("tzomet_session={token}; tzomet_csrf=company-csrf"),
            )
            .header("x-csrf-token", "company-csrf")
    };
    let response = app
        .clone()
        .oneshot(
            builder
                .body(input.map_or_else(Body::empty, |body| Body::from(body.to_string())))
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

async fn create(app: &Router, body: Value) -> Value {
    let (status, result) = request(app, Method::POST, &path(id(2)), "admin", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{result}");
    result
}

fn position_path(position: &Value) -> String {
    format!("{}/{}", path(id(2)), position["id"].as_str().unwrap())
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn positions_reuse_members_and_agents_without_changing_permissions(db: PgPool) {
    let app = fixture(&db).await;
    let original: Value = sqlx::query_scalar(
        "SELECT to_jsonb(membership) FROM memberships AS membership WHERE id=$1",
    )
    .bind(id(107))
    .fetch_one(&db)
    .await
    .unwrap();
    let mut human = draft("  Support manager  ");
    human["occupant_kind"] = json!("human");
    human["user_id"] = json!(id(17));
    human["department_id"] = json!(id(20));
    human["is_department_head"] = json!(true);
    let manager = create(&app, human).await;
    assert_eq!(manager["title"], "Support manager");
    assert_eq!(manager["occupant_name"], "Member");

    let mut manual = draft("External employee");
    manual["occupant_kind"] = json!("human");
    manual["occupant_name"] = json!("  Ann Smith  ");
    manual["reports_to_position_id"] = manager["id"].clone();
    assert_eq!(create(&app, manual).await["occupant_name"], "Ann Smith");
    let mut agent = draft("Support assistant");
    agent["occupant_kind"] = json!("agent");
    agent["ai_profile_id"] = json!(id(30));
    let assistant = create(&app, agent).await;
    assert_eq!(assistant["occupant_name"], "Support agent");
    create(&app, draft("Vacancy")).await;
    sqlx::query("UPDATE users SET display_name='Renamed member' WHERE id=$1")
        .bind(id(17))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE ai_profiles SET name='Renamed agent' WHERE id=$1")
        .bind(id(30))
        .execute(&db)
        .await
        .unwrap();
    let (status, list) = request(&app, Method::GET, &path(id(2)), "admin", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["can_manage"], true);
    let positions = list["positions"].as_array().unwrap();
    assert_eq!(positions.len(), 4);
    assert!(
        positions
            .iter()
            .any(|item| item["occupant_name"] == "Renamed member")
    );
    assert!(
        positions
            .iter()
            .any(|item| item["occupant_name"] == "Renamed agent")
    );
    assert_eq!(list["departments"], json!([{"id":id(20),"name":"Support"}]));
    let agents = list["agents"].as_array().unwrap();
    assert!(
        agents
            .iter()
            .any(|agent| agent == &json!({"id":id(30),"name":"Renamed agent"}))
    );
    // Authentication can provision built-in presets; those company-owned agents are valid options.
    for excluded in [31, 32, 93] {
        assert!(
            !agents
                .iter()
                .any(|agent| agent["id"] == json!(id(excluded)))
        );
    }
    let people = list["people"].as_array().unwrap();
    assert!(people.iter().any(|item| item["id"] == json!(id(10))));
    for excluded in [13, 14, 15, 16] {
        assert!(!people.iter().any(|item| item["id"] == json!(id(excluded))));
    }
    let current: Value = sqlx::query_scalar(
        "SELECT to_jsonb(membership) FROM memberships AS membership WHERE id=$1",
    )
    .bind(id(107))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(original, current);
    let saved_name: Option<String> =
        sqlx::query_scalar("SELECT occupant_name FROM company_positions WHERE ai_profile_id=$1")
            .bind(id(30))
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(
        saved_name.is_none(),
        "linked occupant names must not be cached"
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn reporting_cycles_heads_and_manager_deletion_are_rejected(db: PgPool) {
    let app = fixture(&db).await;
    let mut first = draft("Head");
    first["department_id"] = json!(id(20));
    first["is_department_head"] = json!(true);
    let head = create(&app, first.clone()).await;
    let (status, body) = request(
        &app,
        Method::POST,
        &path(id(2)),
        "admin",
        Some(first.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let mut child_draft = draft("Child");
    child_draft["reports_to_position_id"] = head["id"].clone();
    let child = create(&app, child_draft).await;
    let mut grandchild_draft = draft("Grandchild");
    grandchild_draft["reports_to_position_id"] = child["id"].clone();
    let grandchild = create(&app, grandchild_draft).await;
    for manager in [&head, &grandchild] {
        first["reports_to_position_id"] = manager["id"].clone();
        let (status, body) = request(
            &app,
            Method::PUT,
            &position_path(&head),
            "admin",
            Some(first.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (status, body) = request(&app, Method::DELETE, &position_path(&head), "admin", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    for item in [&grandchild, &child, &head] {
        let (status, body) =
            request(&app, Method::DELETE, &position_path(item), "admin", None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn foreign_and_inactive_references_are_rejected(db: PgPool) {
    let app = fixture(&db).await;
    let (status, foreign_position) = request(
        &app,
        Method::POST,
        &path(id(3)),
        "admin",
        Some(draft("Other manager")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{foreign_position}");
    let mut inputs = Vec::new();
    for job in [42, 94] {
        let mut input = draft("Invalid job position");
        input["job_position_id"] = json!(id(job));
        inputs.push(input);
    }
    for department in [21, 92] {
        let mut input = draft("Invalid department");
        input["department_id"] = json!(id(department));
        inputs.push(input);
    }
    for user in [13, 14, 15, 16] {
        let mut input = draft("Invalid member");
        input["occupant_kind"] = json!("human");
        input["user_id"] = json!(id(user));
        inputs.push(input);
    }
    for agent in [31, 32, 93] {
        let mut input = draft("Invalid agent");
        input["occupant_kind"] = json!("agent");
        input["ai_profile_id"] = json!(id(agent));
        inputs.push(input);
    }
    let mut input = draft("Invalid manager");
    input["reports_to_position_id"] = foreign_position["id"].clone();
    inputs.push(input);
    for input in inputs {
        let (status, body) = request(
            &app,
            Method::POST,
            &path(id(2)),
            "admin",
            Some(input.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{input}: {body}");
    }
    // Composite constraints also reject a foreign reference bypassing the HTTP validator.
    for (field, foreign_id) in [
        ("department_id", id(21)),
        (
            "reports_to_position_id",
            Uuid::parse_str(foreign_position["id"].as_str().unwrap()).unwrap(),
        ),
    ] {
        let error = sqlx::query(&format!("INSERT INTO company_positions (id,tenant_id,project_id,title,occupant_kind,{field}) VALUES ($1,$2,$3,'Invalid','vacant',$4)"))
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(foreign_id).execute(&db).await.unwrap_err();
        assert!(
            error
                .as_database_error()
                .unwrap()
                .is_foreign_key_violation()
        );
    }
    let error = sqlx::query("INSERT INTO company_positions (id,tenant_id,project_id,title,occupant_kind,ai_profile_id) VALUES ($1,$2,$3,'Invalid','agent',$4)")
        .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(31)).execute(&db).await.unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn job_templates_validate_departments_and_preserve_team_assignments(db: PgPool) {
    let app = fixture(&db).await;
    let mut input = draft("Support slot");
    input["job_position_id"] = json!(id(41));
    let (status, body) = request(
        &app,
        Method::POST,
        &path(id(2)),
        "admin",
        Some(input.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "department-specific templates require a matching department: {body}"
    );
    input["department_id"] = json!(id(20));
    let position = create(&app, input).await;
    assert_eq!(position["job_position_id"], json!(id(41)));
    assert_eq!(
        position["title"], "Support slot",
        "linking a template must preserve the slot title"
    );
    sqlx::query("UPDATE memberships SET position_id=$1 WHERE id=$2")
        .bind(id(40))
        .bind(id(107))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE ai_profiles SET position_id=$1 WHERE id=$2")
        .bind(id(40))
        .bind(id(30))
        .execute(&db)
        .await
        .unwrap();
    for (kind, field, occupant_id) in [
        ("human", "user_id", id(17)),
        ("agent", "ai_profile_id", id(30)),
    ] {
        let mut input = draft("Linked slot");
        input["occupant_kind"] = json!(kind);
        input[field] = json!(occupant_id);
        input["job_position_id"] = json!(id(43));
        let (status, body) = request(
            &app,
            Method::POST,
            &path(id(2)),
            "admin",
            Some(input.clone()),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "mismatched assignment: {body}"
        );
        input["job_position_id"] = json!(id(40));
        create(&app, input).await;
    }
    // A current project membership takes precedence over the tenant-wide membership.
    sqlx::query("INSERT INTO memberships (id,tenant_id,project_id,user_id,role,position_id) VALUES ($1,$2,$3,$4,'admin',$5)")
        .bind(id(109)).bind(id(1)).bind(id(2)).bind(id(10)).bind(id(40)).execute(&db).await.unwrap();
    let mut input = draft("Admin slot");
    input["occupant_kind"] = json!("human");
    input["user_id"] = json!(id(10));
    input["job_position_id"] = json!(id(43));
    let (status, body) = request(
        &app,
        Method::POST,
        &path(id(2)),
        "project-admin",
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    for query in [
        "SELECT position_id FROM memberships WHERE id=$1",
        "SELECT position_id FROM ai_profiles WHERE id=$1",
    ] {
        let source_id = if query.contains("memberships") {
            id(107)
        } else {
            id(30)
        };
        let current: Option<Uuid> = sqlx::query_scalar(query)
            .bind(source_id)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(current, Some(id(40)));
    }
    let (status, list) = request(&app, Method::GET, &path(id(2)), "project-admin", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let jobs = list["job_positions"].as_array().unwrap();
    assert_eq!(jobs.len(), 3);
    assert!(jobs.iter().all(|job| {
        [id(40).to_string(), id(41).to_string(), id(43).to_string()]
            .iter()
            .any(|id| job["id"] == *id)
    }));
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn only_password_administrators_can_access_company_structure(db: PgPool) {
    let app = fixture(&db).await;
    let position = create(&app, draft("Sensitive company position")).await;
    for token in ["operator", "director", "company-admin-token", "foreign"] {
        for method in [Method::GET, Method::POST, Method::PUT, Method::DELETE] {
            let item = method == Method::PUT || method == Method::DELETE;
            let input = (method == Method::POST || method == Method::PUT).then(|| draft("Denied"));
            let (status, body) = request(
                &app,
                method,
                &if item {
                    position_path(&position)
                } else {
                    path(id(2))
                },
                token,
                input,
            )
            .await;
            assert!(
                matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
                "{token}: {status} {body}"
            );
            assert!(!body.to_string().contains("Sensitive company position"));
        }
    }
    for project in [id(3), id(91)] {
        let (status, body) =
            request(&app, Method::GET, &path(project), "project-admin", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    let (status, body) = request(&app, Method::GET, &path(id(2)), "project-admin", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for method in [Method::GET, Method::POST, Method::PUT, Method::DELETE] {
        let item = method == Method::PUT || method == Method::DELETE;
        let input =
            (method == Method::POST || method == Method::PUT).then(|| draft("Ordinary project"));
        let url = if item {
            format!("{}/{}", path(id(4)), position["id"].as_str().unwrap())
        } else {
            path(id(4))
        };
        let (status, body) = request(&app, method, &url, "admin", input).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    sqlx::query("UPDATE memberships SET revoked_at=now() WHERE id=$1")
        .bind(id(102))
        .execute(&db)
        .await
        .unwrap();
    let (status, body) = request(&app, Method::GET, &path(id(2)), "project-admin", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn concurrent_edits_cannot_create_cycles_or_duplicate_heads(db: PgPool) {
    let app = fixture(&db).await;
    let first = create(&app, draft("First")).await;
    let second = create(&app, draft("Second")).await;
    let mut first_input = draft("First");
    first_input["reports_to_position_id"] = second["id"].clone();
    let mut second_input = draft("Second");
    second_input["reports_to_position_id"] = first["id"].clone();
    let first_path = position_path(&first);
    let second_path = position_path(&second);
    let (left, right) = tokio::join!(
        request(&app, Method::PUT, &first_path, "admin", Some(first_input)),
        request(&app, Method::PUT, &second_path, "admin", Some(second_input))
    );
    let statuses = [left.0, right.0];
    assert!(
        statuses.contains(&StatusCode::OK) && statuses.contains(&StatusCode::BAD_REQUEST),
        "{left:?} {right:?}"
    );
    let mut head = draft("Concurrent head");
    head["department_id"] = json!(id(20));
    head["is_department_head"] = json!(true);
    let url = path(id(2));
    let (left, right) = tokio::join!(
        request(&app, Method::POST, &url, "admin", Some(head.clone())),
        request(&app, Method::POST, &url, "admin", Some(head))
    );
    let statuses = [left.0, right.0];
    assert!(
        statuses.contains(&StatusCode::CREATED) && statuses.contains(&StatusCode::CONFLICT),
        "{left:?} {right:?}"
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn occupied_agents_must_be_unassigned_before_deletion(db: PgPool) {
    let app = fixture(&db).await;
    let mut input = draft("AI support");
    input["occupant_kind"] = json!("agent");
    input["ai_profile_id"] = json!(id(30));
    let position = create(&app, input).await;
    let url = format!("/api/v1/ai/profiles/{}", id(30));
    let (status, body) = request(&app, Method::DELETE, &url, "admin", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, body) = request(
        &app,
        Method::PUT,
        &position_path(&position),
        "admin",
        Some(draft("AI support")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = request(&app, Method::DELETE, &url, "admin", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM company_positions WHERE tenant_id=$1 AND project_id=$2",
    )
    .bind(id(1))
    .bind(id(2))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(count, 1, "unassigning must preserve the vacant position");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn input_and_company_position_count_are_bounded(db: PgPool) {
    let app = fixture(&db).await;
    let directory_title = "Р".repeat(200);
    sqlx::query("UPDATE job_positions SET name=$1 WHERE id=$2")
        .bind(&directory_title)
        .bind(id(40))
        .execute(&db)
        .await
        .unwrap();
    let (status, list) = request(&app, Method::GET, &path(id(2)), "admin", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let directory = list["job_positions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["id"] == json!(id(40)))
        .unwrap();
    let mut autofilled = draft(directory["name"].as_str().unwrap());
    autofilled["job_position_id"] = directory["id"].clone();
    assert_eq!(create(&app, autofilled).await["title"], directory_title);
    let mut manual = draft("Manual employee");
    manual["occupant_kind"] = json!("human");
    manual["occupant_name"] = json!("A".repeat(160));
    assert_eq!(
        create(&app, manual.clone()).await["occupant_name"],
        "A".repeat(160)
    );
    manual["occupant_name"] = json!("A".repeat(161));
    let mut inputs = vec![
        draft(" "),
        draft(&"x".repeat(201)),
        draft("line\nbreak"),
        manual,
    ];
    for (key, value) in [
        ("is_department_head", json!(true)),
        ("user_id", json!(id(17))),
        ("ai_profile_id", json!(id(30))),
        ("occupant_kind", json!("human")),
        ("occupant_kind", json!("agent")),
        ("occupant_name", json!("Unexpected name")),
    ] {
        let mut input = draft("Invalid");
        input[key] = value;
        inputs.push(input);
    }
    let mut conflicting = draft("Conflicting member");
    conflicting["occupant_kind"] = json!("human");
    conflicting["user_id"] = json!(id(17));
    conflicting["occupant_name"] = json!("Manual name");
    inputs.push(conflicting);
    for input in inputs {
        let (status, body) = request(
            &app,
            Method::POST,
            &path(id(2)),
            "admin",
            Some(input.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{input}: {body}");
    }
    sqlx::query("INSERT INTO company_positions (id,tenant_id,project_id,title,occupant_kind) SELECT gen_random_uuid(),$1,$2,'Vacancy '||n,'vacant' FROM generate_series(1,498) AS n")
        .bind(id(1)).bind(id(2)).execute(&db).await.unwrap();
    let (status, body) = request(
        &app,
        Method::POST,
        &path(id(2)),
        "admin",
        Some(draft("Too many")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = request(&app, Method::GET, &path(id(2)), "admin", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["positions"].as_array().unwrap().len(), 500);
}
