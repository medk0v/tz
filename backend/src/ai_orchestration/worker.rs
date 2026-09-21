//! Persisted plan/execute/review transitions with bounded runtime leases.

use super::{
    AppState, CLAIM_LOCK, ExecutionRow, FromRow, MAX_ATTEMPTS, Postgres, Transaction, Utc, Uuid,
    Value, event, management, plan,
};
use crate::provider_reply::{
    TeamExecutionError, TeamExecutionInput, TeamExecutionOutput, execute_team_step,
};
use anyhow::{Context, Result};
use serde_json::json;
use std::{collections::HashMap, time::Duration};

#[derive(FromRow)]
struct StepJob {
    id: Uuid,
    execution_id: Uuid,
    agent_id: Uuid,
    kind: String,
    title: String,
    input_text: String,
    original_assignment: Option<String>,
    may_have_effects: bool,
    attempts: i32,
}

const AUTHORIZED_ATTEMPT_SQL: &str = r#"
    SELECT a.id FROM ai_task_step_attempts a
      JOIN ai_task_execution_steps s ON s.id=a.step_id AND s.execution_id=a.execution_id
      JOIN ai_task_executions e ON e.id=a.execution_id AND e.tenant_id=a.tenant_id AND e.project_id=a.project_id
      JOIN projects p ON p.tenant_id=e.tenant_id AND p.id=e.project_id
      JOIN ai_profiles profile ON profile.tenant_id=e.tenant_id AND (profile.project_id=e.project_id OR resource_visible(profile.visibility,e.project_id,NULL)) AND profile.id=s.agent_id
      JOIN ai_provider_connections provider ON provider.tenant_id=profile.tenant_id AND provider.id=profile.provider_connection_id
      WHERE a.id=$1 AND a.status='running' AND a.deadline_at>now() AND s.status='running'
        AND e.status IN ('queued','planning','running','reviewing') AND e.deadline_at>now()
        AND p.status='active' AND profile.status='active' AND provider.status='active'
        AND provider.provider_kind IN ('openai','openai_compatible')
"#;

struct Claimed {
    row: ExecutionRow,
    step: StepJob,
    attempt_id: Uuid,
    lease: Uuid,
}

/// Executes at most one ready step, releasing capacity between team members.
///
/// # Errors
/// Returns an error if durable queue state cannot be read or finalized.
pub async fn process_once(state: &AppState) -> Result<bool> {
    let maintained = maintain(state).await?;
    let Some(job) = claim(state).await? else {
        return Ok(maintained);
    };
    let lease = crate::ai_execution::Lease::new(&state.db, job.lease);
    let result = execute(state, &job).await;
    finish(state, &job, result).await?;
    lease.release().await?;
    Ok(true)
}

pub(crate) async fn attempt_is_authorized(state: &AppState, id: Uuid) -> Result<bool> {
    Ok(
        sqlx::query_scalar(&format!("SELECT EXISTS({AUTHORIZED_ATTEMPT_SQL})"))
            .bind(id)
            .fetch_one(&state.db)
            .await?,
    )
}

