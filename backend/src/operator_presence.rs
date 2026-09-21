//! Authenticated operator heartbeats and public Inbox availability.

use axum::{Router, extract::State, http::StatusCode, routing::post};
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError, provider_reply};

/// An operator remains available briefly when a heartbeat is delayed.
pub const OPERATOR_ONLINE_WINDOW_SECONDS: i64 = 60;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/operator-presence",
        post(refresh_operator_presence).delete(clear_operator_presence),
    )
}

async fn refresh_operator_presence(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<StatusCode, AppError> {
    actor.require("conversations:reply")?;
    actor.set_presence(&state.db, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_operator_presence(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<StatusCode, AppError> {
    actor.require("conversations:reply")?;
    actor.set_presence(&state.db, false).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Returns whether an authenticated operator with access to the Inbox has sent
/// a recent explicit presence heartbeat.
///
/// # Errors
///
/// Returns a database error when the availability query cannot be completed.
pub async fn inbox_has_online_operator(
    db: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
) -> Result<bool, AppError> {
    let cutoff = Utc::now() - Duration::seconds(OPERATOR_ONLINE_WINDOW_SECONDS);
    sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM operator_sessions AS session
            LEFT JOIN operator_session_projects AS workspace
              ON workspace.tenant_id = session.tenant_id
             AND workspace.session_id = session.id
             AND workspace.project_id = $2
            JOIN memberships AS membership
              ON membership.tenant_id = session.tenant_id
             AND membership.id = COALESCE(workspace.membership_id, session.membership_id)
             AND membership.user_id = session.user_id
             AND membership.revoked_at IS NULL
            JOIN users AS app_user ON app_user.id = session.user_id
            JOIN project_roles AS project_role
              ON project_role.tenant_id = membership.tenant_id
             AND project_role.project_id = $2
             AND project_role.id = membership.role
            JOIN inboxes AS scoped_inbox
              ON scoped_inbox.tenant_id = session.tenant_id
             AND scoped_inbox.project_id = $2 AND scoped_inbox.id = $3
            JOIN projects AS scoped_project
              ON scoped_project.tenant_id = session.tenant_id AND scoped_project.id = $2
            CROSS JOIN LATERAL (
                SELECT CASE WHEN workspace.session_id IS NULL
                            THEN session.department_id ELSE workspace.department_id END AS department_id,
                       CASE WHEN workspace.session_id IS NULL
                            THEN session.director_mode ELSE workspace.director_mode END AS director_mode,
                       CASE WHEN workspace.session_id IS NULL
                            THEN session.presence_last_seen_at ELSE workspace.presence_last_seen_at END AS presence_last_seen_at
            ) AS active_workspace
            CROSS JOIN LATERAL (
                SELECT COALESCE(active_workspace.department_id, scoped_project.default_department_id, (
                    SELECT id FROM departments
                    WHERE tenant_id = session.tenant_id AND project_id = $2
                    ORDER BY position, created_at, id LIMIT 1
                )) AS department_id
            ) AS selected_department
            WHERE session.tenant_id = $1
              AND active_workspace.presence_last_seen_at >= $4
              AND session.revoked_at IS NULL
              AND session.idle_expires_at > now()
              AND session.absolute_expires_at > now()
              AND app_user.status = 'active'
              AND (workspace.session_id IS NOT NULL OR session.project_id = $2)
              AND 'conversations:reply' = ANY(project_role.permissions)
              AND (membership.project_id IS NULL OR membership.project_id = $2)
              AND (membership.department_id IS NULL OR scoped_inbox.department_id = membership.department_id)
              AND (
                  membership.department_id IS NOT NULL
                  OR (active_workspace.director_mode AND scoped_project.director_enabled
                      AND (project_role.base_role = 'admin' OR membership.director_access))
                  OR (NOT (active_workspace.director_mode AND scoped_project.director_enabled
                           AND (project_role.base_role = 'admin' OR membership.director_access))
                      AND (selected_department.department_id IS NULL
                           OR scoped_inbox.department_id = selected_department.department_id))
              )
            UNION ALL
            SELECT 1
            FROM api_keys AS api_key
            JOIN users AS app_user ON app_user.id = api_key.actor_user_id
            JOIN memberships AS membership
              ON membership.tenant_id = api_key.tenant_id
             AND membership.user_id = api_key.actor_user_id
             AND membership.project_id IS NOT DISTINCT FROM api_key.project_id
             AND membership.role = api_key.role AND membership.revoked_at IS NULL
            LEFT JOIN project_roles AS project_role
              ON project_role.tenant_id = membership.tenant_id
             AND project_role.project_id = COALESCE(api_key.role_project_id, api_key.project_id)
             AND project_role.id = membership.role
            LEFT JOIN projects AS role_source
              ON role_source.tenant_id = api_key.tenant_id AND role_source.id = api_key.role_project_id
            JOIN inboxes AS scoped_inbox
              ON scoped_inbox.tenant_id = api_key.tenant_id
             AND scoped_inbox.project_id = $2 AND scoped_inbox.id = $3
            WHERE api_key.tenant_id = $1
              AND api_key.presence_last_seen_at >= $4
              AND api_key.revoked_at IS NULL
              AND api_key.expires_at > now()
              AND app_user.status = 'active'
              AND (api_key.role_project_id IS NULL OR (project_role.id IS NOT NULL AND role_source.status = 'active'))
              AND (api_key.role_project_id IS NOT NULL OR 'conversations:reply' = ANY(api_key.permissions))
              AND 'conversations:reply' = ANY(COALESCE(project_role.permissions, membership.permissions))
              AND (membership.department_id IS NULL OR scoped_inbox.department_id = membership.department_id)
              AND (api_key.project_id IS NULL OR api_key.project_id = $2)
              AND (api_key.inbox_scope IS NULL OR $3 = ANY(api_key.inbox_scope))
        )
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(cutoff)
    .fetch_one(db)
    .await
    .map_err(AppError::from)
}

/// Returns whether the Inbox can immediately serve a widget conversation
/// through either an online human operator or an active auto-joining AI
/// profile that supports the widget language.
///
/// # Errors
///
/// Returns a database error when the availability query cannot be completed.
pub async fn inbox_has_online_support(
    db: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_connection_id: Uuid,
    widget_language: &str,
) -> Result<bool, AppError> {
    let channel_available = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM channel_connections AS connection
            JOIN inboxes AS inbox
              ON inbox.tenant_id = connection.tenant_id
             AND inbox.project_id = connection.project_id
             AND inbox.id = connection.inbox_id
            WHERE connection.tenant_id = $1
              AND connection.project_id = $2
              AND connection.inbox_id = $3
              AND connection.id = $4
              AND connection.status = 'active'
              AND connection.deleted_at IS NULL
              AND inbox.status = 'active'
        )
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(channel_connection_id)
    .fetch_one(db)
    .await?;
    if !channel_available {
        return Ok(false);
    }
    if inbox_has_online_operator(db, tenant_id, project_id, inbox_id).await? {
        return Ok(true);
    }

    let auto_join_profile_languages = sqlx::query_scalar::<_, String>(
        r#"
        SELECT profile.language
        FROM ai_profile_channel_connections AS assignment
        JOIN ai_profiles AS profile
          ON profile.tenant_id = assignment.tenant_id
         AND profile.id = assignment.ai_profile_id
        JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
        JOIN channel_connections AS connection
          ON connection.tenant_id = assignment.tenant_id
         AND connection.id = assignment.channel_connection_id
        JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
        WHERE assignment.tenant_id = $1
          AND assignment.channel_connection_id = $2
          AND profile.project_id = $3
          AND connection.project_id = $3
          AND connection.inbox_id = $4
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
          AND inbox.status = 'active'
          AND profile.status = 'active'
          AND profile.auto_join_new_conversations
          AND provider.status = 'active'
          AND provider.provider_kind IN ('openai', 'openai_compatible')
        ORDER BY assignment.created_at, profile.id
        "#,
    )
    .bind(tenant_id)
    .bind(channel_connection_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_all(db)
    .await?;

    Ok(support_is_online(
        false,
        &auto_join_profile_languages,
        widget_language,
    ))
}

fn support_is_online(
    operator_online: bool,
    auto_join_profile_languages: &[String],
    widget_language: &str,
) -> bool {
    operator_online
        || auto_join_profile_languages.iter().any(|profile_languages| {
            provider_reply::profile_supports_widget_language(
                profile_languages,
                Some(widget_language),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::support_is_online;

    #[test]
    fn reports_support_online_for_a_human_or_compatible_auto_join_profile() {
        assert!(support_is_online(true, &[], "ru"));
        assert!(support_is_online(false, &["en, ru".to_owned()], "ru-RU"));
        assert!(support_is_online(
            false,
            &["en, ru".to_owned(), "ro".to_owned(), "hi".to_owned()],
            "ro"
        ));
        assert!(support_is_online(
            false,
            &["en, ru".to_owned(), "ro".to_owned(), "hi".to_owned()],
            "hi"
        ));
        assert!(!support_is_online(false, &["en".to_owned()], "ro"));
        assert!(!support_is_online(false, &[], "ru"));
    }
}
