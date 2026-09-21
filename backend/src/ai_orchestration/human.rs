//! Team steps performed by employees: their queue and submitted answers.

use super::{
    AppError, AppState, CLAIM_LOCK, DateTime, ExecutionResponse, ExecutionRow, FromRow, Json, Path,
    SqlJson, State, TeamSnapshot, Utc, Uuid, Value, event, management, plan, worker,
};
use crate::{auth::ActorContext, task_board};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MAX_OUTPUT_CHARS: usize = 30_000;
const MAX_RESULT_CHARS: usize = 40_000;
const MAX_REASON_CHARS: usize = 2_000;
const MAX_INSTRUCTION_CHARS: usize = 10_000;

#[derive(FromRow)]
struct WaitingRow {
    id: Uuid,
    execution_id: Uuid,
    task_id: Uuid,
    kind: String,
    title: String,
    input_text: String,
    original_assignment: Option<String>,
    started_at: Option<DateTime<Utc>>,
    snapshot: SqlJson<TeamSnapshot>,
    rework_round: i32,
}

#[derive(Serialize)]
struct MemberOption {
    id: Uuid,
    name: String,
    role: String,
    kind: &'static str,
}

#[derive(FromRow, Serialize)]
struct StepResult {
    step_key: String,
    title: String,
    participant: String,
    output: String,
}

#[derive(Serialize)]
pub(super) struct WaitingStep {
    id: Uuid,
    execution_id: Uuid,
    task_id: Uuid,
    task_text: String,
    expected_result: String,
    kind: String,
    title: String,
    assignment: String,
    original_assignment: Option<String>,
    started_at: Option<DateTime<Utc>>,
    revision_round_available: bool,
    /// Team members the coordinator can plan work for.
    members: Vec<MemberOption>,
    /// Prerequisite results for work, or every current result for a review.
    results: Vec<StepResult>,
}

#[derive(Serialize)]
pub(super) struct WaitingList {
    items: Vec<WaitingStep>,
}

/// Steps of running team executions that wait for the signed-in employee.
pub(super) async fn mine(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<WaitingList>, AppError> {
    let task_actor = task_board::task_actor(&actor)?;
    let rows: Vec<WaitingRow> = sqlx::query_as(
        r#"
        SELECT s.id, s.execution_id, e.task_id, s.kind, s.title, s.input_text,
               (SELECT original.input_text FROM ai_task_execution_steps AS original
                 WHERE s.kind = 'work' AND s.revision = 1 AND original.execution_id = s.execution_id
                   AND original.step_key || '_revision' = s.step_key) AS original_assignment,
               s.started_at, e.snapshot, e.rework_round
        FROM ai_task_execution_steps AS s
        JOIN ai_task_executions AS e ON e.id = s.execution_id
        WHERE s.tenant_id = $1 AND e.project_id = $2 AND s.assignee_user_id = $3 AND s.status = 'running'
          AND e.status IN ('queued', 'planning', 'running', 'reviewing')
        ORDER BY s.started_at, s.id
        LIMIT 100
        "#,
    )
    .bind(actor.tenant_id)
    .bind(task_actor.project_id)
    .bind(actor.actor_id)
    .fetch_all(&state.db)
    .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let snapshot = row.snapshot.0;
        let members = if row.kind == "planning" {
            snapshot
                .profiles
                .iter()
                .filter(|profile| profile.id != snapshot.coordinator_id)
                .map(|profile| MemberOption {
                    id: profile.id,
                    name: profile.name.clone(),
                    role: profile.role.clone(),
                    kind: "agent",
                })
                .chain(
                    snapshot
                        .employees
                        .iter()
                        .filter(|employee| employee.id != snapshot.coordinator_id)
                        .map(|employee| MemberOption {
                            id: employee.id,
                            name: employee.name.clone(),
                            role: employee.role.clone(),
                            kind: "employee",
                        }),
                )
                .collect()
        } else {
            Vec::new()
        };
        let results = if row.kind == "planning" {
            Vec::new()
        } else {
            sqlx::query_as(
                r#"
                SELECT s.step_key, s.title, s.agent_name AS participant, COALESCE(s.result_text, '') AS output
                FROM ai_task_execution_steps AS s
                WHERE s.execution_id = $1 AND s.status = 'succeeded' AND s.kind = 'work'
                  AND (($3 AND NOT EXISTS (
                          SELECT 1 FROM ai_task_execution_steps AS revised
                          WHERE revised.execution_id = s.execution_id
                            AND revised.step_key = s.step_key || '_revision' AND revised.status = 'succeeded'))
                       OR (NOT $3 AND EXISTS (
                          SELECT 1 FROM ai_task_step_dependencies AS d WHERE d.step_id = $2 AND d.depends_on = s.id)))
                ORDER BY s.created_at, s.id
                "#,
            )
            .bind(row.execution_id)
            .bind(row.id)
            .bind(row.kind == "review")
            .fetch_all(&state.db)
            .await?
        };
        items.push(WaitingStep {
            id: row.id,
            execution_id: row.execution_id,
            task_id: row.task_id,
            task_text: snapshot.text,
            expected_result: snapshot.expected_result,
            kind: row.kind,
            title: row.title,
            assignment: row.input_text,
            original_assignment: row.original_assignment,
            started_at: row.started_at,
            revision_round_available: row.rework_round == 0,
            members,
            results,
        });
    }
    Ok(Json(WaitingList { items }))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RevisionInput {
    step_key: String,
    instructions: String,
}

/// The answer of an employee: work output, a plan or a review decision.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Submission {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    steps: Option<Value>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    revisions: Vec<RevisionInput>,
}

