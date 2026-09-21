//! Durable, project-scoped coordination of existing AI profiles.

mod human;
mod management;
mod plan;
mod worker;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, Postgres, Transaction, types::Json as SqlJson};
use uuid::Uuid;

use crate::{
    AppState,
    ai_tasks::{MAX_PLAN_STEPS, PerformerKind, PlanPerformer, PlanStep},
    auth::ActorContext,
    error::AppError,
    task_board::{self, TaskAccess},
};
pub(crate) use management::materialize_scheduled;
pub(crate) use worker::attempt_is_authorized;
pub use worker::process_once;

const MAX_MEMBERS: usize = 8;
const MAX_WORK_STEPS: usize = 12;
const MAX_TOTAL_STEPS: i64 = 20;
const MAX_ATTEMPTS: i32 = 3;
/// Provider attempts one execution may spend across all of its steps.
const MAX_EXECUTION_ATTEMPTS: i64 = 40;
/// Minutes an execution of agents alone may take.
const AGENT_DEADLINE_MINUTES: i64 = 60;
const CLAIM_LOCK: i64 = 6_075_998_209_141_571_923;

/// Management endpoints share the same project-wide AI permission as tasks.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/ai/tasks/{task_id}/executions",
            get(management::list).post(management::start),
        )
        .route(
            "/api/v1/ai/task-executions/{execution_id}",
            get(management::detail),
        )
        .route(
            "/api/v1/ai/task-executions/{execution_id}/cancel",
            post(management::cancel),
        )
        .route(
            "/api/v1/ai/task-executions/{execution_id}/retry",
            post(management::retry),
        )
        .route("/api/v1/ai/task-steps/mine", get(human::mine))
        .route(
            "/api/v1/ai/task-steps/{step_id}/submit",
            post(human::submit),
        )
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionMode {
    #[default]
    Independent,
    Team,
}

impl ExecutionMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::Team => "team",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentRole {
    pub agent_id: Uuid,
    pub role: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProfileSnapshot {
    id: Uuid,
    name: String,
    role: String,
    instructions: String,
    tool_instructions: String,
    tool_permissions: Value,
}

impl ProfileSnapshot {
    fn may_have_effects(&self) -> bool {
        [
            "capability_http_post",
            "capability_shell",
            "telegram_notify_on_operator_request",
        ]
        .iter()
        .any(|key| self.tool_permissions[*key].as_bool().unwrap_or(false))
    }
}

/// An employee taking part in a team; their steps wait for a submitted result.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct EmployeeSnapshot {
    id: Uuid,
    name: String,
    role: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TeamSnapshot {
    text: String,
    expected_result: String,
    /// An agent id, or a user id when `coordinator_is_employee`.
    coordinator_id: Uuid,
    #[serde(default)]
    coordinator_is_employee: bool,
    profiles: Vec<ProfileSnapshot>,
    #[serde(default)]
    employees: Vec<EmployeeSnapshot>,
    /// The task's predefined plan as it was at the start. Empty when the
    /// coordinator plans the work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    plan: Vec<PlanStep>,
}

/// Who performs a step: an agent profile or an employee.
enum Participant<'a> {
    Agent(&'a ProfileSnapshot),
    Employee(&'a EmployeeSnapshot),
}

impl TeamSnapshot {
    fn profile(&self, id: Uuid) -> Option<&ProfileSnapshot> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    fn employee(&self, id: Uuid) -> Option<&EmployeeSnapshot> {
        self.employees.iter().find(|employee| employee.id == id)
    }

    fn participant(&self, id: Uuid) -> Option<Participant<'_>> {
        self.profile(id)
            .map(Participant::Agent)
            .or_else(|| self.employee(id).map(Participant::Employee))
    }

    fn coordinator(&self) -> Option<Participant<'_>> {
        if self.coordinator_is_employee {
            self.employee(self.coordinator_id)
                .map(Participant::Employee)
        } else {
            self.profile(self.coordinator_id).map(Participant::Agent)
        }
    }

