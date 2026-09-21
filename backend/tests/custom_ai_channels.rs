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

const PATH: &str = "/api/v1/custom-ai-channels";

fn id(value: u128) -> Uuid {
    Uuid::from_u128(90_000 + value)
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Custom channels');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES
          ('{project}','{tenant}','Project','custom'), ('{foreign_project}','{tenant}','Foreign','foreign');
        INSERT INTO users (id,email,display_name) VALUES
          ('{admin}','admin@custom.test','Admin'), ('{scoped}','scoped@custom.test','Scoped');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions,is_system) VALUES
          ('{tenant}','{project}','admin','Admin','admin',ARRAY[
            'projects:read','projects:manage','conversations:read','conversations:reply','conversations:close',
            'contacts:read','contacts:manage','channels:read','channels:manage','access_tokens:manage',
            'visitor_network:read','quality:read','quality:read_all','reviews:read','routing:manage','teams:manage',
            'ai:manage','knowledge:manage', 'notes:read', 'notes:write','reply_templates:manage','processes:read','processes:edit','processes:approve','tasks:own','tasks:manage','tasks:configure','integrations:manage','roles:manage','system:read'],true),
          ('{tenant}','{project}','manager','Manager','manager',ARRAY['channels:read','channels:manage'],true);
        INSERT INTO departments (id,tenant_id,project_id,name) VALUES
          ('{department}','{tenant}','{project}','Support'), ('{other_department}','{tenant}','{project}','HR');
        INSERT INTO inboxes (id,tenant_id,project_id,department_id,name) VALUES
          ('{inbox}','{tenant}','{project}','{department}','Support'),
          ('{other_inbox}','{tenant}','{project}','{other_department}','HR'),
          ('{foreign_inbox}','{tenant}','{foreign_project}',NULL,'Foreign');
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role,department_id) VALUES
          ('{admin}','{tenant}','{project}','{admin}','admin',NULL),
          ('{scoped}','{tenant}','{project}','{scoped}','manager','{department}');
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,instructions,status,created_by) VALUES
          ('{profile}','{tenant}','{project}','Reader','Read messages','draft','{admin}'),
          ('{foreign_profile}','{tenant}','{foreign_project}','Foreign reader','','draft','{admin}');
        UPDATE ai_profiles SET visibility=jsonb_build_object('project_ids',jsonb_build_array(project_id),'department_ids','[]'::jsonb);
    "#, tenant=id(1),project=id(2),foreign_project=id(3),admin=id(4),scoped=id(5),
        department=id(6),other_department=id(7),inbox=id(8),other_inbox=id(9),foreign_inbox=id(10),profile=id(11),foreign_profile=id(12)))
        .execute(db).await.unwrap();
    for (token, user) in [("custom-admin", id(4)), ("custom-scoped", id(5))] {
        sqlx::query("INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at) VALUES ($1,$2,$3,$4,$4,$5,$6,now()+interval '1 hour',now()+interval '2 hours')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(user)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(Sha256::digest(b"custom-csrf").to_vec())
            .execute(db).await.unwrap();
    }
    sqlx::query("UPDATE operator_sessions SET director_mode=true WHERE user_id=$1")
        .bind(id(4))
        .execute(db)
        .await
        .unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    tz_backend::app::router(state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    scoped: bool,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let token = if scoped {
        "custom-scoped"
    } else {
        "custom-admin"
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
                    format!("tzomet_session={token}; tzomet_csrf=custom-csrf"),
                )
                .header("x-csrf-token", "custom-csrf")
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

fn draft(inbox: Uuid) -> Value {
    json!({"name":"Recruiting messages","inbox_id":inbox,"mode":"instructions",
        "source_url":"https://www.linkedin.com/messaging/","source_kind":"website",
        "instructions":"Read new messages and preserve the original thread and message IDs.","ai_profile_id":null})
}

async fn create(app: &Router, input: Value) -> Value {
    let (status, body) = request(app, Method::POST, PATH, false, Some(input)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn drafts_persist_modes_without_activating_a_transport(db: PgPool) {
    let app = fixture(&db).await;
    let created = create(&app, draft(id(8))).await;
    assert_eq!(created["status"], "draft");
    assert_eq!(created["icon"], "bot");
    assert_eq!(created["connection_type"], "ai_agent");
    assert_eq!(created["destination"], "conversations");
    assert_eq!(created["ai_profile_id"], Value::Null);
    let channel_id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let path = format!("{PATH}/{channel_id}");
    let mut input = draft(id(9));
    input["mode"] = json!("agent");
    input["ai_profile_id"] = json!(id(11));
    input["name"] = json!("HR reader");
    input["icon"] = json!("briefcase");
    let (status, updated) = request(&app, Method::PUT, &path, false, Some(input)).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["mode"], "agent");
    assert_eq!(updated["icon"], "briefcase");
    assert_eq!(updated["ai_profile_id"], id(11).to_string());
    assert_eq!(updated["inbox_id"], id(9).to_string());
    let (status, listed) = request(&app, Method::GET, PATH, false, None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["items"][0], updated);
    let stored: (String, String, Uuid) =
        sqlx::query_as("SELECT kind,status,inbox_id FROM channel_connections WHERE id=$1")
            .bind(channel_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(stored, ("custom_ai".into(), "draft".into(), id(9)));
    assert!(
        sqlx::query("UPDATE channel_connections SET status='active' WHERE id=$1")
            .bind(channel_id)
            .execute(&db)
            .await
            .is_err(),
        "draft must not be activated without a transport"
    );
    let (status, _) = request(&app, Method::DELETE, &path, false, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, listed) = request(&app, Method::GET, PATH, false, None).await;
    assert_eq!(listed["items"], json!([]));
    let stored: (String, bool) =
        sqlx::query_as("SELECT status,deleted_at IS NOT NULL FROM channel_connections WHERE id=$1")
            .bind(channel_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(stored, ("disabled".into(), true));
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn validation_and_department_scope_cannot_be_bypassed(db: PgPool) {
    let app = fixture(&db).await;
    let mut own_input = draft(id(8));
    own_input["visibility"] = json!({"project_ids":[id(2)],"department_ids":[id(6)]});
    let mut other_input = draft(id(9));
    other_input["visibility"] = json!({"project_ids":[id(2)],"department_ids":[id(7)]});
    let own = create(&app, own_input).await;
    let other = create(&app, other_input).await;
    let own_path = format!("{PATH}/{}", own["id"].as_str().unwrap());
    let other_path = format!("{PATH}/{}", other["id"].as_str().unwrap());
    let (status, listed) = request(&app, Method::GET, PATH, true, None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["id"], own["id"]);
    for (method, path, value) in [
        (Method::POST, PATH, Some(draft(id(9)))),
        (Method::PUT, own_path.as_str(), Some(draft(id(9)))),
        (Method::PUT, other_path.as_str(), Some(draft(id(8)))),
        (Method::DELETE, other_path.as_str(), None),
    ] {
        let (status, body) = request(&app, method, path, true, value).await;
        assert!(
            matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
            "{status}: {body}"
        );
    }
    for patch in [
        json!({"inbox_id":id(10)}),
        json!({"mode":"agent","ai_profile_id":id(12)}),
        json!({"mode":"agent","ai_profile_id":null}),
        json!({"mode":"instructions","instructions":" "}),
        json!({"ai_profile_id":id(11)}),
        json!({"source_url":"http://example.test/messages"}),
        json!({"source_url":"https://user:password@example.test/messages"}),
        json!({"status":"active"}),
        json!({"icon":"unrecognized-icon"}),
        json!({"connection_type":"unknown"}),
        json!({"destination":"unknown"}),
    ] {
        let mut input = draft(id(8));
        input
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        let (status, body) = request(&app, Method::PUT, &own_path, false, Some(input)).await;
        assert!(status.is_client_error(), "{patch}: {status}: {body}");
    }
    let (_, listed) = request(&app, Method::GET, PATH, false, None).await;
    let preserved = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == own["id"])
        .unwrap();
    assert_eq!(
        preserved, &own,
        "failed updates must preserve every draft field"
    );
    let mut guessed_profile = draft(id(8));
    guessed_profile["mode"] = json!("agent");
    guessed_profile["ai_profile_id"] = json!(id(11));
    let (status, body) = request(&app, Method::POST, PATH, true, Some(guessed_profile)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn external_api_configuration_saves_actions_without_agent_access(db: PgPool) {
    let app = fixture(&db).await;
    let mut input = draft(id(8));
    input["connection_type"] = json!("external_api");
    input["destination"] = json!("tasks");
    input["instructions"] = json!("");
    input.as_object_mut().unwrap().remove("source_url");
    let (status, created) = request(&app, Method::POST, PATH, true, Some(input.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["connection_type"], "external_api");
    assert_eq!(created["destination"], "tasks");
    assert_eq!(created["source_url"], "");
    assert_eq!(created["mode"], "instructions");
    assert_eq!(created["source_kind"], "api");
    assert_eq!(created["ai_profile_id"], Value::Null);
    assert_eq!(created["status"], "draft");
    let path = format!("{PATH}/{}", created["id"].as_str().unwrap());

    input["destination"] = json!("custom");
    let (status, body) = request(&app, Method::PUT, &path, true, Some(input.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    input["instructions"] = json!("Update the matching contact with the received account status.");
    let (status, updated) = request(&app, Method::PUT, &path, true, Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["destination"], "custom");
    assert_eq!(updated["instructions"], input["instructions"]);

    input["ai_profile_id"] = json!(id(11));
    let (status, body) = request(&app, Method::PUT, &path, true, Some(input.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    input["connection_type"] = json!("ai_agent");
    input["mode"] = json!("agent");
    let (status, body) = request(&app, Method::PUT, &path, true, Some(input.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (_, listed) = request(&app, Method::GET, PATH, true, None).await;
    assert_eq!(
        listed["items"][0], updated,
        "rejected changes must preserve the API configuration"
    );
    let (status, switched) = request(&app, Method::PUT, &path, false, Some(input)).await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["connection_type"], "ai_agent");
    assert_eq!(switched["ai_profile_id"], id(11).to_string());
    assert_eq!(switched["destination"], "custom");
    assert_eq!(switched["source_url"], "");
    assert_eq!(switched["status"], "draft");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn draft_channel_cannot_claim_an_operator_reply_was_sent(db: PgPool) {
    let app = fixture(&db).await;
    let created = create(&app, draft(id(8))).await;
    let channel: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("INSERT INTO contacts (id,tenant_id,project_id) VALUES ($1,$2,$3)")
        .bind(id(20))
        .bind(id(1))
        .bind(id(2))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversations (id,tenant_id,project_id,inbox_id,channel_connection_id,contact_id) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id(21)).bind(id(1)).bind(id(2)).bind(id(8)).bind(channel).bind(id(20)).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO conversation_assignments (id,tenant_id,conversation_id,user_id) VALUES ($1,$2,$3,$4)")
        .bind(id(23)).bind(id(1)).bind(id(21)).bind(id(4)).execute(&db).await.unwrap();
    let (status, body) = request(
        &app,
        Method::POST,
        &format!("/api/v1/conversations/{}/messages", id(21)),
        false,
        Some(json!({"client_message_id":id(22),"body":"This must not be marked sent."})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("custom AI channels"),
        "{body}"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE conversation_id=$1")
        .bind(id(21))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
