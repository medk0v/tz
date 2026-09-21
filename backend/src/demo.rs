//! Public demo identity, read-only settings, and IP-scoped conversation replies.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::ConnectInfo,
    http::{Method, request::Parts},
};
use uuid::Uuid;

use crate::{AppState, client_ip, error::AppError};

pub const DEMO_EMAIL: &str = "demo@tzomet.ai";
// Intentionally public: the account is constrained by server-side authorization.
pub const DEMO_PASSWORD: &str = "531b8NQuLlUFxzxMkFEDvbs0q7Avnqix";
pub const DEMO_WIDGET_URL: &str = "https://demo.tzomet.io";
pub const DEMO_WIDGET_ID: Uuid = Uuid::from_u128(0x66f4_db7a_c798_44ce_947c_a9fd_ac35_1d55);
pub const DEMO_CHANNEL_ID: Uuid = Uuid::from_u128(0x9d29_9e1f_b99f_41da_9330_863d_9438_5058);
pub const DEMO_INBOX_ID: Uuid = Uuid::from_u128(0xde67_8595_1e2e_4574_826e_a29d_b5b8_157e);
pub const DEMO_TENANT_ID: Uuid = Uuid::from_u128(0xc11e_bede_f63c_4315_ab9f_b7c4_2e55_15b6);
pub const DEMO_PROJECT_ID: Uuid = Uuid::from_u128(0x1769_0801_2854_4974_a377_284b_fd23_990d);
pub const DEMO_USER_ID: Uuid = Uuid::from_u128(0xeee3_6b29_db1e_41a6_bf9d_fc18_5aad_1cad);
// Expose every UI section; the route policy below enforces actual read-only access.
pub const DEMO_PERMISSIONS: &[&str] = crate::auth::ADMIN_PERMISSIONS;

