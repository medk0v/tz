//! Project-scoped definitions for one-time and recurring AI tasks.

use crate::resource_visibility::{self, Resource, ResourceVisibility};
use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, Result as AnyResult};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch},
};
use chrono::{
    DateTime, Datelike, Days, Duration as ChronoDuration, LocalResult, NaiveDate, NaiveDateTime,
    NaiveTime, TimeZone, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction, types::Json as SqlJson};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::ai_orchestration::{AgentRole, ExecutionMode};
use crate::task_board::{self, TASK_ACCESS_SQL};
use crate::{AppState, auth::ActorContext, error::AppError, outbox, provider_reply};

const MAX_TASK_TEXT_CHARS: usize = 50_000;
const MAX_PROJECT_TASKS: i64 = 500;
const MAX_TASK_AGENTS: usize = 32;
const MAX_YEARLY_DATES: usize = 366;
const MAX_STORED_ERROR_CHARS: usize = 2_000;
const TASK_RUN_LOCK_TIMEOUT_SECONDS: i64 = 10 * 60;
const TASK_RUN_EXECUTION_TIMEOUT: Duration = Duration::from_secs(9 * 60);
const AI_TASK_MATERIALIZE_ADVISORY_LOCK: i64 = 6_075_998_209_141_571_924;
const TASK_RUN_HISTORY_LIMIT: i64 = 100;
/// Most steps a predefined plan may hold; planned by a coordinator, a team gets fewer.
pub(crate) const MAX_PLAN_STEPS: usize = 30;
const MAX_PLAN_STEP_TITLE_CHARS: usize = 200;
const MAX_PLAN_STEP_INSTRUCTIONS_CHARS: usize = 10_000;
const SELECT_DUE_TASK_SQL: &str = r#"
    SELECT task.id, task.tenant_id, task.project_id, task.text,
           task.schedule, task.next_run_at, task.execution_mode
    FROM ai_tasks AS task
    JOIN projects AS project
      ON project.tenant_id = task.tenant_id
     AND project.id = task.project_id
     AND project.status = 'active'
    LEFT JOIN ai_task_project_scheduler_state AS scheduler
      ON scheduler.tenant_id = task.tenant_id
     AND scheduler.project_id = task.project_id
    WHERE task.next_run_at IS NOT NULL AND task.next_run_at <= $1
    ORDER BY scheduler.last_materialized_sequence NULLS FIRST,
             task.next_run_at, task.id
    FOR UPDATE OF task SKIP LOCKED
    LIMIT 1
"#;
const CLAIM_TASK_RUN_SQL: &str = r#"
    WITH candidate AS (
        SELECT run.id
        FROM ai_task_runs AS run
        JOIN ai_tasks AS task
          ON task.tenant_id = run.tenant_id AND task.id = run.task_id
        JOIN projects AS project
          ON project.tenant_id = run.tenant_id
         AND project.id = run.project_id
         AND project.status = 'active'
        LEFT JOIN ai_task_project_scheduler_state AS scheduler
          ON scheduler.tenant_id = run.tenant_id
         AND scheduler.project_id = run.project_id
        WHERE (
                (
                    run.status = 'pending'
                    AND run.available_at <= now()
                    AND run.attempts < $1
                )
                OR (
                    run.status = 'processing'
                    AND run.locked_at <= now() - ($2 * interval '1 second')
                    AND run.attempts < $1
                )
              )
          AND (task.schedule->>'ends_at' IS NULL OR
               (task.schedule->>'ends_at')::timestamp AT TIME ZONE
                   (task.schedule->>'timezone') >= statement_timestamp())
          AND (run.ai_profile_id IS NULL OR NOT (run.ai_profile_id=ANY($4)))
        ORDER BY (
            SELECT count(*)
            FROM ai_task_runs AS project_run
            WHERE project_run.tenant_id = run.tenant_id
              AND project_run.project_id = run.project_id
              AND project_run.status = 'processing'
              AND project_run.locked_at >
                  now() - ($2 * interval '1 second')
        ), scheduler.last_claim_sequence NULLS FIRST,
           run.available_at, run.scheduled_for, run.id
        FOR UPDATE OF run SKIP LOCKED
        LIMIT 1
    )
    UPDATE ai_task_runs AS run
    SET status = 'processing', locked_at = now(), locked_by = $3,
        attempts = run.attempts + 1,
        started_at = COALESCE(run.started_at, now()), updated_at = now()
    FROM candidate
    WHERE run.id = candidate.id
    RETURNING run.id, run.tenant_id, run.project_id, run.task_id,
              run.ai_profile_id, run.task_text, run.attempts
"#;
const ADVANCE_MATERIALIZATION_CURSOR_SQL: &str = r#"
    INSERT INTO ai_task_project_scheduler_state (
        tenant_id, project_id, last_materialized_sequence
    ) VALUES (
        $1, $2, nextval('ai_task_project_scheduler_sequence')
    )
    ON CONFLICT (tenant_id, project_id) DO UPDATE
    SET last_materialized_sequence = EXCLUDED.last_materialized_sequence
"#;
const ADVANCE_CLAIM_CURSOR_SQL: &str = r#"
    INSERT INTO ai_task_project_scheduler_state (
        tenant_id, project_id, last_claim_sequence
    ) VALUES (
        $1, $2, nextval('ai_task_project_scheduler_sequence')
    )
    ON CONFLICT (tenant_id, project_id) DO UPDATE
    SET last_claim_sequence = EXCLUDED.last_claim_sequence
"#;
const PRUNE_TERMINAL_TASK_RUNS_SQL: &str = r#"
    WITH retained AS (
        SELECT id
        FROM ai_task_runs
        WHERE tenant_id = $1
          AND task_id = $2
          AND status IN ('completed', 'failed', 'cancelled')
        ORDER BY scheduled_for DESC, id DESC
        LIMIT $3
    )
    DELETE FROM ai_task_runs AS run
    WHERE run.tenant_id = $1
      AND run.task_id = $2
      AND run.status IN ('completed', 'failed', 'cancelled')
      AND NOT EXISTS (SELECT 1 FROM retained WHERE retained.id = run.id)
"#;
const MAX_DST_FORWARD_MINUTES: i64 = 24 * 60;
const DAILY_SEARCH_DAYS: u64 = 3;
const WEEKLY_SEARCH_DAYS: u64 = 14;
const MONTHLY_SEARCH_DAYS: u64 = 400;
const YEARLY_SEARCH_DAYS: u64 = 8 * 366 + 1;

/// Routes for project-wide AI task schedules.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/tasks", get(list_tasks).post(create_task))
        .route(
            "/api/v1/ai/tasks/{task_id}",
            patch(update_task).delete(delete_task),
        )
        .route(
            "/api/v1/ai/tasks/{task_id}/runs",
            get(list_task_runs).post(run_task_now),
        )
        .merge(crate::task_attachments::router())
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TaskSchedule {
    #[default]
    Manual,
    Once {
        run_at: DateTime<Utc>,
    },
    Daily {
        time: String,
        timezone: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<NaiveDateTime>,
    },
    Weekly {
        time: String,
        timezone: String,
        weekdays: Vec<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<NaiveDateTime>,
    },
    Monthly {
        time: String,
        timezone: String,
        month_days: Vec<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<NaiveDateTime>,
    },
    Yearly {
        time: String,
        timezone: String,
        dates: Vec<YearlyDate>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<NaiveDateTime>,
    },
}

impl TaskSchedule {
    fn ends_at_utc(&self) -> AnyResult<Option<DateTime<Utc>>> {
        let (ends_at, timezone) = match self {
            Self::Daily {
                ends_at, timezone, ..
            }
            | Self::Weekly {
                ends_at, timezone, ..
            }
            | Self::Monthly {
                ends_at, timezone, ..
            }
            | Self::Yearly {
                ends_at, timezone, ..
            } => (ends_at, timezone),
            Self::Manual | Self::Once { .. } => return Ok(None),
        };
        let Some(ends_at) = ends_at else {
            return Ok(None);
        };
        let timezone = timezone
            .trim()
            .parse::<Tz>()
            .map_err(|_| anyhow::anyhow!("timezone must be a valid IANA timezone"))?;
        match timezone.from_local_datetime(ends_at) {
            LocalResult::Single(value) => Ok(Some(value.with_timezone(&Utc))),
            LocalResult::Ambiguous(_, _) => {
                anyhow::bail!("ends_at is ambiguous in the selected timezone")
            }
            LocalResult::None => {
                anyhow::bail!("ends_at does not exist in the selected timezone")
            }
        }
    }

    fn has_ended_at(&self, now: DateTime<Utc>) -> AnyResult<bool> {
        Ok(self.ends_at_utc()?.is_some_and(|ends_at| now > ends_at))
    }

    fn next_occurrence_after(&self, now: DateTime<Utc>) -> AnyResult<Option<DateTime<Utc>>> {
        let ends_at = self.ends_at_utc()?;
        if ends_at.is_some_and(|ends_at| now >= ends_at) {
            return Ok(None);
        }
        let next = match self {
            Self::Manual => Ok(None),
            Self::Once { run_at } => Ok((*run_at > now).then_some(*run_at)),
            Self::Daily { time, timezone, .. } => {
                let (time, timezone) = parse_normalized_recurrence(time, timezone)?;
                next_matching_occurrence(now, timezone, time, DAILY_SEARCH_DAYS, |_| true)
            }
            Self::Weekly {
                time,
                timezone,
                weekdays,
                ..
            } => {
                let (time, timezone) = parse_normalized_recurrence(time, timezone)?;
                next_matching_occurrence(now, timezone, time, WEEKLY_SEARCH_DAYS, |date| {
                    weekdays.contains(&date.weekday().number_from_monday())
                })
            }
            Self::Monthly {
                time,
                timezone,
                month_days,
                ..
            } => {
                let (time, timezone) = parse_normalized_recurrence(time, timezone)?;
                next_matching_occurrence(now, timezone, time, MONTHLY_SEARCH_DAYS, |date| {
                    month_days.contains(&date.day())
                })
            }
            Self::Yearly {
                time,
                timezone,
                dates,
                ..
            } => {
                let (time, timezone) = parse_normalized_recurrence(time, timezone)?;
                next_matching_occurrence(now, timezone, time, YEARLY_SEARCH_DAYS, |date| {
                    dates.iter().any(|configured| {
                        configured.month == date.month() && configured.day == date.day()
                    })
                })
            }
        }?;
        Ok(next.filter(|next| ends_at.is_none_or(|ends_at| *next <= ends_at)))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YearlyDate {
    month: u32,
    day: u32,
}

/// Who performs a step of a predefined plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PerformerKind {
    Agent,
    Employee,
}

/// An AI profile id for an agent, a user id for an employee.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanPerformer {
    pub kind: PerformerKind,
    pub id: Uuid,
}

/// One step of a task's predefined plan, stored and returned in this shape.
/// Steps run in array order; `id` becomes the key of the execution step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanStep {
    pub id: Uuid,
    pub title: String,
    pub instructions: String,
    pub performer: PlanPerformer,
}

/// The approved process version a task was launched from. Only the launch
/// endpoint sets it, and it never changes afterwards.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProcessMarker {
    pub process_id: Uuid,
    pub version: i32,
}

/// A team task with a predefined plan, as the process launcher creates it.
pub(crate) struct PlannedTask {
    pub text: String,
    pub expected_result: String,
    pub schedule: TaskSchedule,
    pub coordinator: PlanPerformer,
    pub plan_steps: Vec<PlanStep>,
    pub list_id: Option<Uuid>,
    pub starts_at: Option<DateTime<Utc>>,
    pub due_at: Option<DateTime<Utc>>,
    pub all_day: bool,
    pub time_zone: Option<String>,
}

// An omitted due date or plan keeps the current one; an explicit null clears it.
#[allow(clippy::option_option)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskRequest {
    visibility: Option<ResourceVisibility>,
    text: String,
    schedule: TaskSchedule,
    agent_ids: Vec<Uuid>,
    #[serde(default)]
    execution_mode: ExecutionMode,
    #[serde(default)]
    coordinator_id: Option<Uuid>,
    #[serde(default)]
    expected_result: String,
    #[serde(default)]
    agent_roles: Vec<AgentRole>,
    /// Employees; omitted on update to keep the current assignment.
    #[serde(default)]
    assignee_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    coordinator_user_id: Option<Uuid>,
    #[serde(default)]
    list_id: Option<Uuid>,
    #[serde(default)]
    column_id: Option<Uuid>,
    /// Start of the span whose end is `due_at`; omitted on update to keep it.
    #[serde(default, deserialize_with = "explicit")]
    starts_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "explicit")]
    due_at: Option<Option<DateTime<Utc>>>,
    /// The span covers whole days in `time_zone`; omitted on update to keep it.
    #[serde(default)]
    all_day: Option<bool>,
    /// IANA zone the span was entered in, which its instants cannot recover.
    #[serde(default, deserialize_with = "explicit")]
    time_zone: Option<Option<String>>,
    /// Minutes before the span opens to remind the people working on the task.
    #[serde(default, deserialize_with = "explicit")]
    reminder_minutes: Option<Option<i32>>,
    #[serde(default)]
    tag_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    attachment_ids: Option<Vec<Uuid>>,
    /// A predefined plan: its performers become the team. Omitted on update
    /// to keep the current plan.
    #[serde(default, deserialize_with = "explicit")]
    plan_steps: Option<Option<Vec<PlanStep>>>,
}

impl From<PlannedTask> for TaskRequest {
    fn from(task: PlannedTask) -> Self {
        let (coordinator_id, coordinator_user_id) = match task.coordinator.kind {
            PerformerKind::Agent => (Some(task.coordinator.id), None),
            PerformerKind::Employee => (None, Some(task.coordinator.id)),
        };
        Self {
            visibility: None,
            text: task.text,
            schedule: task.schedule,
            agent_ids: Vec::new(),
            execution_mode: ExecutionMode::Team,
            coordinator_id,
            expected_result: task.expected_result,
            agent_roles: Vec::new(),
            assignee_ids: None,
            coordinator_user_id,
            list_id: task.list_id,
            column_id: None,
            starts_at: task.starts_at.map(Some),
            due_at: task.due_at.map(Some),
            all_day: Some(task.all_day),
            time_zone: Some(task.time_zone),
            reminder_minutes: None,
            tag_ids: None,
            attachment_ids: None,
            plan_steps: Some(Some(task.plan_steps)),
        }
    }
}

