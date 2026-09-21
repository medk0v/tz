//! Isolated, versioned scenario runs using the production context and tool wire protocol.
use super::*;
use crate::auth::ActorContext;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as RoutePath, State},
    routing::{delete as delete_route, get, post, put},
};
use sha2::{Digest, Sha256};

mod model;
mod recommendations;
mod runner;
mod seeds;
mod semantic_review;
mod sources;
#[cfg(test)]
mod tests;
use model::{
    Fixture, History, Scenario, ScenarioContact, Step, TestResult, ToolCall, evaluate_step, redact,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/profiles/{profile_id}/tests",
            get(list).post(save),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/tests/{scenario_id}",
            put(update).delete(delete),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/tests/{scenario_id}/runs",
            delete_route(clear_scenario_runs),
        )
        .route("/api/v1/ai/profiles/{profile_id}/test-seeds", post(seed))
        .route(
            "/api/v1/ai/profiles/{profile_id}/test-runs",
            get(runs).post(run).delete(clear_runs),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/test-runs/{run_id}",
            get(run_detail).delete(delete_run),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/test-runs/{run_id}/review",
            post(review),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/test-runs/{run_id}/recommendations",
            post(recommendations::generate),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/test-runs/{run_id}/recommendations/apply",
            post(sources::apply_recommendations),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/test-runs/{run_id}/sources/{source_key}",
            get(sources::get).patch(sources::update),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
}

async fn scope(state: &AppState, actor: &ActorContext, profile: Uuid) -> Result<Uuid, AppError> {
    ai_settings::require_management(actor)?;
    let project = crate::resource_visibility::owner_project(
        &state.db,
        actor,
        crate::resource_visibility::Resource::Profile,
        profile,
    )
    .await?;
    let mut tx = state.db.begin().await?;
    ai_settings::lock_profile_for_management_scope(&mut tx, actor, project, profile).await?;
    tx.commit().await?;
    Ok(project)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveRequest {
    scenarios: Vec<Scenario>,
}

async fn list(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath(profile): RoutePath<Uuid>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    let items = sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'revision',revision,'scenario',scenario) FROM ai_test_scenarios WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND deleted_at IS NULL ORDER BY created_at,id")
        .bind(actor.tenant_id).bind(project).bind(profile).fetch_all(&state.db).await?;
    Ok(Json(json!({"items":items})))
}

async fn save(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath(profile): RoutePath<Uuid>,
    Json(input): Json<SaveRequest>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    insert_scenarios(&state, &actor, project, profile, input.scenarios).await?;
    list(State(state), actor, RoutePath(profile)).await
}

async fn insert_scenarios(
    state: &AppState,
    actor: &ActorContext,
    project: Uuid,
    profile: Uuid,
    scenarios: Vec<Scenario>,
) -> Result<(), AppError> {
    if scenarios.is_empty() || scenarios.len() > 200 {
        return Err(AppError::BadRequest("submit 1 to 200 scenarios".into()));
    }
    for scenario in &scenarios {
        scenario
            .validate()
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
    }
    let mut tx = state.db.begin().await?;
    ai_settings::lock_profile_for_management_scope(&mut tx, actor, project, profile).await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_test_scenarios WHERE profile_id=$1 AND deleted_at IS NULL",
    )
    .bind(profile)
    .fetch_one(&mut *tx)
    .await?;
    if count + i64::try_from(scenarios.len()).unwrap_or(i64::MAX) > 1000 {
        return Err(AppError::BadRequest(
            "maximum 1000 scenarios per profile".into(),
        ));
    }
    for scenario in scenarios {
        let id = Uuid::now_v7();
        let value = serde_json::to_value(scenario).map_err(AppError::internal)?;
        let secrets = load_profile_secrets_for(state, actor.tenant_id, profile)
            .await
            .map_err(AppError::internal)?
            .into_values()
            .collect::<Vec<_>>();
        let value = redact(value, &secrets);
        sqlx::query("INSERT INTO ai_test_scenarios(id,tenant_id,project_id,profile_id,scenario) VALUES($1,$2,$3,$4,$5)")
            .bind(id).bind(actor.tenant_id).bind(project).bind(profile).bind(&value).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO ai_test_scenario_versions(scenario_id,revision,scenario) VALUES($1,1,$2)",
        )
        .bind(id)
        .bind(value)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    revision: i64,
    scenario: Scenario,
}

