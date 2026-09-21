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
    Uuid::from_u128(860_000 + n)
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
                    "tzomet_session=visibility-session; tzomet_csrf=visibility-csrf",
                )
                .header("x-csrf-token", "visibility-csrf")
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
          SELECT '{tenant}',id,'manager','Manager','manager',ARRAY['projects:read','channels:read','channels:manage','ai:manage','knowledge:manage','reply_templates:manage','tasks:own','tasks:manage']
          FROM projects WHERE tenant_id='{tenant}';
        INSERT INTO memberships (id,tenant_id,user_id,role,permissions,director_access)
          VALUES ('{member}','{tenant}','{user}','manager',ARRAY['projects:read','channels:read','channels:manage','ai:manage','knowledge:manage','reply_templates:manage','tasks:own','tasks:manage'],true);
        INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at)
          VALUES ('{session}','{tenant}','{a}','{user}','{member}',decode('{token:x}','hex'),decode('{csrf:x}','hex'),now()+interval '1 hour',now()+interval '2 hours');
    "#, tenant=id(1), foreign=id(2), user=id(3), a=id(4), b=id(5), c=id(6), support=id(7), sales=id(8), ops=id(9), foreign_dept=id(10), inbox_a=id(11), inbox_sales=id(12), inbox_b=id(13), member=id(14), session=id(15), token=Sha256::digest(b"visibility-session"), csrf=Sha256::digest(b"visibility-csrf")))
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

fn provider(name: &str) -> Value {
    json!({"name":name,"provider_kind":"openai_compatible","base_url":"http://127.0.0.1:9/v1","default_model":"test-model","status":"active"})
}