/// Distinguishes an omitted field from an explicit null.
fn explicit<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl TaskRequest {
    /// Employees, boards and tags are unavailable in the Lite edition.
    fn uses_board_features(&self) -> bool {
        self.assignee_ids
            .as_ref()
            .is_some_and(|ids| !ids.is_empty())
            || self.coordinator_user_id.is_some()
            || self.list_id.is_some()
            || self.column_id.is_some()
            || self.starts_at.flatten().is_some()
            || self.due_at.flatten().is_some()
            || self.all_day == Some(true)
            || self.time_zone.as_ref().is_some_and(Option::is_some)
            || self.reminder_minutes.flatten().is_some()
            || self.tag_ids.as_ref().is_some_and(|ids| !ids.is_empty())
    }
}

/// An employee performing a step is an employee taking part in the task.
fn plan_uses_employees(plan_steps: Option<&[PlanStep]>) -> bool {
    plan_steps.is_some_and(|steps| {
        steps
            .iter()
            .any(|step| step.performer.kind == PerformerKind::Employee)
    })
}

struct NormalizedTask {
    text: String,
    schedule: TaskSchedule,
    agent_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    execution_mode: ExecutionMode,
    coordinator_id: Option<Uuid>,
    coordinator_user_id: Option<Uuid>,
    expected_result: String,
    agent_roles: Vec<AgentRole>,
    plan_steps: Option<Vec<PlanStep>>,
}

impl NormalizedTask {
    fn execution_agent_ids(&self) -> Vec<Uuid> {
        let mut ids = self.agent_ids.clone();
        if let Some(id) = self.coordinator_id {
            ids.push(id);
        }
        ids.sort_unstable();
        ids
    }

    /// Agents that run on their own or on a schedule need a working model.
    fn requires_executable_agents(&self) -> bool {
        !self.execution_agent_ids().is_empty()
            && (self.execution_mode == ExecutionMode::Independent
                || !matches!(self.schedule, TaskSchedule::Manual))
    }
}

#[derive(Debug, FromRow)]
struct TaskRow {
    visibility: sqlx::types::Json<ResourceVisibility>,
    id: Uuid,
    project_id: Uuid,
    text: String,
    schedule: SqlJson<TaskSchedule>,
    agent_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    tag_ids: Vec<Uuid>,
    attachment_ids: Vec<Uuid>,
    waiting_user_ids: Vec<Uuid>,
    execution_mode: String,
    coordinator_id: Option<Uuid>,
    coordinator_user_id: Option<Uuid>,
    expected_result: String,
    agent_roles: SqlJson<Vec<AgentRole>>,
    list_id: Option<Uuid>,
    column_id: Option<Uuid>,
    position: i32,
    starts_at: Option<DateTime<Utc>>,
    due_at: Option<DateTime<Utc>>,
    all_day: bool,
    time_zone: Option<String>,
    reminder_minutes: Option<i32>,
    completed_at: Option<DateTime<Utc>>,
    latest_status: Option<String>,
    latest_at: Option<DateTime<Utc>>,
    next_run_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    process_id: Option<Uuid>,
    process_version: Option<i32>,
    process_title: Option<String>,
    plan_steps: Option<SqlJson<Vec<PlanStep>>>,
    plan_done: Option<i32>,
    plan_total: Option<i32>,
}

/// The approved process version a task was launched from.
#[derive(Debug, Serialize)]
struct TaskProcess {
    id: Uuid,
    version: i32,
    title: String,
}

