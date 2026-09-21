//! Optional workspace visibility, independent of a resource's execution owner.

use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceVisibility {
    #[serde(default)]
    pub project_ids: Vec<Uuid>,
    #[serde(default)]
    pub department_ids: Vec<Uuid>,
}

#[derive(Clone, Copy)]
pub(crate) enum Resource {
    Provider,
    KnowledgeBase,
    Task,
    Profile,
    ReplyTemplate,
    Channel,
    Integration,
}

impl Resource {
    fn table(self) -> &'static str {
        match self {
            Self::Provider => "ai_provider_connections",
            Self::KnowledgeBase => "knowledge_bases",
            Self::Task => "ai_tasks",
            Self::Profile => "ai_profiles",
            Self::ReplyTemplate => "reply_templates",
            Self::Channel => "channel_connections",
            Self::Integration => "api_integrations",
        }
    }
}

/// Deployment scope, access tokens, and demo sessions never reach another owner.
pub(crate) fn owner_limit(actor: &ActorContext) -> Option<Uuid> {
    if actor.deployment_project_id().is_some() {
        actor.deployment_project_id()
    } else if !actor.is_password_session() || actor.is_demo() {
        actor.project_id
    } else {
        None
    }
}

pub(crate) async fn owner_project(
    db: &PgPool,
    actor: &ActorContext,
    resource: Resource,
    id: Uuid,
) -> Result<Uuid, AppError> {
    let scoped_inbox = matches!(resource, Resource::ReplyTemplate | Resource::Channel);
    let inbox_filter = if scoped_inbox {
        " AND ($6::uuid[] IS NULL OR inbox_id=ANY($6))"
    } else {
        ""
    };
    let project_filter = if matches!(resource, Resource::KnowledgeBase) {
        " AND project_id=$3"
    } else {
        ""
    };
    let sql = format!(
        "SELECT project_id FROM {} WHERE tenant_id=$1 AND id=$2 AND resource_visible(visibility,$3,$4) AND ($5::uuid IS NULL OR project_id=$5){inbox_filter}{project_filter}",
        resource.table()
    );
    let mut query = sqlx::query_scalar(&sql)
        .bind(actor.tenant_id)
        .bind(id)
        .bind(actor.project_id)
        .bind(actor.department_id())
        .bind(owner_limit(actor));
    if scoped_inbox {
        query = query.bind(if actor.is_password_session() {
            None
        } else {
            actor.inbox_scope()
        });
    }
    query.fetch_optional(db).await?.ok_or(AppError::NotFound)
}

pub(crate) async fn save(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    resource: Resource,
    id: Uuid,
    visibility: Option<&ResourceVisibility>,
    is_new: bool,
) -> Result<Option<ResourceVisibility>, AppError> {
    let automatic;
    let visibility = if actor.can_choose_resource_visibility() {
        visibility
    } else if is_new {
        let project = actor
            .project_id
            .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))?;
        let department = if actor.department_id().is_some() {
            actor.department_id()
        } else {
            sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT default_department_id FROM projects WHERE tenant_id=$1 AND id=$2",
            )
            .bind(actor.tenant_id)
            .bind(project)
            .fetch_one(&mut **tx)
            .await?
        };
        automatic = ResourceVisibility {
            project_ids: vec![project],
            department_ids: department.into_iter().collect(),
        };
        Some(&automatic)
    } else {
        // Editing a shared resource never silently changes its visibility.
        if let Some(requested) = visibility {
            let query = format!(
                "SELECT visibility FROM {} WHERE tenant_id=$1 AND id=$2",
                resource.table()
            );
            let existing: sqlx::types::Json<ResourceVisibility> = sqlx::query_scalar(&query)
                .bind(actor.tenant_id)
                .bind(id)
                .fetch_one(&mut **tx)
                .await?;
            if requested != &existing.0 {
                return Err(AppError::Forbidden);
            }
        }
        return Ok(None);
    };
    let mut visibility = match visibility {
        Some(visibility) => visibility.clone(),
        None if is_new && matches!(resource, Resource::KnowledgeBase) => {
            ResourceVisibility::default()
        }
        None => return Ok(None),
    };
    if matches!(resource, Resource::KnowledgeBase) {
        let project_id: Uuid = sqlx::query_scalar(
            "SELECT project_id FROM knowledge_bases WHERE tenant_id=$1 AND id=$2",
        )
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
        if visibility.project_ids.iter().any(|id| *id != project_id) {
            return Err(AppError::BadRequest(
                "knowledge bases must remain in their owning project".to_owned(),
            ));
        }
        visibility.project_ids = vec![project_id];
    }
    visibility.project_ids.sort_unstable();
    visibility.project_ids.dedup();
    visibility.department_ids.sort_unstable();
    visibility.department_ids.dedup();
    for project_id in &visibility.project_ids {
        actor.require_deployment_project(*project_id)?;
    }
    if visibility.project_ids.len() > 100 || visibility.department_ids.len() > 100 {
        return Err(AppError::BadRequest(
            "select at most 100 projects and departments".to_owned(),
        ));
    }
    let projects: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM projects WHERE tenant_id=$1 AND id=ANY($2)")
            .bind(actor.tenant_id)
            .bind(&visibility.project_ids)
            .fetch_all(&mut **tx)
            .await?;
    let departments: Vec<(Uuid, Uuid)> =
        sqlx::query_as("SELECT id, project_id FROM departments WHERE tenant_id=$1 AND id=ANY($2)")
            .bind(actor.tenant_id)
            .bind(&visibility.department_ids)
            .fetch_all(&mut **tx)
            .await?;
    for (_, project_id) in &departments {
        actor.require_deployment_project(*project_id)?;
    }
    if projects.len() != visibility.project_ids.len()
        || departments.len() != visibility.department_ids.len()
        || departments
            .iter()
            .any(|(_, project)| !projects.is_empty() && !projects.contains(project))
    {
        return Err(AppError::BadRequest(
            "projects and departments must belong to this organization and match each other"
                .to_owned(),
        ));
    }
    store(tx, actor, resource, id, &visibility).await?;
    Ok(Some(visibility))
}