    /// Members that the coordinator may assign work to.
    fn member_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.profiles
            .iter()
            .map(|profile| profile.id)
            .chain(self.employees.iter().map(|employee| employee.id))
            .filter(|id| *id != self.coordinator_id)
    }

    /// Whether the performer of a predefined step takes part in this team.
    /// The coordinator may perform such steps too.
    fn includes(&self, performer: PlanPerformer) -> bool {
        match performer.kind {
            PerformerKind::Agent => self.profile(performer.id).is_some(),
            PerformerKind::Employee => self.employee(performer.id).is_some(),
        }
    }

    /// Work steps of the predefined plan; zero when the coordinator plans.
    fn predefined_steps(&self) -> i64 {
        i64::try_from(self.plan.len()).unwrap_or(i64::MAX)
    }

    /// Most work steps the plan of this execution may hold.
    fn max_work_steps(&self) -> usize {
        if self.plan.is_empty() {
            MAX_WORK_STEPS
        } else {
            MAX_PLAN_STEPS
        }
    }

    /// Step rows the execution may hold. A predefined plan can be corrected
    /// in full once: every work step and the review, twice.
    fn step_budget(&self) -> i64 {
        match self.predefined_steps() {
            0 => MAX_TOTAL_STEPS,
            work => work.saturating_mul(2).saturating_add(2),
        }
    }

    /// Provider attempts the execution may spend.
    fn attempt_budget(&self) -> i64 {
        match self.predefined_steps() {
            0 => MAX_EXECUTION_ATTEMPTS,
            work => MAX_EXECUTION_ATTEMPTS.max(work.saturating_mul(2).saturating_add(10)),
        }
    }

    /// Minutes a team of agents alone gets: they run one after another, so a
    /// long predefined plan needs more than the usual hour.
    fn agent_deadline_minutes(&self) -> i64 {
        match self.predefined_steps() {
            0 => AGENT_DEADLINE_MINUTES,
            work => AGENT_DEADLINE_MINUTES.max(work.saturating_mul(10).saturating_add(10)),
        }
    }
}

#[derive(FromRow)]
struct ExecutionRow {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    task_id: Uuid,
    status: String,
    snapshot: SqlJson<TeamSnapshot>,
    result_text: Option<String>,
    error: Option<String>,
    rework_round: i32,
    deadline_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ExecutionResponse {
    id: Uuid,
    task_id: Uuid,
    status: String,
    result_text: Option<String>,
    error: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    steps: Vec<StepResponse>,
    events: Vec<EventResponse>,
}

#[derive(FromRow, Serialize)]
struct StepResponse {
    id: Uuid,
    title: String,
    agent_id: Option<Uuid>,
    assignee_user_id: Option<Uuid>,
    agent_name: String,
    kind: String,
    status: String,
    depends_on: Vec<Uuid>,
    input_text: String,
    result_text: Option<String>,
    error: Option<String>,
    may_have_effects: bool,
    attempts: i32,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(FromRow, Serialize)]
struct EventResponse {
    id: Uuid,
    event_type: String,
    message: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ExecutionList {
    items: Vec<ExecutionResponse>,
}

/// Starting, stopping and retrying executions requires task management.
fn project(actor: &ActorContext) -> Result<Uuid, AppError> {
    task_board::require_manage(actor)
}

/// Participants may follow executions of their own tasks.
fn viewer(actor: &ActorContext) -> Result<TaskAccess, AppError> {
    Ok(task_board::task_actor(actor)?.access)
}

pub(crate) fn idempotency_key(headers: &HeaderMap) -> Result<Uuid, AppError> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| AppError::BadRequest("Idempotency-Key must be a UUID".into()))
}

async fn event(
    tx: &mut Transaction<'_, Postgres>,
    row: &ExecutionRow,
    kind: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO ai_task_execution_events (id,tenant_id,project_id,execution_id,event_type,message) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(Uuid::now_v7()).bind(row.tenant_id).bind(row.project_id).bind(row.id)
        .bind(kind).bind(message).execute(&mut **tx).await?;
    sqlx::query("UPDATE ai_task_executions SET updated_at=now() WHERE id=$1")
        .bind(row.id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
