use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Method, Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_api, ai_tasks};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn api_path() -> String {
    format!("/api/v1/ai/profiles/{}/api", id(4))
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    credential: &str,
    input: Option<Value>,
    idempotency: Option<Uuid>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if credential == "session" {
        builder = builder
            .header("cookie", "tzomet_session=admin-session; tzomet_csrf=csrf")
            .header("x-csrf-token", "csrf");
    } else if !credential.is_empty() {
        builder = builder.header("authorization", format!("Bearer {credential}"));
    }
    if let Some(key) = idempotency {
        builder = builder.header("idempotency-key", key.to_string());
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
    let body = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (
        status,
        if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}

// Test-only queue observation: drive a real POST until persisted, then disconnect.
// PROCESSING is a harness marker, never an HTTP response from the application.
async fn request_until_queued(
    db: &PgPool,
    app: &Router,
    method: Method,
    path: &str,
    credential: &str,
    input: Option<Value>,
    idempotency: Option<Uuid>,
) -> (StatusCode, Value) {
    if method != Method::POST || !path.contains("?wait_seconds=0") {
        return request(app, method, path, credential, input, idempotency).await;
    }
    let key = idempotency.unwrap_or_else(Uuid::now_v7);
    let call = request(app, method, path, credential, input, Some(key));
    tokio::pin!(call);
    if let Ok(response) =
        tokio::time::timeout(std::time::Duration::from_millis(300), &mut call).await
    {
        return response;
    }
    let (run_id, profile, slug): (Uuid, Uuid, String) = sqlx::query_as(
        "SELECT id,ai_profile_id,endpoint_slug FROM ai_api_runs WHERE idempotency_key=$1",
    )
    .bind(key)
    .fetch_one(db)
    .await
    .unwrap();
    (
        StatusCode::PROCESSING,
        json!({"run_id":run_id,"poll_url":format!("/{profile}/api/{slug}/runs/{run_id}")}),
    )
}

async fn fixture(db: &PgPool) -> (AppState, Router) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','API tests');
        INSERT INTO users (id,email,display_name) VALUES ('{manager}','manager@example.test','Manager'),('{admin}','admin@example.test','Admin');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES ('{project}','{tenant}','Project','api-project'),('{other_project}','{tenant}','Other','other');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,is_system,permissions) VALUES
          ('{tenant}','{project}','manager','Manager','manager',false,ARRAY['ai:manage']),
          ('{tenant}','{project}','admin','Admin','admin',true,ARRAY[
            'projects:read','projects:manage','conversations:read','conversations:reply',
            'conversations:close','contacts:read','contacts:manage','channels:read','channels:manage',
            'access_tokens:manage','visitor_network:read','quality:read','quality:read_all',
            'reviews:read','routing:manage','teams:manage','ai:manage','knowledge:manage', 'notes:read', 'notes:write',
            'reply_templates:manage','processes:read','processes:edit','processes:approve','tasks:own','tasks:manage','tasks:configure','integrations:manage','roles:manage','system:read']);
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role) VALUES
          ('{manager}','{tenant}','{project}','{manager}','manager'),('{admin}','{tenant}','{project}','{admin}','admin');
        INSERT INTO ai_provider_connections (id,tenant_id,project_id,name,provider_kind,base_url,default_model,created_by) VALUES
          ('{provider}','{tenant}','{project}','Provider','openai_compatible','http://127.0.0.1:9/v1','test-model','{manager}');
        INSERT INTO ai_profiles (id,tenant_id,project_id,provider_connection_id,name,instructions,status,created_by) VALUES
          ('{profile}','{tenant}','{project}','{provider}','Agent','Use the configured task','active','{manager}'),
          ('{foreign}','{tenant}','{other_project}',NULL,'Other agent','','draft','{manager}');
    "#,tenant=id(1),project=id(2),manager=id(3),profile=id(4),provider=id(5),other_project=id(9),foreign=id(10),admin=id(11))).execute(db).await.unwrap();
    for (token, scope) in [("manager", None), ("scoped", Some(vec![id(90)]))] {
        sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,inbox_scope,role,expires_at) VALUES ($1,$2,$3,$4,$5,$6,ARRAY['ai:manage'],$7,'manager',now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(3)).bind(token).bind(Sha256::digest(token.as_bytes()).to_vec()).bind(scope).execute(db).await.unwrap();
    }
    sqlx::query("INSERT INTO operator_sessions (id,tenant_id,project_id,user_id,membership_id,token_hash,csrf_token_hash,idle_expires_at,absolute_expires_at) VALUES ($1,$2,$3,$4,$4,$5,$6,now()+interval '1 hour',now()+interval '1 day')")
        .bind(id(12)).bind(id(1)).bind(id(2)).bind(id(11)).bind(Sha256::digest(b"admin-session").to_vec()).bind(Sha256::digest(b"csrf").to_vec()).execute(db).await.unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = ai_api::router().with_state(state.clone());
    (state, app)
}

