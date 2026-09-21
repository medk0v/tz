//! Admission, inspection, cancellation and safe retries of team executions.

use super::{
    ActorContext, AgentRole, AppError, AppState, CLAIM_LOCK, DateTime, EmployeeSnapshot,
    EventResponse, ExecutionList, ExecutionResponse, ExecutionRow, HeaderMap, Json, MAX_ATTEMPTS,
    MAX_MEMBERS, MAX_PLAN_STEPS, Participant, Path, PlanStep, Postgres, ProfileSnapshot, SqlJson,
    State, StepResponse, TaskAccess, TeamSnapshot, Transaction, Utc, Uuid, Value, event,
    idempotency_key, plan, project, viewer, worker,
};
use crate::resource_visibility::{self, Resource};
use crate::task_board;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use sqlx::{Acquire, FromRow};

#[derive(FromRow)]
struct Definition {
    text: String,
    expected_result: String,
    coordinator_id: Option<Uuid>,
    coordinator_user_id: Option<Uuid>,
    execution_mode: String,
    agent_roles: SqlJson<Vec<AgentRole>>,
    plan_steps: Option<SqlJson<Vec<PlanStep>>>,
}

#[derive(FromRow)]
struct SnapshotRow {
    id: Uuid,
    name: String,
    instructions: String,
    tool_instructions: String,
    tool_permissions: Value,
}

async fn snapshot(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<TeamSnapshot, AppError> {
    let mut definition: Definition = sqlx::query_as("SELECT text,expected_result,coordinator_id,coordinator_user_id,execution_mode,agent_roles,plan_steps FROM ai_tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(project_id).bind(task_id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)?;
    // The steps of a predefined plan are frozen with the team that performs them.
    let plan = definition
        .plan_steps
        .take()
        .map_or_else(Vec::new, |steps| steps.0);
    if definition.execution_mode != "team" {
        return Err(AppError::BadRequest(
            "This task runs agents independently".into(),
        ));
    }
    let coordinator_id = definition
        .coordinator_id
        .or(definition.coordinator_user_id)
        .ok_or_else(|| AppError::BadRequest("Select a coordinator".into()))?;
    let mut agent_ids: Vec<Uuid> = sqlx::query_scalar("SELECT ai_profile_id FROM ai_task_agents WHERE tenant_id=$1 AND task_id=$2 ORDER BY ai_profile_id")
        .bind(tenant).bind(task_id).fetch_all(&mut **tx).await?;
    let mut employee_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM ai_task_assignees WHERE tenant_id=$1 AND task_id=$2 ORDER BY user_id",
    )
    .bind(tenant)
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await?;
    let members = agent_ids.len() + employee_ids.len();
    if plan.is_empty() {
        if members == 0
            || members > MAX_MEMBERS
            || agent_ids.contains(&coordinator_id)
            || employee_ids.contains(&coordinator_id)
        {
            return Err(AppError::BadRequest(
                "Select 1 to 8 team members distinct from the coordinator".into(),
            ));
        }
    } else if members > MAX_PLAN_STEPS {
        // The performers of a predefined plan are the team: the coordinator may
        // do every step alone, and nobody else takes part without a step.
        return Err(AppError::BadRequest(
            "A predefined plan supports at most 30 team members".into(),
        ));
    }
    if let Some(agent) = definition.coordinator_id
        && !agent_ids.contains(&agent)
    {
        agent_ids.push(agent);
    }
    if let Some(employee) = definition.coordinator_user_id
        && !employee_ids.contains(&employee)
    {
        employee_ids.push(employee);
    }
    let profiles: Vec<SnapshotRow> = sqlx::query_as(
        r#"
        SELECT p.id,p.name,p.instructions,p.tool_instructions,
          jsonb_build_object('capability_http_get',p.capability_http_get,
            'capability_http_post',p.capability_http_post,'capability_shell',p.capability_shell,
            'telegram_notify_on_operator_request',p.telegram_notify_on_operator_request,
            'http_allowed_hosts',p.http_allowed_hosts) AS tool_permissions
        FROM ai_profiles p
        WHERE p.tenant_id=$1 AND (p.project_id=$2 OR resource_visible(p.visibility,$2,NULL)) AND p.id=ANY($3)
        ORDER BY p.id FOR SHARE OF p
    "#,
    )
    .bind(tenant)
    .bind(project_id)
    .bind(&agent_ids)
    .fetch_all(&mut **tx)
    .await?;
    if profiles.len() != agent_ids.len() {
        return Err(AppError::BadRequest(
            "Every team agent must belong to this project".into(),
        ));
    }
    let employees: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT person.id, person.display_name FROM users AS person
        WHERE person.id = ANY($3) AND person.status = 'active' AND EXISTS (
            SELECT 1 FROM memberships AS membership
            WHERE membership.tenant_id = $1 AND membership.user_id = person.id AND membership.revoked_at IS NULL
              AND (membership.project_id = $2 OR membership.project_id IS NULL))
        ORDER BY person.id
        "#,
    )
    .bind(tenant)
    .bind(project_id)
    .bind(&employee_ids)
    .fetch_all(&mut **tx)
    .await?;
    if employees.len() != employee_ids.len() {
        return Err(AppError::BadRequest(
            "Every employee in the team must have access to this project".into(),
        ));
    }
    let role = |id: Uuid| {
        definition
            .agent_roles
            .0
            .iter()
            .find(|r| r.agent_id == id)
            .map_or_else(String::new, |r| r.role.clone())
    };
    Ok(TeamSnapshot {
        text: definition.text.clone(),
        expected_result: definition.expected_result.clone(),
        coordinator_id,
        coordinator_is_employee: definition.coordinator_user_id.is_some(),
        profiles: profiles
            .into_iter()
            .map(|p| ProfileSnapshot {
                role: role(p.id),
                id: p.id,
                name: p.name,
                instructions: p.instructions,
                tool_instructions: p.tool_instructions,
                tool_permissions: p.tool_permissions,
            })
            .collect(),
        employees: employees
            .into_iter()
            .map(|(id, name)| EmployeeSnapshot {
                role: role(id),
                id,
                name,
            })
            .collect(),
        plan,
    })
}

