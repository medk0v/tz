//! Task access scoping shared by tasks, attachments and orchestration.

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    AppState,
    auth::ActorContext,
    error::AppError,
    resource_visibility::{self, Resource},
};

pub(crate) const MAX_TASK_TAGS: usize = 20;

/// Task visibility. Binds: $1 tenant, $2 selected project, $3 department,
/// $4 owner project limit, $5 employee limited to their own tasks.
pub(crate) const TASK_ACCESS_SQL: &str = r#"(
    ($5::uuid IS NULL AND resource_visible(task.visibility,$2,$3) AND ($4::uuid IS NULL OR task.project_id=$4))
    OR ($5::uuid IS NOT NULL AND task.project_id=$2 AND (
        task.coordinator_user_id=$5
        OR EXISTS(SELECT 1 FROM ai_task_assignees AS own_assignee
            WHERE own_assignee.tenant_id=task.tenant_id AND own_assignee.task_id=task.id AND own_assignee.user_id=$5)
        OR EXISTS(SELECT 1 FROM ai_task_executions AS own_execution
            JOIN ai_task_execution_steps AS own_step ON own_step.execution_id=own_execution.id
            WHERE own_execution.tenant_id=task.tenant_id AND own_execution.task_id=task.id
              AND own_step.assignee_user_id=$5))))"#;

/// How an actor may work with tasks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskAccess {
    /// Every task visible in the workspace; create, assign and edit.
    Manage,
    /// Only tasks where this employee participates.
    Own(Uuid),
}

impl TaskAccess {
    pub(crate) fn own_user(self) -> Option<Uuid> {
        match self {
            Self::Manage => None,
            Self::Own(user) => Some(user),
        }
    }
}

pub(crate) struct TaskActor {
    pub project_id: Uuid,
    pub access: TaskAccess,
}

pub(crate) fn task_actor(actor: &ActorContext) -> Result<TaskActor, AppError> {
    let project_id = actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))?;
    if actor.has_restricted_inbox_scope() && !actor.is_password_session() {
        return Err(AppError::Forbidden);
    }
    let has = |permission: &str| actor.permissions().iter().any(|value| value == permission);
    let access = if has("tasks:manage") {
        TaskAccess::Manage
    } else if has("tasks:own") {
        TaskAccess::Own(actor.actor_id)
    } else {
        return Err(AppError::Forbidden);
    };
    Ok(TaskActor { project_id, access })
}

pub(crate) fn require_manage(actor: &ActorContext) -> Result<Uuid, AppError> {
    let task_actor = task_actor(actor)?;
    if task_actor.access != TaskAccess::Manage {
        return Err(AppError::Forbidden);
    }
    Ok(task_actor.project_id)
}

pub(crate) async fn task_project(
    state: &AppState,
    actor: &ActorContext,
    access: TaskAccess,
    task_id: Uuid,
) -> Result<Uuid, AppError> {
    match access {
        TaskAccess::Manage => {
            resource_visibility::owner_project(&state.db, actor, Resource::Task, task_id).await
        }
        TaskAccess::Own(user) => {
            let sql = format!(
                "SELECT task.project_id FROM ai_tasks AS task WHERE task.tenant_id=$1 AND task.id=$6 AND {TASK_ACCESS_SQL}"
            );
            sqlx::query_scalar(&sql)
                .bind(actor.tenant_id)
                .bind(actor.project_id)
                .bind(actor.department_id())
                .bind(resource_visibility::owner_limit(actor))
                .bind(user)
                .bind(task_id)
                .fetch_optional(&state.db)
                .await?
                .ok_or(AppError::NotFound)
        }
    }
}

const AUTOMATIC_MOVE_SQL: &str = r#"
    WITH current AS (
        SELECT task.list_id FROM ai_tasks AS task
        LEFT JOIN task_list_columns AS status ON status.id = task.column_id
        WHERE task.tenant_id = $1 AND task.id = $2 AND task.list_id IS NOT NULL
          AND task.schedule->>'kind' IN ('manual', 'once')
          AND COALESCE(status.kind, 'todo') = ANY($3)
    ), target AS (
        SELECT status.id FROM task_list_columns AS status JOIN current ON status.list_id = current.list_id
        WHERE status.kind = $4 ORDER BY status.position, status.id LIMIT 1
    )
    UPDATE ai_tasks AS task
    SET column_id = target.id,
        position = COALESCE((SELECT min(other.position) FROM ai_tasks AS other
                             WHERE other.tenant_id = $1 AND other.column_id = target.id), 1) - 1,
        completed_at = CASE WHEN $4 = 'done' THEN now() END,
        updated_at = now()
    FROM target
    WHERE task.tenant_id = $1 AND task.id = $2
"#;

/// An agent started a one-time task: a card waiting to be done moves to the
/// list's first in-progress status. Recurring tasks keep their place.
pub(crate) async fn mark_task_started<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    task_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(AUTOMATIC_MOVE_SQL)
        .bind(tenant_id)
        .bind(task_id)
        .bind(vec!["todo"])
        .bind("in_progress")
        .execute(executor)
        .await?;
    Ok(())
}

/// A one-time task finished successfully: its card moves to the first done status.
pub(crate) async fn mark_task_finished<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    task_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(AUTOMATIC_MOVE_SQL)
        .bind(tenant_id)
        .bind(task_id)
        .bind(vec!["todo", "in_progress"])
        .bind("done")
        .execute(executor)
        .await?;
    Ok(())
}