fn endpoint_input() -> Value {
    json!({"name":"Address check","slug":"address-empty","instructions":"Check the address using trusted evidence.","input_example":{"address":"0xabc"},"output_example":{"address_empty":true},"enabled":true,"timeout_seconds":180})
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn agent_proxy_settings_preserve_secrets_and_management_scope(db: PgPool) {
    let (state, _) = fixture(&db).await;
    let app = tz_backend::ai_settings::router().with_state(state);
    let path = format!("/api/v1/ai/profiles/{}/proxy", id(4));
    let call = |method, input| request(&app, method, &path, "manager", input, None);
    let (status, value) = call(Method::GET, None).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["enabled"], false);
    assert_eq!(value["token_configured"], false);
    sqlx::query("INSERT INTO ai_profile_proxy_settings (tenant_id,project_id,ai_profile_id,enabled,service_url,encrypted_token,token_nonce,key_version) VALUES ($1,$2,$3,true,'https://proxy.example/api/proxies/random',$4,$5,'v1')")
        .bind(id(1)).bind(id(2)).bind(id(4)).bind(b"encrypted-fixture".as_slice()).bind([0u8;12].as_slice()).execute(&db).await.unwrap();
    let input = json!({"enabled":true,"service_url":"https://proxy.example/api/proxies/random","country":"de"});
    let (status, value) = call(Method::PUT, Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["country"], "DE");
    assert_eq!(value["token_configured"], true);
    assert!(value.get("token").is_none() && value.get("encrypted_token").is_none());
    let stored: Vec<u8> = sqlx::query_scalar(
        "SELECT encrypted_token FROM ai_profile_proxy_settings WHERE ai_profile_id=$1",
    )
    .bind(id(4))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(stored, b"encrypted-fixture");
    let mut changed = input.clone();
    changed["service_url"] = json!("https://other.example/random");
    assert_eq!(
        call(Method::PUT, Some(changed)).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = input.clone();
    invalid["region"] = json!("europe");
    assert_eq!(
        call(Method::PUT, Some(invalid)).await.0,
        StatusCode::BAD_REQUEST
    );
    for token in ["scoped", "missing"] {
        assert_ne!(
            request(&app, Method::PUT, &path, token, Some(input.clone()), None)
                .await
                .0,
            StatusCode::OK
        );
    }
    let foreign = format!("/api/v1/ai/profiles/{}/proxy", id(10));
    assert_ne!(
        request(&app, Method::GET, &foreign, "manager", None, None)
            .await
            .0,
        StatusCode::OK
    );
    let disabled = json!({"enabled":false,"service_url":"https://proxy.example/api/proxies/random","clear_token":true});
    let (status, value) = call(Method::PUT, Some(disabled)).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["token_configured"], false);
    assert_eq!(
        call(Method::PUT, Some(input)).await.0,
        StatusCode::BAD_REQUEST
    );
}