async fn insert_execution(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    project_id: Uuid,
    task: Uuid,
    scheduled: Option<DateTime<Utc>>,
    key: Option<Uuid>,
) -> Result<Uuid, AppError> {
    let snapshot = snapshot(tx, tenant, project_id, task).await?;
    let predefined =
        plan::predefined(&snapshot).map_err(|error| AppError::BadRequest(error.into()))?;
    if scheduled.is_none() && !snapshot.profiles.is_empty() {
        let ids: Vec<Uuid> = snapshot.profiles.iter().map(|profile| profile.id).collect();
        let ready: Vec<Uuid> = sqlx::query_scalar(r#"
            SELECT p.id FROM ai_profiles p
            JOIN ai_provider_connections c ON c.tenant_id=p.tenant_id AND c.id=p.provider_connection_id
            JOIN projects project ON project.tenant_id=p.tenant_id AND project.id=p.project_id
            WHERE p.tenant_id=$1 AND (p.project_id=$2 OR resource_visible(p.visibility,$2,NULL)) AND p.id=ANY($3)
              AND p.status='active' AND c.status='active' AND project.status='active'
              AND c.provider_kind IN ('openai','openai_compatible')
            ORDER BY p.id FOR SHARE OF p,c,project
        "#).bind(tenant).bind(project_id).bind(ids).fetch_all(&mut **tx).await?;
        if ready.len() != snapshot.profiles.len() {
            return Err(AppError::BadRequest("Activate every team agent and select an active supported model connection before running".into()));
        }
    }
    // Employees answer on their own schedule, so their teams get more time.
    let with_employees = !snapshot.employees.is_empty();
    let id = Uuid::now_v7();
    let row: ExecutionRow = sqlx::query_as("INSERT INTO ai_task_executions (id,tenant_id,project_id,task_id,scheduled_for,idempotency_key,snapshot,deadline_at) VALUES ($1,$2,$3,$4,$5,$6,$7,now() + CASE WHEN $8 THEN interval '30 days' ELSE $9 * interval '1 minute' END) RETURNING *")
        .bind(id).bind(tenant).bind(project_id).bind(task).bind(scheduled).bind(key).bind(SqlJson(&snapshot)).bind(with_employees)
        .bind(snapshot.agent_deadline_minutes())
        .fetch_one(&mut **tx).await?;
    if predefined.is_none() {
        insert_coordinator_step(tx, &row, "_plan", "Plan the task", "planning", 0)
            .await
            .map_err(|_| AppError::NotFound)?;
    }
    event(tx, &row, "execution.queued", "The team task was queued").await?;
    if let Some(plan) = predefined {
        // Nobody plans: the work and the review are laid out right away, and the
        // coordinator, agent or employee, only receives the review.
        worker::install_plan(tx, &row, None, plan)
            .await
            .map_err(AppError::internal)?;
    }
    task_board::mark_task_started(&mut **tx, tenant, task).await?;
    worker::activate_human_steps(tx, &row).await?;
    Ok(id)
}

/// Adds a step performed by the team's coordinator, an agent or an employee.
pub(super) async fn insert_coordinator_step(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    key: &str,
    title: &str,
    kind: &str,
    revision: i32,
) -> anyhow::Result<Uuid> {
    let coordinator = row
        .snapshot
        .coordinator()
        .ok_or_else(|| anyhow::anyhow!("coordinator snapshot missing"))?;
    Ok(insert_participant_step(tx, row, key, title, &coordinator, kind, "", revision).await?)
}

#[allow(clippy::too_many_arguments)] // All fields describe one persisted step.
pub(super) async fn insert_participant_step(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    key: &str,
    title: &str,
    participant: &Participant<'_>,
    kind: &str,
    input: &str,
    revision: i32,
) -> Result<Uuid, sqlx::Error> {
    match participant {
        Participant::Agent(profile) => {
            insert_step(tx, row, key, title, profile, kind, input, revision).await
        }
        Participant::Employee(employee) => {
            insert_human_step(tx, row, key, title, employee, kind, input, revision).await
        }
    }
}

#[allow(clippy::too_many_arguments)] // All fields describe one persisted step.
async fn insert_human_step(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    key: &str,
    title: &str,
    employee: &EmployeeSnapshot,
    kind: &str,
    input: &str,
    revision: i32,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO ai_task_execution_steps (id,tenant_id,project_id,execution_id,step_key,title,assignee_user_id,agent_name,kind,input_text,may_have_effects,revision) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,false,$11)")
        .bind(id).bind(row.tenant_id).bind(row.project_id).bind(row.id).bind(key).bind(title)
        .bind(employee.id).bind(&employee.name).bind(kind).bind(input).bind(revision)
        .execute(&mut **tx).await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)] // All fields describe one persisted step.