async fn create(app: &Router, path: &str, input: &Value) -> Value {
    let (status, body) = request(app, Method::POST, path, Some(input.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "POST {path}: {body}");
    body
}

async fn become_administrator(db: &PgPool) {
    sqlx::query("INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,is_system,permissions) SELECT tenant_id,project_id,'admin','Admin','admin',true,ARRAY['projects:read','projects:manage','conversations:read','conversations:reply','conversations:close','contacts:read','contacts:manage','channels:read','channels:manage','access_tokens:manage','visitor_network:read','quality:read','quality:read_all','reviews:read','routing:manage','teams:manage','ai:manage','knowledge:manage', 'notes:read', 'notes:write','reply_templates:manage','processes:read','processes:edit','processes:approve','tasks:own','tasks:manage','tasks:configure','integrations:manage','roles:manage','system:read'] FROM project_roles WHERE tenant_id=$1 AND id='manager'")
        .bind(id(1)).execute(db).await.unwrap();
    sqlx::query("UPDATE memberships SET role='admin',director_access=false WHERE id=$1")
        .bind(id(14))
        .execute(db)
        .await
        .unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn shared_resources_support_global_project_and_department_visibility(db: PgPool) {
    let app = fixture(&db).await;
    select(&app, id(4), None).await;
    let member = create(&app, "/api/v1/ai/profiles", &profile("Member")).await;
    let coordinator = create(&app, "/api/v1/ai/profiles", &profile("Coordinator")).await;
    let template_a = format!("/api/v1/inboxes/{}/reply-templates", id(11));
    let template_sales = format!("/api/v1/inboxes/{}/reply-templates", id(12));
    let template_b = format!("/api/v1/inboxes/{}/reply-templates", id(13));
    let inputs = vec![
        ("/api/v1/ai/profiles", profile("Shared profile")),
        (
            "/api/v1/ai/tasks",
            json!({"text":"Shared task","schedule":{"kind":"manual"},"execution_mode":"team","agent_ids":[member["id"]],"coordinator_id":coordinator["id"]}),
        ),
        (
            template_a.as_str(),
            json!({"title":"Shared reply","body":"**Hello**"}),
        ),
        (
            "/api/v1/custom-ai-channels",
            json!({"name":"Shared channel","inbox_id":id(11),"connection_type":"external_api","destination":"tasks","mode":"instructions","source_kind":"api"}),
        ),
    ];
    let mut resources = Vec::new();
    for (path, input) in inputs {
        let resource = create(&app, path, &input).await;
        assert_eq!(
            resource["visibility"],
            json!({"project_ids":[],"department_ids":[]})
        );
        resources.push((
            path.to_owned(),
            input,
            resource["id"].as_str().unwrap().to_owned(),
        ));
    }
    // Unbound resources appear in another department and another project.
    for (project, department, template_path) in
        [(id(4), id(8), &template_sales), (id(5), id(9), &template_b)]
    {
        select(&app, project, Some(department)).await;
        for (path, _, resource_id) in &resources {
            let list_path = if path == &template_a {
                template_path
            } else {
                path
            };
            let (status, body) = request(&app, Method::GET, list_path, None).await;
            assert_eq!(status, StatusCode::OK, "GET {list_path}: {body}");
            assert!(
                body["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["id"] == *resource_id),
                "{list_path}: {body}"
            );
        }
    }
    // Shared detail endpoints use their stable owning project.
    for (path, _, resource_id) in &resources {
        let suffix = if path == "/api/v1/ai/tasks" {
            Some("executions")
        } else {
            None
        };
        if let Some(suffix) = suffix {
            let (status, body) = request(
                &app,
                Method::GET,
                &format!("{path}/{resource_id}/{suffix}"),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }
    // Shared agents can be used by newly created resources in Project B.
    create(&app, "/api/v1/ai/tasks", &json!({"text":"Project B task","schedule":{"kind":"manual"},"execution_mode":"team","agent_ids":[member["id"]],"coordinator_id":coordinator["id"]})).await;
    create(&app, "/api/v1/custom-ai-channels", &json!({"name":"Project B channel","inbox_id":id(13),"connection_type":"ai_agent","destination":"tasks","mode":"agent","source_kind":"api","ai_profile_id":member["id"]})).await;
    // Editing from another project saves both selectors atomically.
    select(&app, id(5), None).await;
    for (path, input, resource_id) in &resources {
        let mut input = input.clone();
        input["visibility"] = json!({"project_ids":[id(4)],"department_ids":[id(8)]});
        let path = if path == &template_a {
            &template_b
        } else {
            path
        };
        let method = if path == "/api/v1/custom-ai-channels" {
            Method::PUT
        } else {
            Method::PATCH
        };
        let (status, body) = request(
            &app,
            method,
            &format!("{path}/{resource_id}"),
            Some(input.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "PATCH {path}: {body}");
        assert_eq!(body["visibility"], input["visibility"]);
    }
    for (project, department, template_path, visible) in [
        (id(5), Some(id(9)), &template_b, false),
        (id(4), Some(id(7)), &template_a, false),
        (id(4), Some(id(8)), &template_sales, true),
        (id(4), None, &template_sales, true),
    ] {
        select(&app, project, department).await;
        for (path, _, resource_id) in &resources {
            let list_path = if path == &template_a {
                template_path
            } else {
                path
            };
            let (status, body) = request(&app, Method::GET, list_path, None).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(
                body["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["id"] == *resource_id),
                visible,
                "{path}: {body}"
            );
        }
    }
    // Clearing scopes restores global visibility; old clients preserve scopes when omitted.
    select(&app, id(4), None).await;
    let (path, input, resource_id) = &resources[0];
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("{path}/{resource_id}"),
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["visibility"]["department_ids"], json!([id(8)]));
    let mut clear = input.clone();
    clear["visibility"] = json!({"project_ids":[],"department_ids":[]});
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("{path}/{resource_id}"),
        Some(clear),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    select(&app, id(5), Some(id(9))).await;
    let (status, body) = request(&app, Method::GET, path, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == *resource_id)
    );
    // Reject foreign-tenant and mismatched department assignments with no partial update.
    select(&app, id(5), None).await;
    for visibility in [
        json!({"project_ids":[id(6)],"department_ids":[]}),
        json!({"project_ids":[id(4)],"department_ids":[id(9)]}),
        json!({"project_ids":[],"department_ids":[id(10)]}),
    ] {
        let mut bad = input.clone();
        bad["name"] = json!("Must roll back");
        bad["visibility"] = visibility;
        let (status, body) = request(
            &app,
            Method::PATCH,
            &format!("{path}/{resource_id}"),
            Some(bad),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let name: String = sqlx::query_scalar("SELECT name FROM ai_profiles WHERE id=$1")
        .bind(Uuid::parse_str(resource_id).unwrap())
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(name, "Shared profile");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn ordinary_users_are_bound_automatically_and_only_admin_or_director_can_choose(db: PgPool) {
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
    let (status, options) = request(
        &app,
        Method::GET,
        "/api/v1/resource-visibility-options",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        options,
        json!({"can_choose":false,"projects":[],"departments":[]})
    );
    let member = create(&app, "/api/v1/ai/profiles", &profile("Member")).await;
    let coordinator = create(&app, "/api/v1/ai/profiles", &profile("Coordinator")).await;
    let templates = format!("/api/v1/inboxes/{}/reply-templates", id(11));
    let inputs = vec![
        (
            "/api/v1/knowledge-bases",
            json!({"name":"Automatic knowledge","status":"active"}),
        ),
        ("/api/v1/ai/profiles", profile("Automatic profile")),
        (
            "/api/v1/ai/tasks",
            json!({"text":"Automatic task","schedule":{"kind":"manual"},"execution_mode":"team","agent_ids":[member["id"]],"coordinator_id":coordinator["id"]}),
        ),
        (
            templates.as_str(),
            json!({"title":"Automatic reply","body":"Hello"}),
        ),
        (
            "/api/v1/custom-ai-channels",
            json!({"name":"Automatic channel","inbox_id":id(11),"connection_type":"external_api","destination":"tasks","mode":"instructions","source_kind":"api"}),
        ),
    ];
    let expected = json!({"project_ids":[id(4)],"department_ids":[id(7)]});
    for (path, mut input) in inputs {
        // A forged request cannot turn an ordinary user's creation into a global resource.
        input["visibility"] = json!({"project_ids":[],"department_ids":[]});
        let resource = create(&app, path, &input).await;
        assert_eq!(resource["visibility"], expected, "{path}: {resource}");
        let method = if path == "/api/v1/custom-ai-channels" {
            Method::PUT
        } else {
            Method::PATCH
        };
        let detail = format!("{path}/{}", resource["id"].as_str().unwrap());
        let (status, body) = request(&app, method.clone(), &detail, Some(input.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        input.as_object_mut().unwrap().remove("visibility");
        let (status, body) = request(&app, method, &detail, Some(input)).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["visibility"], expected);
    }
    let forged_knowledge = create(
        &app,
        "/api/v1/knowledge-bases",
        &json!({"name":"Forged knowledge scope","status":"active","visibility":{"project_ids":[id(5)],"department_ids":[id(9)]}}),
    )
    .await;
    assert_eq!(forged_knowledge["visibility"], expected);
    // An assigned department user can also maintain agents already used by a task.
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/ai/profiles/{}", member["id"].as_str().unwrap()),
        Some(profile("Updated member")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sqlx::query("UPDATE memberships SET project_id=NULL,department_id=NULL,director_access=true WHERE id=$1")
        .bind(id(14)).execute(&db).await.unwrap();
    select(&app, id(4), None).await;
    let (_, options) = request(
        &app,
        Method::GET,
        "/api/v1/resource-visibility-options",
        None,
    )
    .await;
    assert_eq!(options["can_choose"], true);
    let project_knowledge = create(
        &app,
        "/api/v1/knowledge-bases",
        &json!({"name":"Director knowledge","status":"active"}),
    )
    .await;
    assert_eq!(
        project_knowledge["visibility"],
        json!({"project_ids":[id(4)],"department_ids":[]})
    );
    select(&app, id(4), Some(id(7))).await;
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &format!(
            "/api/v1/knowledge-bases/{}",
            project_knowledge["id"].as_str().unwrap()
        ),
        Some(json!({"name":"Shared content edit","status":"active"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["visibility"], project_knowledge["visibility"]);
    become_administrator(&db).await;
    let (status, options) = request(
        &app,
        Method::GET,
        "/api/v1/resource-visibility-options",
        None,
    )
    .await;
    assert_eq!(
        options["can_choose"], true,
        "An administrator can choose from a department workspace too"
    );
    assert_eq!(status, StatusCode::OK, "{options}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn knowledge_bases_reject_cross_project_access_even_with_legacy_visibility(db: PgPool) {
    let app = fixture(&db).await;
    for administrator in [false, true] {
        if administrator {
            become_administrator(&db).await;
        }
        for (index, visibility) in [
            json!({"project_ids":[],"department_ids":[]}),
            json!({"project_ids":[id(5)],"department_ids":[id(9)]}),
        ]
        .into_iter()
        .enumerate()
        {
            select(&app, id(4), None).await;
            let input = json!({"name":format!("Owner knowledge {administrator} {index}"),"status":"active"});
            let base = create(&app, "/api/v1/knowledge-bases", &input).await;
            let base_id = Uuid::parse_str(base["id"].as_str().unwrap()).unwrap();
            let base_path = format!("/api/v1/knowledge-bases/{base_id}");
            let articles_path = format!("{base_path}/articles");
            let article_input = json!({"title":"Private payment instructions","body":"Project A only","status":"published"});
            let article = create(&app, &articles_path, &article_input).await;
            let article_path = format!("{articles_path}/{}", article["id"].as_str().unwrap());
            // Old global bindings and forged legacy scopes never override the base's owner.
            sqlx::query("UPDATE knowledge_bases SET visibility=$2 WHERE id=$1")
                .bind(base_id)
                .bind(&visibility)
                .execute(&db)
                .await
                .unwrap();
            if index == 0 {
                let (status, body) = request(&app, Method::GET, &articles_path, None).await;
                assert_eq!(status, StatusCode::OK, "{body}");
                assert_eq!(body["items"][0]["body"], article_input["body"]);
            }

            select(&app, id(5), Some(id(9))).await;
            let (status, listed) =
                request(&app, Method::GET, "/api/v1/knowledge-bases", None).await;
            assert_eq!(status, StatusCode::OK, "{listed}");
            assert!(
                !listed["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["id"] == base["id"]),
                "administrator={administrator}, visibility={visibility}: {listed}"
            );
            for (method, path, payload) in [
                (Method::GET, &articles_path, None),
                (Method::POST, &articles_path, Some(article_input.clone())),
                (Method::PATCH, &article_path, Some(article_input.clone())),
                (Method::DELETE, &article_path, None),
                (Method::PATCH, &base_path, Some(input.clone())),
                (Method::DELETE, &base_path, None),
            ] {
                let (status, body) = request(&app, method.clone(), path, payload).await;
                assert_eq!(
                    status,
                    StatusCode::NOT_FOUND,
                    "administrator={administrator}, {method} {path}: {body}"
                );
            }
            let unchanged: (String, String, i64) = sqlx::query_as(
                "SELECT base.name,article.body,article.version FROM knowledge_bases base JOIN knowledge_articles article ON article.knowledge_base_id=base.id WHERE base.id=$1",
            )
            .bind(base_id)
            .fetch_one(&db)
            .await
            .unwrap();
            assert_eq!(unchanged.0, input["name"].as_str().unwrap());
            assert_eq!(unchanged.1, "Project A only");
            assert_eq!(unchanged.2, article["version"].as_i64().unwrap());
        }
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn knowledge_base_visibility_is_normalized_to_its_owner_and_keeps_department_scopes(
    db: PgPool,
) {
    let app = fixture(&db).await;
    become_administrator(&db).await;
    select(&app, id(4), None).await;
    for (index, visibility) in [
        None,
        Some(json!({"project_ids":[],"department_ids":[]})),
        Some(json!({"project_ids":[],"department_ids":[id(8)]})),
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = json!({"name":format!("Project knowledge {index}"),"status":"active"});
        if let Some(visibility) = visibility {
            input["visibility"] = visibility;
        }
        let base = create(&app, "/api/v1/knowledge-bases", &input).await;
        let expected = json!({"project_ids":[id(4)],"department_ids":if index == 2 {vec![id(8)]} else {vec![]}});
        assert_eq!(base["visibility"], expected);
        let base_path = format!("/api/v1/knowledge-bases/{}", base["id"].as_str().unwrap());
        input.as_object_mut().unwrap().remove("visibility");
        input["name"] = json!(format!("Updated project knowledge {index}"));
        let (status, updated) = request(&app, Method::PATCH, &base_path, Some(input.clone())).await;
        assert_eq!(status, StatusCode::OK, "{updated}");
        assert_eq!(updated["visibility"], expected);

        for visibility in [
            json!({"project_ids":[id(5)],"department_ids":[]}),
            json!({"project_ids":[id(4),id(5)],"department_ids":[]}),
            json!({"project_ids":[],"department_ids":[id(9)]}),
            json!({"project_ids":[id(6)],"department_ids":[]}),
            json!({"project_ids":[],"department_ids":[id(10)]}),
        ] {
            let mut invalid = input.clone();
            invalid["name"] = json!("Must roll back");
            invalid["visibility"] = visibility;
            for (method, path) in [
                (Method::POST, "/api/v1/knowledge-bases"),
                (Method::PATCH, base_path.as_str()),
            ] {
                let (status, body) =
                    request(&app, method.clone(), path, Some(invalid.clone())).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {path}: {body}");
            }
        }
        let stored: (String, Value) =
            sqlx::query_as("SELECT name,visibility FROM knowledge_bases WHERE id=$1")
                .bind(Uuid::parse_str(base["id"].as_str().unwrap()).unwrap())
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(stored.0, input["name"].as_str().unwrap());
        assert_eq!(stored.1, expected);
        if index == 2 {
            for (department, visible) in [(Some(id(7)), false), (Some(id(8)), true), (None, true)] {
                select(&app, id(4), department).await;
                let (status, listed) =
                    request(&app, Method::GET, "/api/v1/knowledge-bases", None).await;
                assert_eq!(status, StatusCode::OK, "{listed}");
                assert_eq!(
                    listed["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["id"] == base["id"]),
                    visible,
                    "{listed}"
                );
            }
        }
        input["visibility"] = json!({"project_ids":[],"department_ids":[]});
        let (status, cleared) = request(&app, Method::PATCH, &base_path, Some(input)).await;
        assert_eq!(status, StatusCode::OK, "{cleared}");
        assert_eq!(
            cleared["visibility"],
            json!({"project_ids":[id(4)],"department_ids":[]})
        );
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn knowledge_from_another_project_cannot_be_assigned_even_with_a_legacy_assignment(
    db: PgPool,
) {
    let app = fixture(&db).await;
    select(&app, id(4), None).await;
    let base = create(
        &app,
        "/api/v1/knowledge-bases",
        &json!({"name":"Project A knowledge","status":"active"}),
    )
    .await;
    let base_id = Uuid::parse_str(base["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE knowledge_bases SET visibility=$2 WHERE id=$1")
        .bind(base_id)
        .bind(json!({"project_ids":[],"department_ids":[]}))
        .execute(&db)
        .await
        .unwrap();
    select(&app, id(5), None).await;
    let agent = create(&app, "/api/v1/ai/profiles", &profile("Project B agent")).await;
    let agent_id = Uuid::parse_str(agent["id"].as_str().unwrap()).unwrap();
    let agent_path = format!("/api/v1/ai/profiles/{agent_id}");
    let mut input = profile("Cross-project knowledge");
    input["knowledge_base_ids"] = json!([base_id]);
    for (method, path) in [
        (Method::POST, "/api/v1/ai/profiles"),
        (Method::PATCH, agent_path.as_str()),
    ] {
        let (status, body) = request(&app, method.clone(), path, Some(input.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {path}: {body}");
    }
    sqlx::query("INSERT INTO ai_profile_knowledge_bases (tenant_id,ai_profile_id,knowledge_base_id) VALUES ($1,$2,$3)")
        .bind(id(1)).bind(agent_id).bind(base_id).execute(&db).await.unwrap();
    for project in [id(5), id(4)] {
        select(&app, project, None).await;
        let (status, body) = request(&app, Method::PATCH, &agent_path, Some(input.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "project={project}: {body}");
    }
    let name: String = sqlx::query_scalar("SELECT name FROM ai_profiles WHERE id=$1")
        .bind(agent_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(name, "Project B agent");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn shared_profile_backups_remain_accessible_only_in_the_owning_project(db: PgPool) {
    let app = fixture(&db).await;
    select(&app, id(4), None).await;
    let base = create(
        &app,
        "/api/v1/knowledge-bases",
        &json!({"name":"Private backup knowledge","status":"active"}),
    )
    .await;
    create(
        &app,
        &format!(
            "/api/v1/knowledge-bases/{}/articles",
            base["id"].as_str().unwrap()
        ),
        &json!({"title":"Private instructions","body":"Project A only","status":"published"}),
    )
    .await;
    let mut input = profile("Shared profile with private backups");
    input["knowledge_base_ids"] = json!([base["id"]]);
    let agent = create(&app, "/api/v1/ai/profiles", &input).await;
    let backups_path = format!(
        "/api/v1/ai/profiles/{}/backups",
        agent["id"].as_str().unwrap()
    );
    create(&app, &backups_path, &json!({})).await;
    let (status, backups) = request(&app, Method::GET, &backups_path, None).await;
    assert_eq!(status, StatusCode::OK, "{backups}");
    assert_eq!(backups["items"][0]["knowledge_base_count"], 1);
    assert_eq!(backups["items"][0]["article_count"], 1);
    let backup_path = format!(
        "{backups_path}/{}",
        backups["items"][0]["id"].as_str().unwrap()
    );
    let restore_path = format!("{backup_path}/restore");

    select(&app, id(5), None).await;
    for (method, path, input) in [
        (Method::GET, &backups_path, None),
        (Method::POST, &backups_path, Some(json!({}))),
        (
            Method::POST,
            &restore_path,
            Some(json!({"create_backup":false})),
        ),
        (Method::DELETE, &backup_path, None),
    ] {
        let (status, body) = request(&app, method.clone(), path, input).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}: {body}");
    }
    select(&app, id(4), None).await;
    let (status, unchanged) = request(&app, Method::GET, &backups_path, None).await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged, backups);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn providers_are_shared_and_scopes_gate_new_selection_without_breaking_assigned_profiles(
    db: PgPool,
) {
    let app = fixture(&db).await;
    select(&app, id(4), None).await;
    let input = provider("Shared model");
    let model = create(&app, "/api/v1/ai/providers", &input).await;
    let global = json!({"project_ids":[],"department_ids":[]});
    assert_eq!(model["visibility"], global);
    let detail = format!("/api/v1/ai/providers/{}", model["id"].as_str().unwrap());

    select(&app, id(5), Some(id(9))).await;
    let (status, listed) = request(&app, Method::GET, "/api/v1/ai/providers", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == model["id"])
    );
    let mut agent_input = profile("Project B agent");
    agent_input["provider_connection_id"] = model["id"].clone();
    agent_input["status"] = json!("active");
    agent_input["instructions"] = json!("Answer the assigned task.");
    let agent = create(&app, "/api/v1/ai/profiles", &agent_input).await;
    assert_eq!(agent["execution_ready"], true, "{agent}");
    let owner: Uuid = sqlx::query_scalar("SELECT project_id FROM ai_profiles WHERE id=$1")
        .bind(Uuid::parse_str(agent["id"].as_str().unwrap()).unwrap())
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(owner, id(5));

    // Provider edits from another project retain the provider's original owner.
    select(&app, id(5), None).await;
    let mut scoped = input.clone();
    scoped["visibility"] = json!({"project_ids":[id(4)],"department_ids":[id(8)]});
    let (status, saved) = request(&app, Method::PATCH, &detail, Some(scoped.clone())).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["visibility"], scoped["visibility"]);
    let owner: Uuid =
        sqlx::query_scalar("SELECT project_id FROM ai_provider_connections WHERE id=$1")
            .bind(Uuid::parse_str(model["id"].as_str().unwrap()).unwrap())
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(owner, id(4));

    for (project, department, visible) in [
        (id(5), None, false),
        (id(4), Some(id(7)), false),
        (id(4), Some(id(8)), true),
        (id(4), None, true),
    ] {
        select(&app, project, department).await;
        let (status, listed) = request(&app, Method::GET, "/api/v1/ai/providers", None).await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        assert_eq!(
            listed["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["id"] == model["id"]),
            visible,
            "{listed}"
        );
    }

    // Clients that omit visibility preserve the existing binding.
    let (status, saved) = request(&app, Method::PATCH, &detail, Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["visibility"], scoped["visibility"]);

    select(&app, id(5), None).await;
    for method in [Method::PATCH, Method::DELETE] {
        let (status, body) = request(&app, method, &detail, Some(input.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/ai/profiles",
        Some(agent_input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let unassigned = create(&app, "/api/v1/ai/profiles", &profile("Unassigned agent")).await;
    let (status, body) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/ai/profiles/{}", unassigned["id"].as_str().unwrap()),
        Some(agent_input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // An existing assignment remains editable and executable after the model is hidden.
    agent_input["instructions"] = json!("Updated instructions for the assigned task.");
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/ai/profiles/{}", agent["id"].as_str().unwrap()),
        Some(agent_input),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["provider_connection_id"], model["id"]);
    assert_eq!(saved["execution_ready"], true, "{saved}");

    select(&app, id(4), None).await;
    scoped["visibility"] = global.clone();
    let (status, saved) = request(&app, Method::PATCH, &detail, Some(scoped)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["visibility"], global);
    select(&app, id(5), Some(id(9))).await;
    let (status, listed) = request(&app, Method::GET, "/api/v1/ai/providers", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == model["id"])
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn provider_visibility_and_selection_cannot_cross_tenants(db: PgPool) {
    let app = fixture(&db).await;
    select(&app, id(4), None).await;
    let input = provider("Tenant model");
    let model = create(&app, "/api/v1/ai/providers", &input).await;
    let detail = format!("/api/v1/ai/providers/{}", model["id"].as_str().unwrap());
    for visibility in [
        json!({"project_ids":[id(6)],"department_ids":[]}),
        json!({"project_ids":[],"department_ids":[id(10)]}),
        json!({"project_ids":[id(4)],"department_ids":[id(9)]}),
    ] {
        let mut invalid = input.clone();
        invalid["name"] = json!("Must roll back");
        invalid["visibility"] = visibility;
        let (status, body) = request(&app, Method::PATCH, &detail, Some(invalid)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (status, listed) = request(&app, Method::GET, "/api/v1/ai/providers", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let unchanged = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == model["id"])
        .unwrap();
    assert_eq!(unchanged["name"], input["name"]);
    assert_eq!(unchanged["visibility"], model["visibility"]);

    sqlx::query("INSERT INTO ai_provider_connections (id,tenant_id,project_id,name,provider_kind,base_url,default_model,status,created_by) VALUES ($1,$2,$3,'Foreign model','openai_compatible','http://127.0.0.1:9/v1','test-model','active',$4)")
        .bind(id(20)).bind(id(2)).bind(id(6)).bind(id(3)).execute(&db).await.unwrap();
    let (status, listed) = request(&app, Method::GET, "/api/v1/ai/providers", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(
        !listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == json!(id(20)))
    );
    for method in [Method::PATCH, Method::DELETE] {
        let (status, body) = request(
            &app,
            method,
            &format!("/api/v1/ai/providers/{}", id(20)),
            Some(input.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    let mut invalid_agent = profile("Foreign model agent");
    invalid_agent["provider_connection_id"] = json!(id(20));
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/ai/profiles",
        Some(invalid_agent),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn provider_visibility_cannot_be_widened_by_ordinary_managers(db: PgPool) {
    let app = fixture(&db).await;
    sqlx::query("UPDATE memberships SET project_id=$2,director_access=false WHERE id=$1")
        .bind(id(14))
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    // The session already owns Project A; ordinary users enter its default department.
    let mut input = provider("Manager model");
    input["visibility"] = json!({"project_ids":[],"department_ids":[]});
    let model = create(&app, "/api/v1/ai/providers", &input).await;
    let automatic = json!({"project_ids":[id(4)],"department_ids":[id(7)]});
    assert_eq!(model["visibility"], automatic);
    let detail = format!("/api/v1/ai/providers/{}", model["id"].as_str().unwrap());
    let (status, body) = request(&app, Method::PATCH, &detail, Some(input.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    input.as_object_mut().unwrap().remove("visibility");
    input["name"] = json!("Updated manager model");
    let (status, saved) = request(&app, Method::PATCH, &detail, Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["visibility"], automatic);

    // Department-only management retains the existing provider-write restriction.
    sqlx::query("UPDATE memberships SET department_id=$2 WHERE id=$1")
        .bind(id(14))
        .bind(id(7))
        .execute(&db)
        .await
        .unwrap();
    input["visibility"] = json!({"project_ids":[],"department_ids":[]});
    for (method, path) in [
        (Method::POST, "/api/v1/ai/providers"),
        (Method::PATCH, detail.as_str()),
        (Method::DELETE, detail.as_str()),
    ] {
        let (status, body) = request(&app, method, path, Some(input.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    let (status, listed) = request(&app, Method::GET, "/api/v1/ai/providers", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["visibility"], model["visibility"]);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn global_providers_do_not_expand_access_tokens_beyond_their_project(db: PgPool) {
    let app = fixture(&db).await;
    select(&app, id(5), None).await;
    let input = provider("Project B global model");
    let model = create(&app, "/api/v1/ai/providers", &input).await;
    sqlx::query("INSERT INTO memberships (id,tenant_id,project_id,user_id,role) VALUES ($1,$2,$3,$4,'manager')")
        .bind(id(22)).bind(id(1)).bind(id(4)).bind(id(3))
        .execute(&db).await.unwrap();
    sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,role,expires_at) VALUES ($1,$2,$3,$4,'Project A token',$5,ARRAY['ai:manage'],'manager',now()+interval '1 hour')")
        .bind(id(21)).bind(id(1)).bind(id(4)).bind(id(3))
        .bind(Sha256::digest(b"visibility-project-token").to_vec())
        .execute(&db).await.unwrap();
    let mut agent = profile("Token agent");
    agent["provider_connection_id"] = model["id"].clone();
    let detail = format!("/api/v1/ai/providers/{}", model["id"].as_str().unwrap());
    for (method, path, input, expected) in [
        (Method::GET, "/api/v1/ai/providers", None, StatusCode::OK),
        (
            Method::PATCH,
            detail.as_str(),
            Some(input.clone()),
            StatusCode::NOT_FOUND,
        ),
        (Method::DELETE, detail.as_str(), None, StatusCode::NOT_FOUND),
        (
            Method::POST,
            "/api/v1/ai/profiles",
            Some(agent),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer visibility-project-token")
                    .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(status, expected, "{method} {path}: {body}");
        if method == Method::GET {
            assert!(
                !body["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["id"] == model["id"])
            );
        }
    }
}