async fn claim(state: &AppState) -> Result<Option<Claimed>> {
    let mut tx = state.db.begin().await?;
    let Some(capacity) =
        crate::ai_execution::available(&mut tx, state.config.worker.ai_run_concurrency).await?
    else {
        tx.commit().await?;
        return Ok(None);
    };
    let step:Option<StepJob>=sqlx::query_as(r#"
        SELECT s.id,s.execution_id,s.agent_id,s.kind,s.title,s.input_text,s.may_have_effects,s.attempts,
          (SELECT original.input_text FROM ai_task_execution_steps original
             WHERE s.kind='work' AND s.revision=1 AND original.execution_id=s.execution_id
               AND original.step_key || '_revision'=s.step_key) AS original_assignment
        FROM ai_task_execution_steps s JOIN ai_task_executions e ON e.id=s.execution_id
        JOIN projects p ON p.tenant_id=e.tenant_id AND p.id=e.project_id
        JOIN ai_tasks task ON task.tenant_id=e.tenant_id AND task.id=e.task_id
        WHERE s.status='pending' AND s.agent_id IS NOT NULL AND s.available_at<=now() AND s.attempts<3
          AND NOT (s.agent_id=ANY($1))
          AND e.status IN ('queued','planning','running','reviewing') AND e.deadline_at>now() AND p.status='active'
          AND (e.started_at IS NOT NULL OR task.schedule->>'ends_at' IS NULL OR
            (task.schedule->>'ends_at')::timestamp AT TIME ZONE
              (task.schedule->>'timezone') >= statement_timestamp())
          AND NOT EXISTS(SELECT 1 FROM ai_task_step_dependencies d JOIN ai_task_execution_steps parent ON parent.id=d.depends_on
            WHERE d.step_id=s.id AND parent.status<>'succeeded')
        ORDER BY (SELECT count(*) FROM ai_task_step_attempts a WHERE a.project_id=e.project_id AND a.tenant_id=e.tenant_id AND a.status='running'),
          e.updated_at,s.available_at,s.created_at,s.id
        LIMIT 1 FOR UPDATE OF e SKIP LOCKED
    "#).bind(&capacity.blocked_profiles).fetch_optional(&mut *tx).await?;
    let Some(mut step) = step else {
        tx.commit().await?;
        return Ok(None);
    };
    let row: ExecutionRow =
        sqlx::query_as("SELECT * FROM ai_task_executions WHERE id=$1 FOR UPDATE")
            .bind(step.execution_id)
            .fetch_one(&mut *tx)
            .await?;
    // Parent first also matches task deletion/cascades outside the queue lock.
    sqlx::query("SELECT id FROM ai_task_execution_steps WHERE id=$1 FOR UPDATE")
        .bind(step.id)
        .execute(&mut *tx)
        .await?;
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ai_task_step_attempts WHERE execution_id=$1")
            .bind(row.id)
            .fetch_one(&mut *tx)
            .await?;
    if total >= row.snapshot.attempt_budget() {
        stop(
            &mut tx,
            &row,
            "needs_attention",
            "The execution attempt budget was reached",
        )
        .await?;
        tx.commit().await?;
        return Ok(None);
    }
    let attempt_id = Uuid::now_v7();
    let lease = Uuid::now_v7();
    crate::ai_execution::reserve(&mut tx, lease, Some(step.agent_id), None, 11 * 60).await?;
    step.attempts += 1;
    // Runtime timeout is nine minutes. The longer lease also covers grant cleanup.
    sqlx::query("INSERT INTO ai_task_step_attempts (id,tenant_id,project_id,execution_id,step_id,attempt_number,locked_by,deadline_at) VALUES ($1,$2,$3,$4,$5,$6,$7,now()+interval '11 minutes')")
        .bind(attempt_id).bind(row.tenant_id).bind(row.project_id).bind(row.id).bind(step.id).bind(step.attempts).bind(lease).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_task_execution_steps SET status='running',attempts=$2,started_at=COALESCE(started_at,now()),finished_at=NULL,error=NULL WHERE id=$1")
        .bind(step.id).bind(step.attempts).execute(&mut *tx).await?;
    let status = match step.kind.as_str() {
        "planning" => "planning",
        "review" => "reviewing",
        _ => "running",
    };
    sqlx::query("UPDATE ai_task_executions SET status=$2,started_at=COALESCE(started_at,now()),updated_at=now() WHERE id=$1")
        .bind(row.id).bind(status).execute(&mut *tx).await?;
    event(&mut tx, &row, "step.started", &step.title).await?;
    tx.commit().await?;
    Ok(Some(Claimed {
        row,
        step,
        attempt_id,
        lease,
    }))
}

async fn execute(
    state: &AppState,
    job: &Claimed,
) -> std::result::Result<TeamExecutionOutput, TeamExecutionError> {
    let profile = job
        .row
        .snapshot
        .profile(job.step.agent_id)
        .ok_or(TeamExecutionError::AgentUnavailable)?;
    let mut input = json!({"goal":job.row.snapshot.text,"expected_result":job.row.snapshot.expected_result,
        "assignment":job.step.input_text,"role":profile.role});
    if let Some(original) = &job.step.original_assignment {
        input["original_assignment"] = json!(original);
    }
    let (instructions, schema) = match job.step.kind.as_str() {
        "planning" => {
            let snapshot = &job.row.snapshot;
            input["team"] = json!(
                snapshot
                    .profiles
                    .iter()
                    .filter(|p| p.id != snapshot.coordinator_id)
                    .map(|p| json!({"agent_id":p.id,"name":p.name,"role":p.role,"kind":"agent"}))
                    .chain(
                        snapshot
                            .employees
                            .iter()
                            .filter(|e| e.id != snapshot.coordinator_id)
                            .map(|e| json!({"agent_id":e.id,"name":e.name,"role":e.role,"kind":"employee"}))
                    )
                    .collect::<Vec<_>>()
            );
            (plan::PLANNING_INSTRUCTIONS, plan::planning_schema())
        }
        "review" => {
            input["revision_round_available"] = json!(job.row.rework_round == 0);
            input["results"] = results(state, job, true, input.to_string().len()).await?;
            (
                plan::REVIEW_INSTRUCTIONS,
                plan::review_schema(job.row.snapshot.max_work_steps()),
            )
        }
        _ => {
            input["dependency_results"] =
                results(state, job, false, input.to_string().len()).await?;
            (plan::WORK_INSTRUCTIONS, plan::work_schema())
        }
    };
    let remaining = (job.row.deadline_at - Utc::now())
        .to_std()
        .map_err(|_| TeamExecutionError::Timeout)?;
    execute_team_step(
        state,
        TeamExecutionInput {
            attempt_id: job.attempt_id,
            operation_id: job.step.id,
            tenant_id: job.row.tenant_id,
            project_id: job.row.project_id,
            profile_id: job.step.agent_id,
            instructions,
            profile_instructions: Some(&profile.instructions),
            profile_tool_instructions: Some(&profile.tool_instructions),
            tool_permissions: Some(&profile.tool_permissions),
            input: &input,
            output_schema: &schema,
            timeout: remaining.min(Duration::from_secs(540)),
            allow_tools: job.step.kind == "work",
        },
    )
    .await
}

async fn results(
    state: &AppState,
    job: &Claimed,
    review: bool,
    base_bytes: usize,
) -> std::result::Result<Value, TeamExecutionError> {
    let rows:Vec<(String,String,String,Option<Value>)>=sqlx::query_as(r#"
        SELECT s.step_key,s.title,s.agent_name,s.result FROM ai_task_execution_steps s
        WHERE s.tenant_id=$1 AND s.project_id=$2 AND s.execution_id=$3 AND s.status='succeeded' AND s.kind='work'
          AND (($5 AND NOT EXISTS(SELECT 1 FROM ai_task_execution_steps revised WHERE revised.execution_id=s.execution_id
              AND revised.step_key=s.step_key || '_revision' AND revised.status='succeeded'))
            OR (NOT $5 AND EXISTS(SELECT 1 FROM ai_task_step_dependencies d WHERE d.step_id=$4 AND d.depends_on=s.id)))
        ORDER BY s.created_at,s.id
    "#).bind(job.row.tenant_id).bind(job.row.project_id).bind(job.row.id).bind(job.step.id).bind(review)
        .fetch_all(&state.db).await.map_err(|_|TeamExecutionError::ProviderFailed)?;
    let results: Vec<_> = rows
        .into_iter()
        .map(|(key, title, agent, result)| (key, title, agent, result.unwrap_or(Value::Null)))
        .collect();
    let sizes: Vec<usize> = results
        .iter()
        .map(|(.., value)| value.to_string().len())
        .collect();
    let cap = result_cap(&sizes, 200_000usize.saturating_sub(base_bytes));
    let items: Vec<_> = results
        .into_iter()
        .zip(sizes)
        .map(|((key, title, agent, mut value), size)| {
            let mut truncated = false;
            if size > cap {
                let output = value["output"].as_str().unwrap_or("");
                value = json!({"output":truncate_utf8(output,cap.saturating_sub(1000))});
                truncated = true;
            }
            json!({"step_key":key,"title":title,"agent":agent,"result":value,"truncated":truncated})
        })
        .collect();
    Ok(json!(items))
}

/// The largest size a single result may keep so that all of them together fit
/// the budget: results below it stay whole and leave their unused share to the
/// larger ones. It is never below an equal share, and nothing is cut when
/// everything fits.
fn result_cap(sizes: &[usize], budget: usize) -> usize {
    let mut sorted = sizes.to_vec();
    sorted.sort_unstable();
    let mut remaining = budget;
    for (index, size) in sorted.iter().enumerate() {
        let share = remaining / (sorted.len() - index);
        if *size > share {
            return share;
        }
        remaining -= size;
    }
    usize::MAX
}

fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

async fn finish(
    state: &AppState,
    job: &Claimed,
    result: std::result::Result<TeamExecutionOutput, TeamExecutionError>,
) -> Result<()> {
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CLAIM_LOCK)
        .execute(&mut *tx)
        .await?;
    let row: Option<ExecutionRow> =
        sqlx::query_as("SELECT * FROM ai_task_executions WHERE id=$1 FOR UPDATE")
            .bind(job.row.id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_task_step_attempts WHERE id=$1 AND locked_by=$2 AND status='running')")
        .bind(job.attempt_id).bind(job.lease).fetch_one(&mut *tx).await?;
    if !active {
        tx.commit().await?;
        return Ok(());
    }
    // Keep current access valid through the same commit that publishes output.
    let authorized = sqlx::query_scalar::<_, Uuid>(&format!(
        "{AUTHORIZED_ATTEMPT_SQL} FOR SHARE OF p,profile,provider"
    ))
    .bind(job.attempt_id)
    .fetch_optional(&mut *tx)
    .await?
    .is_some();
    if !authorized || !execution_is_active(&row.status) {
        sqlx::query("UPDATE ai_task_step_attempts SET status='cancelled',error_code='cancelled',finished_at=now() WHERE id=$1")
            .bind(job.attempt_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE ai_task_execution_steps SET status=$2,error='The execution is no longer authorized',finished_at=now() WHERE id=$1 AND status='running'")
            .bind(job.step.id).bind(interrupted_step_status(&row.status)).execute(&mut *tx).await?;
        if execution_is_active(&row.status) {
            stop(
                &mut tx,
                &row,
                "needs_attention",
                "The agent or project is no longer available for this execution",
            )
            .await?;
        }
        tx.commit().await?;
        return Ok(());
    }
    match result {
        Err(error) => {
            sqlx::query("UPDATE ai_task_step_attempts SET status='failed',error_code=$2,finished_at=now() WHERE id=$1")
                .bind(job.attempt_id).bind(error.code()).execute(&mut *tx).await?;
            let retryable = !job.step.may_have_effects
                && job.step.attempts < MAX_ATTEMPTS
                && matches!(
                    error,
                    TeamExecutionError::ProviderFailed
                        | TeamExecutionError::Timeout
                        | TeamExecutionError::ResultSchemaInvalid
                );
            let message = if job.step.may_have_effects {
                "The outcome of external actions must be verified before retrying".to_owned()
            } else {
                error.to_string()
            };
            if retryable {
                sqlx::query("UPDATE ai_task_execution_steps SET status='pending',error=$2,available_at=now()+interval '5 seconds' WHERE id=$1")
                    .bind(job.step.id).bind(&message).execute(&mut *tx).await?;
                event(&mut tx, &row, "step.retry_scheduled", &message).await?;
            } else {
                sqlx::query("UPDATE ai_task_execution_steps SET status='failed',error=$2,finished_at=now() WHERE id=$1")
                    .bind(job.step.id).bind(&message).execute(&mut *tx).await?;
                stop(&mut tx, &row, "needs_attention", &message).await?;
            }
        }
        Ok(output) => {
            sqlx::query("UPDATE ai_task_step_attempts SET status='succeeded',finished_at=now(),provider_kind=$2,model=$3,usage=$4 WHERE id=$1")
                .bind(job.attempt_id).bind(&output.provider_kind).bind(&output.model).bind(&output.usage).execute(&mut *tx).await?;
            let text = output
                .result
                .get(if job.step.kind == "review" {
                    "result"
                } else {
                    "output"
                })
                .and_then(Value::as_str)
                .unwrap_or("");
            sqlx::query("UPDATE ai_task_execution_steps SET status='succeeded',result=$2,result_text=$3,error=NULL,finished_at=now() WHERE id=$1")
                .bind(job.step.id).bind(&output.result).bind(text).execute(&mut *tx).await?;
            event(&mut tx, &row, "step.completed", &job.step.title).await?;
            match job.step.kind.as_str() {
                "planning" => match plan::validate(output.result, &row.snapshot) {
                    Ok(plan) => install_plan(&mut tx, &row, Some(job.step.id), plan).await?,
                    Err(error) => {
                        sqlx::query("UPDATE ai_task_execution_steps SET status='failed',error=$2 WHERE id=$1")
                            .bind(job.step.id).bind(error).execute(&mut *tx).await?;
                        stop(&mut tx, &row, "needs_attention", error).await?;
                    }
                },
                "review" => review(&mut tx, &row, job.step.id, &output.result).await?,
                _ => {
                    if output.result["status"] == "blocked" {
                        sqlx::query("UPDATE ai_task_execution_steps SET status='blocked',error=result_text WHERE id=$1")
                            .bind(job.step.id).execute(&mut *tx).await?;
                        stop(
                            &mut tx,
                            &row,
                            "needs_attention",
                            "An agent needs additional information or an unavailable capability",
                        )
                        .await?;
                    }
                }
            }
            activate_human_steps(&mut tx, &row).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Hands every ready employee step to its assignee and keeps the execution
/// status in line with the kind of work now waiting.
pub(super) async fn activate_human_steps(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
) -> Result<(), sqlx::Error> {
    let activated: Vec<(String, String)> = sqlx::query_as(
        r#"
        UPDATE ai_task_execution_steps AS s
        SET status = 'running', started_at = COALESCE(s.started_at, now()), finished_at = NULL, error = NULL
        FROM ai_task_executions AS e
        WHERE e.id = s.execution_id AND s.execution_id = $1 AND s.status = 'pending'
          AND s.assignee_user_id IS NOT NULL
          AND e.status IN ('queued', 'planning', 'running', 'reviewing')
          AND NOT EXISTS (
              SELECT 1 FROM ai_task_step_dependencies AS d
              JOIN ai_task_execution_steps AS parent ON parent.id = d.depends_on
              WHERE d.step_id = s.id AND parent.status <> 'succeeded')
        RETURNING s.kind, s.title
        "#,
    )
    .bind(row.id)
    .fetch_all(&mut **tx)
    .await?;
    for (kind, title) in &activated {
        let status = match kind.as_str() {
            "planning" => "planning",
            "review" => "reviewing",
            _ => "running",
        };
        sqlx::query("UPDATE ai_task_executions SET status=$2,started_at=COALESCE(started_at,now()),updated_at=now() WHERE id=$1 AND status IN ('queued','planning','running','reviewing')")
            .bind(row.id).bind(status).execute(&mut **tx).await?;
        event(tx, row, "step.awaiting_employee", title).await?;
    }
    Ok(())
}

/// Writes the work steps of a plan, their dependencies and the final review.
///
/// A plan the coordinator made hangs on its planning step and sets the team
/// running. A predefined plan has no planning step: its execution stays queued
/// until the first step is claimed or handed to an employee, so a schedule
/// that ends in the meantime can still cancel it.
pub(super) async fn install_plan(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    planning_id: Option<Uuid>,
    plan: plan::Plan,
) -> Result<()> {
    let mut ids = HashMap::new();
    for step in &plan.steps {
        let participant = row
            .snapshot
            .participant(step.agent_id)
            .context("validated plan participant missing")?;
        let id = management::insert_participant_step(
            tx,
            row,
            &step.key,
            &step.title,
            &participant,
            "work",
            &step.instructions,
            0,
        )
        .await?;
        ids.insert(step.key.as_str(), id);
    }
    for step in &plan.steps {
        if let Some(planning_id) = planning_id {
            dependency(tx, row, ids[step.key.as_str()], planning_id).await?;
        }
        for parent in &step.depends_on {
            dependency(tx, row, ids[step.key.as_str()], ids[parent.as_str()]).await?;
        }
    }
    let review = management::insert_coordinator_step(
        tx,
        row,
        "_review",
        "Review and combine the results",
        "review",
        0,
    )
    .await?;
    for id in ids.values() {
        dependency(tx, row, review, *id).await?;
    }
    if planning_id.is_none() {
        event(
            tx,
            row,
            "plan.predefined",
            "The steps defined in the task were assigned",
        )
        .await?;
        return Ok(());
    }
    sqlx::query("UPDATE ai_task_executions SET status='running',updated_at=now() WHERE id=$1")
        .bind(row.id)
        .execute(&mut **tx)
        .await?;
    event(
        tx,
        row,
        "plan.accepted",
        "The coordinator's plan was validated and assigned",
    )
    .await?;
    Ok(())
}

async fn dependency(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    step: Uuid,
    parent: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO ai_task_step_dependencies (tenant_id,project_id,execution_id,step_id,depends_on) VALUES ($1,$2,$3,$4,$5)")
        .bind(row.tenant_id).bind(row.project_id).bind(row.id).bind(step).bind(parent).execute(&mut **tx).await?;
    Ok(())
}

#[derive(FromRow)]
struct CorrectionSource {
    id: Uuid,
    step_key: String,
    agent_id: Option<Uuid>,
    assignee_user_id: Option<Uuid>,
    title: String,
    input_text: String,
    may_have_effects: bool,
    depends_on: Vec<Uuid>,
}

pub(super) async fn review(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    review_step: Uuid,
    result: &Value,
) -> Result<()> {
    let text = result["result"].as_str().unwrap_or("");
    if result["status"] == "complete" && !text.trim().is_empty() {
        sqlx::query("UPDATE ai_task_executions SET status='succeeded',result_text=$2,error=NULL,finished_at=now(),updated_at=now() WHERE id=$1")
            .bind(row.id).bind(text).execute(&mut **tx).await?;
        event(
            tx,
            row,
            "execution.completed",
            "The coordinator published the final result",
        )
        .await?;
        crate::task_board::mark_task_finished(&mut **tx, row.tenant_id, row.task_id).await?;
        return Ok(());
    }
    sqlx::query("UPDATE ai_task_executions SET result_text=$2 WHERE id=$1")
        .bind(row.id)
        .bind(text)
        .execute(&mut **tx)
        .await?;
    let revisions = result["revisions"].as_array().cloned().unwrap_or_default();
    if result["status"] == "needs_revision" && row.rework_round == 0 && !revisions.is_empty() {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM ai_task_execution_steps WHERE execution_id=$1",
        )
        .bind(row.id)
        .fetch_one(&mut **tx)
        .await?;
        let originals: Vec<CorrectionSource> = sqlx::query_as(
            r#"
            SELECT s.id,s.step_key,s.agent_id,s.assignee_user_id,s.title,s.input_text,s.may_have_effects,
              ARRAY(SELECT d.depends_on FROM ai_task_step_dependencies d
                JOIN ai_task_execution_steps parent ON parent.id=d.depends_on
                WHERE d.step_id=s.id AND parent.kind='work' AND parent.revision=0
                ORDER BY d.depends_on) AS depends_on
            FROM ai_task_execution_steps s WHERE s.execution_id=$1
              AND s.kind='work' AND s.status='succeeded' AND s.revision=0
            ORDER BY s.created_at,s.id
        "#,
        )
        .bind(row.id)
        .fetch_all(&mut **tx)
        .await?;
        let mut requested = Vec::new();
        let mut instructions_by_key = HashMap::new();
        for revision in revisions {
            let key = revision["step_key"].as_str().unwrap_or("");
            let instructions = revision["instructions"].as_str().unwrap_or("");
            if instructions.trim().is_empty() || instructions.chars().count() > 10000 {
                stop(
                    tx,
                    row,
                    "needs_attention",
                    "The coordinator requested an invalid correction",
                )
                .await?;
                return Ok(());
            }
            requested.push(key.to_owned());
            instructions_by_key.insert(key.to_owned(), instructions.to_owned());
        }
        let nodes: Vec<_> = originals
            .iter()
            .map(|source| plan::RevisionNode {
                id: source.id,
                key: source.step_key.clone(),
                depends_on: source.depends_on.clone(),
                may_have_effects: source.may_have_effects,
            })
            .collect();
        let remaining =
            usize::try_from(row.snapshot.step_budget().saturating_sub(count)).unwrap_or(0);
        let affected = match plan::revision_closure(&nodes, &requested, remaining) {
            Ok(affected) => affected,
            Err(error) => {
                stop(tx, row, "needs_attention", error).await?;
                return Ok(());
            }
        };
        let mut revised_ids = HashMap::new();
        for original in originals
            .iter()
            .filter(|source| affected.contains(&source.id))
        {
            let participant = original
                .agent_id
                .or(original.assignee_user_id)
                .and_then(|id| row.snapshot.participant(id))
                .context("revision participant missing")?;
            let instructions = instructions_by_key
                .get(&original.step_key)
                .unwrap_or(&original.input_text);
            let id = management::insert_participant_step(
                tx,
                row,
                &format!("{}_revision", original.step_key),
                &original.title,
                &participant,
                "work",
                instructions,
                1,
            )
            .await?;
            revised_ids.insert(original.id, id);
        }
        for original in originals
            .iter()
            .filter(|source| affected.contains(&source.id))
        {
            let revised = revised_ids[&original.id];
            // Keep the prior version for comparison, then route every upstream
            // dependency to its correction when one exists.
            dependency(tx, row, revised, original.id).await?;
            dependency(tx, row, revised, review_step).await?;
            for parent in &original.depends_on {
                dependency(tx, row, revised, *revised_ids.get(parent).unwrap_or(parent)).await?;
            }
        }
        let final_id = management::insert_coordinator_step(
            tx,
            row,
            "_review_revision",
            "Review the corrected result",
            "review",
            1,
        )
        .await?;
        for original in &originals {
            dependency(
                tx,
                row,
                final_id,
                *revised_ids.get(&original.id).unwrap_or(&original.id),
            )
            .await?;
        }
        sqlx::query("UPDATE ai_task_executions SET rework_round=1,status='running',updated_at=now() WHERE id=$1")
            .bind(row.id).execute(&mut **tx).await?;
        event(
            tx,
            row,
            "review.revision_requested",
            "The coordinator requested one bounded correction round",
        )
        .await?;
    } else {
        let reason = result["reason"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("The available work does not fully satisfy the requested result");
        stop(tx, row, "needs_attention", reason).await?;
    }
    Ok(())
}

pub(super) async fn stop(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    status: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    if execution_is_terminal(&row.status) {
        return Ok(());
    }
    sqlx::query("UPDATE ai_task_executions SET status=$2,error=$3,updated_at=now(),finished_at=CASE WHEN $2='failed' THEN now() ELSE finished_at END WHERE id=$1")
        .bind(row.id).bind(status).bind(message).execute(&mut **tx).await?;
    sqlx::query("UPDATE ai_task_execution_steps SET status='blocked',error='Waiting for the execution to be resumed' WHERE execution_id=$1 AND status='pending'")
        .bind(row.id).execute(&mut **tx).await?;
    event(tx, row, "execution.needs_attention", message).await?;
    Ok(())
}

async fn maintain(state: &AppState) -> Result<bool> {
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CLAIM_LOCK)
        .execute(&mut *tx)
        .await?;
    let abandoned:Option<(Uuid,Uuid,Uuid,bool,i32)>=sqlx::query_as("SELECT a.id,a.step_id,a.execution_id,s.may_have_effects,s.attempts FROM ai_task_step_attempts a JOIN ai_task_execution_steps s ON s.id=a.step_id JOIN ai_task_executions e ON e.id=a.execution_id WHERE a.status='running' AND a.deadline_at<=now() ORDER BY a.deadline_at LIMIT 1 FOR UPDATE OF e SKIP LOCKED")
        .fetch_optional(&mut *tx).await?;
    let mut changed = false;
    if let Some((attempt, step, execution, effects, attempts)) = abandoned {
        let row: ExecutionRow =
            sqlx::query_as("SELECT * FROM ai_task_executions WHERE id=$1 FOR UPDATE")
                .bind(execution)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("UPDATE ai_task_step_attempts SET status='unknown',error_code='lease_expired',finished_at=now() WHERE id=$1")
            .bind(attempt).execute(&mut *tx).await?;
        if execution_is_active(&row.status)
            && !effects
            && attempts < MAX_ATTEMPTS
            && row.deadline_at > Utc::now()
        {
            sqlx::query("UPDATE ai_task_execution_steps SET status='pending',available_at=now(),error='Recovered after the worker lease expired' WHERE id=$1")
                .bind(step).execute(&mut *tx).await?;
            event(
                &mut tx,
                &row,
                "step.recovered",
                "A safe interrupted step was queued again",
            )
            .await?;
        } else {
            sqlx::query("UPDATE ai_task_execution_steps SET status=$2,error='The previous attempt outcome is unknown',finished_at=now() WHERE id=$1 AND status='running'")
                .bind(step).bind(interrupted_step_status(&row.status)).execute(&mut *tx).await?;
            if execution_is_active(&row.status) {
                stop(&mut tx,&row,"needs_attention","The previous attempt outcome is unknown; verify external actions before continuing").await?;
            }
        }
        changed = true;
    }
    let schedule_ended: Option<ExecutionRow> = sqlx::query_as(
        r#"
        SELECT e.* FROM ai_task_executions e
        JOIN ai_tasks task ON task.tenant_id=e.tenant_id AND task.id=e.task_id
        WHERE e.status='queued' AND e.started_at IS NULL
          AND (task.schedule->>'ends_at')::timestamp AT TIME ZONE
            (task.schedule->>'timezone') < statement_timestamp()
        ORDER BY e.created_at LIMIT 1 FOR UPDATE OF e SKIP LOCKED
    "#,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = schedule_ended {
        stop(&mut tx, &row, "cancelled", "The task schedule has ended").await?;
        changed = true;
    }
    let expired:Option<ExecutionRow>=sqlx::query_as("SELECT e.* FROM ai_task_executions e JOIN projects p ON p.tenant_id=e.tenant_id AND p.id=e.project_id WHERE e.status IN ('queued','planning','running','reviewing','needs_attention') AND (e.deadline_at<=now() OR p.status<>'active') ORDER BY e.created_at LIMIT 1 FOR UPDATE OF e SKIP LOCKED")
        .fetch_optional(&mut *tx).await?;
    if let Some(row) = expired {
        stop(
            &mut tx,
            &row,
            "failed",
            "The execution deadline was reached or the project was paused",
        )
        .await?;
        changed = true;
    }
    // Prune complete execution trees; never remove results needed by an active team.
    sqlx::query("DELETE FROM ai_task_executions e WHERE e.id IN (SELECT id FROM ai_task_executions WHERE status IN ('succeeded','failed','cancelled') AND finished_at<now()-interval '30 days' ORDER BY finished_at LIMIT 20) AND NOT EXISTS(SELECT 1 FROM ai_task_step_attempts a WHERE a.execution_id=e.id AND a.status='running')")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(changed)
}

fn execution_is_active(status: &str) -> bool {
    matches!(status, "queued" | "planning" | "running" | "reviewing")
}

fn execution_is_terminal(status: &str) -> bool {
    matches!(status, "failed" | "succeeded" | "cancelled")
}

fn interrupted_step_status(parent_status: &str) -> &'static str {
    if matches!(parent_status, "cancelled" | "succeeded") {
        "cancelled"
    } else {
        "failed"
    }
}

#[cfg(test)]
mod tests {
    use super::result_cap;

    const BUDGET: usize = 197_000;

    /// What the results take once every one above the cap is cut to it.
    fn kept(sizes: &[usize], cap: usize) -> usize {
        sizes.iter().map(|size| (*size).min(cap)).sum()
    }

    #[test]
    fn results_that_fit_together_are_not_cut() {
        // A long plan of short confirmations around one real deliverable.
        let mut sizes = vec![200; 29];
        sizes.push(19_500);
        assert_eq!(result_cap(&sizes, BUDGET), usize::MAX);
        assert_eq!(result_cap(&[BUDGET], BUDGET), usize::MAX);
        assert_eq!(result_cap(&[], BUDGET), usize::MAX);
    }

    #[test]
    fn small_results_leave_their_share_to_the_large_ones() {
        let sizes = [100_000, 100_000, 1000];
        let cap = result_cap(&sizes, BUDGET);
        assert_eq!(cap, 98_000);
        assert_eq!(kept(&sizes, cap), BUDGET);
        // One byte more would not fit.
        assert!(kept(&sizes, cap + 1) > BUDGET);
        // The order of the results does not matter.
        assert_eq!(result_cap(&[1000, 100_000, 100_000], BUDGET), cap);
    }

    #[test]
    fn results_that_are_all_too_large_share_the_budget_equally() {
        assert_eq!(result_cap(&[150_000; 30], BUDGET), BUDGET / 30);
        assert_eq!(result_cap(&[BUDGET + 1], BUDGET), BUDGET);
        assert_eq!(result_cap(&[5, 5], 0), 0);
    }

    #[test]
    fn the_cap_is_never_below_an_equal_share_and_always_fits() {
        let mixes: [&[usize]; 4] = [
            &[1, 70_000, 70_000, 70_000],
            &[6566, 6567, 190_000],
            &[50_000, 60_000, 90_000, 100, 100],
            &[65_666, 65_667, 65_668],
        ];
        for sizes in mixes {
            let cap = result_cap(sizes, BUDGET);
            assert!(cap >= BUDGET / sizes.len(), "{sizes:?}");
            assert!(kept(sizes, cap) <= BUDGET, "{sizes:?}");
        }
    }
}