async fn update(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, id)): RoutePath<(Uuid, Uuid)>,
    Json(input): Json<UpdateRequest>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    input
        .scenario
        .validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let mut tx = state.db.begin().await?;
    let value = serde_json::to_value(input.scenario).map_err(AppError::internal)?;
    let secrets = load_profile_secrets_for(&state, actor.tenant_id, profile)
        .await
        .map_err(AppError::internal)?
        .into_values()
        .collect::<Vec<_>>();
    let value = redact(value, &secrets);
    let revision: Option<i64> = sqlx::query_scalar("UPDATE ai_test_scenarios SET scenario=$6,revision=revision+1,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4 AND revision=$5 AND deleted_at IS NULL RETURNING revision")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(id).bind(input.revision).bind(&value).fetch_optional(&mut *tx).await?;
    let revision = revision
        .ok_or_else(|| AppError::Conflict("scenario was changed or deleted; reload it".into()))?;
    sqlx::query(
        "INSERT INTO ai_test_scenario_versions(scenario_id,revision,scenario) VALUES($1,$2,$3)",
    )
    .bind(id)
    .bind(revision)
    .bind(&value)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id,"revision":revision,"scenario":value})))
}

async fn delete(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, id)): RoutePath<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, AppError> {
    let project = scope(&state, &actor, profile).await?;
    sqlx::query("UPDATE ai_test_scenarios SET deleted_at=now(),updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(id).execute(&state.db).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// One MVCC snapshot; no instructions or KB writes. Ciphertext digests detect credential rotations.
async fn snapshot(
    state: &AppState,
    tenant: Uuid,
    project: Uuid,
    profile: Uuid,
    skill_project: Uuid,
) -> Result<Value> {
    let mut tx = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let mut value: Value = sqlx::query_scalar(r#"
      SELECT jsonb_build_object(
        'profile',to_jsonb(p),
        'provider',(to_jsonb(pr)-'encrypted_api_key'-'api_key_nonce'-'key_version') || jsonb_build_object('credential_revision',md5(pr.encrypted_api_key::text)),
        'identities',COALESCE((SELECT jsonb_agg(to_jsonb(i) ORDER BY i.language) FROM ai_profile_public_identities i WHERE i.tenant_id=p.tenant_id AND i.ai_profile_id=p.id),'[]'),
        'channels',COALESCE((SELECT jsonb_agg(to_jsonb(c)-'credentials' ORDER BY c.id) FROM ai_profile_channel_connections a JOIN channel_connections c ON c.tenant_id=a.tenant_id AND c.id=a.channel_connection_id WHERE a.tenant_id=p.tenant_id AND a.ai_profile_id=p.id),'[]'),
        'knowledge',COALESCE((SELECT jsonb_agg(jsonb_build_object('article_id',a.id,'version',a.version,'knowledge_base',b.name,'title',a.title,'source_url',a.source_url,'content',a.body) ORDER BY a.id) FROM ai_profile_knowledge_bases x JOIN knowledge_bases b ON b.tenant_id=x.tenant_id AND b.id=x.knowledge_base_id JOIN knowledge_articles a ON a.tenant_id=b.tenant_id AND a.project_id=b.project_id AND a.knowledge_base_id=b.id WHERE x.tenant_id=p.tenant_id AND x.ai_profile_id=p.id AND b.project_id=$4 AND b.project_id=p.project_id AND b.status='active' AND a.status='published' AND length(trim(a.body))>0),'[]'),
        'integrations',COALESCE((SELECT jsonb_agg((to_jsonb(i)-'encrypted_token'-'token_nonce'-'key_version') || jsonb_build_object('credential_revision',md5(i.encrypted_token::text),'actions',(SELECT jsonb_agg(to_jsonb(a) ORDER BY a.position,a.id) FROM api_integration_actions a WHERE a.tenant_id=i.tenant_id AND a.integration_id=i.id)) ORDER BY i.id) FROM ai_profile_api_integrations x JOIN api_integrations i ON i.tenant_id=x.tenant_id AND i.id=x.integration_id AND i.project_id=p.project_id WHERE x.tenant_id=p.tenant_id AND x.ai_profile_id=p.id),'[]'),
        'inbox_runtime',COALESCE((SELECT jsonb_agg(jsonb_build_object('inbox_id',i.id,'status',i.status,'routing',to_jsonb(r),'telegram_credential_revision',md5(cr.encrypted_bot_token::text)) ORDER BY i.id) FROM inboxes i LEFT JOIN inbox_routing_policies r ON r.tenant_id=i.tenant_id AND r.project_id=i.project_id AND r.inbox_id=i.id LEFT JOIN inbox_telegram_credentials cr ON cr.tenant_id=i.tenant_id AND cr.project_id=i.project_id AND cr.inbox_id=i.id WHERE i.tenant_id=p.tenant_id AND i.project_id=p.project_id AND i.id IN (SELECT c.inbox_id FROM ai_profile_channel_connections a JOIN channel_connections c ON c.tenant_id=a.tenant_id AND c.id=a.channel_connection_id WHERE a.tenant_id=p.tenant_id AND a.ai_profile_id=p.id)),'[]'),
        'credential_revisions',COALESCE((SELECT jsonb_agg(jsonb_build_object('key',s.secret_key,'revision',md5(s.encrypted_value::text)) ORDER BY s.secret_key) FROM ai_profile_secrets s WHERE s.tenant_id=p.tenant_id AND s.ai_profile_id=p.id),'[]')
      ) FROM ai_profiles p LEFT JOIN ai_provider_connections pr ON pr.tenant_id=p.tenant_id AND pr.id=p.provider_connection_id
      WHERE p.tenant_id=$1 AND p.project_id=$2 AND p.id=$3
    "#).bind(tenant).bind(project).bind(profile).bind(skill_project).fetch_one(&mut *tx).await?;
    // Profiles and test scenarios retain their owner; skills belong to the workspace
    // requesting this execution, including when the profile is shared there.
    value["skill_project_id"] = json!(skill_project);
    // Legacy runs predate project-isolated knowledge and remain visible only in
    // their owner's workspace. New runs are read through this explicit scope.
    value["knowledge_project_id"] = json!(skill_project);
    value["skills"] = serde_json::to_value(
        ai_skills::load_profile_skills(&mut *tx, tenant, skill_project, profile).await?,
    )?;
    tx.commit().await?;
    // The runtime revision fingerprints the code that drives agent test runs so
    // stale runs are detectable. Default builds hash the source text directly.
    // Obfuscated (client / on-prem) builds hash compile-time digests instead, so
    // the source is not embedded verbatim in the shipped binary — see
    // `crate::obfs`. Both remain sensitive to any change in these files.
    #[cfg(not(feature = "obfuscate"))]
    let revision_inputs = json!([
        include_str!("../../provider_reply.rs"),
        include_str!("runner.rs"),
        include_str!("model.rs"),
        include_str!("semantic_review.rs"),
        include_str!("../../integrations.rs"),
        include_str!("../../../../infra/openclaw/bin/support-integration.mjs"),
        include_str!("../../../../infra/openclaw/bin/support-test-tool.mjs"),
        include_str!("../../../../infra/openclaw/bin/support-browser.mjs"),
        include_str!("../../../../infra/openclaw/bin/support-browser-runner.mjs"),
        include_str!("../../../../infra/openclaw/bin/support-browser-protocol.mjs"),
        include_str!("../../../../infra/openclaw/bin/support-curl.mjs"),
        include_str!("../../../../infra/openclaw/bin/support-public-http.mjs"),
        include_str!("../../../../infra/openclaw/bin/support-public-network.mjs")
    ]);
    #[cfg(feature = "obfuscate")]
    let revision_inputs = json!([
        crate::src_digest!("../../provider_reply.rs"),
        crate::src_digest!("runner.rs"),
        crate::src_digest!("model.rs"),
        crate::src_digest!("semantic_review.rs"),
        crate::src_digest!("../../integrations.rs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-integration.mjs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-test-tool.mjs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-browser.mjs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-browser-runner.mjs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-browser-protocol.mjs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-curl.mjs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-public-http.mjs"),
        crate::src_digest!("../../../../infra/openclaw/bin/support-public-network.mjs")
    ]);
    value["runtime_revision"] = json!(fingerprint(&revision_inputs));
    value["runtime_route"] = json!(state.config.openclaw.canonical_base_url());
    if state
        .config
        .openclaw
        .handles_provider(value["provider"]["base_url"].as_str().unwrap_or_default())
    {
        let config_path = state
            .config
            .openclaw
            .grant_directory
            .parent()
            .map(|root| root.join("state/openclaw.json"));
        value["runtime_model"] = if let Some(path) = config_path
            && let Ok(bytes) = tokio::fs::read(path).await
            && let Ok(config) = serde_json::from_slice::<Value>(&bytes)
        {
            let agent = config["agents"]["list"]
                .as_array()
                .and_then(|list| list.iter().find(|a| a["id"] == "support"));
            json!({"defaults":config["agents"]["defaults"]["model"],"agent":agent.map(|a|&a["model"]),"configuration_revision":fingerprint(&config)})
        } else {
            json!({"verification":"runtime configuration unavailable; configured alias and response model are recorded"})
        };
    }
    Ok(value)
}

fn fingerprint(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

async fn expire_runs(
    state: &AppState,
    tenant: Uuid,
    project: Uuid,
    profile: Uuid,
) -> Result<(), AppError> {
    sqlx::query("UPDATE ai_test_runs SET status='execution_error',finished_at=now(),result=jsonb_build_object('execution_errors',jsonb_build_array('test process interrupted or deadline exceeded')) WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND status='running' AND created_at < now()-interval '10 minutes'")
        .bind(tenant).bind(project).bind(profile).execute(&state.db).await?;
    Ok(())
}

async fn clear_runs(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath(profile): RoutePath<Uuid>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    let deleted = sqlx::query("DELETE FROM ai_test_runs WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND status <> 'running'")
        .bind(actor.tenant_id).bind(project).bind(profile).execute(&state.db).await?.rows_affected();
    Ok(Json(json!({"deleted": deleted})))
}

async fn clear_scenario_runs(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, scenario)): RoutePath<(Uuid, Uuid)>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    let deleted = sqlx::query("DELETE FROM ai_test_runs WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND scenario_id=$4 AND status <> 'running'")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(scenario).execute(&state.db).await?.rows_affected();
    Ok(Json(json!({"deleted": deleted})))
}

async fn delete_run(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, id)): RoutePath<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, AppError> {
    let project = scope(&state, &actor, profile).await?;
    let mut tx = state.db.begin().await?;
    let status: String = sqlx::query_scalar("SELECT status FROM ai_test_runs WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4 FOR UPDATE")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(id).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    if status == "running" {
        return Err(AppError::Conflict("test is still running".into()));
    }
    sqlx::query(
        "DELETE FROM ai_test_runs WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4",
    )
    .bind(actor.tenant_id)
    .bind(project)
    .bind(profile)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn runs(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath(profile): RoutePath<Uuid>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    expire_runs(&state, actor.tenant_id, project, profile).await?;
    let current = snapshot(
        &state,
        actor.tenant_id,
        project,
        profile,
        ai_settings::require_management(&actor)?,
    )
    .await
    .map_err(AppError::internal)?;
    let hash = fingerprint(&current);
    let items = sqlx::query_scalar::<_,Value>("SELECT (to_jsonb(r)-'snapshot'-'result') || jsonb_build_object('snapshot',jsonb_build_object('scenario',r.snapshot->'scenario'),'result',r.result-'trace','stale',r.fingerprint<>$4 OR r.scenario_revision<>s.revision OR s.deleted_at IS NOT NULL) FROM ai_test_runs r JOIN ai_test_scenarios s ON s.id=r.scenario_id WHERE r.tenant_id=$1 AND r.project_id=$2 AND r.profile_id=$3 AND COALESCE(r.snapshot->>'knowledge_project_id',r.project_id::text)=$5 ORDER BY r.created_at DESC LIMIT 50")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(hash).bind(ai_settings::require_management(&actor)?.to_string()).fetch_all(&state.db).await?;
    Ok(Json(json!({"items":items})))
}

async fn run_detail(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, id)): RoutePath<(Uuid, Uuid)>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    let value=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(r) FROM ai_test_runs r WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4 AND COALESCE(snapshot->>'knowledge_project_id',project_id::text)=$5")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(id).bind(ai_settings::require_management(&actor)?.to_string()).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    Ok(Json(value))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRequest {
    scenario_id: Uuid,
    // Cached clients may still send a mode; it no longer controls execution.
    #[serde(default, rename = "mode")]
    _legacy_mode: Option<String>,
}