async fn store(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    resource: Resource,
    id: Uuid,
    visibility: &ResourceVisibility,
) -> Result<(), AppError> {
    let position_assignment = if matches!(resource, Resource::Profile) {
        ", position_id=CASE WHEN visibility IS DISTINCT FROM $3 THEN NULL ELSE position_id END"
    } else {
        ""
    };
    let query = format!(
        "UPDATE {} SET visibility=$3{position_assignment} WHERE tenant_id=$1 AND id=$2",
        resource.table()
    );
    sqlx::query(&query)
        .bind(actor.tenant_id)
        .bind(id)
        .bind(sqlx::types::Json(visibility))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[derive(FromRow, Serialize)]
struct ProjectOption {
    id: Uuid,
    name: String,
}
#[derive(FromRow, Serialize)]
struct DepartmentOption {
    id: Uuid,
    project_id: Uuid,
    name: String,
}
#[derive(Serialize)]
struct VisibilityOptions {
    can_choose: bool,
    projects: Vec<ProjectOption>,
    departments: Vec<DepartmentOption>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/resource-visibility-options", get(options))
}

async fn options(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<VisibilityOptions>, AppError> {
    let can_choose = actor.can_choose_resource_visibility();
    if !can_choose {
        return Ok(Json(VisibilityOptions {
            can_choose,
            projects: Vec::new(),
            departments: Vec::new(),
        }));
    }
    let projects: Vec<ProjectOption> = sqlx::query_as(
        "SELECT project.id, project.name FROM projects project WHERE project.tenant_id=$1
         AND ($3::uuid IS NULL OR project.id=$3) AND EXISTS (
           SELECT 1 FROM memberships m WHERE m.tenant_id=project.tenant_id AND m.user_id=$2
             AND m.revoked_at IS NULL AND (m.project_id IS NULL OR m.project_id=project.id)) ORDER BY project.name,project.id")
        .bind(actor.tenant_id).bind(actor.actor_id).bind(owner_limit(&actor)).fetch_all(&state.db).await?;
    let ids: Vec<Uuid> = projects.iter().map(|p| p.id).collect();
    let departments = sqlx::query_as("SELECT id, project_id, name FROM departments WHERE tenant_id=$1 AND project_id=ANY($2) AND ($3::uuid IS NULL OR id=$3) ORDER BY position,name,id")
        .bind(actor.tenant_id).bind(ids).bind(if actor.department_restricted() { actor.department_id() } else { None })
        .fetch_all(&state.db).await?;
    Ok(Json(VisibilityOptions {
        can_choose,
        projects,
        departments,
    }))
}