/// Finished steps of the latest execution of a predefined plan.
#[derive(Debug, Serialize)]
struct PlanProgress {
    done: i32,
    total: i32,
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskResponse {
    visibility: ResourceVisibility,
    id: Uuid,
    project_id: Uuid,
    text: String,
    schedule: TaskSchedule,
    agent_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    tag_ids: Vec<Uuid>,
    attachment_ids: Vec<Uuid>,
    /// Employees whose step of a running team execution is waiting for them.
    waiting_user_ids: Vec<Uuid>,
    execution_mode: String,
    coordinator_id: Option<Uuid>,
    coordinator_user_id: Option<Uuid>,
    expected_result: String,
    agent_roles: Vec<AgentRole>,
    list_id: Option<Uuid>,
    column_id: Option<Uuid>,
    position: i32,
    starts_at: Option<DateTime<Utc>>,
    due_at: Option<DateTime<Utc>>,
    all_day: bool,
    time_zone: Option<String>,
    reminder_minutes: Option<i32>,
    completed_at: Option<DateTime<Utc>>,
    latest_status: Option<String>,
    latest_at: Option<DateTime<Utc>>,
    next_occurrence_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// Set for a task launched from an approved process.
    process: Option<TaskProcess>,
    /// The predefined plan of a team task, in execution order.
    plan_steps: Option<Vec<PlanStep>>,
    plan_progress: Option<PlanProgress>,
}

impl TaskResponse {
    fn from_row(row: TaskRow) -> Self {
        let process = row
            .process_id
            .zip(row.process_version)
            .map(|(id, version)| TaskProcess {
                id,
                version,
                title: row.process_title.unwrap_or_default(),
            });
        let plan_progress = row
            .plan_done
            .zip(row.plan_total)
            .map(|(done, total)| PlanProgress { done, total });
        Self {
            visibility: row.visibility.0,
            id: row.id,
            project_id: row.project_id,
            text: row.text,
            schedule: row.schedule.0,
            agent_ids: row.agent_ids,
            assignee_ids: row.assignee_ids,
            tag_ids: row.tag_ids,
            attachment_ids: row.attachment_ids,
            waiting_user_ids: row.waiting_user_ids,
            execution_mode: row.execution_mode,
            coordinator_id: row.coordinator_id,
            coordinator_user_id: row.coordinator_user_id,
            expected_result: row.expected_result,
            agent_roles: row.agent_roles.0,
            list_id: row.list_id,
            column_id: row.column_id,
            position: row.position,
            starts_at: row.starts_at,
            due_at: row.due_at,
            all_day: row.all_day,
            time_zone: row.time_zone,
            reminder_minutes: row.reminder_minutes,
            completed_at: row.completed_at,
            latest_status: row.latest_status,
            latest_at: row.latest_at,
            next_occurrence_at: row.next_run_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
            process,
            plan_steps: row.plan_steps.map(|steps| steps.0),
            plan_progress,
        }
    }
}

/// Task fields with participants, tags and the latest occurrence status.
/// It takes no bind parameters, so every caller numbers its own.
/// A step of the plan is done once it succeeded and no correction of it is
/// still open: a step sent back by the review counts again when it is redone.
const TASK_SELECT_SQL: &str = r#"
    SELECT task.visibility, task.id, task.project_id, task.text, task.schedule, task.execution_mode, task.coordinator_id,
           task.coordinator_user_id, task.expected_result, task.agent_roles,
           ARRAY(SELECT agent.ai_profile_id FROM ai_task_agents AS agent
                 WHERE agent.tenant_id = task.tenant_id AND agent.task_id = task.id
                 ORDER BY agent.ai_profile_id) AS agent_ids,
           ARRAY(SELECT assignee.user_id FROM ai_task_assignees AS assignee
                 WHERE assignee.tenant_id = task.tenant_id AND assignee.task_id = task.id
                 ORDER BY assignee.user_id) AS assignee_ids,
           ARRAY(SELECT link.tag_id FROM ai_task_tags AS link
                 WHERE link.task_id = task.id ORDER BY link.tag_id) AS tag_ids,
           ARRAY(SELECT attachment.id FROM ai_task_attachments AS attachment
                 WHERE attachment.tenant_id = task.tenant_id AND attachment.task_id = task.id
                 ORDER BY attachment.position, attachment.id) AS attachment_ids,
           ARRAY(SELECT DISTINCT step.assignee_user_id FROM ai_task_executions AS execution
                 JOIN ai_task_execution_steps AS step ON step.execution_id = execution.id
                 WHERE execution.tenant_id = task.tenant_id AND execution.task_id = task.id
                   AND execution.status IN ('queued', 'planning', 'running', 'reviewing')
                   AND step.status = 'running' AND step.assignee_user_id IS NOT NULL) AS waiting_user_ids,
           task.list_id, task.column_id, task.position,
           task.starts_at, task.due_at, task.all_day, task.time_zone,
           task.reminder_minutes, task.completed_at,
           latest.status AS latest_status, latest.at AS latest_at,
           task.next_run_at, task.created_at, task.updated_at,
           task.process_id, task.process_version, pinned.content->>'title' AS process_title,
           task.plan_steps,
           CASE WHEN task.plan_steps IS NOT NULL THEN COALESCE(progress.done, 0) END AS plan_done,
           CASE WHEN task.plan_steps IS NOT NULL
                THEN COALESCE(NULLIF(progress.total, 0), jsonb_array_length(task.plan_steps))
           END AS plan_total
    FROM ai_tasks AS task
    LEFT JOIN LATERAL (
        SELECT candidate.status, candidate.at FROM (
            (SELECT CASE
                        WHEN bool_or(run.status = 'processing') THEN 'running'
                        WHEN bool_or(run.status = 'pending') THEN 'queued'
                        WHEN bool_or(run.status = 'failed') THEN 'failed'
                        WHEN bool_and(run.status = 'cancelled') THEN 'cancelled'
                        ELSE 'succeeded'
                    END AS status,
                    run.scheduled_for AS at
             FROM ai_task_runs AS run
             WHERE run.tenant_id = task.tenant_id AND run.task_id = task.id
             GROUP BY run.scheduled_for ORDER BY run.scheduled_for DESC LIMIT 1)
            UNION ALL
            (SELECT CASE WHEN execution.status IN ('planning', 'reviewing') THEN 'running'
                         ELSE execution.status END,
                    COALESCE(execution.scheduled_for, execution.created_at)
             FROM ai_task_executions AS execution
             WHERE execution.tenant_id = task.tenant_id AND execution.task_id = task.id
             ORDER BY execution.created_at DESC LIMIT 1)
        ) AS candidate
        ORDER BY candidate.at DESC LIMIT 1
    ) AS latest ON true
    LEFT JOIN process_versions AS pinned
      ON pinned.tenant_id = task.tenant_id AND pinned.process_id = task.process_id
     AND pinned.version = task.process_version
    LEFT JOIN LATERAL (
        SELECT (count(*) FILTER (WHERE step.status = 'succeeded' AND NOT EXISTS (
                    SELECT 1 FROM ai_task_execution_steps AS correction
                    WHERE correction.execution_id = step.execution_id
                      AND correction.step_key = step.step_key || '_revision'
                      AND correction.status <> 'succeeded')))::integer AS done,
               count(*)::integer AS total
        FROM ai_task_execution_steps AS step
        WHERE step.execution_id = (
                SELECT execution.id FROM ai_task_executions AS execution
                WHERE execution.tenant_id = task.tenant_id AND execution.task_id = task.id
                ORDER BY execution.created_at DESC, execution.id DESC LIMIT 1)
          AND step.kind = 'work' AND step.revision = 0
    ) AS progress ON task.plan_steps IS NOT NULL
"#;

#[derive(Debug, Serialize)]
struct TaskListResponse {
    items: Vec<TaskResponse>,
}

#[derive(Debug, FromRow)]
struct TaskRunRow {
    id: Uuid,
    ai_profile_id: Option<Uuid>,
    agent_name: String,
    task_text: String,
    scheduled_for: DateTime<Utc>,
    status: String,
    attempts: i32,
    output: Option<String>,
    error: Option<String>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct TaskRunResponse {
    id: Uuid,
    agent_id: Option<Uuid>,
    agent_name: String,
    task_text: String,
    scheduled_for: DateTime<Utc>,
    status: String,
    attempts: i32,
    output: Option<String>,
    error: Option<String>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TaskRunRow> for TaskRunResponse {
    fn from(row: TaskRunRow) -> Self {
        Self {
            id: row.id,
            agent_id: row.ai_profile_id,
            agent_name: row.agent_name,
            task_text: row.task_text,
            scheduled_for: row.scheduled_for,
            status: row.status,
            attempts: row.attempts,
            output: row.output,
            error: row.error,
            started_at: row.started_at,
            completed_at: row.completed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskRunListResponse {
    items: Vec<TaskRunResponse>,
}

#[derive(Debug, FromRow)]
struct DueTaskRow {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    text: String,
    schedule: SqlJson<TaskSchedule>,
    next_run_at: DateTime<Utc>,
    execution_mode: String,
}

#[derive(Debug, FromRow)]
struct TaskRunJob {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    task_id: Uuid,
    ai_profile_id: Option<Uuid>,
    task_text: String,
    attempts: i32,
}

#[derive(Clone, Debug, Eq, FromRow, PartialEq)]
struct AgentExecutionConfig {
    id: Uuid,
    base_url: String,
    model: String,
}

async fn list_tasks(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<TaskListResponse>, AppError> {
    let task_actor = task_board::task_actor(&actor)?;
    let sql = format!(
        "{TASK_SELECT_SQL} WHERE task.tenant_id=$1 AND {TASK_ACCESS_SQL} ORDER BY task.created_at DESC, task.id DESC"
    );
    let rows = sqlx::query_as::<_, TaskRow>(&sql)
        .bind(actor.tenant_id)
        .bind(task_actor.project_id)
        .bind(actor.department_id())
        .bind(resource_visibility::owner_limit(&actor))
        .bind(task_actor.access.own_user())
        .fetch_all(&state.db)
        .await?;
    let items = rows.into_iter().map(TaskResponse::from_row).collect();
    Ok(Json(TaskListResponse { items }))
}

async fn list_task_runs(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(task_id): Path<Uuid>,
) -> Result<Json<TaskRunListResponse>, AppError> {
    let task_actor = task_board::task_actor(&actor)?;
    let project_id = task_board::task_project(&state, &actor, task_actor.access, task_id).await?;
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM ai_tasks
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let rows = sqlx::query_as::<_, TaskRunRow>(
        r#"
        SELECT id, ai_profile_id, agent_name, task_text, scheduled_for, status, attempts,
               output, error, started_at, completed_at, created_at, updated_at
        FROM ai_task_runs
        WHERE tenant_id = $1 AND project_id = $2 AND task_id = $3
        ORDER BY scheduled_for DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .bind(TASK_RUN_HISTORY_LIMIT)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(TaskRunListResponse {
        items: rows.into_iter().map(TaskRunResponse::from).collect(),
    }))
}

async fn run_task_now(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(task_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<TaskRunListResponse>, AppError> {
    require_project_wide_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Task, task_id).await?;
    let key = crate::ai_orchestration::idempotency_key(&headers)?;
    let mut tx = state.db.begin().await?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM ai_tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await?;
    if id.is_none() {
        return Err(AppError::NotFound);
    }
    let task = load_task_row(&mut tx, actor.tenant_id, project_id, task_id).await?;
    if task.execution_mode != "independent" {
        return Err(AppError::BadRequest(
            "Use a team execution for this task".into(),
        ));
    }
    if task.agent_ids.is_empty() {
        return Err(AppError::BadRequest(
            "Assign at least one agent to run this task".into(),
        ));
    }
    let existing:Option<DateTime<Utc>>=sqlx::query_scalar("SELECT scheduled_for FROM ai_task_manual_triggers WHERE tenant_id=$1 AND task_id=$2 AND idempotency_key=$3")
        .bind(actor.tenant_id).bind(task_id).bind(key).fetch_optional(&mut *tx).await?;
    if existing.is_none() {
        ensure_task_schedule_open(&mut tx, actor.tenant_id, task_id).await?;
        let active_project: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM projects WHERE tenant_id=$1 AND id=$2 AND status='active' FOR SHARE",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await?;
        if active_project.is_none() {
            return Err(AppError::BadRequest(
                "Activate this project before running a task".into(),
            ));
        }
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_task_runs WHERE tenant_id=$1 AND task_id=$2 AND status IN ('pending','processing')) OR EXISTS(SELECT 1 FROM ai_task_executions WHERE tenant_id=$1 AND task_id=$2 AND status IN ('queued','planning','running','reviewing','needs_attention'))")
            .bind(actor.tenant_id).bind(task_id).fetch_one(&mut *tx).await?;
        if busy {
            return Err(AppError::BadRequest(
                "This task already has an unfinished execution".into(),
            ));
        }
        let configs =
            preflight_task_agents(&state, actor.tenant_id, project_id, &task.agent_ids).await?;
        ensure_agents_executable(
            &mut tx,
            actor.tenant_id,
            project_id,
            &task.agent_ids,
            &configs,
        )
        .await?;
        let scheduled:DateTime<Utc>=sqlx::query_scalar("INSERT INTO ai_task_manual_triggers (tenant_id,project_id,task_id,idempotency_key) VALUES ($1,$2,$3,$4) RETURNING scheduled_for")
            .bind(actor.tenant_id).bind(project_id).bind(task_id).bind(key).fetch_one(&mut *tx).await?;
        for profile in &task.agent_ids {
            sqlx::query("INSERT INTO ai_task_runs (id,tenant_id,project_id,task_id,ai_profile_id,agent_name,task_text,scheduled_for,available_at) SELECT $1,$2,$3,$4,id,name,$6,$7,now() FROM ai_profiles WHERE tenant_id=$2 AND (project_id=$3 OR resource_visible(visibility,$3,NULL)) AND id=$5")
                .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project_id).bind(task_id).bind(profile).bind(&task.text).bind(scheduled).execute(&mut *tx).await?;
        }
        insert_audit(&mut tx, &actor, project_id, "ai_task.started", task_id).await?;
    }
    tx.commit().await?;
    list_task_runs(State(state), actor, Path(task_id)).await
}

pub(crate) async fn ensure_task_schedule_open(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    task_id: Uuid,
) -> Result<(), AppError> {
    let ended: bool = sqlx::query_scalar(
        r#"
        SELECT COALESCE((schedule->>'ends_at')::timestamp AT TIME ZONE
            (schedule->>'timezone') < clock_timestamp(), false)
        FROM ai_tasks WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(task_id)
    .fetch_one(&mut **tx)
    .await?;
    if ended {
        return Err(AppError::BadRequest("The task schedule has ended".into()));
    }
    Ok(())
}

/// When a task is worked on. `due_at` is the deadline and the end of the span,
/// `starts_at` is when work may begin; a task without a start is a bare
/// deadline, the shape every task had before spans. An instant cannot recover
/// the zone it was entered in, so `time_zone` carries it -- without it `all_day`
/// cannot say whose day it means.
#[derive(Debug, Default)]
pub(crate) struct TaskSpan {
    starts_at: Option<DateTime<Utc>>,
    due_at: Option<DateTime<Utc>>,
    all_day: bool,
    time_zone: Option<String>,
    reminder_minutes: Option<i32>,
}

/// Four weeks, the longest lead time a deadline can usefully carry.
const MAX_REMINDER_MINUTES: i32 = 40_320;

impl TaskSpan {
    /// Checked here rather than left to the `ai_tasks_duration` constraint: an
    /// update patches each end on its own, so the constraint would surface a
    /// one-sided change as a 500 instead of a 400. A span may lie in the past,
    /// as a deadline always could.
    fn new(
        starts_at: Option<DateTime<Utc>>,
        due_at: Option<DateTime<Utc>>,
        all_day: bool,
        time_zone: Option<String>,
        reminder_minutes: Option<i32>,
    ) -> Result<Self, AppError> {
        if starts_at.is_some_and(|start| due_at.is_none_or(|due| start > due)) {
            return Err(AppError::BadRequest(
                "a task must end no earlier than it starts".to_owned(),
            ));
        }
        if all_day && due_at.is_none() {
            return Err(AppError::BadRequest(
                "an all-day task needs a date".to_owned(),
            ));
        }
        if let Some(minutes) = reminder_minutes {
            // The reminder counts back from the start, or from the deadline
            // where there is no start, so it needs one of them to count from.
            if starts_at.is_none() && due_at.is_none() {
                return Err(AppError::BadRequest(
                    "a reminder needs a date to count back from".to_owned(),
                ));
            }
            if !(0..=MAX_REMINDER_MINUTES).contains(&minutes) {
                return Err(AppError::BadRequest(format!(
                    "a reminder must be between 0 and {MAX_REMINDER_MINUTES} minutes ahead"
                )));
            }
        }
        let time_zone = match time_zone {
            Some(zone) => Some(
                zone.trim()
                    .parse::<Tz>()
                    .map_err(|_| {
                        AppError::BadRequest("timezone must be a valid IANA timezone".to_owned())
                    })?
                    .to_string(),
            ),
            None => None,
        };
        Ok(Self {
            starts_at,
            due_at,
            all_day,
            time_zone,
            reminder_minutes,
        })
    }
}

fn board_unavailable() -> AppError {
    AppError::BadRequest(
        "Task boards, employees and tags are not available in this edition".to_owned(),
    )
}

async fn ensure_agent_access(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    input: &NormalizedTask,
) -> Result<Vec<AgentExecutionConfig>, AppError> {
    let execution_ids = input.execution_agent_ids();
    for id in &execution_ids {
        resource_visibility::owner_project(&state.db, actor, Resource::Profile, *id).await?;
    }
    if input.requires_executable_agents() {
        preflight_task_agents(state, actor.tenant_id, project_id, &execution_ids).await
    } else {
        Ok(Vec::new())
    }
}

async fn lock_task_agents(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    input: &NormalizedTask,
    agent_configs: &[AgentExecutionConfig],
) -> Result<(), AppError> {
    let execution_ids = input.execution_agent_ids();
    if input.requires_executable_agents() {
        ensure_agents_executable(
            transaction,
            tenant_id,
            project_id,
            &execution_ids,
            agent_configs,
        )
        .await
    } else {
        ensure_team_agents_exist(transaction, tenant_id, project_id, &execution_ids).await
    }
}

/// A validated new task that is ready to be stored.
pub(crate) struct PreparedTask {
    input: NormalizedTask,
    agent_configs: Vec<AgentExecutionConfig>,
    visibility: Option<ResourceVisibility>,
    attachment_ids: Option<Vec<Uuid>>,
    tag_ids: Option<Vec<Uuid>>,
    list_id: Option<Uuid>,
    column_id: Option<Uuid>,
    span: TaskSpan,
}

/// Validates a new task for a project the actor manages. It reads the
/// database and may call providers, so it runs before the storing transaction.
pub(crate) async fn prepare_task(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    mut request: TaskRequest,
) -> Result<PreparedTask, AppError> {
    let plan_steps = request.plan_steps.take().flatten();
    if request.uses_board_features() || plan_uses_employees(plan_steps.as_deref()) {
        return Err(board_unavailable());
    }
    let validation_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
        .fetch_one(&state.db)
        .await?;
    let visibility = request.visibility.clone();
    let attachment_ids = request.attachment_ids.clone();
    let tag_ids = normalize_tag_ids(request.tag_ids.clone())?;
    let (list_id, column_id) = (request.list_id, request.column_id);
    let span = TaskSpan::new(
        request.starts_at.flatten(),
        request.due_at.flatten(),
        request.all_day.unwrap_or_default(),
        request.time_zone.clone().flatten(),
        request.reminder_minutes.flatten(),
    )?;
    let assignee_ids = request.assignee_ids.clone().unwrap_or_default();
    let input = normalize_task(request, assignee_ids, plan_steps, validation_now)?;
    let agent_configs = ensure_agent_access(state, actor, project_id, &input).await?;
    Ok(PreparedTask {
        input,
        agent_configs,
        visibility,
        attachment_ids,
        tag_ids,
        list_id,
        column_id,
        span,
    })
}

/// Stores a prepared task with its participants, placement and audit entry
/// inside the caller's transaction. `process` marks a launched process.
pub(crate) async fn insert_prepared_task(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    task: PreparedTask,
    process: Option<ProcessMarker>,
) -> Result<TaskResponse, AppError> {
    let PreparedTask {
        input,
        agent_configs,
        visibility,
        attachment_ids,
        tag_ids,
        list_id,
        column_id,
        span,
    } = task;
    ensure_task_capacity(transaction, actor.tenant_id, project_id).await?;
    let now = transaction_now(transaction).await?;
    lock_task_agents(
        transaction,
        actor.tenant_id,
        project_id,
        &input,
        &agent_configs,
    )
    .await?;
    let next_run_at = required_next_occurrence(&input.schedule, now)?;
    let _ = (list_id, column_id);
    let placement: Option<(Uuid, Uuid, String)> = None;
    let position = 0;
    let task_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO ai_tasks (
            id, tenant_id, project_id, text, schedule, next_run_at, created_by, execution_mode, coordinator_id,
            expected_result, agent_roles, coordinator_user_id, list_id, column_id, position, due_at, completed_at,
            process_id, process_version, plan_steps, starts_at, all_day, time_zone, reminder_minutes
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                  CASE WHEN $17 THEN now() END, $18, $19, $20, $21, $22, $23, $24)
        "#,
    )
    .bind(task_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&input.text)
    .bind(SqlJson(&input.schedule))
    .bind(next_run_at)
    .bind(actor.actor_id)
    .bind(input.execution_mode.as_str())
    .bind(input.coordinator_id)
    .bind(&input.expected_result)
    .bind(SqlJson(&input.agent_roles))
    .bind(input.coordinator_user_id)
    .bind(placement.as_ref().map(|(list, _, _)| *list))
    .bind(placement.as_ref().map(|(_, column, _)| *column))
    .bind(position)
    .bind(span.due_at)
    .bind(placement.as_ref().is_some_and(|(_, _, kind)| kind == "done"))
    .bind(process.map(|marker| marker.process_id))
    .bind(process.map(|marker| marker.version))
    .bind(input.plan_steps.as_ref().map(SqlJson))
    .bind(span.starts_at)
    .bind(span.all_day)
    .bind(span.time_zone.as_deref())
    .bind(span.reminder_minutes)
    .execute(&mut **transaction)
    .await?;
    replace_task_agents(transaction, actor.tenant_id, task_id, &input.agent_ids).await?;
    let _ = tag_ids;
    resource_visibility::save(
        transaction,
        actor,
        Resource::Task,
        task_id,
        visibility.as_ref(),
        true,
    )
    .await?;
    crate::task_attachments::replace(
        transaction,
        actor,
        project_id,
        task_id,
        attachment_ids.as_deref(),
    )
    .await?;
    let row = load_task_row(transaction, actor.tenant_id, project_id, task_id).await?;
    insert_audit(transaction, actor, project_id, "ai_task.created", task_id).await?;
    Ok(TaskResponse::from_row(row))
}

async fn create_task(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<TaskRequest>,
) -> Result<(StatusCode, Json<TaskResponse>), AppError> {
    let project_id = require_project_wide_management(&actor)?;
    let task = prepare_task(&state, &actor, project_id, request).await?;
    let mut transaction = state.db.begin().await?;
    let created = insert_prepared_task(&mut transaction, &actor, project_id, task, None).await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_task(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(task_id): Path<Uuid>,
    Json(mut request): Json<TaskRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    require_project_wide_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Task, task_id).await?;
    if request.uses_board_features() {
        return Err(board_unavailable());
    }
    let plan_sent = request.plan_steps.is_some();
    let plan_steps = updated_plan_steps(
        &state,
        &actor,
        project_id,
        task_id,
        request.plan_steps.take(),
    )
    .await?;
    // A plan the request does not name is written back as it was read here.
    let kept_plan = (!plan_sent).then(|| plan_steps.clone());
    if plan_uses_employees(plan_steps.as_deref()) {
        return Err(board_unavailable());
    }
    let validation_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
        .fetch_one(&state.db)
        .await?;
    let visibility = request.visibility.clone();
    let attachment_ids = request.attachment_ids.clone();
    let _tag_ids = normalize_tag_ids(request.tag_ids.clone())?;
    // Without a plan the employees are rewritten when the request names them.
    let names_people = request.assignee_ids.is_some() || request.coordinator_user_id.is_some();
    let assignee_ids = match request.assignee_ids.clone() {
        Some(ids) => ids,
        None => {
            sqlx::query_scalar(
                "SELECT user_id FROM ai_task_assignees WHERE tenant_id=$1 AND task_id=$2 ORDER BY user_id",
            )
            .bind(actor.tenant_id)
            .bind(task_id)
            .fetch_all(&state.db)
            .await?
        }
    };
    let (_list_id, _column_id, due_at) = (request.list_id, request.column_id, request.due_at);
    let (starts_at, all_day, time_zone, reminder_minutes) = (
        request.starts_at,
        request.all_day,
        request.time_zone.clone(),
        request.reminder_minutes,
    );
    let input = normalize_task(request, assignee_ids, plan_steps, validation_now)?;
    let agent_configs = ensure_agent_access(&state, &actor, project_id, &input).await?;
    let mut transaction = state.db.begin().await?;
    let stored = lock_stored_team(&mut transaction, actor.tenant_id, project_id, task_id).await?;
    if kept_plan.is_some_and(|kept| kept != stored.plan_steps) {
        return Err(AppError::Conflict(
            "the task changed; reload it before saving".into(),
        ));
    }
    // Each column below is patched on its own, so an update touching one end of
    // the span is checked against the other as the locked row still holds it.
    let span = TaskSpan::new(
        starts_at.unwrap_or(stored.starts_at),
        due_at.unwrap_or(stored.due_at),
        all_day.unwrap_or(stored.all_day),
        time_zone.clone().unwrap_or(stored.time_zone),
        reminder_minutes.unwrap_or(stored.reminder_minutes),
    )?;
    // The performers of a plan are its team: its employees are checked and
    // rewritten when the request names the plan or changes who takes part.
    let _replace_people = if input.plan_steps.is_some() {
        plan_sent
            || input.assignee_ids != stored.assignee_ids
            || input.coordinator_user_id != stored.coordinator_user_id
    } else {
        names_people
    };
    let now = transaction_now(&mut transaction).await?;
    lock_task_agents(
        &mut transaction,
        actor.tenant_id,
        project_id,
        &input,
        &agent_configs,
    )
    .await?;
    let next_run_at = required_next_occurrence(&input.schedule, now)?;
    let current: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        r#"
        UPDATE ai_tasks
        SET text = $4, schedule = $5, next_run_at = $6, execution_mode=$7, coordinator_id=$8, expected_result=$9,
            agent_roles=$10, coordinator_user_id=$11, due_at = CASE WHEN $12 THEN $13 ELSE due_at END,
            plan_steps = $14,
            starts_at = CASE WHEN $15 THEN $16 ELSE starts_at END,
            all_day = CASE WHEN $17 THEN $18 ELSE all_day END,
            time_zone = CASE WHEN $19 THEN $20 ELSE time_zone END,
            reminder_minutes = CASE WHEN $21 THEN $22 ELSE reminder_minutes END,
            updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        RETURNING list_id, column_id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .bind(&input.text)
    .bind(SqlJson(&input.schedule))
    .bind(next_run_at)
    .bind(input.execution_mode.as_str())
    .bind(input.coordinator_id)
    .bind(&input.expected_result)
    .bind(SqlJson(&input.agent_roles))
    .bind(input.coordinator_user_id)
    .bind(due_at.is_some())
    .bind(span.due_at)
    .bind(input.plan_steps.as_ref().map(SqlJson))
    .bind(starts_at.is_some())
    .bind(span.starts_at)
    .bind(all_day.is_some())
    .bind(span.all_day)
    .bind(time_zone.is_some())
    .bind(span.time_zone.as_deref())
    .bind(reminder_minutes.is_some())
    .bind(span.reminder_minutes)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some((_current_list, _current_column)) = current else {
        return Err(AppError::NotFound);
    };
    sqlx::query(
        r#"
        UPDATE ai_task_runs
        SET status = 'cancelled',
            error = 'cancelled because the task was updated',
            completed_at = now(), updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND task_id = $3
          AND status = 'pending'
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .execute(&mut *transaction)
    .await?;
    prune_terminal_task_runs_in_transaction(&mut transaction, actor.tenant_id, task_id).await?;
    replace_task_agents(&mut transaction, actor.tenant_id, task_id, &input.agent_ids).await?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Task,
        task_id,
        visibility.as_ref(),
        false,
    )
    .await?;
    crate::task_attachments::replace(
        &mut transaction,
        &actor,
        project_id,
        task_id,
        attachment_ids.as_deref(),
    )
    .await?;
    let row = load_task_row(&mut transaction, actor.tenant_id, project_id, task_id).await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_task.updated",
        task_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(TaskResponse::from_row(row)))
}

async fn delete_task(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(task_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_project_wide_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Task, task_id).await?;
    let mut transaction = state.db.begin().await?;
    let locked_task = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM ai_tasks
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if locked_task.is_none() {
        return Err(AppError::NotFound);
    }
    let team_busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_task_executions WHERE tenant_id=$1 AND task_id=$2 AND status IN ('queued','planning','running','reviewing','needs_attention')) OR EXISTS(SELECT 1 FROM ai_task_step_attempts a JOIN ai_task_executions e ON e.id=a.execution_id WHERE e.tenant_id=$1 AND e.task_id=$2 AND a.status='running')")
        .bind(actor.tenant_id).bind(task_id).fetch_one(&mut *transaction).await?;
    if team_busy {
        return Err(AppError::BadRequest(
            "Stop the team execution before deleting its task".into(),
        ));
    }
    let run_statuses = sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM ai_task_runs
        WHERE tenant_id = $1 AND project_id = $2 AND task_id = $3
          AND status IN ('pending', 'processing')
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .fetch_all(&mut *transaction)
    .await?;
    if run_statuses.iter().any(|status| status == "processing") {
        return Err(AppError::BadRequest(
            "AI task cannot be deleted while a run is processing".to_owned(),
        ));
    }
    sqlx::query("DELETE FROM ai_tasks WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(task_id)
        .execute(&mut *transaction)
        .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_task.deleted",
        task_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Materializes due task occurrences and executes at most one per-agent run.
///
/// # Errors
///
/// Returns an error when scheduling, claiming, provider execution, or run
/// finalization fails.
pub async fn process_once(state: &AppState) -> AnyResult<bool> {
    let expired = cancel_expired_task_run(state).await?;
    let terminalized = terminalize_exhausted_stale_run(state).await?;
    let materialized = materialize_due_runs(state).await?;
    let lease_id = new_task_run_lease_id();
    let Some(job) = claim_run(state, lease_id).await? else {
        return Ok(expired || terminalized || materialized);
    };

    let capacity_lease = crate::ai_execution::Lease::new(&state.db, lease_id);
    let Some(ai_profile_id) = job.ai_profile_id else {
        fail_run(
            state,
            &job,
            lease_id,
            "assigned AI profile no longer exists",
            true,
        )
        .await?;
        capacity_lease.release().await?;
        return Ok(true);
    };

    let execution = tokio::time::timeout(
        TASK_RUN_EXECUTION_TIMEOUT,
        provider_reply::execute_task_run(
            state,
            job.id,
            job.tenant_id,
            job.project_id,
            ai_profile_id,
            &job.task_text,
        ),
    )
    .await;
    match execution {
        Err(_) => {
            fail_run(
                state,
                &job,
                lease_id,
                "AI task execution exceeded the per-attempt time limit",
                false,
            )
            .await?;
        }
        Ok(Ok(provider_reply::TaskExecutionResult::Completed(output))) => {
            let completed = sqlx::query(
                r#"
                UPDATE ai_task_runs
                SET status = 'completed', output = $3, error = NULL,
                    locked_at = NULL, locked_by = NULL,
                    completed_at = now(), updated_at = now()
                WHERE id = $1 AND locked_by = $2 AND status = 'processing'
                "#,
            )
            .bind(job.id)
            .bind(lease_id)
            .bind(output)
            .execute(&state.db)
            .await
            .context("failed to complete AI task run")?;
            if completed.rows_affected() == 1 {
                finish_board_task_if_occurrence_completed(state, &job).await?;
                prune_terminal_task_runs(&state.db, job.tenant_id, job.task_id).await?;
                info!(run_id = %job.id, task_id = %job.task_id, "AI task run completed");
            } else {
                warn!(
                    run_id = %job.id,
                    task_id = %job.task_id,
                    "AI task run completion skipped because its lease changed"
                );
            }
        }
        Ok(Ok(provider_reply::TaskExecutionResult::AgentUnavailable)) => {
            fail_run(
                state,
                &job,
                lease_id,
                "assigned AI profile is no longer active or executable",
                true,
            )
            .await?;
        }
        Ok(Ok(provider_reply::TaskExecutionResult::ProjectPaused)) => {
            pause_run_for_disabled_project(state, &job, lease_id).await?;
        }
        Ok(Err(execution_error)) => {
            let error_message = bounded_error(&execution_error);
            let permanent = provider_reply::task_execution_error_is_permanent(&execution_error);
            fail_run(state, &job, lease_id, &error_message, permanent).await?;
        }
    }

    capacity_lease.release().await?;
    Ok(true)
}

/// A one-time task is done once every agent of its occurrence completed.
async fn finish_board_task_if_occurrence_completed(
    state: &AppState,
    job: &TaskRunJob,
) -> AnyResult<()> {
    let complete: bool = sqlx::query_scalar(
        r#"
        SELECT NOT EXISTS (
            SELECT 1 FROM ai_task_runs AS other
            JOIN ai_task_runs AS finished ON finished.id = $3
            WHERE other.tenant_id = $1 AND other.task_id = $2
              AND other.scheduled_for = finished.scheduled_for AND other.status <> 'completed')
        "#,
    )
    .bind(job.tenant_id)
    .bind(job.task_id)
    .bind(job.id)
    .fetch_one(&state.db)
    .await?;
    if complete {
        task_board::mark_task_finished(&state.db, job.tenant_id, job.task_id).await?;
    }
    Ok(())
}

async fn cancel_expired_task_run(state: &AppState) -> AnyResult<bool> {
    let cancelled = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        WITH candidate AS (
            SELECT run.id FROM ai_task_runs AS run
            JOIN ai_tasks AS task
              ON task.tenant_id = run.tenant_id AND task.id = run.task_id
            WHERE (run.status = 'pending' OR
                   (run.status = 'processing' AND
                    run.locked_at <= now() - ($1 * interval '1 second')))
              AND (task.schedule->>'ends_at')::timestamp AT TIME ZONE
                  (task.schedule->>'timezone') < statement_timestamp()
            ORDER BY run.scheduled_for, run.id
            FOR UPDATE OF run SKIP LOCKED LIMIT 1
        )
        UPDATE ai_task_runs AS run
        SET status = 'cancelled', error = 'The task schedule has ended',
            locked_at = NULL, locked_by = NULL,
            completed_at = now(), updated_at = now()
        FROM candidate WHERE run.id = candidate.id
        RETURNING run.tenant_id, run.task_id
        "#,
    )
    .bind(TASK_RUN_LOCK_TIMEOUT_SECONDS)
    .fetch_optional(&state.db)
    .await
    .context("failed to cancel an expired AI task run")?;
    if let Some((tenant_id, task_id)) = cancelled {
        prune_terminal_task_runs(&state.db, tenant_id, task_id).await?;
        return Ok(true);
    }
    Ok(false)
}

async fn terminalize_exhausted_stale_run(state: &AppState) -> AnyResult<bool> {
    let terminalized = sqlx::query_as::<_, (Uuid, Uuid, Uuid)>(
        r#"
        WITH candidate AS (
            SELECT id
            FROM ai_task_runs
            WHERE status = 'processing'
              AND locked_at <= now() - ($1 * interval '1 second')
              AND attempts >= $2
            ORDER BY locked_at, id
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE ai_task_runs AS run
        SET status = 'failed',
            error = 'run lease expired after the maximum number of attempts',
            locked_at = NULL, locked_by = NULL,
            completed_at = now(), updated_at = now()
        FROM candidate
        WHERE run.id = candidate.id
        RETURNING run.id, run.tenant_id, run.task_id
        "#,
    )
    .bind(TASK_RUN_LOCK_TIMEOUT_SECONDS)
    .bind(state.config.outbox.max_attempts)
    .fetch_optional(&state.db)
    .await
    .context("failed to terminalize an exhausted stale AI task run")?;
    if let Some((run_id, tenant_id, task_id)) = terminalized {
        prune_terminal_task_runs(&state.db, tenant_id, task_id).await?;
        error!(%run_id, "exhausted stale AI task run terminalized");
        return Ok(true);
    }
    Ok(false)
}

async fn materialize_due_runs(state: &AppState) -> AnyResult<bool> {
    let mut transaction = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AI_TASK_MATERIALIZE_ADVISORY_LOCK)
        .execute(&mut *transaction)
        .await?;
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
        .fetch_one(&mut *transaction)
        .await?;
    let task = sqlx::query_as::<_, DueTaskRow>(SELECT_DUE_TASK_SQL)
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .context("failed to claim a due AI task")?;
    let Some(task) = task else {
        transaction.rollback().await?;
        return Ok(false);
    };

    let active_project = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM projects
        WHERE tenant_id = $1 AND id = $2 AND status = 'active'
        FOR KEY SHARE
        "#,
    )
    .bind(task.tenant_id)
    .bind(task.project_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if active_project.is_none() {
        transaction.rollback().await?;
        return Ok(false);
    }

    let has_nonterminal_run = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM ai_task_runs
            WHERE tenant_id = $1
              AND task_id = $2
              AND status IN ('pending', 'processing')
        ) OR EXISTS (
            SELECT 1 FROM ai_task_executions
            WHERE tenant_id=$1 AND task_id=$2
              AND status IN ('queued','planning','running','reviewing','needs_attention')
        )
        "#,
    )
    .bind(task.tenant_id)
    .bind(task.id)
    .fetch_one(&mut *transaction)
    .await?;

    let schedule_ended = task.schedule.0.has_ended_at(now)?;
    if !schedule_ended && task.execution_mode == "team" {
        if !has_nonterminal_run {
            crate::ai_orchestration::materialize_scheduled(
                &mut transaction,
                task.tenant_id,
                task.project_id,
                task.id,
                task.next_run_at,
            )
            .await?;
        }
    } else if !schedule_ended && should_materialize_occurrence(has_nonterminal_run) {
        let agents = sqlx::query_as::<_, (Uuid, String)>(
            r#"
        SELECT assignment.ai_profile_id, profile.name
        FROM ai_task_agents AS assignment
        JOIN ai_profiles AS profile
          ON profile.tenant_id = assignment.tenant_id
         AND profile.id = assignment.ai_profile_id
        WHERE assignment.tenant_id = $1 AND assignment.task_id = $2
        ORDER BY assignment.ai_profile_id
        "#,
        )
        .bind(task.tenant_id)
        .bind(task.id)
        .fetch_all(&mut *transaction)
        .await?;
        for (ai_profile_id, agent_name) in agents {
            sqlx::query(
                r#"
            INSERT INTO ai_task_runs (
                id, tenant_id, project_id, task_id, ai_profile_id,
                agent_name, task_text, scheduled_for, available_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
            ON CONFLICT (tenant_id, task_id, ai_profile_id, scheduled_for) DO NOTHING
            "#,
            )
            .bind(Uuid::now_v7())
            .bind(task.tenant_id)
            .bind(task.project_id)
            .bind(task.id)
            .bind(ai_profile_id)
            .bind(agent_name)
            .bind(&task.text)
            .bind(task.next_run_at)
            .execute(&mut *transaction)
            .await?;
        }
    }

    // At most one missed occurrence is materialized above. Advancing from the
    // current database time coalesces both long outages and a due occurrence
    // whose prior per-agent runs are still outstanding.
    let next_run_at = task
        .schedule
        .0
        .next_occurrence_after(now)
        .context("stored AI task schedule is invalid")?;
    sqlx::query(
        r#"
        UPDATE ai_tasks
        SET next_run_at = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(task.tenant_id)
    .bind(task.id)
    .bind(next_run_at)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(ADVANCE_MATERIALIZATION_CURSOR_SQL)
        .bind(task.tenant_id)
        .bind(task.project_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(true)
}

const fn should_materialize_occurrence(has_nonterminal_run: bool) -> bool {
    !has_nonterminal_run
}

fn new_task_run_lease_id() -> Uuid {
    Uuid::now_v7()
}

async fn claim_run(state: &AppState, lease_id: Uuid) -> AnyResult<Option<TaskRunJob>> {
    let mut transaction = state.db.begin().await?;
    let Some(capacity) =
        crate::ai_execution::available(&mut transaction, state.config.worker.ai_run_concurrency)
            .await?
    else {
        transaction.commit().await?;
        return Ok(None);
    };
    let job = sqlx::query_as::<_, TaskRunJob>(CLAIM_TASK_RUN_SQL)
        .bind(state.config.outbox.max_attempts)
        .bind(TASK_RUN_LOCK_TIMEOUT_SECONDS)
        .bind(lease_id)
        .bind(&capacity.blocked_profiles)
        .fetch_optional(&mut *transaction)
        .await
        .context("failed to claim an AI task run")?;
    if let Some(job) = &job {
        crate::ai_execution::reserve(
            &mut transaction,
            lease_id,
            job.ai_profile_id,
            None,
            TASK_RUN_LOCK_TIMEOUT_SECONDS,
        )
        .await?;
        sqlx::query(ADVANCE_CLAIM_CURSOR_SQL)
            .bind(job.tenant_id)
            .bind(job.project_id)
            .execute(&mut *transaction)
            .await?;
        task_board::mark_task_started(&mut *transaction, job.tenant_id, job.task_id).await?;
    }
    transaction.commit().await?;
    Ok(job)
}

async fn fail_run(
    state: &AppState,
    job: &TaskRunJob,
    worker_id: Uuid,
    error_message: &str,
    permanent: bool,
) -> AnyResult<()> {
    let terminal = permanent || job.attempts >= state.config.outbox.max_attempts;
    let delay = outbox::retry_delay(job.attempts);
    let failed = sqlx::query(
        r#"
        UPDATE ai_task_runs
        SET status = CASE WHEN $3 THEN 'failed' ELSE 'pending' END,
            available_at = now() + ($4 * interval '1 second'),
            locked_at = NULL, locked_by = NULL, error = $5,
            completed_at = CASE WHEN $3 THEN now() ELSE NULL END,
            updated_at = now()
        WHERE id = $1 AND locked_by = $2 AND status = 'processing'
        "#,
    )
    .bind(job.id)
    .bind(worker_id)
    .bind(terminal)
    .bind(i64::try_from(delay.as_secs()).unwrap_or(i64::MAX))
    .bind(error_message)
    .execute(&state.db)
    .await
    .context("failed to reschedule an AI task run")?;
    if failed.rows_affected() == 1 {
        if terminal {
            prune_terminal_task_runs(&state.db, job.tenant_id, job.task_id).await?;
        }
        error!(
            run_id = %job.id,
            task_id = %job.task_id,
            attempts = job.attempts,
            terminal,
            error = %error_message,
            "AI task run failed"
        );
    } else {
        warn!(
            run_id = %job.id,
            task_id = %job.task_id,
            "AI task run failure skipped because its lease changed"
        );
    }
    Ok(())
}

async fn pause_run_for_disabled_project(
    state: &AppState,
    job: &TaskRunJob,
    lease_id: Uuid,
) -> AnyResult<()> {
    let paused = sqlx::query(
        r#"
        UPDATE ai_task_runs
        SET status = 'pending', attempts = GREATEST(attempts - 1, 0),
            available_at = now(), locked_at = NULL, locked_by = NULL,
            error = NULL, completed_at = NULL, updated_at = now()
        WHERE id = $1 AND locked_by = $2 AND status = 'processing'
        "#,
    )
    .bind(job.id)
    .bind(lease_id)
    .execute(&state.db)
    .await
    .context("failed to pause an AI task run for a disabled project")?;
    if paused.rows_affected() == 1 {
        info!(
            run_id = %job.id,
            task_id = %job.task_id,
            "AI task run returned to pending because the project is disabled"
        );
    } else {
        warn!(
            run_id = %job.id,
            task_id = %job.task_id,
            "AI task project-pause skipped because its lease changed"
        );
    }
    Ok(())
}

async fn prune_terminal_task_runs(
    db: &sqlx::PgPool,
    tenant_id: Uuid,
    task_id: Uuid,
) -> AnyResult<()> {
    sqlx::query(PRUNE_TERMINAL_TASK_RUNS_SQL)
        .bind(tenant_id)
        .bind(task_id)
        .bind(TASK_RUN_HISTORY_LIMIT)
        .execute(db)
        .await
        .context("failed to prune terminal AI task runs")?;
    Ok(())
}

async fn prune_terminal_task_runs_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    task_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(PRUNE_TERMINAL_TASK_RUNS_SQL)
        .bind(tenant_id)
        .bind(task_id)
        .bind(TASK_RUN_HISTORY_LIMIT)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn bounded_error(error: &anyhow::Error) -> String {
    format!("{error:#}")
        .chars()
        .take(MAX_STORED_ERROR_CHARS)
        .collect()
}

/// The plan a task holds after an update: an omitted field keeps the stored
/// plan, null clears it and an array replaces it.
// Distinguishes an omitted plan from an explicit null.
#[allow(clippy::option_option)]
async fn updated_plan_steps(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    task_id: Uuid,
    requested: Option<Option<Vec<PlanStep>>>,
) -> Result<Option<Vec<PlanStep>>, AppError> {
    let (process_id, stored): (Option<Uuid>, Option<SqlJson<Vec<PlanStep>>>) = sqlx::query_as(
        "SELECT process_id, plan_steps FROM ai_tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    resolve_plan_update(requested, stored.map(|steps| steps.0), process_id.is_some())
}

/// The plan and the employees of a task as they are stored.
struct StoredTeam {
    plan_steps: Option<Vec<PlanStep>>,
    coordinator_user_id: Option<Uuid>,
    assignee_ids: Vec<Uuid>,
    starts_at: Option<DateTime<Utc>>,
    due_at: Option<DateTime<Utc>>,
    all_day: bool,
    time_zone: Option<String>,
    reminder_minutes: Option<i32>,
}

/// Reads the stored plan and employees of a task under its row lock, so an
/// update decides what it keeps from what no other update can change any more.
async fn lock_stored_team(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<StoredTeam, AppError> {
    #[allow(clippy::type_complexity)]
    let (plan_steps, coordinator_user_id, starts_at, due_at, all_day, time_zone, reminder_minutes): (
        Option<SqlJson<Vec<PlanStep>>>,
        Option<Uuid>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        bool,
        Option<String>,
        Option<i32>,
    ) = sqlx::query_as(
        "SELECT plan_steps, coordinator_user_id, starts_at, due_at, all_day, time_zone, reminder_minutes
         FROM ai_tasks WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let assignee_ids = sqlx::query_scalar(
        "SELECT user_id FROM ai_task_assignees WHERE tenant_id=$1 AND task_id=$2 ORDER BY user_id",
    )
    .bind(tenant_id)
    .bind(task_id)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(StoredTeam {
        plan_steps: plan_steps.map(|steps| steps.0),
        coordinator_user_id,
        assignee_ids,
        starts_at,
        due_at,
        all_day,
        time_zone,
        reminder_minutes,
    })
}

/// A launched process keeps a plan: its steps can be edited but not removed.
// Distinguishes an omitted plan from an explicit null.
#[allow(clippy::option_option)]
fn resolve_plan_update(
    requested: Option<Option<Vec<PlanStep>>>,
    stored: Option<Vec<PlanStep>>,
    launched_from_process: bool,
) -> Result<Option<Vec<PlanStep>>, AppError> {
    match requested {
        None => Ok(stored),
        Some(None) if launched_from_process => Err(AppError::BadRequest(
            "The steps of a launched process can be edited but not removed".into(),
        )),
        Some(plan_steps) => Ok(plan_steps),
    }
}

/// Trims and checks a predefined plan; the order of the steps is kept.
fn normalize_plan_steps(steps: Vec<PlanStep>) -> Result<Vec<PlanStep>, AppError> {
    if !(1..=MAX_PLAN_STEPS).contains(&steps.len()) {
        return Err(AppError::BadRequest(format!(
            "plan_steps must contain between 1 and {MAX_PLAN_STEPS} steps"
        )));
    }
    let mut ids = BTreeSet::new();
    steps
        .into_iter()
        .map(|step| {
            if step.id.is_nil() || !ids.insert(step.id) {
                return Err(AppError::BadRequest(
                    "plan step ids must be unique and nonzero".into(),
                ));
            }
            Ok(PlanStep {
                id: step.id,
                title: plan_step_text(step.title, MAX_PLAN_STEP_TITLE_CHARS, "titles")?,
                instructions: plan_step_text(
                    step.instructions,
                    MAX_PLAN_STEP_INSTRUCTIONS_CHARS,
                    "instructions",
                )?,
                performer: step.performer,
            })
        })
        .collect()
}

fn plan_step_text(value: String, maximum: usize, field: &str) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if !(1..=maximum).contains(&value.chars().count()) || value.contains('\0') {
        return Err(AppError::BadRequest(format!(
            "plan step {field} must contain between 1 and {maximum} characters"
        )));
    }
    Ok(value)
}

/// The team of a plan task: its distinct performers without the coordinator,
/// as sorted agent ids and employee ids. The coordinator may perform steps.
fn plan_members(
    plan_steps: &[PlanStep],
    coordinator_id: Option<Uuid>,
    coordinator_user_id: Option<Uuid>,
) -> (Vec<Uuid>, Vec<Uuid>) {
    let members = |kind: PerformerKind, coordinator: Option<Uuid>| {
        plan_steps
            .iter()
            .filter(|step| step.performer.kind == kind && Some(step.performer.id) != coordinator)
            .map(|step| step.performer.id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };
    (
        members(PerformerKind::Agent, coordinator_id),
        members(PerformerKind::Employee, coordinator_user_id),
    )
}

fn normalize_task(
    request: TaskRequest,
    assignee_ids: Vec<Uuid>,
    plan_steps: Option<Vec<PlanStep>>,
    now: DateTime<Utc>,
) -> Result<NormalizedTask, AppError> {
    let text = request.text.trim().to_owned();
    let text_characters = text.chars().count();
    // PostgreSQL text cannot hold a NUL character.
    if !(1..=MAX_TASK_TEXT_CHARS).contains(&text_characters) || text.contains('\0') {
        return Err(AppError::BadRequest(format!(
            "text must contain between 1 and {MAX_TASK_TEXT_CHARS} characters"
        )));
    }
    let schedule = normalize_schedule(request.schedule, now)?;
    // The performers of a predefined plan replace whatever members were sent.
    let (mut agent_ids, mut assignee_ids) = if plan_steps.is_some() {
        (Vec::new(), Vec::new())
    } else {
        (
            normalize_agent_ids(request.agent_ids)?,
            normalize_participant_ids(assignee_ids, "assignee_ids")?,
        )
    };
    let expected_result = request.expected_result.trim().to_owned();
    if expected_result.chars().count() > 5000 {
        return Err(AppError::BadRequest(
            "expected_result must contain at most 5000 characters".into(),
        ));
    }
    let plan_steps = plan_steps.map(normalize_plan_steps).transpose()?;
    let mut roles = request.agent_roles;
    if request.execution_mode == ExecutionMode::Team {
        if request.coordinator_id.is_some() == request.coordinator_user_id.is_some() {
            return Err(AppError::BadRequest(
                "Select one coordinator: an agent or an employee".into(),
            ));
        }
        let performs = |id: Uuid| {
            plan_steps
                .as_ref()
                .is_some_and(|steps| steps.iter().any(|step| step.performer.id == id))
        };
        if let Some(plan_steps) = &plan_steps {
            (agent_ids, assignee_ids) = plan_members(
                plan_steps,
                request.coordinator_id,
                request.coordinator_user_id,
            );
            // Roles of people who left the plan go with them.
            roles.retain(|role| performs(role.agent_id));
        } else {
            let members = agent_ids.len() + assignee_ids.len();
            if !(1..=8).contains(&members)
                || request
                    .coordinator_id
                    .is_some_and(|coordinator| agent_ids.contains(&coordinator))
                || request
                    .coordinator_user_id
                    .is_some_and(|coordinator| assignee_ids.contains(&coordinator))
            {
                return Err(AppError::BadRequest(
                    "Select 1 to 8 team members distinct from the coordinator".into(),
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for role in &mut roles {
            role.role = role.role.trim().to_owned();
            // A coordinator who performs a step of the plan may have a role too.
            let member = agent_ids.contains(&role.agent_id)
                || assignee_ids.contains(&role.agent_id)
                || performs(role.agent_id);
            if !member || !seen.insert(role.agent_id) || role.role.chars().count() > 500 {
                return Err(AppError::BadRequest("Team roles must reference distinct selected members and contain at most 500 characters".into()));
            }
        }
    } else {
        if plan_steps.is_some() {
            return Err(AppError::BadRequest(
                "plan_steps require team execution mode".into(),
            ));
        }
        if request.coordinator_id.is_some()
            || request.coordinator_user_id.is_some()
            || !roles.is_empty()
        {
            return Err(AppError::BadRequest(
                "Coordinator and team roles require team execution mode".into(),
            ));
        }
        if agent_ids.is_empty() && !matches!(schedule, TaskSchedule::Manual) {
            return Err(AppError::BadRequest(
                "A scheduled task needs at least one agent to run it".into(),
            ));
        }
    }
    Ok(NormalizedTask {
        text,
        schedule,
        agent_ids,
        assignee_ids,
        execution_mode: request.execution_mode,
        coordinator_id: request.coordinator_id,
        coordinator_user_id: request.coordinator_user_id,
        expected_result,
        agent_roles: roles,
        plan_steps,
    })
}

fn normalize_schedule(
    schedule: TaskSchedule,
    now: DateTime<Utc>,
) -> Result<TaskSchedule, AppError> {
    if let Some(ends_at) = schedule
        .ends_at_utc()
        .map_err(|error| AppError::BadRequest(error.to_string()))?
        && ends_at <= now
    {
        return Err(AppError::BadRequest("ends_at must be in the future".into()));
    }
    match schedule {
        TaskSchedule::Manual => Ok(TaskSchedule::Manual),
        TaskSchedule::Once { run_at } => Ok(TaskSchedule::Once { run_at }),
        TaskSchedule::Daily {
            time,
            timezone,
            ends_at,
        } => {
            let (time, timezone) = normalize_recurrence(time, timezone)?;
            Ok(TaskSchedule::Daily {
                time,
                timezone,
                ends_at,
            })
        }
        TaskSchedule::Weekly {
            time,
            timezone,
            weekdays,
            ends_at,
        } => {
            let (time, timezone) = normalize_recurrence(time, timezone)?;
            let weekdays = normalize_numbers(weekdays, 1, 7, "weekdays")?;
            Ok(TaskSchedule::Weekly {
                time,
                timezone,
                weekdays,
                ends_at,
            })
        }
        TaskSchedule::Monthly {
            time,
            timezone,
            month_days,
            ends_at,
        } => {
            let (time, timezone) = normalize_recurrence(time, timezone)?;
            let month_days = normalize_numbers(month_days, 1, 31, "month_days")?;
            Ok(TaskSchedule::Monthly {
                time,
                timezone,
                month_days,
                ends_at,
            })
        }
        TaskSchedule::Yearly {
            time,
            timezone,
            mut dates,
            ends_at,
        } => {
            let (time, timezone) = normalize_recurrence(time, timezone)?;
            if dates.is_empty() {
                return Err(AppError::BadRequest(
                    "dates must contain at least one date".to_owned(),
                ));
            }
            if dates.len() > MAX_YEARLY_DATES {
                return Err(AppError::BadRequest(format!(
                    "dates must contain at most {MAX_YEARLY_DATES} dates"
                )));
            }
            if dates
                .iter()
                .any(|date| NaiveDate::from_ymd_opt(2000, date.month, date.day).is_none())
            {
                return Err(AppError::BadRequest(
                    "dates contain an invalid month and day".to_owned(),
                ));
            }
            dates.sort_unstable();
            dates.dedup();
            Ok(TaskSchedule::Yearly {
                time,
                timezone,
                dates,
                ends_at,
            })
        }
    }
}

fn normalize_recurrence(time: String, timezone: String) -> Result<(String, String), AppError> {
    let time = normalize_time(&time)?;
    let timezone = timezone.trim();
    let timezone = timezone
        .parse::<Tz>()
        .map_err(|_| AppError::BadRequest("timezone must be a valid IANA timezone".to_owned()))?;
    Ok((time.format("%H:%M").to_string(), timezone.to_string()))
}

fn normalize_time(value: &str) -> Result<NaiveTime, AppError> {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || !bytes[..2].iter().all(u8::is_ascii_digit)
        || !bytes[3..].iter().all(u8::is_ascii_digit)
    {
        return Err(AppError::BadRequest(
            "time must use the HH:MM format".to_owned(),
        ));
    }
    let hour = u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0');
    let minute = u32::from(bytes[3] - b'0') * 10 + u32::from(bytes[4] - b'0');
    NaiveTime::from_hms_opt(hour, minute, 0)
        .ok_or_else(|| AppError::BadRequest("time must be a valid clock time".to_owned()))
}

fn normalize_numbers(
    mut values: Vec<u32>,
    minimum: u32,
    maximum: u32,
    field: &str,
) -> Result<Vec<u32>, AppError> {
    if values.is_empty() {
        return Err(AppError::BadRequest(format!(
            "{field} must contain at least one value"
        )));
    }
    if values
        .iter()
        .any(|value| !(minimum..=maximum).contains(value))
    {
        return Err(AppError::BadRequest(format!(
            "{field} values must be between {minimum} and {maximum}"
        )));
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

fn normalize_agent_ids(agent_ids: Vec<Uuid>) -> Result<Vec<Uuid>, AppError> {
    normalize_participant_ids(agent_ids, "agent_ids")
}

fn normalize_participant_ids(mut ids: Vec<Uuid>, field: &str) -> Result<Vec<Uuid>, AppError> {
    ids.sort_unstable();
    ids.dedup();
    if ids.len() > MAX_TASK_AGENTS {
        return Err(AppError::BadRequest(format!(
            "{field} must contain at most {MAX_TASK_AGENTS} participants"
        )));
    }
    Ok(ids)
}

fn normalize_tag_ids(tag_ids: Option<Vec<Uuid>>) -> Result<Option<Vec<Uuid>>, AppError> {
    let Some(mut tag_ids) = tag_ids else {
        return Ok(None);
    };
    tag_ids.sort_unstable();
    tag_ids.dedup();
    if tag_ids.len() > task_board::MAX_TASK_TAGS {
        return Err(AppError::BadRequest(format!(
            "tag_ids must contain at most {} tags",
            task_board::MAX_TASK_TAGS
        )));
    }
    Ok(Some(tag_ids))
}

fn required_next_occurrence(
    schedule: &TaskSchedule,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, AppError> {
    if matches!(schedule, TaskSchedule::Manual | TaskSchedule::Once { .. }) {
        return schedule
            .next_occurrence_after(now)
            .map_err(AppError::internal);
    }
    schedule
        .next_occurrence_after(now)
        .map_err(AppError::internal)?
        .map(Some)
        .ok_or_else(|| AppError::BadRequest("schedule has no future occurrence".to_owned()))
}

async fn ensure_team_agents_exist(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    agent_ids: &[Uuid],
) -> Result<(), AppError> {
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM ai_profiles WHERE tenant_id=$1 AND (project_id=$2 OR resource_visible(visibility,$2,NULL)) AND id=ANY($3) ORDER BY id FOR KEY SHARE")
        .bind(tenant_id).bind(project_id).bind(agent_ids).fetch_all(&mut **transaction).await?;
    if ids != agent_ids {
        return Err(AppError::BadRequest(
            "Every agent must belong to the selected project".into(),
        ));
    }
    Ok(())
}

fn parse_normalized_recurrence(time: &str, timezone: &str) -> AnyResult<(NaiveTime, Tz)> {
    let time =
        NaiveTime::parse_from_str(time, "%H:%M").context("stored AI task time is invalid")?;
    let timezone = timezone
        .parse::<Tz>()
        .map_err(|_| anyhow::anyhow!("stored AI task timezone is invalid"))?;
    Ok((time, timezone))
}

fn next_matching_occurrence(
    now: DateTime<Utc>,
    timezone: Tz,
    time: NaiveTime,
    maximum_days: u64,
    matches: impl Fn(NaiveDate) -> bool,
) -> AnyResult<Option<DateTime<Utc>>> {
    let start_date = now.with_timezone(&timezone).date_naive();
    for offset in 0..=maximum_days {
        let date = start_date
            .checked_add_days(Days::new(offset))
            .context("AI task recurrence exceeds the supported date range")?;
        if !matches(date) {
            continue;
        }
        let local = date.and_time(time);
        let Some(candidate) = resolve_local_datetime(timezone, local) else {
            continue;
        };
        let candidate = candidate.with_timezone(&Utc);
        if candidate > now {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn resolve_local_datetime(timezone: Tz, local: NaiveDateTime) -> Option<DateTime<Tz>> {
    for minute in 0..=MAX_DST_FORWARD_MINUTES {
        let candidate = local.checked_add_signed(ChronoDuration::minutes(minute))?;
        match timezone.from_local_datetime(&candidate) {
            LocalResult::Single(value) => return Some(value),
            LocalResult::Ambiguous(first, second) => return Some(first.min(second)),
            LocalResult::None => {}
        }
    }
    None
}

async fn transaction_now(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<DateTime<Utc>, AppError> {
    sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut **transaction)
        .await
        .map_err(AppError::from)
}

async fn ensure_agents_executable(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    agent_ids: &[Uuid],
    expected: &[AgentExecutionConfig],
) -> Result<(), AppError> {
    let executable = sqlx::query_as::<_, AgentExecutionConfig>(
        r#"
        SELECT profile.id, provider.base_url,
               COALESCE(profile.model, provider.default_model) AS model
        FROM ai_profiles AS profile
        JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
        WHERE profile.tenant_id = $1
          AND (profile.project_id = $2 OR resource_visible(profile.visibility,$2,NULL))
          AND profile.id = ANY($3)
          AND profile.status = 'active'
          AND provider.status = 'active'
          AND provider.provider_kind IN ('openai', 'openai_compatible')
        ORDER BY profile.id
        FOR KEY SHARE OF profile, provider
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(agent_ids)
    .fetch_all(&mut **transaction)
    .await?;
    let executable_ids = executable
        .iter()
        .map(|configuration| configuration.id)
        .collect::<Vec<_>>();
    if executable_ids != agent_ids {
        return Err(AppError::BadRequest(
            "agent_ids must reference active agents with active OpenAI-compatible providers in the selected project"
                .to_owned(),
        ));
    }
    if executable != expected {
        return Err(AppError::BadRequest(
            "an assigned agent configuration changed while the task was being saved; retry the request"
                .to_owned(),
        ));
    }
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT knowledge_base.id
        FROM ai_profile_knowledge_bases AS assignment
        JOIN knowledge_bases AS knowledge_base
          ON knowledge_base.tenant_id = assignment.tenant_id
         AND knowledge_base.id = assignment.knowledge_base_id
        WHERE assignment.tenant_id = $1
          AND assignment.ai_profile_id = ANY($2)
        ORDER BY knowledge_base.id
        FOR KEY SHARE OF knowledge_base
        "#,
    )
    .bind(tenant_id)
    .bind(agent_ids)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(())
}

async fn preflight_task_agents(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    agent_ids: &[Uuid],
) -> Result<Vec<AgentExecutionConfig>, AppError> {
    let executable = sqlx::query_as::<_, AgentExecutionConfig>(
        r#"
        SELECT profile.id, provider.base_url,
               COALESCE(profile.model, provider.default_model) AS model
        FROM ai_profiles AS profile
        JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
        WHERE profile.tenant_id = $1
          AND (profile.project_id = $2 OR resource_visible(profile.visibility,$2,NULL))
          AND profile.id = ANY($3)
          AND profile.status = 'active'
          AND provider.status = 'active'
          AND provider.provider_kind IN ('openai', 'openai_compatible')
        ORDER BY profile.id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(agent_ids)
    .fetch_all(&state.db)
    .await?;
    let executable_ids = executable
        .iter()
        .map(|configuration| configuration.id)
        .collect::<Vec<_>>();
    if executable_ids != agent_ids {
        return Err(AppError::BadRequest(
            "agent_ids must reference active agents with active OpenAI-compatible providers in the selected project"
                .to_owned(),
        ));
    }
    for configuration in &executable {
        if provider_reply::model_targets_openclaw(&configuration.model)
            && !state
                .config
                .openclaw
                .handles_provider(&configuration.base_url)
        {
            return Err(AppError::BadRequest(
                "OpenClaw task agents must use the configured OpenClaw provider endpoint"
                    .to_owned(),
            ));
        }
    }
    let base_urls = executable
        .iter()
        .map(|configuration| configuration.base_url.as_str())
        .collect::<BTreeSet<_>>();
    futures_util::future::try_join_all(
        base_urls
            .into_iter()
            .map(|base_url| provider_reply::validate_task_provider_base_url(state, base_url)),
    )
    .await
    .map_err(|_| {
        AppError::BadRequest(
            "scheduled task agents must use the configured OpenClaw runtime or a public HTTPS provider endpoint"
                .to_owned(),
        )
    })?;
    Ok(executable)
}

async fn replace_task_agents(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    task_id: Uuid,
    agent_ids: &[Uuid],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM ai_task_agents WHERE tenant_id = $1 AND task_id = $2")
        .bind(tenant_id)
        .bind(task_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        r#"
        INSERT INTO ai_task_agents (tenant_id, task_id, ai_profile_id)
        SELECT $1, $2, agent_id
        FROM unnest($3::uuid[]) AS agent_id
        "#,
    )
    .bind(tenant_id)
    .bind(task_id)
    .bind(agent_ids)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_task_row(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    task_id: Uuid,
) -> Result<TaskRow, AppError> {
    sqlx::query_as::<_, TaskRow>(&format!(
        "{TASK_SELECT_SQL} WHERE task.tenant_id = $1 AND task.project_id = $2 AND task.id = $3"
    ))
    .bind(tenant_id)
    .bind(project_id)
    .bind(task_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

async fn ensure_task_capacity(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM projects WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM ai_tasks WHERE tenant_id = $1 AND project_id = $2",
    )
    .bind(tenant_id)
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    if count >= MAX_PROJECT_TASKS {
        return Err(AppError::BadRequest(format!(
            "a project may contain at most {MAX_PROJECT_TASKS} AI tasks"
        )));
    }
    Ok(())
}

fn require_project_wide_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    task_board::require_manage(actor)
}

pub(crate) fn require_project_wide_task_dependency(
    actor: &ActorContext,
    assigned_to_task: bool,
) -> Result<(), AppError> {
    if task_dependency_is_forbidden(
        actor.has_restricted_inbox_scope() && !actor.is_password_session(),
        assigned_to_task,
    ) {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

fn task_dependency_is_forbidden(inbox_scoped: bool, assigned_to_task: bool) -> bool {
    inbox_scoped && assigned_to_task
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    action: &str,
    task_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5, 'ai_task', $6, '{}'::jsonb)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(action)
    .bind(task_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{
        ADVANCE_CLAIM_CURSOR_SQL, ADVANCE_MATERIALIZATION_CURSOR_SQL, CLAIM_TASK_RUN_SQL,
        MAX_TASK_AGENTS, SELECT_DUE_TASK_SQL, TASK_RUN_HISTORY_LIMIT, TaskSchedule, YearlyDate,
        new_task_run_lease_id, normalize_agent_ids, normalize_schedule,
        should_materialize_occurrence, task_dependency_is_forbidden,
    };

    #[test]
    fn terminal_run_retention_matches_the_exposed_history_limit() {
        assert_eq!(TASK_RUN_HISTORY_LIMIT, 100);
    }

    #[test]
    fn outstanding_occurrences_are_coalesced_and_claims_get_distinct_leases() {
        assert!(should_materialize_occurrence(false));
        assert!(!should_materialize_occurrence(true));
        assert_ne!(new_task_run_lease_id(), new_task_run_lease_id());
    }

    #[test]
    fn durable_project_cursors_round_robin_single_slot_work_across_restart() {
        assert!(
            SELECT_DUE_TASK_SQL
                .contains("ORDER BY scheduler.last_materialized_sequence NULLS FIRST")
        );
        assert!(CLAIM_TASK_RUN_SQL.contains("scheduler.last_claim_sequence NULLS FIRST"));
        assert!(
            ADVANCE_MATERIALIZATION_CURSOR_SQL
                .contains("nextval('ai_task_project_scheduler_sequence')")
        );
        assert!(ADVANCE_CLAIM_CURSOR_SQL.contains("nextval('ai_task_project_scheduler_sequence')"));
        let migration = include_str!("../migrations/0051_ai_task_execution.sql");
        assert!(migration.contains("CREATE SEQUENCE ai_task_project_scheduler_sequence"));
        assert!(migration.contains("last_materialized_sequence bigint"));
        assert!(migration.contains("last_claim_sequence bigint"));

        let queues = [vec!["A1", "A2"], vec!["B1"]];
        let mut consumed = [0_usize; 2];
        let mut durable_cursors = [None::<u64>; 2];
        let choose_project = |cursors: &[Option<u64>; 2], consumed: &[usize; 2]| {
            (0..queues.len())
                .filter(|&project| consumed[project] < queues[project].len())
                .min_by_key(|&project| (cursors[project], project))
                .unwrap()
        };

        let first = choose_project(&durable_cursors, &consumed);
        assert_eq!(queues[first][consumed[first]], "A1");
        consumed[first] += 1;
        durable_cursors[first] = Some(1);

        let second = choose_project(&durable_cursors, &consumed);
        assert_eq!(queues[second][consumed[second]], "B1");
        consumed[second] += 1;
        durable_cursors[second] = Some(2);

        // A worker restart loses no fairness because both cursors and the next
        // sequence value live in PostgreSQL.
        let restored_cursors = durable_cursors;
        let third = choose_project(&restored_cursors, &consumed);
        assert_eq!(queues[third][consumed[third]], "A2");
    }

    #[test]
    fn stale_processing_attempts_do_not_inflate_project_load() {
        let normalized_sql = CLAIM_TASK_RUN_SQL
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(normalized_sql.contains(
            "project_run.status = 'processing' AND project_run.locked_at > now() - ($2 * interval '1 second')"
        ));

        let lock_timeout_seconds = 600;
        let processing_lease_ages = [601, 900];
        assert!(
            processing_lease_ages
                .into_iter()
                .all(|age| age > lock_timeout_seconds)
        );
    }

    #[test]
    fn only_inbox_scoped_mutations_of_task_dependencies_are_forbidden() {
        assert!(task_dependency_is_forbidden(true, true));
        assert!(!task_dependency_is_forbidden(false, true));
        assert!(!task_dependency_is_forbidden(true, false));
    }

    #[test]
    fn task_agents_are_sorted_deduplicated_and_bounded() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        assert_eq!(
            normalize_agent_ids(vec![second, first, second]).unwrap(),
            vec![first, second]
        );
        assert_eq!(normalize_agent_ids(Vec::new()).unwrap(), Vec::<Uuid>::new());
        assert!(
            normalize_agent_ids(
                (0..=MAX_TASK_AGENTS)
                    .map(|value| Uuid::from_u128(value as u128 + 1))
                    .collect()
            )
            .is_err()
        );
    }

    #[test]
    fn overdue_recurrence_advances_once_from_now_without_a_catch_up_storm() {
        let schedule = TaskSchedule::Daily {
            time: "09:00".to_owned(),
            timezone: "UTC".to_owned(),
            ends_at: None,
        };
        let now = Utc.with_ymd_and_hms(2026, 8, 27, 12, 0, 0).unwrap();
        assert_eq!(
            schedule.next_occurrence_after(now).unwrap(),
            Some(Utc.with_ymd_and_hms(2026, 8, 28, 9, 0, 0).unwrap())
        );
    }

    #[test]
    fn normalizes_recurrence_values_and_rejects_invalid_inputs() {
        let now = Utc.with_ymd_and_hms(2026, 8, 27, 4, 0, 0).unwrap();
        let weekly = normalize_schedule(
            TaskSchedule::Weekly {
                time: "09:05".to_owned(),
                timezone: " Europe/Istanbul ".to_owned(),
                ends_at: None,
                weekdays: vec![7, 1, 7, 3],
            },
            now,
        )
        .unwrap();
        assert_eq!(
            weekly,
            TaskSchedule::Weekly {
                time: "09:05".to_owned(),
                timezone: "Europe/Istanbul".to_owned(),
                ends_at: None,
                weekdays: vec![1, 3, 7],
            }
        );

        let yearly = normalize_schedule(
            TaskSchedule::Yearly {
                time: "00:00".to_owned(),
                timezone: "UTC".to_owned(),
                ends_at: None,
                dates: vec![
                    YearlyDate { month: 12, day: 1 },
                    YearlyDate { month: 2, day: 29 },
                    YearlyDate { month: 12, day: 1 },
                ],
            },
            now,
        )
        .unwrap();
        assert_eq!(
            yearly,
            TaskSchedule::Yearly {
                time: "00:00".to_owned(),
                timezone: "UTC".to_owned(),
                ends_at: None,
                dates: vec![
                    YearlyDate { month: 2, day: 29 },
                    YearlyDate { month: 12, day: 1 },
                ],
            }
        );

        assert!(
            normalize_schedule(
                TaskSchedule::Monthly {
                    time: "9:00".to_owned(),
                    timezone: "UTC".to_owned(),
                    ends_at: None,
                    month_days: vec![1],
                },
                now,
            )
            .is_err()
        );
        assert!(
            normalize_schedule(
                TaskSchedule::Yearly {
                    time: "09:00".to_owned(),
                    timezone: "UTC".to_owned(),
                    ends_at: None,
                    dates: vec![YearlyDate { month: 2, day: 30 }],
                },
                now,
            )
            .is_err()
        );
        assert!(
            normalize_schedule(
                TaskSchedule::Yearly {
                    time: "09:00".to_owned(),
                    timezone: "UTC".to_owned(),
                    ends_at: None,
                    dates: vec![YearlyDate { month: 1, day: 1 }; 367],
                },
                now,
            )
            .is_err()
        );
    }

    #[test]
    fn recurring_tasks_resolve_dst_gaps_and_overlaps_once() {
        let spring = TaskSchedule::Daily {
            time: "02:30".to_owned(),
            timezone: "Europe/Berlin".to_owned(),
            ends_at: None,
        };
        assert_eq!(
            spring
                .next_occurrence_after(Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).unwrap())
                .unwrap(),
            Some(Utc.with_ymd_and_hms(2026, 3, 29, 1, 0, 0).unwrap())
        );

        let autumn = TaskSchedule::Daily {
            time: "02:30".to_owned(),
            timezone: "Europe/Berlin".to_owned(),
            ends_at: None,
        };
        assert_eq!(
            autumn
                .next_occurrence_after(Utc.with_ymd_and_hms(2026, 10, 24, 12, 0, 0).unwrap())
                .unwrap(),
            Some(Utc.with_ymd_and_hms(2026, 10, 25, 0, 30, 0).unwrap())
        );
    }

    #[test]
    fn monthly_dates_skip_short_months_and_yearly_dates_keep_leap_days() {
        let monthly = TaskSchedule::Monthly {
            time: "09:00".to_owned(),
            timezone: "Europe/Istanbul".to_owned(),
            ends_at: None,
            month_days: vec![31],
        };
        assert_eq!(
            monthly
                .next_occurrence_after(Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap())
                .unwrap(),
            Some(Utc.with_ymd_and_hms(2026, 5, 31, 6, 0, 0).unwrap())
        );

        let yearly = TaskSchedule::Yearly {
            time: "08:00".to_owned(),
            timezone: "Europe/Istanbul".to_owned(),
            ends_at: None,
            dates: vec![YearlyDate { month: 2, day: 29 }],
        };
        assert_eq!(
            yearly
                .next_occurrence_after(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap())
                .unwrap(),
            Some(Utc.with_ymd_and_hms(2028, 2, 29, 5, 0, 0).unwrap())
        );
    }

    #[test]
    fn one_time_tasks_allow_past_instants_without_scheduling_another_run() {
        let now = Utc.with_ymd_and_hms(2026, 8, 27, 4, 0, 0).unwrap();
        for offset in [-1, 0, 1] {
            let run_at = now + chrono::Duration::minutes(offset);
            let schedule = normalize_schedule(TaskSchedule::Once { run_at }, now).unwrap();
            assert!(matches!(schedule, TaskSchedule::Once { run_at: saved } if saved == run_at));
            assert_eq!(
                super::required_next_occurrence(&schedule, now).unwrap(),
                (offset > 0).then_some(run_at)
            );
        }
    }
    #[test]
    fn recurring_deadlines_use_the_schedule_timezone_and_include_the_boundary() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-11T05:59:59Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let cutoff = now + chrono::Duration::seconds(1);
        for mut value in [
            serde_json::json!({"kind":"daily"}),
            serde_json::json!({"kind":"weekly","weekdays":[5]}),
            serde_json::json!({"kind":"monthly","month_days":[11]}),
            serde_json::json!({"kind":"yearly","dates":[{"month":9,"day":11}]}),
        ] {
            value["time"] = serde_json::json!("09:00");
            value["timezone"] = serde_json::json!("Europe/Istanbul");
            value["ends_at"] = serde_json::json!("2026-09-11T09:00:00");
            let schedule: TaskSchedule = serde_json::from_value(value.clone()).unwrap();
            let schedule = normalize_schedule(schedule, now).unwrap();
            assert_eq!(schedule.next_occurrence_after(now).unwrap(), Some(cutoff));
            assert_eq!(schedule.next_occurrence_after(cutoff).unwrap(), None);
            assert!(!schedule.has_ended_at(cutoff).unwrap());
            assert!(
                schedule
                    .has_ended_at(cutoff + chrono::Duration::seconds(1))
                    .unwrap()
            );
            value["ends_at"] = serde_json::json!("2026-09-11T08:59:59");
            let expired: TaskSchedule = serde_json::from_value(value).unwrap();
            assert!(normalize_schedule(expired, now).is_err());
        }
    }

    #[test]
    fn recurring_deadlines_reject_dst_gaps_overlaps_and_later_occurrences() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        for ends_at in ["2026-03-29T02:30:00", "2026-10-25T02:30:00"] {
            let schedule = serde_json::from_value(serde_json::json!({
                "kind":"daily","time":"09:00","timezone":"Europe/Berlin","ends_at":ends_at
            }))
            .unwrap();
            assert!(normalize_schedule(schedule, now).is_err());
        }
        let schedule: TaskSchedule = serde_json::from_value(serde_json::json!({
            "kind":"daily","time":"09:00","timezone":"UTC","ends_at":"2026-01-01T08:00:00"
        }))
        .unwrap();
        assert_eq!(schedule.next_occurrence_after(now).unwrap(), None);
        assert!(super::required_next_occurrence(&schedule, now).is_err());
    }

    #[test]
    fn recurring_schedules_without_deadlines_keep_the_existing_contract() {
        for ends_at in [None, Some(serde_json::Value::Null)] {
            let mut value = serde_json::json!({"kind":"daily","time":"09:00","timezone":"UTC"});
            if let Some(ends_at) = ends_at {
                value["ends_at"] = ends_at;
            }
            let schedule: TaskSchedule = serde_json::from_value(value).unwrap();
            assert_eq!(schedule.ends_at_utc().unwrap(), None);
            assert!(
                serde_json::to_value(schedule)
                    .unwrap()
                    .get("ends_at")
                    .is_none()
            );
        }
    }

    fn plan_step(id: u128, kind: super::PerformerKind, performer: u128) -> super::PlanStep {
        super::PlanStep {
            id: Uuid::from_u128(id),
            title: format!(" Step {id} "),
            instructions: format!(" Do part {id} "),
            performer: super::PlanPerformer {
                kind,
                id: Uuid::from_u128(performer),
            },
        }
    }

    fn task_request(fields: serde_json::Value) -> super::TaskRequest {
        let mut value = serde_json::json!({
            "text": "Onboard", "schedule": {"kind": "manual"}, "agent_ids": [],
        });
        for (key, field) in fields.as_object().unwrap() {
            value[key] = field.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    fn normalize(
        fields: serde_json::Value,
        plan_steps: Option<Vec<super::PlanStep>>,
    ) -> Result<super::NormalizedTask, crate::error::AppError> {
        let now = Utc.with_ymd_and_hms(2026, 8, 27, 4, 0, 0).unwrap();
        super::normalize_task(task_request(fields), Vec::new(), plan_steps, now)
    }

    #[test]
    fn plan_steps_are_trimmed_bounded_and_uniquely_identified() {
        use super::{MAX_PLAN_STEPS, PerformerKind::Agent, normalize_plan_steps};
        let steps =
            normalize_plan_steps(vec![plan_step(2, Agent, 9), plan_step(1, Agent, 9)]).unwrap();
        assert_eq!(
            steps
                .iter()
                .map(|step| (
                    step.id.as_u128(),
                    step.title.as_str(),
                    step.instructions.as_str()
                ))
                .collect::<Vec<_>>(),
            [(2, "Step 2", "Do part 2"), (1, "Step 1", "Do part 1")]
        );
        let many = |count: usize| {
            (1..=count)
                .map(|id| plan_step(id as u128, Agent, 9))
                .collect::<Vec<_>>()
        };
        assert!(normalize_plan_steps(many(MAX_PLAN_STEPS)).is_ok());
        assert!(normalize_plan_steps(many(MAX_PLAN_STEPS + 1)).is_err());
        assert!(normalize_plan_steps(Vec::new()).is_err());
        assert!(
            normalize_plan_steps(vec![plan_step(1, Agent, 9), plan_step(1, Agent, 8)]).is_err()
        );
        assert!(normalize_plan_steps(vec![plan_step(0, Agent, 9)]).is_err());
        for (title, instructions, valid) in [
            ("é".repeat(200), "é".repeat(10_000), true),
            ("é".repeat(201), "ok".to_owned(), false),
            ("ok".to_owned(), "é".repeat(10_001), false),
            ("  ".to_owned(), "ok".to_owned(), false),
            ("ok".to_owned(), " \n ".to_owned(), false),
            ("o\0k".to_owned(), "ok".to_owned(), false),
        ] {
            let mut step = plan_step(1, Agent, 9);
            step.title = title;
            step.instructions = instructions;
            assert_eq!(normalize_plan_steps(vec![step]).is_ok(), valid);
        }
    }

    #[test]
    fn plan_teams_are_the_distinct_performers_without_the_coordinator() {
        use super::PerformerKind::{Agent, Employee};
        let steps = [
            plan_step(1, Agent, 12),
            plan_step(2, Employee, 21),
            plan_step(3, Agent, 11),
            plan_step(4, Agent, 12),
            plan_step(5, Employee, 20),
        ];
        let ids = |values: &[u128]| {
            values
                .iter()
                .copied()
                .map(Uuid::from_u128)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            super::plan_members(&steps, None, Some(Uuid::from_u128(30))),
            (ids(&[11, 12]), ids(&[20, 21]))
        );
        // A coordinator may perform steps and still is not a member.
        assert_eq!(
            super::plan_members(&steps, Some(Uuid::from_u128(12)), None),
            (ids(&[11]), ids(&[20, 21]))
        );
        assert_eq!(
            super::plan_members(&steps[1..2], None, Some(Uuid::from_u128(21))),
            (Vec::new(), Vec::new())
        );
    }

    #[test]
    fn plan_tasks_derive_their_team_and_drop_roles_of_absent_members() {
        use super::PerformerKind::{Agent, Employee};
        let coordinator = Uuid::from_u128(12);
        let task = normalize(
            serde_json::json!({
                "execution_mode": "team", "coordinator_id": coordinator,
                "agent_ids": [Uuid::from_u128(77)], "assignee_ids": [Uuid::from_u128(78)],
                "agent_roles": [
                    {"agent_id": Uuid::from_u128(77), "role": "left the plan"},
                    {"agent_id": coordinator, "role": " checks the result "},
                    {"agent_id": Uuid::from_u128(21), "role": "welcomes"},
                ],
            }),
            Some(vec![plan_step(1, Agent, 12), plan_step(2, Employee, 21)]),
        )
        .unwrap();
        assert_eq!(task.agent_ids, Vec::<Uuid>::new());
        assert_eq!(task.assignee_ids, vec![Uuid::from_u128(21)]);
        assert_eq!(task.execution_agent_ids(), vec![coordinator]);
        assert_eq!(
            task.agent_roles
                .iter()
                .map(|role| (role.agent_id.as_u128(), role.role.as_str()))
                .collect::<Vec<_>>(),
            [(12, "checks the result"), (21, "welcomes")]
        );
        assert_eq!(task.plan_steps.unwrap()[0].title, "Step 1");

        // Thirty performers fit a plan although a planned team holds eight.
        let wide = (1..=30).map(|id| plan_step(id, Agent, 100 + id)).collect();
        let task = normalize(
            serde_json::json!({"execution_mode": "team", "coordinator_user_id": Uuid::from_u128(5)}),
            Some(wide),
        )
        .unwrap();
        assert_eq!(task.agent_ids.len(), 30);
    }

    #[test]
    fn task_text_is_trimmed_required_and_free_of_nul_characters() {
        assert!(
            normalize(serde_json::json!({"text": " Onboard Dana "}), None)
                .is_ok_and(|task| task.text == "Onboard Dana")
        );
        // PostgreSQL would refuse the NUL character with a server error.
        for text in ["  ", "Onboard\u{0}Dana"] {
            assert!(
                normalize(serde_json::json!({"text": text}), None).is_err(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn plans_require_team_mode_and_one_coordinator() {
        use super::PerformerKind::Agent;
        let plan = || Some(vec![plan_step(1, Agent, 12)]);
        assert!(normalize(serde_json::json!({}), plan()).is_err());
        assert!(normalize(serde_json::json!({"execution_mode": "team"}), plan()).is_err());
        assert!(
            normalize(
                serde_json::json!({
                    "execution_mode": "team", "coordinator_id": Uuid::from_u128(1),
                    "coordinator_user_id": Uuid::from_u128(2),
                }),
                plan(),
            )
            .is_err()
        );
    }

    #[test]
    fn teams_without_a_plan_keep_one_to_eight_members_apart_from_the_coordinator() {
        let coordinator = Uuid::from_u128(1);
        let team = |agents: Vec<Uuid>| {
            normalize(
                serde_json::json!({
                    "execution_mode": "team", "coordinator_id": coordinator, "agent_ids": agents,
                }),
                None,
            )
        };
        let members = |count: u128| (2..count + 2).map(Uuid::from_u128).collect::<Vec<_>>();
        assert!(team(Vec::new()).is_err());
        assert!(team(members(8)).is_ok());
        assert!(team(members(9)).is_err());
        assert!(team(vec![coordinator, Uuid::from_u128(2)]).is_err());
        assert!(team(members(1)).unwrap().plan_steps.is_none());
    }

    #[test]
    fn an_omitted_plan_is_kept_and_a_launched_process_never_loses_its_plan() {
        use super::{PerformerKind::Agent, resolve_plan_update};
        let stored = || Some(vec![plan_step(1, Agent, 12)]);
        let replacement = vec![plan_step(2, Agent, 13)];
        assert_eq!(task_request(serde_json::json!({})).plan_steps, None);
        assert_eq!(
            task_request(serde_json::json!({"plan_steps": null})).plan_steps,
            Some(None)
        );
        assert_eq!(
            task_request(serde_json::json!({"plan_steps": [{
                "id": Uuid::from_u128(2), "title": " Step 2 ", "instructions": " Do part 2 ",
                "performer": {"kind": "agent", "id": Uuid::from_u128(13)},
            }]}))
            .plan_steps,
            Some(Some(replacement.clone()))
        );
        for launched in [false, true] {
            assert_eq!(
                resolve_plan_update(None, stored(), launched).unwrap(),
                stored()
            );
            assert_eq!(
                resolve_plan_update(Some(Some(replacement.clone())), stored(), launched).unwrap(),
                Some(replacement.clone())
            );
        }
        assert_eq!(
            resolve_plan_update(Some(None), stored(), false).unwrap(),
            None
        );
        assert!(resolve_plan_update(Some(None), stored(), true).is_err());
    }

    #[test]
    fn employees_of_a_plan_take_part_in_the_task() {
        use super::PerformerKind::{Agent, Employee};
        assert!(!super::plan_uses_employees(None));
        assert!(!super::plan_uses_employees(Some(&[plan_step(
            1, Agent, 12
        )])));
        assert!(super::plan_uses_employees(Some(&[
            plan_step(1, Agent, 12),
            plan_step(2, Employee, 21),
        ])));
        assert_eq!(
            serde_json::to_value(plan_step(2, Employee, 21).performer).unwrap(),
            serde_json::json!({"kind": "employee", "id": Uuid::from_u128(21)})
        );
    }

    #[test]
    fn a_span_ends_no_earlier_than_it_starts_and_may_lie_in_the_past() {
        use super::TaskSpan;
        let start = Utc.with_ymd_and_hms(2020, 3, 1, 9, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2020, 3, 1, 17, 0, 0).unwrap();
        // A deadline has never had to be in the future, and a span inherits that.
        let past = TaskSpan::new(Some(start), Some(end), false, None, None).unwrap();
        assert_eq!(past.starts_at, Some(start));
        assert_eq!(past.due_at, Some(end));
        assert!(TaskSpan::new(Some(end), Some(start), false, None, None).is_err());
        assert!(TaskSpan::new(Some(start), Some(start), false, None, None).is_ok());
        // A start alone has no span to be the start of.
        assert!(TaskSpan::new(Some(start), None, false, None, None).is_err());
        assert!(TaskSpan::new(None, Some(end), false, None, None).is_ok());
        assert!(TaskSpan::new(None, None, true, None, None).is_err());
    }

    #[test]
    fn a_span_keeps_its_zone_in_the_canonical_spelling() {
        use super::TaskSpan;
        let end = Utc.with_ymd_and_hms(2026, 3, 1, 17, 0, 0).unwrap();
        let span = TaskSpan::new(
            None,
            Some(end),
            true,
            Some("  Europe/Istanbul  ".to_owned()),
            None,
        )
        .unwrap();
        assert_eq!(span.time_zone.as_deref(), Some("Europe/Istanbul"));
        assert!(span.all_day);
        assert!(
            TaskSpan::new(
                None,
                Some(end),
                false,
                Some("Mars/Olympus".to_owned()),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn planned_tasks_are_team_tasks_coordinated_by_an_agent_or_an_employee() {
        use super::{PerformerKind, PlanPerformer, PlannedTask, TaskRequest};
        let starts_at = Utc.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
        let due_at = Utc.with_ymd_and_hms(2026, 9, 1, 17, 0, 0).unwrap();
        for kind in [PerformerKind::Agent, PerformerKind::Employee] {
            let request = TaskRequest::from(PlannedTask {
                text: "Onboard".to_owned(),
                expected_result: "Ready".to_owned(),
                schedule: TaskSchedule::Manual,
                coordinator: PlanPerformer {
                    kind,
                    id: Uuid::from_u128(7),
                },
                plan_steps: vec![plan_step(1, PerformerKind::Agent, 12)],
                list_id: None,
                starts_at: Some(starts_at),
                due_at: Some(due_at),
                all_day: false,
                time_zone: Some("Europe/Istanbul".to_owned()),
            });
            assert_eq!(request.execution_mode, super::ExecutionMode::Team);
            assert_eq!(
                request.coordinator_id.is_some(),
                kind == PerformerKind::Agent
            );
            assert_eq!(
                request.coordinator_user_id.is_some(),
                kind == PerformerKind::Employee
            );
            assert_eq!(request.starts_at, Some(Some(starts_at)));
            assert_eq!(request.due_at, Some(Some(due_at)));
            assert_eq!(request.all_day, Some(false));
            assert_eq!(request.time_zone, Some(Some("Europe/Istanbul".to_owned())));
            assert_eq!(
                request.plan_steps.as_ref().unwrap().as_ref().unwrap().len(),
                1
            );
        }
    }

    #[test]
    fn task_rows_carry_the_process_marker_and_progress_without_bind_parameters() {
        let sql = super::TASK_SELECT_SQL;
        assert!(!sql.contains('$'));
        for column in [
            "task.process_id",
            "task.process_version",
            "AS process_title",
            "task.plan_steps",
            "AS plan_done",
            "AS plan_total",
        ] {
            assert!(sql.contains(column), "{column}");
        }
        let migration = include_str!("../migrations/0124_process_runs.sql");
        for column in [
            "process_id uuid",
            "process_version integer",
            "plan_steps jsonb",
        ] {
            assert!(migration.contains(column), "{column}");
        }
        // TaskRow is a FromRow: a span column missing from the select fails at
        // runtime in every list, create and update rather than at compile time.
        for column in [
            "task.starts_at",
            "task.due_at",
            "task.all_day",
            "task.time_zone",
        ] {
            assert!(sql.contains(column), "{column}");
        }
        let duration = include_str!("../migrations/0132_task_duration.sql");
        for clause in [
            "starts_at timestamptz",
            "all_day boolean NOT NULL DEFAULT false",
            "time_zone text",
            "CONSTRAINT ai_tasks_duration",
            "starts_at <= due_at",
        ] {
            assert!(duration.contains(clause), "{clause}");
        }
    }
}