fn bounded(value: Option<String>, maximum: usize, field: &str) -> Result<String, AppError> {
    let value = value.unwrap_or_default().trim().to_owned();
    if value.chars().count() > maximum {
        return Err(AppError::BadRequest(format!(
            "{field} must contain at most {maximum} characters"
        )));
    }
    Ok(value)
}

pub(super) async fn submit(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(step_id): Path<Uuid>,
    Json(input): Json<Submission>,
) -> Result<Json<ExecutionResponse>, AppError> {
    let task_actor = task_board::task_actor(&actor)?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CLAIM_LOCK)
        .execute(&mut *tx)
        .await?;
    let step: Option<(Uuid, String, String, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT execution_id, kind, title, assignee_user_id, status FROM ai_task_execution_steps WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(task_actor.project_id)
    .bind(step_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((execution_id, kind, title, assignee, status)) = step else {
        return Err(AppError::NotFound);
    };
    if assignee != Some(actor.actor_id) {
        return Err(AppError::NotFound);
    }
    let row: ExecutionRow =
        sqlx::query_as("SELECT * FROM ai_task_executions WHERE id=$1 FOR UPDATE")
            .bind(execution_id)
            .fetch_one(&mut *tx)
            .await?;
    if status != "running"
        || !matches!(
            row.status.as_str(),
            "queued" | "planning" | "running" | "reviewing"
        )
    {
        return Err(AppError::BadRequest(
            "This step is no longer waiting for your answer".into(),
        ));
    }
    match kind.as_str() {
        "work" => submit_work(&mut tx, &row, step_id, &title, input).await?,
        "planning" => submit_plan(&mut tx, &row, step_id, &title, input).await?,
        "review" => submit_review(&mut tx, &row, step_id, &title, input).await?,
        _ => return Err(AppError::BadRequest("Unsupported step".into())),
    }
    worker::activate_human_steps(&mut tx, &row).await?;
    sqlx::query("INSERT INTO audit_log (id,tenant_id,project_id,actor_id,action,resource_kind,resource_id,metadata) VALUES ($1,$2,$3,$4,'ai_task_execution.step_submitted','ai_task_execution',$5,'{}'::jsonb)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(row.project_id).bind(actor.actor_id).bind(row.id)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        management::load(&state, actor.tenant_id, row.project_id, execution_id).await?,
    ))
}