pub(super) async fn insert_step(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    key: &str,
    title: &str,
    profile: &ProfileSnapshot,
    kind: &str,
    input: &str,
    revision: i32,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO ai_task_execution_steps (id,tenant_id,project_id,execution_id,step_key,title,agent_id,agent_name,kind,input_text,may_have_effects,revision) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(id).bind(row.tenant_id).bind(row.project_id).bind(row.id).bind(key).bind(title)
        .bind(profile.id).bind(&profile.name).bind(kind).bind(input).bind(kind=="work" && profile.may_have_effects()).bind(revision)
        .execute(&mut **tx).await?;
    Ok(id)
}

pub(crate) async fn materialize_scheduled(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    project_id: Uuid,
    task: Uuid,
    scheduled: DateTime<Utc>,
) -> Result<bool, AppError> {
    let busy: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_task_executions WHERE tenant_id=$1 AND task_id=$2 AND (status IN ('queued','planning','running','reviewing','needs_attention') OR scheduled_for=$3))")
        .bind(tenant).bind(task).bind(scheduled).fetch_one(&mut **tx).await?;
    if busy {
        return Ok(false);
    }
    // A team that cannot start, such as one whose performer lost access, must
    // not fail the scheduler: the caller would roll back, find the same task
    // due again and never reach another one. The savepoint undoes the attempt,
    // the occurrence is recorded as failed and the schedule moves on.
    let mut attempt = tx.begin().await?;
    match insert_execution(
        &mut attempt,
        tenant,
        project_id,
        task,
        Some(scheduled),
        None,
    )
    .await
    {
        Ok(_) => {
            attempt.commit().await?;
            Ok(true)
        }
        Err(AppError::BadRequest(reason)) => {
            attempt.rollback().await?;
            record_unstarted(tx, tenant, project_id, task, scheduled, &reason).await?;
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Leaves a failed execution for a scheduled occurrence whose team could not
/// start, so the reason is visible in the history of the task.
async fn record_unstarted(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    project_id: Uuid,
    task: Uuid,
    scheduled: DateTime<Utc>,
    reason: &str,
) -> Result<(), AppError> {
    let (text, expected_result, agent, employee): (String, String, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT text,expected_result,coordinator_id,coordinator_user_id FROM ai_tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(project_id)
    .bind(task)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)?;
    // Nobody was admitted, so the snapshot names no participants.
    let snapshot = TeamSnapshot {
        text,
        expected_result,
        coordinator_id: agent.or(employee).unwrap_or_default(),
        coordinator_is_employee: employee.is_some(),
        profiles: Vec::new(),
        employees: Vec::new(),
        plan: Vec::new(),
    };
    let reason: String = reason.chars().take(2000).collect();
    let row: ExecutionRow = sqlx::query_as("INSERT INTO ai_task_executions (id,tenant_id,project_id,task_id,scheduled_for,status,snapshot,error,finished_at) VALUES ($1,$2,$3,$4,$5,'failed',$6,$7,now()) RETURNING *")
        .bind(Uuid::now_v7()).bind(tenant).bind(project_id).bind(task).bind(scheduled).bind(SqlJson(&snapshot)).bind(&reason)
        .fetch_one(&mut **tx).await?;
    event(tx, &row, "execution.not_started", &reason).await?;
    Ok(())
}

pub(super) async fn start(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(task): Path<Uuid>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ExecutionResponse>), AppError> {
    project(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Task, task).await?;
    let key = idempotency_key(&headers)?;
    let mut tx = state.db.begin().await?;
    let exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM ai_tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task)
    .fetch_optional(&mut *tx)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let (id, created) = start_in_transaction(&mut tx, &actor, project_id, task, key).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    tx.commit().await?;
    Ok((
        status,
        Json(load(&state, actor.tenant_id, project_id, id).await?),
    ))
}

/// Starts a manual execution of a task whose row the caller's transaction has
/// locked, or finds the one already started with this key. The flag tells
/// whether the execution is new.
pub(crate) async fn start_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    task: Uuid,
    key: Uuid,
) -> Result<(Uuid, bool), AppError> {
    let existing: Option<Uuid>=sqlx::query_scalar("SELECT id FROM ai_task_executions WHERE tenant_id=$1 AND project_id=$2 AND task_id=$3 AND idempotency_key=$4")
        .bind(actor.tenant_id).bind(project_id).bind(task).bind(key).fetch_optional(&mut **tx).await?;
    if let Some(id) = existing {
        return Ok((id, false));
    }
    crate::ai_tasks::ensure_task_schedule_open(tx, actor.tenant_id, task).await?;
    let busy: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_task_executions WHERE tenant_id=$1 AND task_id=$2 AND status IN ('queued','planning','running','reviewing','needs_attention')) OR EXISTS(SELECT 1 FROM ai_task_runs WHERE tenant_id=$1 AND task_id=$2 AND status IN ('pending','processing'))")
        .bind(actor.tenant_id).bind(task).fetch_one(&mut **tx).await?;
    if busy {
        return Err(AppError::BadRequest(
            "This task already has an unfinished execution".into(),
        ));
    }
    let id = insert_execution(tx, actor.tenant_id, project_id, task, None, Some(key)).await?;
    audit(tx, actor, project_id, id, "ai_task_execution.started").await?;
    Ok((id, true))
}

