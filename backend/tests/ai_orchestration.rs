//! End-to-end contracts against a local deterministic provider and disposable DB.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::{Barrier, Mutex, Notify, Semaphore};
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_orchestration};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

struct Provider {
    answers: Mutex<VecDeque<Value>>,
    requests: Mutex<Vec<Value>>,
    blocked: AtomicBool,
    entered: Notify,
    release: Semaphore,
}

async fn provider(
    State(provider): State<Arc<Provider>>,
    headers: HeaderMap,
    Json(mut request): Json<Value>,
) -> Json<Value> {
    request["_idempotency_key"] = json!(headers.get("idempotency-key").unwrap().to_str().unwrap());
    request["_agent"] = json!(
        headers
            .get("x-openclaw-agent-id")
            .unwrap()
            .to_str()
            .unwrap()
    );
    provider.requests.lock().await.push(request);
    let answer = provider
        .answers
        .lock()
        .await
        .pop_front()
        .expect("unexpected provider invocation");
    if provider.blocked.load(Ordering::SeqCst) {
        provider.entered.notify_one();
        provider.release.acquire().await.unwrap().forget();
    }
    Json(json!({"choices":[{"finish_reason":"stop","message":{
        "role":"assistant","content":json!({"status":"completed","result":answer}).to_string()
    }}],"usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18,"private_debug":"do not persist"}}))
}

struct Fixture {
    state: AppState,
    app: Router,
    provider: Arc<Provider>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    runtime: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.runtime);
    }
}

async fn fixture(db: &PgPool, answers: Vec<Value>) -> Fixture {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Team tests'),('{foreign_tenant}','Other tenant');
        INSERT INTO users (id,email,display_name) VALUES
          ('{manager}','team-manager@example.test','Manager'),('{foreign_manager}','other-manager@example.test','Other manager');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES
          ('{project}','{tenant}','Team project','team-project'),
          ('{foreign_project}','{foreign_tenant}','Other project','other-project');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,is_system,permissions) VALUES
          ('{tenant}','{project}','manager','Manager','manager',false,ARRAY['ai:manage','tasks:own','tasks:manage']),
          ('{foreign_tenant}','{foreign_project}','manager','Manager','manager',false,ARRAY['ai:manage','tasks:own','tasks:manage']);
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role) VALUES
          ('{manager}','{tenant}','{project}','{manager}','manager'),
          ('{foreign_manager}','{foreign_tenant}','{foreign_project}','{foreign_manager}','manager');
        INSERT INTO ai_provider_connections (id,tenant_id,project_id,name,provider_kind,base_url,default_model,created_by) VALUES
          ('{provider}','{tenant}','{project}','Provider','openai_compatible','http://127.0.0.1:9/v1','test-model','{manager}');
        -- These test agents only analyze data and have no notification side effects.
        INSERT INTO ai_profiles (id,tenant_id,project_id,provider_connection_id,name,instructions,status,created_by,telegram_notify_on_operator_request) VALUES
          ('{coordinator}','{tenant}','{project}','{provider}','Coordinator','Coordinator baseline','active','{manager}',false),
          ('{researcher}','{tenant}','{project}','{provider}','Researcher','Researcher baseline','active','{manager}',false),
          ('{analyst}','{tenant}','{project}','{provider}','Analyst','Analyst baseline','active','{manager}',false),
          ('{unselected}','{tenant}','{project}','{provider}','Unselected','Not a team member','active','{manager}',false),
          ('{foreign_agent}','{foreign_tenant}','{foreign_project}',NULL,'Foreign agent','Private foreign instructions','draft','{foreign_manager}',false);
        INSERT INTO ai_tasks (id,tenant_id,project_id,text,expected_result,execution_mode,coordinator_id,schedule,created_by) VALUES
          ('{task}','{tenant}','{project}','Prepare a supported launch plan','A launch plan with sources and risks','team','{coordinator}','{{"kind":"manual"}}','{manager}');
        INSERT INTO ai_task_agents (tenant_id,task_id,ai_profile_id) VALUES
          ('{tenant}','{task}','{researcher}'),('{tenant}','{task}','{analyst}');
    "#,tenant=id(1),project=id(2),manager=id(3),coordinator=id(4),provider=id(5),
        researcher=id(6),analyst=id(7),unselected=id(8),task=id(20),foreign_tenant=id(101),
        foreign_project=id(102),foreign_manager=id(103),foreign_agent=id(104)))
        .execute(db).await.unwrap();
    for (token, tenant, project, user, scope) in [
        ("manager", id(1), id(2), id(3), None),
        ("scoped", id(1), id(2), id(3), Some(vec![id(90)])),
        ("foreign", id(101), id(102), id(103), None),
    ] {
        sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,inbox_scope,role,expires_at) VALUES ($1,$2,$3,$4,$5,$6,ARRAY['ai:manage','tasks:own','tasks:manage'],$7,'manager',now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(tenant).bind(project).bind(user).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(scope).execute(db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let runtime = std::env::temp_dir().join(format!("tzomet-team-tests-{}", Uuid::now_v7()));
    config.openclaw.base_url = Some(base_url.clone());
    config.openclaw.grant_directory = runtime.join("grants");
    config.openclaw.action_directory = runtime.join("actions");
    sqlx::query("UPDATE ai_provider_connections SET base_url=$1 WHERE id=$2")
        .bind(base_url)
        .bind(id(5))
        .execute(db)
        .await
        .unwrap();
    let provider_state = Arc::new(Provider {
        answers: Mutex::new(answers.into()),
        requests: Mutex::default(),
        blocked: AtomicBool::new(false),
        entered: Notify::new(),
        release: Semaphore::new(0),
    });
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(provider))
                .with_state(provider_state.clone()),
        )
        .into_future(),
    );
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = ai_orchestration::router().with_state(state.clone());
    Fixture {
        state,
        app,
        provider: provider_state,
        server,
        runtime,
    }
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    key: Option<Uuid>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(path);
    if !token.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(key) = key {
        request = request.header("idempotency-key", key.to_string());
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

fn start_path() -> String {
    format!("/api/v1/ai/tasks/{}/executions", id(20))
}
fn detail_path(execution: &Value) -> String {
    format!(
        "/api/v1/ai/task-executions/{}",
        execution["id"].as_str().unwrap()
    )
}

async fn start(fixture: &Fixture) -> Value {
    let (status, result) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(Uuid::now_v7()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{result}");
    result
}

fn plan() -> Value {
    json!({"steps":[
        {"key":"research","title":"Research requirements","agent_id":id(6),
            "instructions":"Collect evidence with sources","depends_on":[]},
        {"key":"analysis","title":"Analyze launch risks","agent_id":id(7),
            "instructions":"Use the research evidence to assess risk","depends_on":["research"]}
    ]})
}

fn work(output: &str) -> Value {
    json!({"status":"completed","output":output,"sources":[{"title":"Evidence","url":"https://example.test/source"}],"limitations":[]})
}

fn review() -> Value {
    json!({"status":"complete","result":"Verified launch plan: validate requirements, address risks, then launch.","reason":"","revisions":[]})
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn team_passes_dependency_results_to_distinct_profiles_and_collects_one_result(db: PgPool) {
    let fixture = fixture(
        &db,
        vec![
            plan(),
            work("RESEARCH_EVIDENCE_MARKER"),
            work("ANALYSIS_MARKER"),
            review(),
        ],
    )
    .await;
    let execution = start(&fixture).await;
    sqlx::query("UPDATE ai_profiles SET instructions='Changed after snapshot' WHERE id=$1")
        .bind(id(6))
        .execute(&db)
        .await
        .unwrap();
    for _ in 0..4 {
        assert!(
            ai_orchestration::process_once(&fixture.state)
                .await
                .unwrap()
        );
    }
    let (status, result) = request(
        &fixture.app,
        Method::GET,
        &detail_path(&execution),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(result["result_text"], review()["result"]);
    let steps = result["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 4);
    assert!(steps.iter().all(|step| step["status"] == "succeeded"));
    let research = steps
        .iter()
        .find(|step| step["agent_id"] == id(6).to_string())
        .unwrap();
    let analysis = steps
        .iter()
        .find(|step| step["agent_id"] == id(7).to_string())
        .unwrap();
    assert!(
        analysis["depends_on"]
            .as_array()
            .unwrap()
            .contains(&research["id"])
    );
    let requests = fixture.provider.requests.lock().await;
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|request| request["_agent"] == "tzomet" && request["model"] == "openclaw/tzomet")
    );
    assert_eq!(requests[0]["tool_choice"], "none");
    assert_eq!(requests[3]["tool_choice"], "none");
    assert!(
        requests[1]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Researcher baseline")
    );
    assert!(!requests[1].to_string().contains("Changed after snapshot"));
    assert!(
        requests[2]["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("RESEARCH_EVIDENCE_MARKER")
    );
    assert!(
        requests[3]["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("ANALYSIS_MARKER")
    );
    let keys: std::collections::HashSet<_> = requests
        .iter()
        .map(|request| request["_idempotency_key"].as_str().unwrap())
        .collect();
    assert_eq!(keys.len(), 4);
    drop(requests);
    let usage: Vec<Value> =
        sqlx::query_scalar("SELECT usage FROM ai_task_step_attempts WHERE execution_id=$1")
            .bind(execution["id"].as_str().unwrap().parse::<Uuid>().unwrap())
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        usage,
        vec![json!({"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}); 4]
    );
    assert!(
        !ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn manual_runs_enforce_scope_idempotency_and_revoke_active_specialist_grants(db: PgPool) {
    let fixture = fixture(
        &db,
        vec![plan(), work("Must not become a completed result")],
    )
    .await;
    for (token, expected) in [
        ("", StatusCode::UNAUTHORIZED),
        ("scoped", StatusCode::FORBIDDEN),
        ("foreign", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            request(
                &fixture.app,
                Method::POST,
                &start_path(),
                token,
                Some(Uuid::now_v7())
            )
            .await
            .0,
            expected
        );
    }
    assert_eq!(
        request(&fixture.app, Method::POST, &start_path(), "manager", None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let key = Uuid::now_v7();
    let (status, execution) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{execution}");
    let (status, replay) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], execution["id"]);
    assert_eq!(
        request(
            &fixture.app,
            Method::POST,
            &start_path(),
            "manager",
            Some(Uuid::now_v7())
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let path = detail_path(&execution);
    for (method, url) in [
        (Method::GET, path.clone()),
        (Method::POST, format!("{path}/cancel")),
        (Method::GET, start_path()),
    ] {
        assert_eq!(
            request(&fixture.app, method, &url, "foreign", None).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert!(
        ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
    fixture.provider.blocked.store(true, Ordering::SeqCst);
    let worker_state = fixture.state.clone();
    let worker = tokio::spawn(async move { ai_orchestration::process_once(&worker_state).await });
    tokio::time::timeout(Duration::from_secs(10), fixture.provider.entered.notified())
        .await
        .unwrap();
    assert!(
        std::fs::read_dir(fixture.runtime.join("grants"))
            .unwrap()
            .next()
            .is_some()
    );
    let (status, cancelled) = request(
        &fixture.app,
        Method::POST,
        &format!("{path}/cancel"),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "cancelled");
    assert!(
        tokio::time::timeout(Duration::from_secs(10), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    fixture.provider.release.add_permits(1);
    assert_eq!(
        std::fs::read_dir(fixture.runtime.join("grants"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(fixture.runtime.join("actions"))
            .unwrap()
            .count(),
        0
    );
    let (_, result) = request(&fixture.app, Method::GET, &path, "manager", None).await;
    assert_eq!(result["status"], "cancelled");
    assert_eq!(result["result_text"], Value::Null);
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ai_task_step_attempts WHERE status='running'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        active, 0,
        "the cancelled owner must release its lane after grant revocation"
    );
    assert!(
        !ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
    let (status, replay) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], execution["id"]);
    assert_eq!(replay["status"], "cancelled");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn invalid_plans_never_start_unknown_agents_or_unknown_dependencies(db: PgPool) {
    let mut outside = plan();
    outside["steps"][0]["agent_id"] = json!(id(8));
    let mut foreign = plan();
    foreign["steps"][0]["agent_id"] = json!(id(104));
    let mut unknown_dependency = plan();
    unknown_dependency["steps"][1]["depends_on"] = json!(["missing"]);
    let fixture = fixture(&db, vec![outside, foreign, unknown_dependency]).await;
    for call_count in 1..=3 {
        let execution = start(&fixture).await;
        assert!(
            ai_orchestration::process_once(&fixture.state)
                .await
                .unwrap()
        );
        let path = detail_path(&execution);
        let (_, result) = request(&fixture.app, Method::GET, &path, "manager", None).await;
        assert!(
            ["failed", "needs_attention"].contains(&result["status"].as_str().unwrap()),
            "{result}"
        );
        assert_eq!(
            result["steps"].as_array().unwrap().len(),
            1,
            "invalid plans must create no work steps"
        );
        assert_eq!(fixture.provider.requests.lock().await.len(), call_count);
        if result["status"] == "needs_attention" {
            assert_eq!(
                request(
                    &fixture.app,
                    Method::POST,
                    &format!("{path}/cancel"),
                    "manager",
                    None
                )
                .await
                .0,
                StatusCode::OK
            );
        }
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn a_research_correction_recomputes_downstream_analysis_before_publication(db: PgPool) {
    let correction = json!({"status":"needs_revision","result":"Initial analysis needs corrected evidence",
        "reason":"The source omits a launch requirement",
        "revisions":[{"step_key":"research","instructions":"Include the missing requirement and cite it"}]});
    let fixture = fixture(
        &db,
        vec![
            plan(),
            work("ORIGINAL_RESEARCH"),
            work("ORIGINAL_ANALYSIS"),
            correction,
            work("CORRECTED_RESEARCH"),
            work("CORRECTED_ANALYSIS"),
            review(),
        ],
    )
    .await;
    let execution = start(&fixture).await;
    for phase in 1..=7 {
        if !ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
        {
            let (_, detail) = request(
                &fixture.app,
                Method::GET,
                &detail_path(&execution),
                "manager",
                None,
            )
            .await;
            panic!("No runnable work at expected phase {phase}: {detail}");
        }
    }
    let (_, result) = request(
        &fixture.app,
        Method::GET,
        &detail_path(&execution),
        "manager",
        None,
    )
    .await;
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(result["result_text"], review()["result"]);
    let steps = result["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 7);
    assert!(steps.iter().all(|step| step["status"] == "succeeded"));
    for original in ["ORIGINAL_RESEARCH", "ORIGINAL_ANALYSIS"] {
        assert!(steps.iter().any(|step| step["result_text"] == original));
    }
    let corrected_research = steps
        .iter()
        .find(|step| step["result_text"] == "CORRECTED_RESEARCH")
        .unwrap();
    let corrected_analysis = steps
        .iter()
        .find(|step| step["result_text"] == "CORRECTED_ANALYSIS")
        .unwrap();
    assert!(
        corrected_analysis["depends_on"]
            .as_array()
            .unwrap()
            .contains(&corrected_research["id"])
    );

    let requests = fixture.provider.requests.lock().await;
    assert_eq!(requests.len(), 7);
    let input = |index: usize| {
        serde_json::from_str::<Value>(requests[index]["messages"][1]["content"].as_str().unwrap())
            .unwrap()["input"]
            .clone()
    };
    let research_input = input(4);
    assert_eq!(
        research_input["assignment"],
        "Include the missing requirement and cite it"
    );
    assert_eq!(
        research_input["original_assignment"],
        "Collect evidence with sources"
    );
    let analysis_input = input(5);
    assert_eq!(
        analysis_input["original_assignment"],
        "Use the research evidence to assess risk"
    );
    let dependencies = analysis_input["dependency_results"].as_array().unwrap();
    assert!(
        dependencies
            .iter()
            .any(|dependency| dependency["step_key"] == "research_revision"
                && dependency["result"]["output"] == "CORRECTED_RESEARCH")
    );
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency["step_key"] == "research")
    );
    let final_input = input(6);
    assert_eq!(final_input["revision_round_available"], false);
    let final_results = final_input["results"].as_array().unwrap();
    assert_eq!(final_results.len(), 2);
    assert!(
        final_results
            .iter()
            .all(|entry| entry["step_key"].as_str().unwrap().ends_with("_revision"))
    );
    assert!(
        final_results
            .iter()
            .any(|entry| entry["result"]["output"] == "CORRECTED_ANALYSIS")
    );
    drop(requests);
    assert!(
        !ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn cancellation_racing_multiple_claims_preserves_terminal_state_and_releases_capacity(
    db: PgPool,
) {
    let fixture = fixture(&db, vec![plan()]).await;
    let execution = start(&fixture).await;
    fixture.provider.blocked.store(true, Ordering::SeqCst);
    let barrier = Arc::new(Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let state = fixture.state.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                ai_orchestration::process_once(&state).await
            })
        })
        .collect();
    let app = fixture.app.clone();
    let path = detail_path(&execution);
    let cancel_path = format!("{path}/cancel");
    let cancellation = tokio::spawn(async move {
        barrier.wait().await;
        request(&app, Method::POST, &cancel_path, "manager", None).await
    });
    let (status, cancelled) = tokio::time::timeout(Duration::from_secs(10), cancellation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["status"], "cancelled");
    fixture.provider.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(10), async {
        for worker in workers {
            worker.await.unwrap().unwrap();
        }
    })
    .await
    .unwrap();
    let (_, after) = request(&fixture.app, Method::GET, &path, "manager", None).await;
    assert_eq!(after["status"], "cancelled", "{after}");
    assert_eq!(after["result_text"], Value::Null);
    assert!(
        after["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step["status"] == "cancelled")
    );
    assert!(
        fixture.provider.requests.lock().await.len() <= 1,
        "the shared OpenClaw lane must admit at most one concurrent provider call"
    );
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ai_task_step_attempts WHERE status='running'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(active, 0);
    assert!(
        !ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
}

async fn expire_task_schedule(db: &PgPool) {
    sqlx::query("UPDATE ai_tasks SET schedule=jsonb_build_object('kind','daily','time','09:00','timezone','Europe/Istanbul','ends_at',to_char((now()-interval '1 hour') AT TIME ZONE 'Europe/Istanbul','YYYY-MM-DD\"T\"HH24:MI:SS')) WHERE id=$1")
        .bind(id(20)).execute(db).await.unwrap();
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn schedule_cutoff_skips_missed_occurrences_for_both_execution_modes(db: PgPool) {
    let fixture = fixture(&db, vec![]).await;
    expire_task_schedule(&db).await;
    for mode in ["team", "independent"] {
        sqlx::query("UPDATE ai_tasks SET execution_mode=$2,coordinator_id=CASE WHEN $2='team' THEN coordinator_id ELSE NULL END,next_run_at=now()-interval '2 hours' WHERE id=$1")
            .bind(id(20)).bind(mode).execute(&db).await.unwrap();
        assert!(
            tz_backend::ai_tasks::process_once(&fixture.state)
                .await
                .unwrap()
        );
        let next: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT next_run_at FROM ai_tasks WHERE id=$1")
                .bind(id(20))
                .fetch_one(&db)
                .await
                .unwrap();
        assert!(next.is_none());
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM ai_task_runs)+(SELECT count(*) FROM ai_task_executions)",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(count, 0);
    assert!(fixture.provider.requests.lock().await.is_empty());
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn schedule_cutoff_cancels_queued_team_and_rejects_new_manual_start(db: PgPool) {
    let fixture = fixture(&db, vec![]).await;
    let key = Uuid::now_v7();
    let (status, execution) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    expire_task_schedule(&db).await;
    assert!(
        ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
    let (_, result) = request(
        &fixture.app,
        Method::GET,
        &detail_path(&execution),
        "manager",
        None,
    )
    .await;
    assert_eq!(result["status"], "cancelled");
    assert!(result["started_at"].is_null());
    let (status, _) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(Uuid::now_v7()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, result) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(key),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["id"], execution["id"]);
    assert!(fixture.provider.requests.lock().await.is_empty());
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn schedule_cutoff_allows_an_already_started_team_to_finish(db: PgPool) {
    let fixture = fixture(
        &db,
        vec![plan(), work("Research"), work("Analysis"), review()],
    )
    .await;
    let execution = start(&fixture).await;
    assert!(
        ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
    expire_task_schedule(&db).await;
    for _ in 0..3 {
        assert!(
            ai_orchestration::process_once(&fixture.state)
                .await
                .unwrap()
        );
    }
    let (_, result) = request(
        &fixture.app,
        Method::GET,
        &detail_path(&execution),
        "manager",
        None,
    )
    .await;
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(fixture.provider.requests.lock().await.len(), 4);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn schedule_cutoff_cancels_independent_queue_and_retries_but_keeps_active_lease(db: PgPool) {
    let fixture = fixture(&db, vec![]).await;
    sqlx::query("UPDATE ai_tasks SET execution_mode='independent',coordinator_id=NULL WHERE id=$1")
        .bind(id(20))
        .execute(&db)
        .await
        .unwrap();
    expire_task_schedule(&db).await;
    for (number, status, attempts, lock_age) in [
        (30, "pending", 0, 0),
        (31, "pending", 1, 0),
        (32, "processing", 1, 11),
        (33, "processing", 1, 0),
    ] {
        sqlx::query("INSERT INTO ai_task_runs (id,tenant_id,project_id,task_id,ai_profile_id,agent_name,task_text,scheduled_for,available_at,status,attempts,locked_at,locked_by) VALUES ($1,$2,$3,$4,$5,'Researcher','Work',now()+($6*interval '1 second'),now()+interval '1 hour',$7,$8,CASE WHEN $7='processing' THEN now()-($9*interval '1 minute') END,CASE WHEN $7='processing' THEN $1 END)")
            .bind(id(number)).bind(id(1)).bind(id(2)).bind(id(20)).bind(id(6)).bind(i64::try_from(number).unwrap()).bind(status).bind(attempts).bind(lock_age).execute(&db).await.unwrap();
    }
    for _ in 0..3 {
        assert!(
            tz_backend::ai_tasks::process_once(&fixture.state)
                .await
                .unwrap()
        );
    }
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,status FROM ai_task_runs ORDER BY id")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            (id(30), "cancelled".into()),
            (id(31), "cancelled".into()),
            (id(32), "cancelled".into()),
            (id(33), "processing".into())
        ]
    );
    let app = tz_backend::ai_tasks::router().with_state(fixture.state.clone());
    let path = format!("/api/v1/ai/tasks/{}/runs", id(20));
    let (status, _) = request(&app, Method::POST, &path, "manager", Some(Uuid::now_v7())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(fixture.provider.requests.lock().await.is_empty());
}

/// Gives the fixture's task a predefined plan: a chain of steps that the
/// researcher and the analyst perform in turns.
async fn predefine_plan(db: &PgPool, steps: u128) {
    let plan: Vec<Value> = (1..=steps)
        .map(|number| {
            json!({"id":id(900 + number),"title":format!("Step {number}"),
                "instructions":format!("Do part {number}"),
                "performer":{"kind":"agent","id":id(if number % 2 == 1 { 6 } else { 7 })}})
        })
        .collect();
    sqlx::query("UPDATE ai_tasks SET plan_steps=$2 WHERE id=$1")
        .bind(id(20))
        .bind(json!(plan))
        .execute(db)
        .await
        .unwrap();
}

async fn schedule_task_daily_and_make_it_due(db: &PgPool) {
    sqlx::query("UPDATE ai_tasks SET schedule=jsonb_build_object('kind','daily','time','09:00','timezone','Europe/Istanbul'),next_run_at=now()-interval '1 minute' WHERE id=$1")
        .bind(id(20)).execute(db).await.unwrap();
}

async fn next_occurrence_is_ahead(db: &PgPool) -> bool {
    sqlx::query_scalar::<_, Option<bool>>("SELECT next_run_at>now() FROM ai_tasks WHERE id=$1")
        .bind(id(20))
        .fetch_one(db)
        .await
        .unwrap()
        .unwrap_or(false)
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn a_long_predefined_chain_is_corrected_from_its_first_step_within_its_own_budget(
    db: PgPool,
) {
    // Ten steps, their review, ten corrections and the final review make 22
    // step rows: more than a plan made by the coordinator may ever hold.
    let correction = json!({"status":"needs_revision","result":"The first part misses an input",
        "reason":"Everything after it must be redone",
        "revisions":[{"step_key":id(901).to_string(),"instructions":"Redo part 1 with the missing input"}]});
    let mut answers: Vec<Value> = (1..=10)
        .map(|number| work(&format!("ORIGINAL_{number}")))
        .collect();
    answers.push(correction);
    answers.extend((1..=10).map(|number| work(&format!("CORRECTED_{number}"))));
    answers.push(review());
    let fixture = fixture(&db, answers).await;
    predefine_plan(&db, 10).await;
    let execution = start(&fixture).await;
    assert_eq!(execution["status"], "queued", "{execution}");
    let kinds = |execution: &Value, kind: &str| {
        execution["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|step| step["kind"] == kind)
            .count()
    };
    assert_eq!(kinds(&execution, "planning"), 0, "{execution}");
    assert_eq!(kinds(&execution, "work"), 10);
    assert_eq!(kinds(&execution, "review"), 1);
    for phase in 1..=22 {
        if !ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
        {
            let (_, detail) = request(
                &fixture.app,
                Method::GET,
                &detail_path(&execution),
                "manager",
                None,
            )
            .await;
            panic!("No runnable work at expected phase {phase}: {detail}");
        }
    }
    let (_, result) = request(
        &fixture.app,
        Method::GET,
        &detail_path(&execution),
        "manager",
        None,
    )
    .await;
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(result["result_text"], review()["result"]);
    let steps = result["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 22);
    assert!(steps.iter().all(|step| step["status"] == "succeeded"));
    assert_eq!(kinds(&result, "planning"), 0);
    let events = result["events"].as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event["event_type"] == "plan.predefined")
    );
    assert!(
        !events
            .iter()
            .any(|event| event["event_type"] == "plan.accepted")
    );

    let requests = fixture.provider.requests.lock().await;
    assert_eq!(requests.len(), 22);
    let input = |index: usize| {
        serde_json::from_str::<Value>(requests[index]["messages"][1]["content"].as_str().unwrap())
            .unwrap()["input"]
            .clone()
    };
    // The very first call already is the first step of the task.
    assert_eq!(input(0)["assignment"], "Do part 1");
    assert!(input(0).get("team").is_none());
    assert_eq!(
        input(1)["dependency_results"][0]["step_key"],
        id(901).to_string()
    );
    let corrected_first = input(11);
    assert_eq!(
        corrected_first["assignment"],
        "Redo part 1 with the missing input"
    );
    assert_eq!(corrected_first["original_assignment"], "Do part 1");
    let corrected_second = input(12);
    assert_eq!(corrected_second["assignment"], "Do part 2");
    assert_eq!(corrected_second["original_assignment"], "Do part 2");
    assert!(
        corrected_second["dependency_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |dependency| dependency["step_key"] == format!("{}_revision", id(901))
                    && dependency["result"]["output"] == "CORRECTED_1"
            )
    );
    let final_input = input(21);
    assert_eq!(final_input["revision_round_available"], false);
    let final_results = final_input["results"].as_array().unwrap();
    assert_eq!(final_results.len(), 10);
    assert!(
        final_results
            .iter()
            .all(|entry| entry["step_key"].as_str().unwrap().ends_with("_revision"))
    );
    drop(requests);
    assert!(
        !ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn a_scheduled_occurrence_is_seeded_from_the_plan_and_obeys_the_schedule_cutoff(db: PgPool) {
    let fixture = fixture(&db, vec![]).await;
    predefine_plan(&db, 3).await;
    schedule_task_daily_and_make_it_due(&db).await;
    assert!(
        tz_backend::ai_tasks::process_once(&fixture.state)
            .await
            .unwrap()
    );
    let (execution, status, started): (Uuid, String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT id,status,started_at FROM ai_task_executions WHERE task_id=$1 AND scheduled_for IS NOT NULL",
        )
        .bind(id(20))
        .fetch_one(&db)
        .await
        .unwrap();
    // Seeded and waiting for the first claim, exactly like a team that still has to plan.
    assert_eq!(status, "queued");
    assert!(started.is_none());
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM ai_task_execution_steps WHERE execution_id=$1 ORDER BY kind",
    )
    .bind(execution)
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(kinds, ["review", "work", "work", "work"]);
    assert!(next_occurrence_is_ahead(&db).await);

    expire_task_schedule(&db).await;
    assert!(
        ai_orchestration::process_once(&fixture.state)
            .await
            .unwrap()
    );
    let (_, result) = request(
        &fixture.app,
        Method::GET,
        &format!("/api/v1/ai/task-executions/{execution}"),
        "manager",
        None,
    )
    .await;
    assert_eq!(result["status"], "cancelled", "{result}");
    assert!(result["started_at"].is_null());
    assert!(fixture.provider.requests.lock().await.is_empty());
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn a_scheduled_team_that_cannot_start_leaves_a_failed_trace_and_the_schedule_moves_on(
    db: PgPool,
) {
    let fixture = fixture(&db, vec![]).await;
    // A predefined step whose performer no longer belongs to the team.
    predefine_plan(&db, 2).await;
    sqlx::query("DELETE FROM ai_task_agents WHERE task_id=$1 AND ai_profile_id=$2")
        .bind(id(20))
        .bind(id(7))
        .execute(&db)
        .await
        .unwrap();
    schedule_task_daily_and_make_it_due(&db).await;
    assert!(
        tz_backend::ai_tasks::process_once(&fixture.state)
            .await
            .unwrap()
    );
    assert!(next_occurrence_is_ahead(&db).await);
    // The occurrence is not attempted again and the scheduler is free for other tasks.
    assert!(
        !tz_backend::ai_tasks::process_once(&fixture.state)
            .await
            .unwrap()
    );
    let (status, list) = request(&fixture.app, Method::GET, &start_path(), "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{list}");
    assert_eq!(items[0]["status"], "failed");
    assert_eq!(
        items[0]["error"],
        "A step of the plan is assigned to somebody outside this team"
    );
    assert!(items[0]["finished_at"].is_string());
    assert!(items[0]["steps"].as_array().unwrap().is_empty());
    assert!(
        items[0]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event_type"] == "execution.not_started")
    );
    let scheduled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_task_executions WHERE task_id=$1 AND scheduled_for IS NOT NULL",
    )
    .bind(id(20))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(scheduled, 1);

    // The same holds for a team whose coordinator plans the work.
    sqlx::query("UPDATE ai_tasks SET plan_steps=NULL WHERE id=$1")
        .bind(id(20))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM ai_task_agents WHERE task_id=$1")
        .bind(id(20))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE ai_tasks SET next_run_at=now()-interval '1 second' WHERE id=$1")
        .bind(id(20))
        .execute(&db)
        .await
        .unwrap();
    assert!(
        tz_backend::ai_tasks::process_once(&fixture.state)
            .await
            .unwrap()
    );
    assert!(next_occurrence_is_ahead(&db).await);
    let errors: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT error FROM ai_task_executions WHERE task_id=$1 AND status='failed' ORDER BY created_at,id",
    )
    .bind(id(20))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(
        errors[1].as_deref(),
        Some("Select 1 to 8 team members distinct from the coordinator")
    );
    assert!(fixture.provider.requests.lock().await.is_empty());
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn a_predefined_plan_may_involve_more_members_than_a_coordinator_could_plan_for(db: PgPool) {
    let fixture = fixture(&db, vec![]).await;
    // Nine more agents join the researcher and the analyst: eleven members.
    sqlx::query("INSERT INTO ai_profiles (id,tenant_id,project_id,provider_connection_id,name,instructions,status,created_by,telegram_notify_on_operator_request) SELECT ('00000000-0000-0000-0000-0000000005' || lpad(n::text,2,'0'))::uuid,$1,$2,$3,'Extra ' || n,'Extra baseline','active',$4,false FROM generate_series(1,9) AS n")
        .bind(id(1)).bind(id(2)).bind(id(5)).bind(id(3)).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO ai_task_agents (tenant_id,task_id,ai_profile_id) SELECT $1,$2,id FROM ai_profiles WHERE name LIKE 'Extra %'")
        .bind(id(1)).bind(id(20)).execute(&db).await.unwrap();
    let performers: Vec<Uuid> = (1..=9)
        .map(|number| id(0x500 + number))
        .chain([id(6), id(7)])
        .collect();
    let plan: Vec<Value> = performers
        .iter()
        .zip(1_u128..)
        .map(|(performer, number)| {
            json!({"id":id(900 + number),"title":format!("Step {number}"),
                "instructions":format!("Do part {number}"),
                "performer":{"kind":"agent","id":performer}})
        })
        .collect();
    sqlx::query("UPDATE ai_tasks SET plan_steps=$2 WHERE id=$1")
        .bind(id(20))
        .bind(json!(plan))
        .execute(&db)
        .await
        .unwrap();
    let execution = start(&fixture).await;
    let steps = execution["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 12, "{execution}");
    let agents: std::collections::HashSet<&str> = steps
        .iter()
        .filter(|step| step["kind"] == "work")
        .map(|step| step["agent_id"].as_str().unwrap())
        .collect();
    assert_eq!(agents.len(), 11);

    // Without the plan the same team is too large for a coordinator to plan for.
    let (status, _) = request(
        &fixture.app,
        Method::POST,
        &format!("{}/cancel", detail_path(&execution)),
        "manager",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE ai_tasks SET plan_steps=NULL WHERE id=$1")
        .bind(id(20))
        .execute(&db)
        .await
        .unwrap();
    let (status, error) = request(
        &fixture.app,
        Method::POST,
        &start_path(),
        "manager",
        Some(Uuid::now_v7()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert!(fixture.provider.requests.lock().await.is_empty());
}