async fn run(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath(profile): RoutePath<Uuid>,
    Json(input): Json<RunRequest>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    expire_runs(&state, actor.tenant_id, project, profile).await?;
    let (revision,scenario): (i64,Value) = sqlx::query_as("SELECT revision,scenario FROM ai_test_scenarios WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4 AND deleted_at IS NULL")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(input.scenario_id).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    let scenario: Scenario = serde_json::from_value(scenario).map_err(AppError::internal)?;
    scenario
        .validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let snapshot = snapshot(
        &state,
        actor.tenant_id,
        project,
        profile,
        ai_settings::require_management(&actor)?,
    )
    .await
    .map_err(AppError::internal)?;
    let hash = fingerprint(&snapshot);
    let secrets = load_profile_secrets_for(&state, actor.tenant_id, profile)
        .await
        .map_err(AppError::internal)?
        .into_values()
        .collect::<Vec<_>>();
    let mut stored = redact(snapshot.clone(), &secrets);
    stored["scenario"] = serde_json::to_value(&scenario).map_err(AppError::internal)?;
    let id = Uuid::now_v7();
    let inserted = sqlx::query("INSERT INTO ai_test_runs(id,tenant_id,project_id,profile_id,scenario_id,scenario_revision,mode,status,fingerprint,snapshot,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,'running',$8,$9,$10) ON CONFLICT DO NOTHING")
        .bind(id).bind(actor.tenant_id).bind(project).bind(profile).bind(input.scenario_id).bind(revision).bind("live_read_only").bind(hash).bind(stored).bind(actor.actor_id).execute(&state.db).await?;
    if inserted.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "another test for this agent is running".into(),
        ));
    }
    tokio::spawn(async move {
        let mut result = TestResult::default();
        let execution = tokio::time::timeout(
            Duration::from_secs(540),
            runner::execute(
                &state,
                &actor,
                project,
                profile,
                &scenario,
                &snapshot,
                &mut result,
            ),
        )
        .await;
        match execution {
            Ok(Ok(())) => {}
            Ok(Err(error)) => result.execution_errors.push(runner::public_error(&error)),
            Err(_) => result
                .execution_errors
                .push("test deadline exceeded (9 minutes)".into()),
        }
        let status = result.status();
        let result = redact(serde_json::to_value(result).unwrap_or_default(), &secrets);
        if let Err(error) = sqlx::query("UPDATE ai_test_runs SET status=$2,result=$3,finished_at=now() WHERE id=$1 AND status='running'").bind(id).bind(status).bind(result).execute(&state.db).await { warn!(%id,?error,"could not finish AI test run"); }
    });
    Ok(Json(json!({"id":id,"status":"running"})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    step: usize,
    verdict: String,
    note: String,
}
async fn review(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, id)): RoutePath<(Uuid, Uuid)>,
    Json(input): Json<Review>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    if !["pass", "fail", "manual_review"].contains(&input.verdict.as_str())
        || input.note.trim().is_empty()
        || input.note.len() > 2000
    {
        return Err(AppError::BadRequest(
            "a verdict and review explanation are required".into(),
        ));
    }
    let mut tx = state.db.begin().await?;
    let (status,mut result): (String,Value) = sqlx::query_as("SELECT status,result FROM ai_test_runs WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4 AND COALESCE(snapshot->>'knowledge_project_id',project_id::text)=$5 FOR UPDATE")
        .bind(actor.tenant_id).bind(project).bind(profile).bind(id).bind(ai_settings::require_management(&actor)?.to_string()).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    if status == "running" {
        return Err(AppError::Conflict("test is still running".into()));
    }
    let reviews = result["semantic_review"]
        .as_array_mut()
        .ok_or(AppError::NotFound)?;
    let item = reviews.get_mut(input.step).ok_or(AppError::NotFound)?;
    item["verdict"] = json!(input.verdict);
    item["note"] = json!(input.note);
    item["reviewer"] = json!(actor.actor_id);
    item["source"] = json!("human");
    let log = item.clone();
    if !result["review_history"].is_array() {
        result["review_history"] = json!([]);
    }
    result["review_history"]
        .as_array_mut()
        .unwrap()
        .push(json!({"step":input.step,"review":log,"at":Utc::now()}));
    let errors = result["execution_errors"]
        .as_array()
        .is_some_and(|a| !a.is_empty());
    let failures = result["failures"].as_array().is_some_and(|a| !a.is_empty());
    let reviews = result["semantic_review"].as_array().unwrap();
    let status = if errors {
        "execution_error"
    } else if failures || reviews.iter().any(|v| v["verdict"] == "fail") {
        "behavior_error"
    } else if !reviews.is_empty() && reviews.iter().all(|v| v["verdict"] == "pass") {
        "passed"
    } else {
        "manual_review"
    };
    sqlx::query("UPDATE ai_test_runs SET status=$2,result=$3 WHERE id=$1")
        .bind(id)
        .bind(status)
        .bind(&result)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"status":status,"result":result})))
}

async fn seed(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath(profile): RoutePath<Uuid>,
) -> Result<Json<Value>, AppError> {
    let project = scope(&state, &actor, profile).await?;
    let snapshot = snapshot(
        &state,
        actor.tenant_id,
        project,
        profile,
        ai_settings::require_management(&actor)?,
    )
    .await
    .map_err(AppError::internal)?;
    let scenarios = seeds::build(&snapshot).map_err(AppError::internal)?;
    if scenarios.is_empty() {
        return Err(AppError::BadRequest(
            "no assigned published knowledge articles to generate scenarios from".into(),
        ));
    }
    insert_scenarios(&state, &actor, project, profile, scenarios).await?;
    list(State(state), actor, RoutePath(profile)).await
}