async fn complete_step(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &ExecutionRow,
    step_id: Uuid,
    title: &str,
    result: &Value,
    text: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE ai_task_execution_steps SET status='succeeded',result=$2,result_text=$3,error=NULL,finished_at=now() WHERE id=$1")
        .bind(step_id).bind(result).bind(text).execute(&mut **tx).await?;
    event(tx, row, "step.completed", title).await?;
    Ok(())
}

async fn submit_work(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &ExecutionRow,
    step_id: Uuid,
    title: &str,
    input: Submission,
) -> Result<(), AppError> {
    let output = bounded(input.output, MAX_OUTPUT_CHARS, "output")?;
    if output.is_empty() {
        return Err(AppError::BadRequest(
            "Describe the result of your work".into(),
        ));
    }
    let blocked = match input.status.as_deref() {
        None | Some("completed") => false,
        Some("blocked") => true,
        Some(_) => {
            return Err(AppError::BadRequest(
                "status must be completed or blocked".into(),
            ));
        }
    };
    let result = json!({"status": if blocked { "blocked" } else { "completed" },
        "output": output, "sources": [], "limitations": []});
    if blocked {
        sqlx::query("UPDATE ai_task_execution_steps SET status='blocked',result=$2,result_text=$3,error=$3,finished_at=now() WHERE id=$1")
            .bind(step_id).bind(&result).bind(&output).execute(&mut **tx).await?;
        worker::stop(
            tx,
            row,
            "needs_attention",
            "An employee needs additional information to continue",
        )
        .await?;
    } else {
        complete_step(tx, row, step_id, title, &result, &output).await?;
    }
    Ok(())
}

async fn submit_plan(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &ExecutionRow,
    step_id: Uuid,
    title: &str,
    input: Submission,
) -> Result<(), AppError> {
    let steps = input
        .steps
        .ok_or_else(|| AppError::BadRequest("Add at least one step to the plan".into()))?;
    let value = json!({ "steps": steps });
    let plan = plan::validate(value.clone(), &row.snapshot)
        .map_err(|error| AppError::BadRequest(error.to_owned()))?;
    let summary = plan
        .steps
        .iter()
        .map(|step| step.title.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    complete_step(tx, row, step_id, title, &value, &summary).await?;
    worker::install_plan(tx, row, Some(step_id), plan)
        .await
        .map_err(AppError::internal)?;
    Ok(())
}

async fn submit_review(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &ExecutionRow,
    step_id: Uuid,
    title: &str,
    input: Submission,
) -> Result<(), AppError> {
    let status = input.status.as_deref().unwrap_or("complete");
    let result = bounded(input.result, MAX_RESULT_CHARS, "result")?;
    let reason = bounded(input.reason, MAX_REASON_CHARS, "reason")?;
    if input.revisions.len() > row.snapshot.max_work_steps()
        || input.revisions.iter().any(|revision| {
            revision.instructions.trim().is_empty()
                || revision.instructions.chars().count() > MAX_INSTRUCTION_CHARS
        })
    {
        return Err(AppError::BadRequest(
            "Each correction needs instructions of at most 10000 characters".into(),
        ));
    }
    match status {
        "complete" if result.is_empty() => {
            return Err(AppError::BadRequest("Write the final result".into()));
        }
        "needs_revision" if row.rework_round != 0 || input.revisions.is_empty() => {
            return Err(AppError::BadRequest(
                "Select the steps to correct; only one correction round is available".into(),
            ));
        }
        "incomplete" if reason.is_empty() => {
            return Err(AppError::BadRequest(
                "Explain why the result is incomplete".into(),
            ));
        }
        "complete" | "needs_revision" | "incomplete" => {}
        _ => {
            return Err(AppError::BadRequest(
                "status must be complete, needs_revision or incomplete".into(),
            ));
        }
    }
    let value =
        json!({"status": status, "result": result, "reason": reason, "revisions": input.revisions});
    complete_step(tx, row, step_id, title, &value, &result).await?;
    worker::review(tx, row, step_id, &value)
        .await
        .map_err(AppError::internal)?;
    Ok(())
}