pub(crate) fn require_identity(
    actor_id: Uuid,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<(), AppError> {
    if actor_id == DEMO_USER_ID
        && tenant_id == DEMO_TENANT_ID
        && project_id == Some(DEMO_PROJECT_ID)
    {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Resolves a demo request's address only from the peer and trusted proxy header.
pub(crate) fn request_ip(parts: &Parts, state: &AppState) -> Result<IpAddr, AppError> {
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .ok_or(AppError::Unauthorized)?;
    Ok(client_ip::resolve(
        peer.0.ip(),
        &parts.headers,
        &state.config.server.trusted_proxies,
    )?
    .address)
}

pub(crate) fn require_same_ip(stored: Option<&str>, current: IpAddr) -> Result<(), AppError> {
    if stored.and_then(|value| value.parse::<IpAddr>().ok()) == Some(current) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Uses matched route templates so future endpoints remain denied by default.
pub(crate) fn route_allowed(method: &Method, route: &str) -> bool {
    match *method {
        Method::GET => matches!(
            route,
            "/api/v1/me"
                | "/api/v1/inboxes"
                | "/api/v1/inboxes/{inbox_id}/conversations"
                | "/api/v1/conversations/{conversation_id}/messages"
                | "/api/v1/conversations/{conversation_id}/visitor-intelligence"
                | "/api/v1/conversations/{conversation_id}/reply-suggestion-agents"
                | "/api/v1/projects"
                | "/api/v1/projects/{project_id}/members"
                | "/api/v1/projects/{project_id}/team"
                | "/api/v1/projects/{project_id}/positions"
                | "/api/v1/roles"
                | "/api/v1/access-tokens"
                | "/api/v1/contacts"
                | "/api/v1/channels"
                | "/api/v1/widgets"
                | "/api/v1/inboxes/{inbox_id}/online-visitors"
                | "/api/v1/widgets/{widget_id}/online-visitors"
                | "/api/v1/inboxes/{inbox_id}/routing"
                | "/api/v1/inboxes/{inbox_id}/reply-templates"
                | "/api/v1/teams"
                | "/api/v1/processes"
                | "/api/v1/processes/{id}"
                | "/api/v1/process-positions"
                | "/api/v1/email-settings"
                | "/api/v1/support-quality"
                | "/api/v1/integrations/apis"
                | "/api/v1/knowledge-bases"
                | "/api/v1/knowledge-bases/{knowledge_base_id}/articles"
                | "/api/v1/ai/providers"
                | "/api/v1/ai/profiles"
                | "/api/v1/ai/skills"
                | "/api/v1/ai/channel-options"
                | "/api/v1/ai/tasks"
                | "/api/v1/ai/tasks/{task_id}/runs"
                | "/api/v1/ai/tasks/{task_id}/executions"
                | "/api/v1/ai/task-executions/{execution_id}"
                | "/api/v1/ai/task-calendar"
                | "/api/v1/ai/task-steps/mine"
                | "/api/v1/task-board"
                | "/api/v1/ai/profiles/{profile_id}/api"
                | "/api/v1/ai/profiles/{profile_id}/api/runs"
                | "/api/v1/ai/profiles/{profile_id}/api/runs/{run_id}"
                | "/api/v1/ai/profiles/{profile_id}/backups"
                | "/api/v1/ai/profiles/{profile_id}/tests"
                | "/api/v1/ai/profiles/{profile_id}/test-runs"
                | "/api/v1/ai/profiles/{profile_id}/test-runs/{run_id}"
                | "/api/v1/ai/profiles/{profile_id}/test-runs/{run_id}/sources/{source_key}"
        ),
        Method::POST => matches!(
            route,
            "/api/v1/auth/logout"
                | "/api/v1/operator-presence"
                | "/api/v1/realtime-tickets"
                | "/api/v1/conversations/{conversation_id}/messages"
                | "/api/v1/conversations/{conversation_id}/read"
                | "/api/v1/conversations/{conversation_id}/join"
        ),
        Method::DELETE => route == "/api/v1/operator-presence",
        _ => false,
    }
}

/// Public credentials must not allow one visitor to lock out every other visitor.
pub(crate) async fn limit_login(state: &AppState, address: IpAddr) -> Result<(), AppError> {
    let window_seconds =
        i64::try_from(state.config.auth.login_window_seconds).map_err(AppError::internal)?;
    sqlx::query(
        "DELETE FROM demo_login_attempts WHERE window_started_at < now() - interval '1 day'",
    )
    .execute(&state.db)
    .await?;
    let attempts = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO demo_login_attempts (client_ip, attempts, window_started_at)
        VALUES ($1::text::inet, 1, now())
        ON CONFLICT (client_ip) DO UPDATE SET
            attempts = CASE
                WHEN demo_login_attempts.window_started_at > now() - ($2::bigint * interval '1 second')
                    THEN LEAST(demo_login_attempts.attempts + 1, 1000000)
                ELSE 1
            END,
            window_started_at = CASE
                WHEN demo_login_attempts.window_started_at > now() - ($2::bigint * interval '1 second')
                    THEN demo_login_attempts.window_started_at
                ELSE now()
            END
        RETURNING attempts
        "#,
    )
    .bind(address.to_string())
    .bind(window_seconds)
    .fetch_one(&state.db)
    .await?;
    if u32::try_from(attempts).unwrap_or(u32::MAX) > state.config.auth.login_max_attempts {
        return Err(AppError::InvalidCredentials);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{require_same_ip, route_allowed};
    use axum::http::Method;

    #[test]
    fn demo_allows_read_only_pages_and_conversation_session_operations() {
        for route in [
            "/api/v1/me",
            "/api/v1/inboxes",
            "/api/v1/inboxes/{inbox_id}/conversations",
            "/api/v1/conversations/{conversation_id}/messages",
            "/api/v1/ai/tasks/{task_id}/executions",
            "/api/v1/ai/task-executions/{execution_id}",
            "/api/v1/ai/task-calendar",
            "/api/v1/ai/task-steps/mine",
            "/api/v1/task-board",
            "/api/v1/projects/{project_id}/team",
            "/api/v1/projects/{project_id}/positions",
        ] {
            assert!(route_allowed(&Method::GET, route), "{route}");
        }
        for route in [
            "/api/v1/auth/logout",
            "/api/v1/realtime-tickets",
            "/api/v1/operator-presence",
            "/api/v1/conversations/{conversation_id}/messages",
            "/api/v1/conversations/{conversation_id}/read",
            "/api/v1/conversations/{conversation_id}/join",
        ] {
            assert!(route_allowed(&Method::POST, route), "{route}");
        }
        for route in [
            "/api/v1/auth/project",
            "/api/v1/access-tokens",
            "/api/v1/projects",
            "/api/v1/contacts",
            "/api/v1/channels",
            "/api/v1/conversations/{conversation_id}/resolve",
            "/api/v1/conversations/{conversation_id}/attachments",
            "/api/v1/conversations/{conversation_id}/widget-attachments",
            "/api/v1/conversations/{conversation_id}/reply-suggestions",
            "/api/v1/operators/{operator_id}/chat-profile",
            "/api/v1/future-feature",
            "/api/v1/ai/tasks/{task_id}/executions",
            "/api/v1/ai/task-executions/{execution_id}/cancel",
            "/api/v1/ai/task-executions/{execution_id}/retry",
            "/api/v1/ai/task-steps/{step_id}/submit",
            "/api/v1/ai/tasks/{task_id}/move",
            "/api/v1/task-lists",
            "/api/v1/task-tags",
            "/api/v1/projects/{project_id}/positions",
            "/api/v1/projects/{project_id}/positions/{position_id}",
            "/api/v1/projects/{project_id}/positions/{position_id}/approve-kpis",
            "/api/v1/projects/{project_id}/positions/{position_id}/generate-kpis",
            "/api/v1/projects/{project_id}/team/members/{user_id}/position",
            "/api/v1/projects/{project_id}/team/agents/{profile_id}/position",
            "/api/v1/processes/{id}/launch",
        ] {
            for method in [Method::POST, Method::PATCH, Method::DELETE, Method::PUT] {
                assert!(!route_allowed(&method, route), "{method} {route}");
            }
        }
        // A demo visitor reads processes and neither launches them nor sees their runs.
        assert!(route_allowed(&Method::GET, "/api/v1/processes/{id}"));
        for route in [
            "/api/v1/processes/{id}/launch-preview",
            "/api/v1/processes/{id}/runs",
        ] {
            assert!(!route_allowed(&Method::GET, route), "{route}");
        }
        assert!(route_allowed(&Method::GET, "/api/v1/projects"));
        assert!(route_allowed(&Method::GET, "/api/v1/contacts"));
        assert!(route_allowed(&Method::GET, "/api/v1/channels"));
        assert!(!route_allowed(&Method::GET, "/api/v1/system"));
        assert!(!route_allowed(
            &Method::GET,
            "/api/v1/conversations/{conversation_id}/attachments"
        ));
        assert!(!route_allowed(
            &Method::GET,
            "/api/v1/inboxes/id/conversations"
        ));
    }

    #[test]
    fn demo_ip_binding_fails_closed_and_compares_parsed_addresses() {
        let address = "2001:db8::42".parse().unwrap();
        assert!(require_same_ip(Some("2001:0db8:0:0:0:0:0:42"), address).is_ok());
        for stored in [
            None,
            Some("invalid"),
            Some("2001:db8::43"),
            Some("127.0.0.1"),
        ] {
            assert!(require_same_ip(stored, address).is_err());
        }
    }
}