pub(super) async fn list(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(task): Path<Uuid>,
) -> Result<Json<ExecutionList>, AppError> {
    let access = viewer(&actor)?;
    let project_id = task_board::task_project(&state, &actor, access, task).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ai_tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3)",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM ai_task_executions WHERE tenant_id=$1 AND project_id=$2 AND task_id=$3 ORDER BY created_at DESC,id DESC LIMIT 30")
        .bind(actor.tenant_id).bind(project_id).bind(task).fetch_all(&state.db).await?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        items.push(load(&state, actor.tenant_id, project_id, id).await?);
    }
    Ok(Json(ExecutionList { items }))
}

pub(super) async fn detail(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(id): Path<Uuid>,
) -> Result<Json<ExecutionResponse>, AppError> {
    Ok(Json(
        load(
            &state,
            actor.tenant_id,
            execution_project(&state, &actor, viewer(&actor)?, id).await?,
            id,
        )
        .await?,
    ))
}

async fn execution_project(
    state: &AppState,
    actor: &ActorContext,
    access: TaskAccess,
    id: Uuid,
) -> Result<Uuid, AppError> {
    let task: Uuid =
        sqlx::query_scalar("SELECT task_id FROM ai_task_executions WHERE tenant_id=$1 AND id=$2")
            .bind(actor.tenant_id)
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
    task_board::task_project(state, actor, access, task).await
}