async fn configure(app: &Router) -> (Uuid, String) {
    let (status, endpoint) = request(
        app,
        Method::POST,
        &format!("{}/endpoints", api_path()),
        "manager",
        Some(endpoint_input()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{endpoint}");
    let endpoint = endpoint["id"].as_str().unwrap().parse().unwrap();
    let (status, key) = request(
        app,
        Method::POST,
        &format!("{}/key", api_path()),
        "session",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{key}");
    let (status, _) = request(
        app,
        Method::PATCH,
        &api_path(),
        "manager",
        Some(json!({"enabled":true})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    (endpoint, key["key"].as_str().unwrap().to_owned())
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn api_auth_schema_idempotency_and_key_rotation(db: PgPool) {
    let (_, app) = fixture(&db).await;
    let (_, settings) = request(&app, Method::GET, &api_path(), "manager", None, None).await;
    assert_eq!(
        settings,
        json!({"enabled":false,"key_configured":false,"key_prefix":null,"can_manage_key":false,"endpoints":[]})
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_api_settings")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0, "GET settings must not write");
    for method in [Method::GET, Method::PATCH] {
        let (status, _) = request_until_queued(
            &db,
            &app,
            method,
            &api_path(),
            "scoped",
            Some(json!({"enabled":true})),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (status, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/ai/profiles/{}/api", id(10)),
        "manager",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &format!("{}/key", api_path()),
        "manager",
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "machine tokens cannot mint credentials"
    );
    let mut invalid = endpoint_input();
    invalid["output_example"] = json!({"items":[]});
    let (status, _) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &format!("{}/endpoints", api_path()),
        "manager",
        Some(invalid),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (endpoint, key) = configure(&app).await;
    let (_, settings) = request(&app, Method::GET, &api_path(), "session", None, None).await;
    assert_eq!(settings["can_manage_key"], true);
    assert_eq!(
        settings["endpoints"][0]["output_schema"]["additionalProperties"],
        false
    );
    assert!(!settings.to_string().contains(&key));
    let hash: Vec<u8> =
        sqlx::query_scalar("SELECT key_hash FROM ai_api_settings WHERE ai_profile_id=$1")
            .bind(id(4))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(hash, Sha256::digest(key.as_bytes()).to_vec());
    let public = format!("/{}/api/address-empty?wait_seconds=0", id(4));
    for credential in ["", "manager"] {
        assert_eq!(
            request_until_queued(
                &db,
                &app,
                Method::POST,
                &public,
                credential,
                Some(json!({"address":"0xabc"})),
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    for invalid in [
        json!({"address":false}),
        json!({}),
        json!({"address":"x","extra":1}),
    ] {
        assert_eq!(
            request_until_queued(&db, &app, Method::POST, &public, &key, Some(invalid), None)
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let idempotency = Uuid::now_v7();
    let (status, first) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"0xabc"})),
        Some(idempotency),
    )
    .await;
    assert_eq!(status, StatusCode::PROCESSING, "{first}");
    let (_, again) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"0xabc"})),
        Some(idempotency),
    )
    .await;
    assert_eq!(first["run_id"], again["run_id"]);
    assert_eq!(
        request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"different"})),
            Some(idempotency)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let poll = first["poll_url"].as_str().unwrap();
    let (status, run) = request(&app, Method::GET, poll, &key, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(run["endpoint_id"], endpoint.to_string());
    let mut renamed = endpoint_input();
    renamed["slug"] = json!("renamed-check");
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &format!("{}/endpoints/{endpoint}", api_path()),
            "manager",
            Some(renamed),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, renamed_replay) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &format!("/{}/api/renamed-check?wait_seconds=0", id(4)),
        &key,
        Some(json!({"address":"0xabc"})),
        Some(idempotency),
    )
    .await;
    assert_eq!(
        renamed_replay["poll_url"], first["poll_url"],
        "renaming the method must preserve the original run polling URL"
    );
    assert_eq!(
        request(
            &app,
            Method::GET,
            renamed_replay["poll_url"].as_str().unwrap(),
            &key,
            None,
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, rotated) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &format!("{}/key", api_path()),
        "session",
        None,
        None,
    )
    .await;
    let new_key = rotated["key"].as_str().unwrap();
    assert_ne!(key, new_key);
    assert_eq!(
        request(&app, Method::GET, poll, &key, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, Method::GET, poll, new_key, None, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, history) = request(
        &app,
        Method::GET,
        &format!("{}/runs", api_path()),
        "manager",
        None,
        None,
    )
    .await;
    assert_eq!(history["items"][0]["status"], "cancelled");
    assert_eq!(history["items"][0]["error"]["code"], "key_rotated");
    assert_eq!(
        request(
            &app,
            Method::DELETE,
            &format!("{}/key", api_path()),
            "session",
            None,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            new_key,
            Some(json!({"address":"x"})),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    // Draft tests require management scope, but do not require an exposed API or key.
    request(
        &app,
        Method::PATCH,
        &api_path(),
        "manager",
        Some(json!({"enabled":false})),
        None,
    )
    .await;
    let mut draft = endpoint_input();
    draft["enabled"] = json!(false);
    request(
        &app,
        Method::PATCH,
        &format!("{}/endpoints/{endpoint}", api_path()),
        "manager",
        Some(draft),
        None,
    )
    .await;
    let test = format!("{}/endpoints/{endpoint}/test", api_path());
    assert_eq!(
        request_until_queued(
            &db,
            &app,
            Method::POST,
            &test,
            "scoped",
            Some(json!({"address":"x"})),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, test_run) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &test,
        "manager",
        Some(json!({"address":"x"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{test_run}");
    request(
        &app,
        Method::DELETE,
        &format!("{}/endpoints/{endpoint}", api_path()),
        "manager",
        None,
        None,
    )
    .await;
    let (_, test_run) = request(
        &app,
        Method::GET,
        &format!("{}/runs/{}", api_path(), test_run["id"].as_str().unwrap()),
        "manager",
        None,
        None,
    )
    .await;
    assert_eq!(test_run["status"], "cancelled");
}

async fn provider(State(output): State<Arc<RwLock<Value>>>) -> Json<Value> {
    let mut answer = output.read().await.clone();
    let delay = answer
        .as_object_mut()
        .unwrap()
        .remove("_test_delay_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    Json(json!({"choices":[{"message":{"role":"assistant","content":answer.to_string()}}]}))
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn reply_claims_serialize_one_conversation_without_blocking_other_conversations(db: PgPool) {
    let (state, _) = fixture(&db).await;
    let busy_conversation = id(51);
    let other_conversation = id(52);
    for (job, conversation) in [(id(61), busy_conversation), (id(62), other_conversation)] {
        let payload = json!({"ai_profile_id":id(4),"project_id":id(2),"inbox_id":id(70),
            "contact_id":id(71),"conversation_id":conversation,
            "triggering_message_id":id(72),"triggering_sequence":1});
        sqlx::query(
            "INSERT INTO outbox_events(id,tenant_id,aggregate_type,aggregate_id,event_type,payload)
            VALUES ($1,$2,'provider_reply',$3,'provider.reply.requested',$4)",
        )
        .bind(job)
        .bind(id(1))
        .bind(conversation)
        .bind(payload)
        .execute(&db)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO ai_execution_leases(id,ai_profile_id,conversation_id,expires_at) VALUES ($1,$2,$3,now()+interval '1 minute')")
        .bind(id(80)).bind(id(4)).bind(busy_conversation).execute(&db).await.unwrap();
    assert!(
        tz_backend::provider_reply::process_once(&state, Uuid::now_v7())
            .await
            .unwrap()
    );
    let statuses: Vec<String> = sqlx::query_scalar("SELECT status FROM outbox_events ORDER BY id")
        .fetch_all(&db)
        .await
        .unwrap();
    // The other conversation has no remaining participant, so it completes
    // without a provider call. The busy conversation must not be claimed.
    assert_eq!(statuses, vec!["pending", "completed"]);
    assert!(
        !tz_backend::provider_reply::process_once(&state, Uuid::now_v7())
            .await
            .unwrap()
    );
    sqlx::query("DELETE FROM ai_execution_leases WHERE id=$1")
        .bind(id(80))
        .execute(&db)
        .await
        .unwrap();
    assert!(
        tz_backend::provider_reply::process_once(&state, Uuid::now_v7())
            .await
            .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ai_execution_leases")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn profile_concurrency_is_validated_saved_and_preserved_for_older_clients(db: PgPool) {
    let (state, _) = fixture(&db).await;
    let app = tz_backend::ai_settings::router().with_state(state);
    let path = format!("/api/v1/ai/profiles/{}", id(4));
    let mut input = json!({"name":"Agent","status":"active","provider_connection_id":id(5),
        "instructions":"Answer requests","language":"en","max_output_tokens":800,
        "public_identities":[{"language":"en","display_name":"Agent"}],"max_concurrent_runs":3});
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &path,
        "manager",
        Some(input.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["max_concurrent_runs"], 3);
    input.as_object_mut().unwrap().remove("max_concurrent_runs");
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &path,
        "manager",
        Some(input.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["max_concurrent_runs"], 3);
    for limit in [0, 17] {
        input["max_concurrent_runs"] = json!(limit);
        assert_eq!(
            request(
                &app,
                Method::PATCH,
                &path,
                "manager",
                Some(input.clone()),
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    input.as_object_mut().unwrap().remove("max_concurrent_runs");
    input["name"] = json!("New agent");
    let (status, created) = request(
        &app,
        Method::POST,
        "/api/v1/ai/profiles",
        "manager",
        Some(input),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["max_concurrent_runs"], 2);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn same_agent_api_calls_overlap_and_excess_calls_wait_without_mixing_results(db: PgPool) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    let (mut state, app) = fixture(&db).await;
    let (_, key) = configure(&app).await;
    let started = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = Arc::make_mut(&mut state.config);
    let runtime = std::env::temp_dir().join(format!("tzomet-parallel-api-{}", Uuid::now_v7()));
    config.openclaw.base_url = Some(format!("http://{address}/v1"));
    config.openclaw.grant_directory = runtime.join("grants");
    config.openclaw.action_directory = runtime.join("actions");
    // Profile limit (2) is independently enforced below the server limit (3).
    config.worker.ai_run_concurrency = 3;
    let provider = {
        let started = started.clone();
        let release = release.clone();
        Router::new().route("/v1/chat/completions", post(move |Json(input): Json<Value>| {
            let started = started.clone(); let release = release.clone();
            async move {
                started.fetch_add(1, Ordering::SeqCst);
                release.acquire().await.unwrap().forget();
                let answer = json!({"status":"completed","result":{"address_empty":input.to_string().contains("parallel-0")}});
                Json(json!({"choices":[{"message":{"content":answer.to_string()}}]}))
            }
        }))
    };
    let server = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    sqlx::query("UPDATE ai_provider_connections SET base_url=$1 WHERE id=$2")
        .bind(format!("http://{address}/v1"))
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    let public = format!("/{}/api/address-empty?wait_seconds=0", id(4));
    let mut runs = Vec::new();
    for index in 0..3 {
        let (_, queued) = request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":format!("parallel-{index}")})),
            None,
        )
        .await;
        runs.push(queued["run_id"].as_str().unwrap().parse::<Uuid>().unwrap());
    }
    let first = {
        let state = state.clone();
        tokio::spawn(async move { ai_api::process_once(&state).await })
    };
    let second = {
        let state = state.clone();
        tokio::spawn(async move { ai_api::process_once(&state).await })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while started.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !ai_api::process_once(&state).await.unwrap(),
        "third call must stay queued at the profile limit"
    );
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM ai_api_runs ORDER BY created_at,id")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(statuses, vec!["processing", "processing", "pending"]);
    release.add_permits(2);
    tokio::time::timeout(Duration::from_secs(5), async {
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
    })
    .await
    .unwrap();
    release.add_permits(1);
    assert!(ai_api::process_once(&state).await.unwrap());
    for (index, run) in runs.iter().enumerate() {
        let result: Value = sqlx::query_scalar("SELECT result FROM ai_api_runs WHERE id=$1")
            .bind(run)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(result, json!({"address_empty":index == 0}));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ai_execution_leases")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    server.abort();
    std::fs::remove_dir_all(runtime).unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn shared_provider_receives_only_current_project_assigned_skills(db: PgPool) {
    let (mut state, app) = fixture(&db).await;
    let captured = Arc::new(RwLock::new(Vec::new()));
    let requests = captured.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime_directory =
        std::env::temp_dir().join(format!("tzomet-api-skills-{}", Uuid::now_v7()));
    let config = Arc::make_mut(&mut state.config);
    config.openclaw.base_url = Some(format!("http://{address}/v1"));
    config.openclaw.grant_directory = runtime_directory.join("grants");
    config.openclaw.action_directory = runtime_directory.join("actions");
    let provider = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(request): Json<Value>| {
            let requests = requests.clone();
            async move {
                requests.write().await.push(request);
                Json(json!({"choices":[{"message":{"content":
                    "{\"status\":\"completed\",\"result\":{\"address_empty\":false}}"
                }}]}))
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    sqlx::query("UPDATE ai_provider_connections SET base_url=$1,project_id=$3 WHERE id=$2")
        .bind(format!("http://{address}/v1"))
        .bind(id(5))
        .bind(id(9))
        .execute(&db)
        .await
        .unwrap();
    for (skill, project, name, instructions, profile) in [
        (20, 2, "B policy", "Apply the second policy.", Some(4)),
        (
            21,
            2,
            "A policy",
            "Ask for an address before checking.",
            Some(4),
        ),
        (
            22,
            9,
            "Foreign project policy",
            "FOREIGN_PROJECT_SKILL",
            Some(4),
        ),
        (
            23,
            2,
            "Foreign agent policy",
            "FOREIGN_AGENT_SKILL",
            Some(10),
        ),
        (24, 2, "Unused policy", "UNASSIGNED_SKILL", None),
    ] {
        sqlx::query("INSERT INTO ai_skills(id,tenant_id,project_id,name,description,instructions) VALUES($1,$2,$3,$4,'Relevant reusable instructions',$5)")
            .bind(id(skill)).bind(id(1)).bind(id(project)).bind(name).bind(instructions)
            .execute(&db).await.unwrap();
        if let Some(profile) = profile {
            sqlx::query("INSERT INTO ai_profile_skills(tenant_id,project_id,skill_id,ai_profile_id) VALUES($1,$2,$3,$4)")
                .bind(id(1)).bind(id(project)).bind(id(skill)).bind(id(profile))
                .execute(&db).await.unwrap();
        }
    }
    let (_, key) = configure(&app).await;
    let public = format!("/{}/api/address-empty?wait_seconds=0", id(4));
    for (phase, expected) in [
        (0, Some("Ask for an address before checking.")),
        (1, Some("Confirm the updated address first.")),
        (2, None),
    ] {
        if phase == 1 {
            // Visibility changes affect model selection, while the assigned
            // provider keeps executing in the agent's project.
            sqlx::query("UPDATE ai_provider_connections SET visibility=jsonb_build_object('project_ids',jsonb_build_array(project_id),'department_ids','[]'::jsonb) WHERE id=$1")
                .bind(id(5)).execute(&db).await.unwrap();
            sqlx::query("UPDATE ai_skills SET instructions=$1 WHERE id=$2")
                .bind(expected.unwrap())
                .bind(id(21))
                .execute(&db)
                .await
                .unwrap();
        } else if phase == 2 {
            sqlx::query("DELETE FROM ai_skills WHERE id=$1")
                .bind(id(21))
                .execute(&db)
                .await
                .unwrap();
        }
        let (status, queued) = request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"x"})),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::PROCESSING, "{queued}");
        assert!(ai_api::process_once(&state).await.unwrap());
        let (_, run) = request(
            &app,
            Method::GET,
            queued["poll_url"].as_str().unwrap(),
            &key,
            None,
            None,
        )
        .await;
        assert_eq!(run["status"], "completed", "{run}");
        let captured = captured.read().await;
        let system = captured.last().unwrap()["messages"][0]["content"]
            .as_str()
            .unwrap();
        assert!(system.starts_with("Use the configured task"));
        assert!(system.contains("<tzomet_api_execution>"));
        assert!(system.contains("Skills never grant tools"));
        assert!(system.contains("Apply the second policy."));
        for excluded in [
            "FOREIGN_PROJECT_SKILL",
            "FOREIGN_AGENT_SKILL",
            "UNASSIGNED_SKILL",
        ] {
            assert!(!system.contains(excluded));
        }
        if let Some(expected) = expected {
            assert!(system.contains(expected));
            assert!(system.find("A policy").unwrap() < system.find("B policy").unwrap());
        } else {
            assert!(!system.contains("A policy"));
        }
    }
    assert_eq!(captured.read().await.len(), 3);
    server.abort();
    let _ = tokio::fs::remove_dir_all(runtime_directory).await;
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn durable_worker_validates_results_and_never_retries(db: PgPool) {
    let (mut state, app) = fixture(&db).await;
    let output = Arc::new(RwLock::new(
        json!({"status":"completed","result":{"address_empty":false}}),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime_directory =
        std::env::temp_dir().join(format!("tzomet-api-runtime-{}", Uuid::now_v7()));
    let config = Arc::make_mut(&mut state.config);
    config.openclaw.base_url = Some(format!("http://{address}/v1"));
    config.openclaw.grant_directory = runtime_directory.join("grants");
    config.openclaw.action_directory = runtime_directory.join("actions");
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(provider))
                .with_state(output.clone()),
        )
        .into_future(),
    );
    sqlx::query("UPDATE ai_provider_connections SET base_url=$1 WHERE id=$2")
        .bind(format!("http://{address}/v1"))
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    let (_, key) = configure(&app).await;
    let public = format!("/{}/api/address-empty?wait_seconds=0", id(4));
    for (answer, status, error_code) in [
        (
            json!({"status":"completed","result":{"address_empty":true}}),
            "completed",
            None,
        ),
        (
            json!({"status":"completed","result":{"address_empty":false}}),
            "completed",
            None,
        ),
        (
            json!({"status":"completed","result":{"address_empty":"false"}}),
            "failed",
            Some("result_schema_invalid"),
        ),
        (
            json!({"status":"failed","error":{"code":"verification_failed"}}),
            "failed",
            Some("verification_failed"),
        ),
    ] {
        *output.write().await = answer.clone();
        let idempotency = Uuid::now_v7();
        let (_, queued) = request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"x"})),
            Some(idempotency),
        )
        .await;
        assert!(ai_api::process_once(&state).await.unwrap());
        let (_, run) = request(
            &app,
            Method::GET,
            queued["poll_url"].as_str().unwrap(),
            &key,
            None,
            None,
        )
        .await;
        assert_eq!(run["status"], status, "{run}");
        if let Some(code) = error_code {
            assert_eq!(run["error"]["code"], code);
            assert_eq!(run["result"], Value::Null);
        } else {
            assert_eq!(run["result"], answer["result"]);
        }
        let (http_status, replay) = request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"x"})),
            Some(idempotency),
        )
        .await;
        if status == "completed" {
            assert_eq!(http_status, StatusCode::OK);
            assert_eq!(
                replay, answer["result"],
                "public success returns the raw typed object"
            );
        } else {
            assert_eq!(http_status, StatusCode::BAD_GATEWAY);
            assert_eq!(replay["error"]["code"], error_code.unwrap());
        }
        assert!(
            !ai_api::process_once(&state).await.unwrap(),
            "a terminal run must never be retried"
        );
    }
    *output.write().await = json!({"status":"completed","result":{"address_empty":false}});
    let sync_app = app.clone();
    let sync_key = key.clone();
    let sync_address = format!("fresh-sync-{}", Uuid::now_v7());
    let request_address = sync_address.clone();
    let synchronous = tokio::spawn(async move {
        request(
            &sync_app,
            Method::POST,
            &format!("/{}/api/address-empty?wait_seconds=5", id(4)),
            &sync_key,
            Some(json!({"address":request_address})),
            None,
        )
        .await
    });
    let (status, result) = tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_api_runs WHERE ai_profile_id=$1 AND input->>'address'=$2 AND status='pending')")
                .bind(id(4)).bind(&sync_address).fetch_one(&db).await.unwrap();
            if pending { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(ai_api::process_once(&state).await.unwrap());
        synchronous.await.unwrap()
    }).await.unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "a fresh waiting invocation returns HTTP 200"
    );
    assert_eq!(
        result,
        json!({"address_empty":false}),
        "the initial request returns only the raw typed result"
    );
    let (_, queued) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"x"})),
        None,
    )
    .await;
    sqlx::query("UPDATE ai_api_runs SET queue_deadline_at=now()-interval '1 second' WHERE id=$1")
        .bind(queued["run_id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .execute(&db)
        .await
        .unwrap();
    assert!(!ai_api::process_once(&state).await.unwrap());
    let (_, run) = request(
        &app,
        Method::GET,
        queued["poll_url"].as_str().unwrap(),
        &key,
        None,
        None,
    )
    .await;
    assert_eq!(run["error"]["code"], "queue_timeout");
    let (_, queued) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"x"})),
        None,
    )
    .await;
    sqlx::query("UPDATE ai_api_runs SET status='processing',locked_by=id,started_at=now()-interval '2 minutes',deadline_at=now()-interval '1 second' WHERE id=$1")
        .bind(queued["run_id"].as_str().unwrap().parse::<Uuid>().unwrap()).execute(&db).await.unwrap();
    assert!(
        !ai_api::process_once(&state).await.unwrap(),
        "a crashed processing run must fail without retry"
    );
    let (_, run) = request(
        &app,
        Method::GET,
        queued["poll_url"].as_str().unwrap(),
        &key,
        None,
        None,
    )
    .await;
    assert_eq!(run["error"]["code"], "timeout");
    *output.write().await =
        json!({"status":"completed","result":{"address_empty":true},"_test_delay_ms":1500});
    let (_, queued) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"x"})),
        None,
    )
    .await;
    let run_id = queued["run_id"].as_str().unwrap().parse::<Uuid>().unwrap();
    let worker_state = state.clone();
    let worker = tokio::spawn(async move { ai_api::process_once(&worker_state).await.unwrap() });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let processing: bool =
                sqlx::query_scalar("SELECT status='processing' FROM ai_api_runs WHERE id=$1")
                    .bind(run_id)
                    .fetch_one(&db)
                    .await
                    .unwrap();
            if processing {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("UPDATE ai_profiles SET status='disabled' WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    assert!(worker.await.unwrap());
    let (_, run) = request(
        &app,
        Method::GET,
        queued["poll_url"].as_str().unwrap(),
        &key,
        None,
        None,
    )
    .await;
    assert_eq!(
        run["status"], "cancelled",
        "revoked agent must not publish a late provider result"
    );
    assert_eq!(run["result"], Value::Null);
    let lease: Option<Uuid> = sqlx::query_scalar("SELECT locked_by FROM ai_api_runs WHERE id=$1")
        .bind(run_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(
        lease.is_none(),
        "cancelled execution must release its lease after cleanup"
    );
    server.abort();
    tokio::fs::remove_dir_all(runtime_directory).await.unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn admission_limits_and_immutable_endpoint_snapshot(db: PgPool) {
    let (_, app) = fixture(&db).await;
    let (endpoint, key) = configure(&app).await;
    let public = format!("/{}/api/address-empty?wait_seconds=0", id(4));
    let idempotency = Uuid::now_v7();
    for index in 0..30 {
        let (status, body) = request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"x"})),
            (index == 0).then_some(idempotency),
        )
        .await;
        assert_eq!(status, StatusCode::PROCESSING, "{body}");
    }
    let (status, body) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"x"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(
        request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"x"})),
            Some(idempotency)
        )
        .await
        .0,
        StatusCode::PROCESSING,
        "replay must not consume admission capacity"
    );
    let mut updated = endpoint_input();
    updated["instructions"] = json!("Changed task");
    updated["output_example"] = json!({"address_empty":"unknown"});
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &format!("{}/endpoints/{endpoint}", api_path()),
            "manager",
            Some(updated),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let snapshot: (String, Value, i64) = sqlx::query_as(
        "SELECT instructions,output_schema,endpoint_version FROM ai_api_runs LIMIT 1",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(snapshot.0, "Check the address using trusted evidence.");
    assert_eq!(snapshot.1["properties"]["address_empty"]["type"], "boolean");
    assert_eq!(snapshot.2, 1);
    sqlx::query("UPDATE ai_api_runs SET created_at=now()-interval '2 minutes'")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ai_api_runs (id,tenant_id,project_id,ai_profile_id,endpoint_id,endpoint_name,endpoint_slug,endpoint_version,key_id,instructions,input_schema,output_schema,input,timeout_seconds,input_hash,created_at) SELECT md5('api-capacity-'||n::text)::uuid,run.tenant_id,run.project_id,run.ai_profile_id,run.endpoint_id,run.endpoint_name,run.endpoint_slug,run.endpoint_version,run.key_id,run.instructions,run.input_schema,run.output_schema,run.input,run.timeout_seconds,run.input_hash,now()-interval '2 minutes' FROM (SELECT * FROM ai_api_runs LIMIT 1) AS run CROSS JOIN generate_series(1,70) AS n").execute(&db).await.unwrap();
    assert_eq!(
        request_until_queued(
            &db,
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"x"})),
            None
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS,
        "project queue must remain bounded after the rate window expires"
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn api_and_scheduled_runs_share_openclaw_capacity(db: PgPool) {
    let (state, app) = fixture(&db).await;
    sqlx::query("UPDATE ai_profiles SET max_concurrent_runs=1 WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    let (_, key) = configure(&app).await;
    let public = format!("/{}/api/address-empty?wait_seconds=0", id(4));
    let (_, queued) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"x"})),
        None,
    )
    .await;
    sqlx::raw_sql(&format!(r#"
        INSERT INTO ai_tasks (id,tenant_id,project_id,text,schedule,created_by) VALUES ('{task}','{tenant}','{project}','Check address','{{"kind":"once","run_at":"2099-01-01T00:00:00Z"}}','{user}');
        INSERT INTO ai_task_agents (tenant_id,task_id,ai_profile_id) VALUES ('{tenant}','{task}','{profile}');
        INSERT INTO ai_task_runs (id,tenant_id,project_id,task_id,ai_profile_id,agent_name,task_text,scheduled_for,status,locked_at,locked_by) VALUES ('{run}','{tenant}','{project}','{task}','{profile}','Agent','Check address',now(),'processing',now(),'{run}');
    "#,task=id(20),tenant=id(1),project=id(2),user=id(3),profile=id(4),run=id(21))).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO ai_execution_leases (id,ai_profile_id,expires_at) VALUES ($1,$2,now()+interval '10 minutes')")
        .bind(id(21)).bind(id(4)).execute(&db).await.unwrap();
    assert!(
        !ai_api::process_once(&state).await.unwrap(),
        "fresh scheduled lease must block API claim"
    );
    let run_id = queued["run_id"].as_str().unwrap().parse::<Uuid>().unwrap();
    sqlx::query(
        "UPDATE ai_task_runs SET status='pending',locked_at=NULL,locked_by=NULL WHERE id=$1",
    )
    .bind(id(21))
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("DELETE FROM ai_execution_leases WHERE id=$1")
        .bind(id(21))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ai_execution_leases (id,ai_profile_id,expires_at) VALUES ($1,$2,now()+interval '1 minute')")
        .bind(run_id).bind(id(4)).execute(&db).await.unwrap();
    sqlx::query("UPDATE ai_api_runs SET status='processing',locked_by=$1,started_at=now(),deadline_at=now()+interval '1 minute' WHERE id=$1").bind(run_id).execute(&db).await.unwrap();
    assert!(
        !ai_tasks::process_once(&state).await.unwrap(),
        "fresh API lease must block scheduled claim"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM ai_task_runs WHERE id=$1")
        .bind(id(21))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "pending");
    sqlx::query("UPDATE ai_api_runs SET status='cancelled',error_code='key_revoked',completed_at=now() WHERE id=$1")
        .bind(run_id).execute(&db).await.unwrap();
    assert!(
        !ai_tasks::process_once(&state).await.unwrap(),
        "cancelled API work retains its lane until runtime cleanup"
    );
    let (status, _) = request_until_queued(
        &db,
        &app,
        Method::POST,
        &public,
        &key,
        Some(json!({"address":"y"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::PROCESSING);
    assert!(
        !ai_api::process_once(&state).await.unwrap(),
        "a cancelled active lease also blocks the next API run"
    );
    sqlx::query("UPDATE ai_api_runs SET deadline_at=now()-interval '1 second' WHERE id=$1")
        .bind(run_id)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE ai_execution_leases SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(run_id)
        .execute(&db)
        .await
        .unwrap();
    // Provider is deliberately unavailable; an expired API lease still releases capacity.
    assert!(ai_api::process_once(&state).await.unwrap());
    assert!(ai_tasks::process_once(&state).await.unwrap());
}

async fn cache_due(db: &PgPool, endpoint: Uuid) {
    sqlx::query(
        "UPDATE ai_api_endpoints SET cache_next_refresh_at=now()-interval '1 second' WHERE id=$1",
    )
    .bind(endpoint)
    .execute(db)
    .await
    .unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn cached_api_refreshes_without_requests_and_preserves_last_success(db: PgPool) {
    let (mut state, app) = fixture(&db).await;
    let output = Arc::new(RwLock::new(
        json!({"status":"completed","result":{"address_empty":false}}),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime_directory =
        std::env::temp_dir().join(format!("tzomet-cache-runtime-{}", Uuid::now_v7()));
    let config = Arc::make_mut(&mut state.config);
    config.openclaw.base_url = Some(format!("http://{address}/v1"));
    config.openclaw.grant_directory = runtime_directory.join("grants");
    config.openclaw.action_directory = runtime_directory.join("actions");
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(provider))
                .with_state(output.clone()),
        )
        .into_future(),
    );
    sqlx::query("UPDATE ai_provider_connections SET base_url=$1 WHERE id=$2")
        .bind(format!("http://{address}/v1"))
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    let (endpoint, key) = configure(&app).await;
    let management = format!("{}/endpoints/{endpoint}", api_path());
    let public = format!("/{}/api/address-empty", id(4));
    let mut input = endpoint_input();
    for interval in [0, 59, 86401] {
        input["cache_refresh_seconds"] = json!(interval);
        assert_eq!(
            request(
                &app,
                Method::PATCH,
                &management,
                "manager",
                Some(input.clone()),
                None
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    input["cache_refresh_seconds"] = json!(300);
    input["response_wait_seconds"] = json!(300);
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &management,
            "manager",
            Some(input.clone()),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    let invoke = || {
        request(
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"0xabc"})),
            Some(Uuid::now_v7()),
        )
    };
    let (status, cold) = tokio::time::timeout(std::time::Duration::from_secs(2), invoke())
        .await
        .unwrap();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(cold["error"]["code"], "cache_warming");
    assert_eq!(
        request(
            &app,
            Method::POST,
            &public,
            &key,
            Some(json!({"address":"other"})),
            None
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ai_api_runs")
            .fetch_one(&db)
            .await
            .unwrap(),
        0,
        "cache reads must not queue runs"
    );

    // Both workers may observe a due method, but admission must enqueue only once.
    let (first, second) = tokio::join!(ai_api::process_once(&state), ai_api::process_once(&state));
    assert_ne!(first.unwrap(), second.unwrap());
    let (status, warm) = tokio::time::timeout(std::time::Duration::from_secs(2), invoke())
        .await
        .unwrap();
    let outcomes: Vec<(String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT status,error_code,error_message FROM ai_api_runs")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(status, StatusCode::OK, "{warm}; {outcomes:?}");
    assert_eq!(warm["result"], json!({"address_empty":false}));
    assert_eq!(warm["stale"], false);
    assert!(warm["updated_at"].as_str().is_some());
    for _ in 0..3 {
        assert_eq!(invoke().await.1, warm);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ai_api_runs")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert!(!ai_api::process_once(&state).await.unwrap());

    // A failed due refresh keeps the last successful value and its timestamp.
    sqlx::query("UPDATE ai_api_cache SET updated_at=now()-interval '6 minutes'")
        .execute(&db)
        .await
        .unwrap();
    let old = invoke().await.1;
    assert_eq!(old["stale"], true);
    *output.write().await = json!({"status":"failed","error":{"code":"verification_failed"}});
    cache_due(&db, endpoint).await;
    assert!(ai_api::process_once(&state).await.unwrap());
    assert_eq!(invoke().await.1, old);
    assert!(
        !ai_api::process_once(&state).await.unwrap(),
        "failed refresh must wait until the next interval"
    );
    *output.write().await = json!({"status":"completed","result":{"address_empty":true}});
    cache_due(&db, endpoint).await;
    assert!(ai_api::process_once(&state).await.unwrap());
    assert_eq!(invoke().await.1["result"]["address_empty"], true);
    assert_eq!(invoke().await.1["stale"], false);

    // A cached value never bypasses API/profile/provider authorization.
    request(
        &app,
        Method::PATCH,
        &api_path(),
        "manager",
        Some(json!({"enabled":false})),
        None,
    )
    .await;
    assert_eq!(invoke().await.0, StatusCode::NOT_FOUND);
    cache_due(&db, endpoint).await;
    assert!(!ai_api::process_once(&state).await.unwrap());
    request(
        &app,
        Method::PATCH,
        &api_path(),
        "manager",
        Some(json!({"enabled":true})),
        None,
    )
    .await;
    sqlx::query("UPDATE ai_profiles SET status='draft' WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(invoke().await.0, StatusCode::NOT_FOUND);
    assert!(!ai_api::process_once(&state).await.unwrap());
    sqlx::query("UPDATE ai_profiles SET status='active' WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE ai_api_endpoints SET cache_next_refresh_at=now()+interval '1 day' WHERE id=$1",
    )
    .bind(endpoint)
    .execute(&db)
    .await
    .unwrap();
    let (_, rotated) = request(
        &app,
        Method::POST,
        &format!("{}/key", api_path()),
        "session",
        None,
        None,
    )
    .await;
    assert_eq!(invoke().await.0, StatusCode::UNAUTHORIZED);
    let key = rotated["key"].as_str().unwrap();
    assert_eq!(
        request(
            &app,
            Method::POST,
            &public,
            key,
            Some(json!({"address":"0xabc"})),
            None
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(ai_api::process_once(&state).await.unwrap());
    assert_eq!(
        request(
            &app,
            Method::POST,
            &public,
            key,
            Some(json!({"address":"0xabc"})),
            None
        )
        .await
        .0,
        StatusCode::OK
    );

    // Editing a method while a refresh is executing invalidates both old snapshots and publication.
    *output.write().await =
        json!({"_test_delay_ms":500,"status":"completed","result":{"address_empty":false}});
    cache_due(&db, endpoint).await;
    let worker_state = state.clone();
    let worker = tokio::spawn(async move { ai_api::process_once(&worker_state).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM ai_api_runs WHERE status='processing')",
            )
            .fetch_one(&db)
            .await
            .unwrap()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    input["instructions"] = json!("Updated task.");
    assert_eq!(
        request(
            &app,
            Method::PATCH,
            &management,
            "manager",
            Some(input.clone()),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(worker.await.unwrap().unwrap());
    assert_eq!(
        request(
            &app,
            Method::POST,
            &public,
            key,
            Some(json!({"address":"0xabc"})),
            None
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ai_api_cache")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    *output.write().await = json!({"status":"completed","result":{"address_empty":true}});
    assert!(ai_api::process_once(&state).await.unwrap());
    assert_eq!(
        request(
            &app,
            Method::POST,
            &public,
            key,
            Some(json!({"address":"0xabc"})),
            None
        )
        .await
        .1["result"]["address_empty"],
        true
    );
    server.abort();
    let _ = tokio::fs::remove_dir_all(runtime_directory).await;
}

async fn wait_for_pending_api_run(db: &PgPool) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM ai_api_runs WHERE status='pending')",
            )
            .fetch_one(db)
            .await
            .unwrap()
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn public_invocation_waits_until_terminal_even_with_legacy_zero_wait(db: PgPool) {
    let (mut state, app) = fixture(&db).await;
    let output = Arc::new(RwLock::new(
        json!({"_test_delay_ms":21000,"status":"completed","result":{"address_empty":false}}),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime_directory =
        std::env::temp_dir().join(format!("tzomet-wait-runtime-{}", Uuid::now_v7()));
    let config = Arc::make_mut(&mut state.config);
    config.openclaw.base_url = Some(format!("http://{address}/v1"));
    config.openclaw.grant_directory = runtime_directory.join("grants");
    config.openclaw.action_directory = runtime_directory.join("actions");
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(provider))
                .with_state(output.clone()),
        )
        .into_future(),
    );
    sqlx::query("UPDATE ai_provider_connections SET base_url=$1 WHERE id=$2")
        .bind(format!("http://{address}/v1"))
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    let (endpoint, key) = configure(&app).await;
    let management = format!("{}/endpoints/{endpoint}", api_path());
    let public = format!("/{}/api/address-empty", id(4));
    let (_, settings) = request(&app, Method::GET, &api_path(), "manager", None, None).await;
    assert_eq!(
        settings["endpoints"][0]["response_wait_seconds"], 20,
        "older clients keep the existing wait"
    );
    let mut input = endpoint_input();
    for wait in [-1, 301] {
        input["response_wait_seconds"] = json!(wait);
        assert_eq!(
            request(
                &app,
                Method::PATCH,
                &management,
                "manager",
                Some(input.clone()),
                None
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    input["response_wait_seconds"] = json!(180);
    let (status, saved) = request(
        &app,
        Method::PATCH,
        &management,
        "manager",
        Some(input.clone()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["response_wait_seconds"], 180);
    let caller = |url: String, idempotency| {
        let app = app.clone();
        let key = key.clone();
        tokio::spawn(async move {
            request(
                &app,
                Method::POST,
                &url,
                &key,
                Some(json!({"address":"0xabc"})),
                Some(idempotency),
            )
            .await
        })
    };
    // The saved setting must apply without a query override, beyond the old 20s cap.
    let started = std::time::Instant::now();
    let waiting = caller(public.clone(), Uuid::now_v7());
    wait_for_pending_api_run(&db).await;
    let (status, result) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        assert!(ai_api::process_once(&state).await.unwrap());
        waiting.await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result, json!({"address_empty":false}));
    assert!(started.elapsed() >= std::time::Duration::from_secs(20));
    assert!(started.elapsed() < std::time::Duration::from_secs(30));

    // A short legacy wait must not return while the worker is still executing.
    input["response_wait_seconds"] = json!(1);
    request(
        &app,
        Method::PATCH,
        &management,
        "manager",
        Some(input.clone()),
        None,
    )
    .await;
    *output.write().await =
        json!({"_test_delay_ms":2200,"status":"completed","result":{"address_empty":true}});
    let idempotency = Uuid::now_v7();
    let mut waiting = caller(public.clone(), idempotency);
    wait_for_pending_api_run(&db).await;
    let worker_state = state.clone();
    let worker = tokio::spawn(async move { ai_api::process_once(&worker_state).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1200), &mut waiting)
            .await
            .is_err()
    );
    assert!(worker.await.unwrap().unwrap());
    assert_eq!(
        waiting.await.unwrap(),
        (StatusCode::OK, json!({"address_empty":true}))
    );
    assert_eq!(
        caller(public.clone(), idempotency).await.unwrap(),
        (StatusCode::OK, json!({"address_empty":true}))
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ai_api_runs")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );

    // Zero in either legacy setting must still wait, including time in the queue.
    input["response_wait_seconds"] = json!(0);
    request(
        &app,
        Method::PATCH,
        &management,
        "manager",
        Some(input),
        None,
    )
    .await;
    *output.write().await = json!({"status":"completed","result":{"address_empty":false}});
    for url in [public.clone(), format!("{public}?wait_seconds=0")] {
        let mut waiting = caller(url, Uuid::now_v7());
        wait_for_pending_api_run(&db).await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), &mut waiting)
                .await
                .is_err()
        );
        assert!(ai_api::process_once(&state).await.unwrap());
        assert_eq!(
            waiting.await.unwrap(),
            (StatusCode::OK, json!({"address_empty":false}))
        );
    }
    server.abort();
    let _ = tokio::fs::remove_dir_all(runtime_directory).await;
}