pub(super) async fn load(
    state: &AppState,
    tenant: Uuid,
    project_id: Uuid,
    id: Uuid,
) -> Result<ExecutionResponse, AppError> {
    // A repeatable snapshot keeps parent and step states coherent during polling.
    let mut tx = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let row: ExecutionRow = sqlx::query_as(
        "SELECT * FROM ai_task_executions WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(project_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    let steps=sqlx::query_as::<_,StepResponse>("SELECT s.id,s.title,s.agent_id,s.assignee_user_id,s.agent_name,s.kind,s.status,s.input_text,s.result_text,s.error,s.may_have_effects,s.attempts,s.started_at,s.finished_at,ARRAY(SELECT d.depends_on FROM ai_task_step_dependencies d WHERE d.step_id=s.id ORDER BY d.depends_on) AS depends_on FROM ai_task_execution_steps s WHERE s.tenant_id=$1 AND s.project_id=$2 AND s.execution_id=$3 ORDER BY s.created_at,s.id")
        .bind(tenant).bind(project_id).bind(id).fetch_all(&mut *tx).await?;
    let events=sqlx::query_as::<_,EventResponse>("SELECT id,event_type,message,created_at FROM ai_task_execution_events WHERE tenant_id=$1 AND project_id=$2 AND execution_id=$3 ORDER BY created_at,id LIMIT 200")
        .bind(tenant).bind(project_id).bind(id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(ExecutionResponse {
        id: row.id,
        task_id: row.task_id,
        status: row.status,
        result_text: row.result_text,
        error: row.error,
        created_at: row.created_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
        updated_at: row.updated_at,
        steps,
        events,
    })
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(id): Path<Uuid>,
) -> Result<Json<ExecutionResponse>, AppError> {
    project(&actor)?;
    let project_id = execution_project(&state, &actor, TaskAccess::Manage, id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CLAIM_LOCK)
        .execute(&mut *tx)
        .await?;
    let row:ExecutionRow=sqlx::query_as("SELECT * FROM ai_task_executions WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE")
        .bind(actor.tenant_id).bind(project_id).bind(id).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    if !["succeeded", "cancelled", "failed"].contains(&row.status.as_str()) {
        sqlx::query("UPDATE ai_task_executions SET status='cancelled',finished_at=now(),updated_at=now(),error=NULL WHERE id=$1")
            .bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE ai_task_execution_steps SET status='cancelled',finished_at=now() WHERE execution_id=$1 AND status IN ('pending','running','blocked')")
            .bind(id).execute(&mut *tx).await?;
        // Running attempts keep the runtime lane until the owner revokes its grant.
        event(
            &mut tx,
            &row,
            "execution.cancelled",
            "The execution was cancelled",
        )
        .await?;
        audit(
            &mut tx,
            &actor,
            project_id,
            id,
            "ai_task_execution.cancelled",
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load(&state, actor.tenant_id, project_id, id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RetryInput {
    step_id: Uuid,
}

pub(super) async fn retry(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(id): Path<Uuid>,
    Json(input): Json<RetryInput>,
) -> Result<Json<ExecutionResponse>, AppError> {
    project(&actor)?;
    let project_id = execution_project(&state, &actor, TaskAccess::Manage, id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CLAIM_LOCK)
        .execute(&mut *tx)
        .await?;
    let row:ExecutionRow=sqlx::query_as("SELECT * FROM ai_task_executions WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE")
        .bind(actor.tenant_id).bind(project_id).bind(id).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    if !["needs_attention", "failed"].contains(&row.status.as_str())
        || row.deadline_at <= Utc::now()
    {
        return Err(AppError::BadRequest(
            "This execution cannot be resumed; start a new execution if needed".into(),
        ));
    }
    let step:Option<(String,bool,i32)>=sqlx::query_as("SELECT kind,may_have_effects,attempts FROM ai_task_execution_steps WHERE execution_id=$1 AND id=$2 AND status IN ('failed','blocked') FOR UPDATE")
        .bind(id).bind(input.step_id).fetch_optional(&mut *tx).await?;
    let Some((kind, effects, attempts)) = step else {
        return Err(AppError::BadRequest(
            "Select a failed or blocked step".into(),
        ));
    };
    if effects {
        return Err(AppError::BadRequest(
            "The outcome of external actions must be verified before this step can be repeated"
                .into(),
        ));
    }
    if attempts >= MAX_ATTEMPTS {
        return Err(AppError::BadRequest(
            "The step attempt limit has been reached".into(),
        ));
    }
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_task_step_attempts WHERE execution_id=$1 AND status='running')")
        .bind(id).fetch_one(&mut *tx).await?;
    if active {
        return Err(AppError::BadRequest(
            "Wait until the previous attempt has stopped".into(),
        ));
    }
    sqlx::query("UPDATE ai_task_execution_steps SET status='pending',error=NULL,finished_at=NULL,available_at=now() WHERE id=$1")
        .bind(input.step_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_task_execution_steps SET status='pending',error=NULL,finished_at=NULL WHERE execution_id=$1 AND status='blocked' AND attempts=0")
        .bind(id).execute(&mut *tx).await?;
    let next = match kind.as_str() {
        "planning" => "planning",
        "review" => "reviewing",
        _ => "running",
    };
    sqlx::query("UPDATE ai_task_executions SET status=$2,error=NULL,finished_at=NULL,updated_at=now() WHERE id=$1")
        .bind(id).bind(next).execute(&mut *tx).await?;
    event(
        &mut tx,
        &row,
        "step.retry_requested",
        "A safe step retry was requested",
    )
    .await?;
    worker::activate_human_steps(&mut tx, &row).await?;
    audit(&mut tx, &actor, project_id, id, "ai_task_execution.retried").await?;
    tx.commit().await?;
    Ok(Json(load(&state, actor.tenant_id, project_id, id).await?))
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    id: Uuid,
    action: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_log (id,tenant_id,project_id,actor_id,action,resource_kind,resource_id,metadata) VALUES ($1,$2,$3,$4,$5,'ai_task_execution',$6,$7)")
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project_id).bind(actor.actor_id).bind(action).bind(id).bind(json!({})).execute(&mut **tx).await?;
    Ok(())
}
