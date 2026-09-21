//! Password sessions, access tokens, roles, and server-side scope checks.

use std::{net::SocketAddr, str::FromStr};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Extension, FromRequestParts, MatchedPath, Path, Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header, request::Parts},
    routing::{delete, get, patch, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{AppState, client_ip, demo, error::AppError};

const LOCAL_SESSION_COOKIE: &str = "tz_session";
const SECURE_SESSION_COOKIE: &str = "__Host-tz_session";
const LOCAL_CSRF_COOKIE: &str = "tz_csrf";
const SECURE_CSRF_COOKIE: &str = "__Host-tz_csrf";
const CSRF_HEADER: &str = "x-csrf-token";
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=65536,t=4,p=1$VlFQbFhQdTdXclRUcnRvUA$1/DI8gntCfvU5r3yAYUC/1bK2qldIX0UEHCxsnSEDaU";

pub(crate) const ADMIN_PERMISSIONS: &[&str] = &[
    "projects:read",
    "projects:manage",
    "conversations:read",
    "conversations:reply",
    "conversations:close",
    "contacts:read",
    "contacts:manage",
    "channels:read",
    "channels:manage",
    "access_tokens:manage",
    "visitor_network:read",
    "quality:read",
    "quality:read_all",
    "reviews:read",
    "routing:manage",
    "teams:manage",
    "ai:manage",
    "knowledge:manage",
    "reply_templates:manage",
    "notes:read",
    "notes:write",
    "processes:read",
    "processes:edit",
    "processes:approve",
    "tasks:own",
    "tasks:manage",
    "tasks:configure",
    "integrations:manage",
    "roles:manage",
    "system:read",
];
const MANAGER_PERMISSIONS: &[&str] = &[
    "projects:read",
    "conversations:read",
    "conversations:reply",
    "conversations:close",
    "contacts:read",
    "contacts:manage",
    "channels:read",
    "channels:manage",
    "visitor_network:read",
    "quality:read",
    "quality:read_all",
    "reviews:read",
    "routing:manage",
    "teams:manage",
    "ai:manage",
    "knowledge:manage",
    "reply_templates:manage",
    "notes:read",
    "notes:write",
    "processes:read",
    "processes:edit",
    "processes:approve",
    "tasks:own",
    "tasks:manage",
    "integrations:manage",
];
const OPERATOR_PERMISSIONS: &[&str] = &[
    "projects:read",
    "conversations:read",
    "conversations:reply",
    "conversations:close",
    "contacts:read",
    "contacts:manage",
    "channels:read",
    "quality:read",
    "notes:read",
    "notes:write",
    "processes:read",
    "tasks:own",
];
const EDITABLE_ROLE_PERMISSIONS: &[&str] = &[
    "projects:read",
    "conversations:read",
    "conversations:reply",
    "conversations:close",
    "contacts:read",
    "contacts:manage",
    "channels:read",
    "channels:manage",
    "visitor_network:read",
    "quality:read",
    "quality:read_all",
    "reviews:read",
    "routing:manage",
    "teams:manage",
    "ai:manage",
    "knowledge:manage",
    "reply_templates:manage",
    "notes:read",
    "notes:write",
    "processes:read",
    "processes:edit",
    "processes:approve",
    "tasks:own",
    "tasks:manage",
    "tasks:configure",
    "integrations:manage",
    "system:read",
];
const ACCESS_TOKEN_DENIED_PERMISSIONS: &[&str] =
    &["access_tokens:manage", "projects:manage", "roles:manage"];

/// Fixed authorization scopes used underneath project-defined roles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Manager,
    Operator,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Manager => "manager",
            Self::Operator => "operator",
        }
    }

    pub(crate) fn permissions(self) -> Vec<String> {
        let values = match self {
            Self::Admin => ADMIN_PERMISSIONS,
            Self::Manager => MANAGER_PERMISSIONS,
            Self::Operator => OPERATOR_PERMISSIONS,
        };
        values.iter().map(|value| (*value).to_owned()).collect()
    }
}

fn access_token_permissions_from(permissions: &[String]) -> Vec<String> {
    permissions
        .iter()
        .filter(|permission| !ACCESS_TOKEN_DENIED_PERMISSIONS.contains(&permission.as_str()))
        .cloned()
        .collect()
}

fn normalize_role_permissions(permissions: Vec<String>) -> Result<Vec<String>, AppError> {
    let mut normalized = Vec::with_capacity(permissions.len());
    for permission in EDITABLE_ROLE_PERMISSIONS {
        if permissions.iter().any(|value| value == permission) {
            normalized.push((*permission).to_owned());
        }
    }
    if normalized.len() != permissions.len() {
        return Err(AppError::BadRequest(
            "permissions contain an unsupported or duplicate value".to_owned(),
        ));
    }
    let includes = |permission: &str| normalized.iter().any(|value| value == permission);
    if (includes("conversations:reply") || includes("conversations:close"))
        && !includes("conversations:read")
    {
        return Err(AppError::BadRequest(
            "conversation reply and close permissions require conversations:read".to_owned(),
        ));
    }
    if includes("contacts:manage") && !includes("contacts:read") {
        return Err(AppError::BadRequest(
            "contacts:manage requires contacts:read".to_owned(),
        ));
    }
    if includes("channels:manage") && !includes("channels:read") {
        return Err(AppError::BadRequest(
            "channels:manage requires channels:read".to_owned(),
        ));
    }
    if includes("quality:read_all") && !includes("quality:read") {
        return Err(AppError::BadRequest(
            "quality:read_all requires quality:read".to_owned(),
        ));
    }
    if ["processes:edit", "processes:approve"]
        .iter()
        .any(|permission| includes(permission))
        && !includes("processes:read")
    {
        return Err(AppError::BadRequest(
            "process management permissions require processes:read".to_owned(),
        ));
    }
    if includes("notes:write") && !includes("notes:read") {
        return Err(AppError::BadRequest(
            "notes:write requires notes:read".to_owned(),
        ));
    }
    if includes("tasks:manage") && !includes("tasks:own") {
        return Err(AppError::BadRequest(
            "tasks:manage requires tasks:own".to_owned(),
        ));
    }
    if includes("tasks:configure") && !includes("tasks:manage") {
        return Err(AppError::BadRequest(
            "tasks:configure requires tasks:manage".to_owned(),
        ));
    }
    Ok(normalized)
}

impl FromStr for Role {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "admin" => Ok(Self::Admin),
            "manager" => Ok(Self::Manager),
            "operator" => Ok(Self::Operator),
            _ => Err(AppError::internal(anyhow::anyhow!(
                "unknown role stored in database"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AuthMethod {
    Session,
    AccessToken,
}

#[derive(Clone, Debug)]
enum AuthSource {
    Session {
        session_id: Uuid,
        membership_id: Uuid,
    },
    AccessToken {
        api_key_id: Uuid,
    },
}

/// Authenticated operator, administrator, or access-token identity.
#[derive(Clone, Debug)]
pub struct ActorContext {
    pub actor_id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Option<Uuid>,
    role: Role,
    role_id: String,
    role_name: String,
    email: Option<String>,
    display_name: Option<String>,
    chat_display_name: Option<String>,
    avatar_url: Option<String>,
    permissions: Vec<String>,
    inbox_scope: Option<Vec<Uuid>>,
    department: DepartmentContext,
    source: AuthSource,
    is_demo: bool,
    demo_login_ip: Option<String>,
    demo_read_only_request: bool,
    access_all_projects: bool,
    deployment_project_id: Option<Uuid>,
}

#[derive(Clone, Debug, Default)]
struct DepartmentContext {
    id: Option<Uuid>,
    name: Option<String>,
    default_page: Option<String>,
    director_default_page: Option<String>,
    project_name: Option<String>,
    restricted: bool,
    can_access_director: bool,
    is_director: bool,
}

impl ActorContext {
    pub(crate) fn can_choose_resource_visibility(&self) -> bool {
        self.role == Role::Admin || self.department.is_director
    }

    pub fn department_id(&self) -> Option<Uuid> {
        self.department.id
    }

    pub fn department_restricted(&self) -> bool {
        self.department.restricted
    }

    pub fn can_access_director(&self) -> bool {
        self.department.can_access_director
    }

    /// The selected department is a view filter for unrestricted sessions.
    /// Explicit token scopes and assigned departments remain authorization boundaries.
    pub(crate) fn has_restricted_inbox_scope(&self) -> bool {
        self.inbox_scope.is_some()
            && (self.department.restricted
                || self.department.id.is_none()
                || matches!(self.source, AuthSource::AccessToken { .. }))
    }

    pub(crate) fn access_all_projects(&self) -> bool {
        self.access_all_projects
    }

    pub(crate) fn deployment_project_id(&self) -> Option<Uuid> {
        self.deployment_project_id
    }

    /// Keeps tenant-wide administrator grants inside the fixed workspace.
    pub(crate) fn require_deployment_project(&self, project_id: Uuid) -> Result<(), AppError> {
        if self
            .deployment_project_id
            .is_some_and(|fixed| fixed != project_id)
        {
            Err(AppError::Forbidden)
        } else {
            Ok(())
        }
    }

    /// Whether this is the public, strictly restricted demo account.
    pub fn is_demo(&self) -> bool {
        self.is_demo
    }

    /// Address captured at password login, used to scope demo conversations.
    pub fn demo_login_ip(&self) -> Option<&str> {
        self.demo_login_ip.as_deref()
    }

    /// Returns the actor's granted permissions for session introspection.
    pub fn permissions(&self) -> &[String] {
        &self.permissions
    }

    /// Returns the optional Inbox scope attached to the identity.
    pub fn inbox_scope(&self) -> Option<&[Uuid]> {
        self.inbox_scope.as_deref()
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// Requires a named permission.
    ///
    /// # Errors
    ///
    /// Returns `Forbidden` when the permission was not granted.
    pub fn require(&self, permission: &str) -> Result<(), AppError> {
        if self.permissions.iter().any(|value| value == permission) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }

    /// Requires a human administrator session. Access tokens can never mint or
    /// revoke other tokens, including tokens carrying the `admin` role.
    ///
    /// # Errors
    ///
    /// Returns `Forbidden` for a non-admin role or an access-token identity.
    pub fn require_session_admin(&self) -> Result<(), AppError> {
        if self.role == Role::Admin
            && !self.department.restricted
            && (!self.is_demo || self.demo_read_only_request)
            && matches!(self.source, AuthSource::Session { .. })
        {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }

    /// Requires a human identity authenticated by a password-backed session.
    ///
    /// # Errors
    ///
    /// Returns Forbidden for every access-token identity.
    pub fn require_password_session(&self) -> Result<(), AppError> {
        if matches!(self.source, AuthSource::Session { .. }) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }

    /// Requires access to one project.
    ///
    /// # Errors
    ///
    /// Returns `Forbidden` when the authenticated credential is scoped to a
    /// different project.
    pub fn require_project(&self, project_id: Uuid) -> Result<(), AppError> {
        self.require_deployment_project(project_id)?;
        if (self.is_demo && project_id != demo::DEMO_PROJECT_ID)
            || self.project_id.is_some_and(|allowed| allowed != project_id)
        {
            Err(AppError::Forbidden)
        } else {
            Ok(())
        }
    }

    /// Requires access to an Inbox and its project.
    ///
    /// # Errors
    ///
    /// Returns `Forbidden` when either scope excludes the target.
    pub fn require_inbox(&self, project_id: Uuid, inbox_id: Uuid) -> Result<(), AppError> {
        self.require_project(project_id)?;
        if self.is_demo && inbox_id != demo::DEMO_INBOX_ID {
            return Err(AppError::Forbidden);
        }
        if self
            .inbox_scope
            .as_ref()
            .is_some_and(|scope| !scope.contains(&inbox_id))
        {
            return Err(AppError::Forbidden);
        }
        Ok(())
    }

    pub(crate) fn session_id(&self) -> Result<Uuid, AppError> {
        match self.source {
            AuthSource::Session { session_id, .. } => Ok(session_id),
            AuthSource::AccessToken { .. } => Err(AppError::Forbidden),
        }
    }

    pub(crate) fn realtime_credential(&self) -> (&'static str, Uuid) {
        match self.source {
            AuthSource::Session { session_id, .. } => ("operator_session", session_id),
            AuthSource::AccessToken { api_key_id } => ("access_token", api_key_id),
        }
    }

    pub(crate) fn is_password_session(&self) -> bool {
        matches!(self.source, AuthSource::Session { .. })
    }

    /// Marks the current authenticated session or access token online/offline.
    ///
    /// # Errors
    ///
    /// Returns `Unauthorized` when the credential is no longer active, or a
    /// database error when the presence timestamp cannot be persisted.
    pub(crate) async fn set_presence(&self, db: &PgPool, online: bool) -> Result<(), AppError> {
        let updated = match self.source {
            AuthSource::Session {
                session_id,
                membership_id,
            } if self.project_id.is_some() => {
                sqlx::query(
                    r#"
                    INSERT INTO operator_session_projects (
                        session_id, tenant_id, project_id, membership_id,
                        department_id, director_mode, presence_last_seen_at
                    )
                    SELECT session.id, session.tenant_id, $3, membership.id,
                           $5, $6, CASE WHEN $7 THEN now() ELSE NULL END
                    FROM operator_sessions AS session
                    JOIN memberships AS membership
                      ON membership.id = $4 AND membership.tenant_id = session.tenant_id
                     AND membership.user_id = session.user_id AND membership.revoked_at IS NULL
                     AND (membership.project_id IS NULL OR membership.project_id = $3)
                    WHERE session.id = $1 AND session.tenant_id = $2 AND session.revoked_at IS NULL
                    ON CONFLICT (session_id, project_id) DO UPDATE
                    SET presence_last_seen_at = EXCLUDED.presence_last_seen_at
                    "#,
                )
                .bind(session_id)
                .bind(self.tenant_id)
                .bind(self.project_id)
                .bind(membership_id)
                .bind(self.department.id)
                .bind(self.department.is_director)
                .bind(online)
                .execute(db)
                .await?
            }
            AuthSource::Session { session_id, .. } => {
                sqlx::query(
                    r#"
                    UPDATE operator_sessions
                    SET presence_last_seen_at = CASE WHEN $2 THEN now() ELSE NULL END
                    WHERE id = $1 AND revoked_at IS NULL
                    "#,
                )
                .bind(session_id)
                .bind(online)
                .execute(db)
                .await?
            }
            AuthSource::AccessToken { api_key_id } => {
                sqlx::query(
                    r#"
                    UPDATE api_keys
                    SET presence_last_seen_at = CASE WHEN $2 THEN now() ELSE NULL END
                    WHERE id = $1 AND revoked_at IS NULL AND expires_at > now()
                    "#,
                )
                .bind(api_key_id)
                .bind(online)
                .execute(db)
                .await?
            }
        };
        if updated.rows_affected() == 0 {
            return Err(AppError::Unauthorized);
        }
        Ok(())
    }

    fn response(&self) -> ActorResponse {
        ActorResponse {
            actor_id: self.actor_id,
            tenant_id: self.tenant_id,
            project_id: self.project_id,
            role: self.role,
            role_id: self.role_id.clone(),
            role_name: self.role_name.clone(),
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            chat_display_name: self.chat_display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            access_token_project_scope: (!self.is_password_session()).then_some(
                if self.access_all_projects {
                    AccessTokenProjectScope::All
                } else {
                    AccessTokenProjectScope::Project
                },
            ),
            auth_method: match self.source {
                AuthSource::Session { .. } => AuthMethod::Session,
                AuthSource::AccessToken { .. } => AuthMethod::AccessToken,
            },
            permissions: self.permissions.clone(),
            inbox_scope: self.inbox_scope.clone(),
            is_demo: self.is_demo,
            department_id: self.department.id,
            department_name: self.department.name.clone(),
            department_default_page: self.department.default_page.clone(),
            department_restricted: self.department.restricted,
            can_access_director: self.department.can_access_director,
            is_director: self.department.is_director,
            director_default_page: self.department.director_default_page.clone(),
            project_name: self.department.project_name.clone(),
        }
    }
}

#[derive(FromRow)]
struct ApiKeyRow {
    role_project_id: Option<Uuid>,
    id: Uuid,
    actor_user_id: Option<Uuid>,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    role: String,
    role_id: String,
    role_name: String,
    permissions: Vec<String>,
    inbox_scope: Option<Vec<Uuid>>,
    email: String,
    display_name: String,
    chat_display_name: String,
    avatar_url: Option<String>,
}

#[derive(FromRow)]
struct SessionRow {
    session_id: Uuid,
    membership_id: Uuid,
    actor_id: Uuid,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    role: String,
    role_id: String,
    role_name: String,
    permissions: Vec<String>,
    email: String,
    display_name: String,
    chat_display_name: String,
    avatar_url: Option<String>,
    csrf_token_hash: Vec<u8>,
    inbox_scope: Option<Vec<Uuid>>,
    absolute_expires_at: DateTime<Utc>,
    remember_me: bool,
    is_demo: bool,
    login_ip: Option<String>,
}

impl FromRequestParts<AppState> for ActorContext {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let mut actor = if let Some(token) = optional_bearer_token(parts)? {
            authenticate_access_token(state, token, requested_project(parts)?).await?
        } else {
            authenticate_session(state, parts, requested_project(parts)?).await?
        };
        if actor.is_demo() {
            let route = parts
                .extensions
                .get::<MatchedPath>()
                .map(MatchedPath::as_str);
            if !route.is_some_and(|route| demo::route_allowed(&parts.method, route)) {
                return Err(AppError::Forbidden);
            }
            actor.demo_read_only_request = parts.method == Method::GET;
        }
        Ok(actor)
    }
}

fn requested_project(parts: &Parts) -> Result<Option<Uuid>, AppError> {
    let Some(value) = parts.headers.get(crate::config::PROJECT_HEADER) else {
        return Ok(None);
    };
    value
        .to_str()
        .ok()
        .and_then(|value| Uuid::parse_str(value).ok())
        .map(Some)
        .ok_or_else(|| AppError::BadRequest("X-Tz-Project-Id must be a project UUID".to_owned()))
}

async fn authenticate_access_token(
    state: &AppState,
    token: &str,
    project_id: Option<Uuid>,
) -> Result<ActorContext, AppError> {
    load_access_token_actor(state, Some(hash_token(token)), None, project_id).await
}

async fn load_access_token_actor(
    state: &AppState,
    token_hash: Option<Vec<u8>>,
    credential_id: Option<Uuid>,
    selected_project_id: Option<Uuid>,
) -> Result<ActorContext, AppError> {
    let selected_project_id = state.config.product.resolve_project(selected_project_id)?;
    let record_usage = token_hash.is_some();
    let row = sqlx::query_as::<_, ApiKeyRow>(
        r#"
        SELECT api_key.id, api_key.actor_user_id, api_key.tenant_id,
               api_key.project_id, api_key.role_project_id,
               COALESCE(project_role.base_role, api_key.role) AS role,
               api_key.role AS role_id,
               COALESCE(project_role.name, initcap(api_key.role)) AS role_name,
               CASE WHEN api_key.role_project_id IS NOT NULL
                    THEN project_role.permissions ELSE api_key.permissions END AS permissions,
               api_key.inbox_scope,
               app_user.email, app_user.display_name,
               COALESCE(profile.display_name, app_user.display_name) AS chat_display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS avatar_url
        FROM api_keys AS api_key
        JOIN users AS app_user
          ON app_user.id = api_key.actor_user_id
         AND app_user.status = 'active'
         AND NOT app_user.is_demo
        LEFT JOIN project_roles AS project_role
          ON project_role.tenant_id = api_key.tenant_id
         AND project_role.project_id = COALESCE(api_key.role_project_id, api_key.project_id)
         AND project_role.id = api_key.role
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = api_key.tenant_id
         AND profile.project_id = COALESCE($3::uuid, api_key.project_id, api_key.role_project_id)
         AND profile.user_id = api_key.actor_user_id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = api_key.tenant_id
         AND stored_avatar.project_id = COALESCE($3::uuid, api_key.project_id, api_key.role_project_id)
         AND stored_avatar.user_id = api_key.actor_user_id
        WHERE (($1::bytea IS NOT NULL AND api_key.token_hash = $1)
               OR ($2::uuid IS NOT NULL AND api_key.id = $2))
          AND api_key.revoked_at IS NULL
          AND api_key.expires_at > now()
          AND (
              api_key.project_id IS NULL
              OR EXISTS (
                  SELECT 1
                  FROM projects
                  WHERE projects.tenant_id = api_key.tenant_id
                    AND projects.id = api_key.project_id
                    AND projects.status = 'active'
              )
          )
          AND (COALESCE(api_key.role_project_id, api_key.project_id) IS NULL OR project_role.id IS NOT NULL)
          AND (api_key.role_project_id IS NULL OR EXISTS (
              SELECT 1 FROM projects AS role_source
              WHERE role_source.tenant_id = api_key.tenant_id
                AND role_source.id = api_key.role_project_id AND role_source.status = 'active'
          ))
          AND EXISTS (
              SELECT 1
              FROM memberships AS membership
              WHERE membership.tenant_id = api_key.tenant_id
                AND membership.user_id = api_key.actor_user_id
                AND membership.revoked_at IS NULL
                AND membership.role = api_key.role
                AND (
                    membership.project_id = api_key.project_id
                    OR (api_key.project_id IS NULL AND membership.project_id IS NULL)
                )
          )
        "#,
    )
    .bind(token_hash)
    .bind(credential_id)
    .bind(selected_project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    state.config.product.resolve_project(row.role_project_id)?;
    if record_usage {
        sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
            .bind(row.id)
            .execute(&state.db)
            .await?;
    }

    // One fixed project per installation: a token never spans projects.
    let access_all_projects = false;
    let project_id = selected_project_id
        .or(row.project_id)
        .or(row.role_project_id);
    if selected_project_id
        .is_some_and(|selected| row.project_id.is_some_and(|bound| bound != selected))
    {
        return Err(AppError::Forbidden);
    }
    if let Some(project_id) = project_id {
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM projects WHERE tenant_id=$1 AND id=$2 AND status='active')",
        ).bind(row.tenant_id).bind(project_id).fetch_one(&state.db).await?;
        if !active {
            return Err(AppError::Forbidden);
        }
    }
    let actor = ActorContext {
        actor_id: row.actor_user_id.ok_or(AppError::Unauthorized)?,
        tenant_id: row.tenant_id,
        project_id,
        role: Role::from_str(&row.role)?,
        role_id: row.role_id,
        role_name: row.role_name,
        email: Some(row.email),
        display_name: Some(row.display_name),
        chat_display_name: Some(row.chat_display_name),
        avatar_url: row.avatar_url,
        permissions: if row.role_project_id.is_some() {
            access_token_permissions_from(&row.permissions)
        } else {
            row.permissions
        },
        inbox_scope: row.inbox_scope,
        department: DepartmentContext::default(),
        source: AuthSource::AccessToken { api_key_id: row.id },
        is_demo: false,
        demo_login_ip: None,
        demo_read_only_request: false,
        access_all_projects,
        deployment_project_id: state.config.product.fixed_project_id(),
    };
    apply_department_scope(state, actor).await
}

#[derive(FromRow)]
#[allow(clippy::struct_excessive_bools)] // Independent workspace flags read in one query.
struct DepartmentMembershipRow {
    assigned_department_id: Option<Uuid>,
    selected_department_id: Option<Uuid>,
    preferred_department_id: Option<Uuid>,
    director_access: bool,
    director_mode: bool,
    preferred_director_mode: bool,
    director_enabled: bool,
    default_director_workspace: bool,
    director_default_page: Option<String>,
    project_name: Option<String>,
    default_department_id: Option<Uuid>,
    permissions: Vec<String>,
}

/// Reload membership restrictions for both HTTP and long-lived credentials.
/// Token grants are an upper bound, never a substitute for the owner's live grants.
async fn apply_department_scope(
    state: &AppState,
    mut actor: ActorContext,
) -> Result<ActorContext, AppError> {
    actor.project_id = state.config.product.resolve_project(actor.project_id)?;
    actor.deployment_project_id = state.config.product.fixed_project_id();
    let (session_id, membership_id) = match actor.source {
        AuthSource::Session {
            session_id,
            membership_id,
        } => (Some(session_id), Some(membership_id)),
        AuthSource::AccessToken { .. } => (None, None),
    };
    let membership = sqlx::query_as::<_, DepartmentMembershipRow>(
        r#"
        SELECT membership.department_id AS assigned_department_id,
               CASE WHEN workspace.session_id IS NOT NULL THEN workspace.department_id
                    WHEN session.project_id = $3 THEN session.department_id END AS selected_department_id,
               membership.director_access,
               CASE WHEN workspace.session_id IS NOT NULL THEN workspace.director_mode
                    WHEN session.project_id = $3 THEN session.director_mode
                    ELSE false END AS director_mode,
               COALESCE(project.director_enabled, false) AS director_enabled,
               COALESCE(project.default_director_workspace, false) AS default_director_workspace,
               preference.department_id AS preferred_department_id,
               COALESCE(preference.director_mode, false) AS preferred_director_mode,
               project.director_menu->>'default_page' AS director_default_page,
               project.name AS project_name, project.default_department_id,
               COALESCE(project_role.permissions, membership.permissions) AS permissions
        FROM memberships AS membership
        LEFT JOIN operator_sessions AS session ON session.id = $5
        LEFT JOIN operator_session_projects AS workspace
          ON workspace.session_id = session.id AND workspace.project_id = $3
        LEFT JOIN user_project_workspaces AS preference
          ON preference.tenant_id = membership.tenant_id
         AND preference.user_id = membership.user_id AND preference.project_id = $3
        LEFT JOIN projects AS project
          ON project.tenant_id = membership.tenant_id AND project.id = $3
        LEFT JOIN project_roles AS project_role
          ON project_role.tenant_id = membership.tenant_id
         AND project_role.project_id = $3 AND project_role.id = membership.role AND NOT $6
        WHERE membership.tenant_id = $1 AND membership.user_id = $2
          AND membership.revoked_at IS NULL
          AND (membership.project_id = $3 OR membership.project_id IS NULL)
          AND (($5::uuid IS NOT NULL AND membership.id = $7)
               OR ($5::uuid IS NULL AND membership.role = $4
                   AND membership.project_id IS NOT DISTINCT FROM CASE WHEN $6 THEN NULL::uuid ELSE $3 END))
        ORDER BY membership.created_at, membership.id
        LIMIT 1
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(actor.project_id)
    .bind(&actor.role_id)
    .bind(session_id)
    .bind(actor.access_all_projects)
    .bind(membership_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    if matches!(actor.source, AuthSource::AccessToken { .. }) && !actor.access_all_projects {
        actor
            .permissions
            .retain(|permission| membership.permissions.contains(permission));
    }
    actor.department.director_default_page = membership.director_default_page;
    actor.department.project_name = membership.project_name;
    actor.department.restricted = membership.assigned_department_id.is_some();
    actor.department.can_access_director = actor.project_id.is_some()
        && !actor.department.restricted
        && membership.director_enabled
        && (actor.role == Role::Admin || membership.director_access)
        && actor.is_password_session();
    // Workspace precedence: this session's own switch, then the workspace the member last
    // opened (kept across logins), then the project default.
    let session_choice = membership.director_mode || membership.selected_department_id.is_some();
    let remembered_choice =
        membership.preferred_director_mode || membership.preferred_department_id.is_some();
    let wants_director = if session_choice {
        membership.director_mode
    } else if remembered_choice {
        membership.preferred_director_mode
    } else {
        membership.default_director_workspace
    };
    actor.department.is_director = wants_director && actor.department.can_access_director;

    let Some(project_id) = actor.project_id else {
        if actor.department.restricted {
            return Err(AppError::Unauthorized);
        }
        return Ok(actor);
    };
    let chosen_department = if let Some(assigned) = membership.assigned_department_id {
        Some(assigned)
    } else if actor.department.is_director || !actor.is_password_session() {
        None
    } else {
        membership
            .selected_department_id
            .or(membership.preferred_department_id)
            .or(membership.default_department_id)
    };
    // Validate selection against the current project. A deleted selection falls
    // back to the project's first remaining department; a revoked assignment fails closed.
    let department = if actor.department.is_director
        || (!actor.is_password_session() && chosen_department.is_none())
    {
        None
    } else {
        sqlx::query_as::<_, (Uuid, String, String)>(
            r#"
            SELECT id, name, default_page FROM departments
            WHERE tenant_id = $1 AND project_id = $2
              AND (NOT $4 OR id = $3)
            ORDER BY CASE WHEN id = $3 THEN 0 ELSE 1 END, position, created_at, id
            LIMIT 1
            "#,
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(chosen_department)
        .bind(actor.department.restricted)
        .fetch_optional(&state.db)
        .await?
    };
    if actor.department.restricted && department.is_none() {
        return Err(AppError::Unauthorized);
    }

    if let Some((department_id, name, default_page)) = department {
        let inbox_ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND department_id = $3",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(department_id)
        .fetch_all(&state.db)
        .await?;
        actor.inbox_scope = Some(match actor.inbox_scope.take() {
            Some(scope) => scope
                .into_iter()
                .filter(|id| inbox_ids.contains(id))
                .collect(),
            None => inbox_ids,
        });
        actor.department.id = Some(department_id);
        actor.department.name = Some(name);
        actor.department.default_page = Some(default_page);
    }
    Ok(actor)
}

async fn authenticate_session(
    state: &AppState,
    parts: &Parts,
    project_id: Option<Uuid>,
) -> Result<ActorContext, AppError> {
    let cookie_name = session_cookie_name(state.config.auth.cookie_secure);
    let token = cookie_value(parts, cookie_name).ok_or(AppError::Unauthorized)?;
    let row = load_session_row(state, Some(hash_token(token)), None, project_id).await?;

    validate_csrf(parts, &row.csrf_token_hash)?;
    if row.is_demo {
        demo::require_same_ip(row.login_ip.as_deref(), demo::request_ip(parts, state)?)?;
    }
    let idle_expires_at = std::cmp::min(
        row.absolute_expires_at,
        Utc::now() + chrono_duration(state.config.auth.idle_ttl_seconds(row.remember_me))?,
    );
    sqlx::query(
        r#"
        UPDATE operator_sessions
        SET last_seen_at = now(), idle_expires_at = $2
        WHERE id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(row.session_id)
    .bind(idle_expires_at)
    .execute(&state.db)
    .await?;

    apply_department_scope(state, session_actor(row)?).await
}

async fn load_session_row(
    state: &AppState,
    token_hash: Option<Vec<u8>>,
    credential_id: Option<Uuid>,
    selected_project_id: Option<Uuid>,
) -> Result<SessionRow, AppError> {
    let selected_project_id = state.config.product.resolve_project(selected_project_id)?;
    let row = sqlx::query_as::<_, SessionRow>(
        r#"
        SELECT session.id AS session_id, membership.id AS membership_id,
               session.user_id AS actor_id, session.tenant_id, scope.project_id,
               COALESCE(project_role.base_role, membership.role) AS role,
               membership.role AS role_id,
               COALESCE(project_role.name, initcap(membership.role)) AS role_name,
               COALESCE(project_role.permissions, membership.permissions) AS permissions,
               app_user.email, app_user.display_name,
               COALESCE(profile.display_name, app_user.display_name) AS chat_display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS avatar_url,
               session.csrf_token_hash,
               CASE WHEN scope.project_id IS NULL
                   THEN ARRAY[]::uuid[]
                   ELSE NULL::uuid[]
               END AS inbox_scope,
               session.absolute_expires_at, session.remember_me,
               app_user.is_demo, host(session.login_ip) AS login_ip
        FROM operator_sessions AS session
        JOIN users AS app_user ON app_user.id = session.user_id
        CROSS JOIN LATERAL (
            SELECT COALESCE($3::uuid, session.project_id) AS project_id
        ) AS scope
        LEFT JOIN operator_session_projects AS workspace
          ON workspace.session_id = session.id AND workspace.project_id = scope.project_id
        JOIN LATERAL (
            SELECT candidate.* FROM memberships AS candidate
            LEFT JOIN project_roles AS candidate_role
              ON candidate_role.tenant_id = candidate.tenant_id
             AND candidate_role.project_id = scope.project_id
             AND candidate_role.id = candidate.role
            WHERE candidate.tenant_id = session.tenant_id
              AND candidate.user_id = session.user_id
              AND candidate.revoked_at IS NULL
              AND (candidate.project_id = scope.project_id OR candidate.project_id IS NULL)
              AND (scope.project_id IS NULL OR candidate_role.id IS NOT NULL)
              AND ($3::uuid IS NOT NULL OR candidate.id = session.membership_id)
              AND (workspace.session_id IS NULL OR candidate.id = workspace.membership_id)
            ORDER BY
                CASE COALESCE(candidate_role.base_role, candidate.role) WHEN 'admin' THEN 0 ELSE 1 END,
                CASE WHEN candidate.project_id IS NULL THEN 0 ELSE 1 END,
                candidate.created_at, candidate.id
            LIMIT 1
        ) AS membership ON true
        LEFT JOIN project_roles AS project_role
          ON project_role.tenant_id = session.tenant_id
         AND project_role.project_id = scope.project_id
         AND project_role.id = membership.role
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = session.tenant_id
         AND profile.project_id = scope.project_id
         AND profile.user_id = session.user_id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = session.tenant_id
         AND stored_avatar.project_id = scope.project_id
         AND stored_avatar.user_id = session.user_id
        WHERE (($1::bytea IS NOT NULL AND session.token_hash = $1)
               OR ($2::uuid IS NOT NULL AND session.id = $2))
          AND session.revoked_at IS NULL
          AND session.idle_expires_at > now()
          AND session.absolute_expires_at > now()
          AND app_user.status = 'active'
          AND (
              scope.project_id IS NULL
              OR EXISTS (
                  SELECT 1 FROM projects
                  WHERE projects.tenant_id = session.tenant_id
                    AND projects.id = scope.project_id
                    AND projects.status = 'active'
              )
          )
        "#,
    )
    .bind(token_hash.as_deref())
    .bind(credential_id)
    .bind(selected_project_id)
    .fetch_optional(&state.db)
    .await?;
    if let Some(row) = row {
        return Ok(row);
    }
    // Invalid credentials remain 401; a valid session cannot silently fall back
    // to another project when its explicitly requested project is unavailable.
    if selected_project_id.is_some() {
        let session_active: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM operator_sessions AS session
                JOIN users AS app_user ON app_user.id = session.user_id
                WHERE (($1::bytea IS NOT NULL AND session.token_hash = $1)
                       OR ($2::uuid IS NOT NULL AND session.id = $2))
                  AND session.revoked_at IS NULL AND session.idle_expires_at > now()
                  AND session.absolute_expires_at > now() AND app_user.status = 'active'
            )
            "#,
        )
        .bind(token_hash.as_deref())
        .bind(credential_id)
        .fetch_one(&state.db)
        .await?;
        if session_active {
            return Err(AppError::Forbidden);
        }
    }
    Err(AppError::Unauthorized)
}

fn session_actor(row: SessionRow) -> Result<ActorContext, AppError> {
    if row.is_demo {
        demo::require_identity(row.actor_id, row.tenant_id, row.project_id)?;
    }
    let role = if row.is_demo {
        Role::Admin
    } else {
        Role::from_str(&row.role)?
    };
    Ok(ActorContext {
        actor_id: row.actor_id,
        tenant_id: row.tenant_id,
        project_id: row.project_id,
        role,
        role_id: row.role_id,
        role_name: row.role_name,
        email: Some(row.email),
        display_name: Some(row.display_name),
        chat_display_name: Some(row.chat_display_name),
        avatar_url: row.avatar_url,
        permissions: if row.is_demo {
            demo::DEMO_PERMISSIONS
                .iter()
                .map(|value| (*value).to_owned())
                .collect()
        } else {
            row.permissions
        },
        inbox_scope: if row.is_demo { None } else { row.inbox_scope },
        is_demo: row.is_demo,
        demo_login_ip: row.login_ip,
        demo_read_only_request: false,
        access_all_projects: false,
        deployment_project_id: None,
        department: DepartmentContext::default(),
        source: AuthSource::Session {
            session_id: row.session_id,
            membership_id: row.membership_id,
        },
    })
}

/// Reloads the credential without extending HTTP session idle expiration.
#[cfg(test)]
pub(crate) async fn authenticate_realtime_actor(
    state: &AppState,
    credential_kind: &str,
    credential_id: Uuid,
) -> Result<ActorContext, AppError> {
    authenticate_realtime_actor_for_project(state, credential_kind, credential_id, None).await
}

pub(crate) async fn authenticate_realtime_actor_for_project(
    state: &AppState,
    credential_kind: &str,
    credential_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<ActorContext, AppError> {
    match credential_kind {
        "operator_session" => {
            apply_department_scope(
                state,
                session_actor(
                    load_session_row(state, None, Some(credential_id), project_id).await?,
                )?,
            )
            .await
        }
        "access_token" => {
            load_access_token_actor(state, None, Some(credential_id), project_id).await
        }
        _ => Err(AppError::Unauthorized),
    }
}

/// Authenticated anonymous visitor created by a widget session.
#[derive(Clone, Debug, FromRow)]
pub struct WidgetSessionContext {
    pub session_id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub inbox_id: Uuid,
    pub channel_connection_id: Uuid,
    pub contact_id: Uuid,
    pub language: String,
    pub client_ip: Option<String>,
}

impl FromRequestParts<AppState> for WidgetSessionContext {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = required_bearer_token(parts)?;
        let session = load_widget_session(state, Some(hash_token(token)), None).await?;
        if session.channel_connection_id == demo::DEMO_CHANNEL_ID {
            demo::require_same_ip(
                session.client_ip.as_deref(),
                demo::request_ip(parts, state)?,
            )?;
        }
        Ok(session)
    }
}

pub(crate) async fn load_widget_session(
    state: &AppState,
    token_hash: Option<Vec<u8>>,
    credential_id: Option<Uuid>,
) -> Result<WidgetSessionContext, AppError> {
    let session = sqlx::query_as::<_, WidgetSessionContext>(
        r#"
        SELECT id AS session_id, tenant_id, project_id, inbox_id,
               channel_connection_id, contact_id, language, host(client_ip) AS client_ip
        FROM widget_sessions
        WHERE (($1::bytea IS NOT NULL AND token_hash = $1)
               OR ($2::uuid IS NOT NULL AND id = $2))
          AND expires_at > now()
          AND revoked_at IS NULL
          AND EXISTS (
              SELECT 1 FROM projects
              WHERE projects.tenant_id = widget_sessions.tenant_id
                AND projects.id = widget_sessions.project_id
                AND projects.deleted_at IS NULL
          )
        "#,
    )
    .bind(token_hash)
    .bind(credential_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;
    state
        .config
        .product
        .resolve_project(Some(session.project_id))?;
    Ok(session)
}

/// Routes for password login, session introspection, and access-token lifecycle.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/project", post(select_project))
        .route("/api/v1/auth/department", post(select_department))
        .route("/api/v1/me", get(me))
        .route("/api/v1/roles", get(list_roles).post(create_role))
        .route(
            "/api/v1/roles/{role_id}",
            patch(update_role).delete(delete_role),
        )
        .route(
            "/api/v1/access-tokens",
            get(list_access_tokens).post(create_access_token),
        )
        .route(
            "/api/v1/access-tokens/{token_id}",
            delete(delete_access_token),
        )
        .route(
            "/api/v1/access-tokens/{token_id}/revoke",
            post(revoke_access_token),
        )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    email: String,
    password: String,
    #[serde(default)]
    remember_me: bool,
}

#[derive(Debug, FromRow)]
struct LoginRow {
    user_id: Uuid,
    status: String,
    password_hash: Option<String>,
    is_demo: bool,
    locked_until: Option<DateTime<Utc>>,
    membership_id: Uuid,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)] // Independent flags in the public session response.
struct ActorResponse {
    actor_id: Uuid,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    role: Role,
    role_id: String,
    role_name: String,
    email: Option<String>,
    display_name: Option<String>,
    chat_display_name: Option<String>,
    avatar_url: Option<String>,
    auth_method: AuthMethod,
    access_token_project_scope: Option<AccessTokenProjectScope>,
    permissions: Vec<String>,
    inbox_scope: Option<Vec<Uuid>>,
    is_demo: bool,
    department_id: Option<Uuid>,
    department_name: Option<String>,
    department_default_page: Option<String>,
    department_restricted: bool,
    can_access_director: bool,
    is_director: bool,
    director_default_page: Option<String>,
    project_name: Option<String>,
}

async fn login(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<ActorResponse>), AppError> {
    let email = request.email.trim().to_lowercase();
    // The shared demo identity is not part of this product.
    if email == demo::DEMO_EMAIL {
        return Err(AppError::InvalidCredentials);
    }
    let login_ip = if email == demo::DEMO_EMAIL {
        let Extension(ConnectInfo(peer)) = peer.ok_or(AppError::InvalidCredentials)?;
        let address =
            client_ip::resolve(peer.ip(), &headers, &state.config.server.trusted_proxies)?.address;
        demo::limit_login(&state, address).await?;
        Some(address.to_string())
    } else {
        None
    };
    if email.is_empty()
        || email.len() > 320
        || request.password.is_empty()
        || request.password.len() > 128
    {
        verify_password(request.password, None).await?;
        return Err(AppError::InvalidCredentials);
    }

    let row = sqlx::query_as::<_, LoginRow>(
        r#"
        SELECT app_user.id AS user_id, app_user.email, app_user.display_name,
               app_user.status, app_user.password_hash, app_user.locked_until, app_user.is_demo,
               membership.id AS membership_id, membership.tenant_id,
               membership.project_id,
               COALESCE(project_role.base_role, membership.role) AS role,
               membership.role AS role_id,
               COALESCE(project_role.name, initcap(membership.role)) AS role_name,
               COALESCE(project_role.permissions, membership.permissions) AS permissions
        FROM users AS app_user
        JOIN LATERAL (
            SELECT candidate.id, candidate.tenant_id, COALESCE($2::uuid, candidate.project_id) AS project_id,
                   candidate.role, candidate.permissions, candidate.created_at
            FROM memberships AS candidate
            WHERE candidate.user_id = app_user.id
              AND candidate.revoked_at IS NULL
              AND ($2::uuid IS NULL OR (
                  (candidate.project_id IS NULL OR candidate.project_id = $2)
                  AND EXISTS (SELECT 1 FROM projects AS fixed_project
                      WHERE fixed_project.id = $2 AND fixed_project.tenant_id = candidate.tenant_id
                        AND fixed_project.status = 'active')
              ))
              AND (
                  candidate.project_id IS NULL
                  OR EXISTS (
                      SELECT 1
                      FROM projects
                      WHERE projects.tenant_id = candidate.tenant_id
                        AND projects.id = candidate.project_id
                        AND projects.status = 'active'
                  )
              )
            ORDER BY
                CASE candidate.role WHEN 'admin' THEN 0 ELSE 1 END,
                candidate.created_at ASC,
                candidate.id ASC
            LIMIT 1
        ) AS membership ON true
        LEFT JOIN project_roles AS project_role
          ON project_role.tenant_id = membership.tenant_id
         AND project_role.project_id = membership.project_id
         AND project_role.id = membership.role
        WHERE lower(app_user.email) = $1
          AND (membership.project_id IS NULL OR project_role.id IS NOT NULL)
        "#,
    )
    .bind(&email)
    .bind(state.config.product.fixed_project_id())
    .fetch_optional(&state.db)
    .await?;

    let password_valid = verify_password(
        request.password,
        row.as_ref().and_then(|value| value.password_hash.clone()),
    )
    .await?;
    let Some(row) = row else {
        return Err(AppError::InvalidCredentials);
    };
    if !row.is_demo
        && row
            .locked_until
            .is_some_and(|locked_until| locked_until > Utc::now())
    {
        return Err(AppError::InvalidCredentials);
    }
    if !password_valid || row.status != "active" || row.password_hash.is_none() {
        if !row.is_demo {
            register_failed_login(&state, row.user_id).await?;
        }
        return Err(AppError::InvalidCredentials);
    }

    if row.is_demo {
        if login_ip.is_none() {
            return Err(AppError::InvalidCredentials);
        }
        demo::require_identity(row.user_id, row.tenant_id, row.project_id)?;
    }
    let session_id = Uuid::now_v7();
    let session_token = generate_token();
    let csrf_token = generate_token();
    let now = Utc::now();
    let max_age = state.config.auth.absolute_ttl_seconds(request.remember_me);
    let absolute_expires_at = now + chrono_duration(max_age)?;
    let idle_expires_at = std::cmp::min(
        absolute_expires_at,
        now + chrono_duration(state.config.auth.idle_ttl_seconds(request.remember_me))?,
    );
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        UPDATE users
        SET failed_login_attempts = 0,
            failed_login_window_started_at = NULL,
            locked_until = NULL,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(row.user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO operator_sessions (
            id, tenant_id, project_id, user_id, membership_id,
            token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at, remember_me, login_ip
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::text::inet)
        "#,
    )
    .bind(session_id)
    .bind(row.tenant_id)
    .bind(row.project_id)
    .bind(row.user_id)
    .bind(row.membership_id)
    .bind(hash_token(&session_token))
    .bind(hash_token(&csrf_token))
    .bind(idle_expires_at)
    .bind(absolute_expires_at)
    .bind(request.remember_me)
    .bind(&login_ip)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: row.tenant_id,
            project_id: row.project_id,
            actor_id: row.user_id,
            action: "auth.login",
            resource_kind: "session",
            resource_id: Some(session_id),
            metadata: serde_json::json!({ "method": "password", "remember_me": request.remember_me }),
        },
    )
    .await?;
    transaction.commit().await?;

    let actor = apply_department_scope(
        &state,
        session_actor(load_session_row(&state, None, Some(session_id), None).await?)?,
    )
    .await?;
    let headers = session_cookie_headers(
        state.config.auth.cookie_secure,
        &session_token,
        &csrf_token,
        max_age,
    )?;
    Ok((headers, Json(actor.response())))
}

async fn register_failed_login(state: &AppState, user_id: Uuid) -> Result<(), AppError> {
    let now = Utc::now();
    let window_started_after = now - chrono_duration(state.config.auth.login_window_seconds)?;
    let locked_until = now + chrono_duration(state.config.auth.login_lockout_seconds)?;
    sqlx::query(
        r#"
        UPDATE users
        SET failed_login_attempts = CASE
                WHEN failed_login_window_started_at >= $2
                    THEN failed_login_attempts + 1
                ELSE 1
            END,
            failed_login_window_started_at = CASE
                WHEN failed_login_window_started_at >= $2
                    THEN failed_login_window_started_at
                ELSE $3
            END,
            locked_until = CASE
                WHEN (CASE
                    WHEN failed_login_window_started_at >= $2
                        THEN failed_login_attempts + 1
                    ELSE 1
                END) >= $4
                    THEN $5
                ELSE locked_until
            END,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .bind(window_started_after)
    .bind(now)
    .bind(i32::try_from(state.config.auth.login_max_attempts).map_err(AppError::internal)?)
    .bind(locked_until)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn verify_password(
    password: String,
    password_hash: Option<String>,
) -> Result<bool, AppError> {
    tokio::task::spawn_blocking(move || {
        let encoded = password_hash.as_deref().unwrap_or(DUMMY_PASSWORD_HASH);
        PasswordHash::new(encoded).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
                && password_hash.is_some()
        })
    })
    .await
    .map_err(AppError::internal)
}

async fn me(actor: ActorContext) -> Json<ActorResponse> {
    Json(actor.response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectProjectRequest {
    project_id: Uuid,
}

#[derive(Debug, FromRow)]
struct ProjectMembershipRow {
    membership_id: Uuid,
}

async fn select_project(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<SelectProjectRequest>,
) -> Result<Json<ActorResponse>, AppError> {
    state
        .config
        .product
        .resolve_project(Some(request.project_id))?;
    actor.require_password_session()?;
    let session_id = actor.session_id()?;
    let mut transaction = state.db.begin().await?;
    sqlx::query("SELECT id FROM operator_sessions WHERE id = $1 AND revoked_at IS NULL FOR UPDATE")
        .bind(session_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Unauthorized)?;
    let membership = sqlx::query_as::<_, ProjectMembershipRow>(
        r#"
        SELECT membership.id AS membership_id,
               COALESCE(project_role.base_role, membership.role) AS role,
               membership.role AS role_id,
               COALESCE(project_role.name, initcap(membership.role)) AS role_name,
               COALESCE(project_role.permissions, membership.permissions) AS permissions
        FROM memberships AS membership
        JOIN projects AS project
          ON project.tenant_id = membership.tenant_id
         AND project.id = $3
         AND project.status = 'active'
        LEFT JOIN project_roles AS project_role
          ON project_role.tenant_id = membership.tenant_id
         AND project_role.project_id = project.id
         AND project_role.id = membership.role
        WHERE membership.tenant_id = $1
          AND membership.user_id = $2
          AND membership.revoked_at IS NULL
          AND (membership.project_id = project.id OR membership.project_id IS NULL)
          AND project_role.id IS NOT NULL
        ORDER BY
            CASE project_role.base_role WHEN 'admin' THEN 0 ELSE 1 END,
            CASE WHEN membership.project_id IS NULL THEN 0 ELSE 1 END,
            membership.created_at ASC,
            membership.id ASC
        LIMIT 1
        FOR SHARE OF membership, project
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(request.project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;

    // Keep the legacy default URL usable while preserving each open project's
    // department and presence when the default selection changes.
    sqlx::query(
        r#"
        INSERT INTO operator_session_projects (
            session_id, tenant_id, project_id, membership_id,
            department_id, director_mode, presence_last_seen_at
        )
        SELECT id, tenant_id, project_id, membership_id,
               department_id, director_mode, presence_last_seen_at
        FROM operator_sessions WHERE id = $1 AND project_id IS NOT NULL
        ON CONFLICT (session_id, project_id) DO NOTHING
        "#,
    )
    .bind(session_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO operator_session_projects (session_id, tenant_id, project_id, membership_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (session_id, project_id) DO UPDATE SET membership_id = EXCLUDED.membership_id
        "#,
    )
    .bind(session_id)
    .bind(actor.tenant_id)
    .bind(request.project_id)
    .bind(membership.membership_id)
    .execute(&mut *transaction)
    .await?;
    let updated = sqlx::query(
        r#"
        UPDATE operator_sessions AS session
        SET project_id = $3, membership_id = $4, department_id = workspace.department_id,
            director_mode = workspace.director_mode, presence_last_seen_at = NULL, last_seen_at = now()
        FROM operator_session_projects AS workspace
        WHERE session.id = $1 AND session.tenant_id = $2 AND session.revoked_at IS NULL
          AND workspace.session_id = session.id AND workspace.project_id = $3
        "#,
    )
    .bind(session_id)
    .bind(actor.tenant_id)
    .bind(request.project_id)
    .bind(membership.membership_id)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Unauthorized);
    }
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: Some(request.project_id),
            actor_id: actor.actor_id,
            action: "auth.project_selected",
            resource_kind: "project",
            resource_id: Some(request.project_id),
            metadata: serde_json::json!({ "session_id": session_id }),
        },
    )
    .await?;
    transaction.commit().await?;

    let refreshed =
        load_session_row(&state, None, Some(session_id), Some(request.project_id)).await?;
    Ok(Json(
        apply_department_scope(&state, session_actor(refreshed)?)
            .await?
            .response(),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectDepartmentRequest {
    #[serde(deserialize_with = "deserialize_department_selection")]
    department_id: Option<Uuid>,
}

fn deserialize_department_selection<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Uuid>, D::Error> {
    Option::<Uuid>::deserialize(deserializer)
}

async fn select_department(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<SelectDepartmentRequest>,
) -> Result<Json<ActorResponse>, AppError> {
    actor.require_password_session()?;
    let AuthSource::Session {
        session_id,
        membership_id,
    } = actor.source
    else {
        return Err(AppError::Forbidden);
    };
    let project_id = actor.project_id.ok_or(AppError::Forbidden)?;
    if let Some(department_id) = request.department_id {
        if actor.department_restricted() && actor.department_id() != Some(department_id) {
            return Err(AppError::Forbidden);
        }
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM departments WHERE tenant_id = $1 AND project_id = $2 AND id = $3)",
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(department_id)
        .fetch_one(&state.db)
        .await?;
        if !exists {
            return Err(AppError::Forbidden);
        }
    } else if !actor.can_access_director() {
        return Err(AppError::Forbidden);
    }
    let mut transaction = state.db.begin().await?;
    sqlx::query("SELECT id FROM operator_sessions WHERE id = $1 AND revoked_at IS NULL FOR UPDATE")
        .bind(session_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Unauthorized)?;
    // Recheck the membership inside the write to cover concurrent access edits.
    let updated = sqlx::query(
        r#"
        INSERT INTO operator_session_projects (
            session_id, tenant_id, project_id, membership_id,
            department_id, director_mode, presence_last_seen_at
        )
        SELECT session.id, session.tenant_id, $3, membership.id, $4, ($4::uuid IS NULL), NULL
        FROM operator_sessions AS session, memberships AS membership, projects AS project,
             project_roles AS project_role
        WHERE session.id = $1 AND session.tenant_id = $2
          AND session.revoked_at IS NULL
          AND membership.id = $5 AND membership.revoked_at IS NULL
          AND membership.tenant_id = session.tenant_id AND membership.user_id = session.user_id
          AND (membership.project_id IS NULL OR membership.project_id = $3)
          AND project.tenant_id = session.tenant_id AND project.id = $3
          AND project.status = 'active'
          AND project_role.tenant_id = session.tenant_id AND project_role.project_id = $3
          AND project_role.id = membership.role
          AND (membership.department_id IS NULL OR membership.department_id = $4)
          AND ($4::uuid IS NOT NULL OR (
              membership.department_id IS NULL AND project.director_enabled
              AND (project_role.base_role = 'admin' OR membership.director_access)
          ))
        ON CONFLICT (session_id, project_id) DO UPDATE
        SET membership_id = EXCLUDED.membership_id, department_id = EXCLUDED.department_id,
            director_mode = EXCLUDED.director_mode, presence_last_seen_at = NULL
        "#,
    )
    .bind(session_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(request.department_id)
    .bind(membership_id)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"
        UPDATE operator_sessions
        SET department_id = $3, director_mode = ($3::uuid IS NULL),
            presence_last_seen_at = NULL, last_seen_at = now()
        WHERE id = $1 AND project_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(session_id)
    .bind(project_id)
    .bind(request.department_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO user_project_workspaces (tenant_id, user_id, project_id, department_id, director_mode)
        VALUES ($1, $2, $3, $4, $4::uuid IS NULL)
        ON CONFLICT (tenant_id, user_id, project_id)
        DO UPDATE SET department_id = EXCLUDED.department_id,
                      director_mode = EXCLUDED.director_mode, updated_at = now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(project_id)
    .bind(request.department_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: Some(project_id),
            actor_id: actor.actor_id,
            action: "auth.department_selected",
            resource_kind: "department",
            resource_id: request.department_id,
            metadata: serde_json::json!({ "director_mode": request.department_id.is_none() }),
        },
    )
    .await?;
    transaction.commit().await?;
    let refreshed = load_session_row(&state, None, Some(session_id), Some(project_id)).await?;
    Ok(Json(
        apply_department_scope(&state, session_actor(refreshed)?)
            .await?
            .response(),
    ))
}

#[derive(Clone, Debug, FromRow)]
pub(crate) struct ProjectRoleDefinition {
    pub id: String,
    pub name: String,
    base_role: String,
    pub permissions: Vec<String>,
    pub is_system: bool,
}

impl ProjectRoleDefinition {
    pub(crate) fn base_role(&self) -> Result<Role, AppError> {
        Role::from_str(&self.base_role)
    }
}

#[derive(Debug, FromRow)]
struct ProjectRoleListRow {
    id: String,
    name: String,
    base_role: String,
    permissions: Vec<String>,
    is_system: bool,
    member_count: i64,
    active_token_count: i64,
}

#[derive(Debug, Serialize)]
struct RoleResponse {
    id: String,
    name: String,
    base_role: Role,
    permissions: Vec<String>,
    access_token_permissions: Vec<String>,
    is_system: bool,
    member_count: i64,
    active_token_count: i64,
}

#[derive(Debug, Serialize)]
struct RoleListResponse {
    items: Vec<RoleResponse>,
}

pub(crate) async fn lock_project_role(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    role_id: &str,
) -> Result<ProjectRoleDefinition, AppError> {
    validate_role_id(role_id)?;
    sqlx::query_as::<_, ProjectRoleDefinition>(
        r#"
        SELECT id, name, base_role, permissions, is_system
        FROM project_roles
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR SHARE
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(role_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

fn role_response(row: ProjectRoleListRow) -> Result<RoleResponse, AppError> {
    Ok(RoleResponse {
        id: row.id,
        name: row.name,
        base_role: Role::from_str(&row.base_role)?,
        access_token_permissions: access_token_permissions_from(&row.permissions),
        permissions: row.permissions,
        is_system: row.is_system,
        member_count: row.member_count,
        active_token_count: row.active_token_count,
    })
}

#[derive(Default, Deserialize)]
struct RoleListQuery {
    project_id: Option<Uuid>,
}

async fn list_roles(
    State(state): State<AppState>,
    actor: ActorContext,
    Query(query): Query<RoleListQuery>,
) -> Result<Json<RoleListResponse>, AppError> {
    actor.require_session_admin()?;
    let project_id = query
        .project_id
        .or(actor.project_id)
        .ok_or_else(|| AppError::BadRequest("select a project before managing roles".to_owned()))?;
    crate::projects::require_project_admin_access(&state.db, &actor, project_id).await?;
    let rows = sqlx::query_as::<_, ProjectRoleListRow>(
        r#"
        SELECT role.id, role.name, role.base_role, role.permissions, role.is_system,
               (SELECT COUNT(*) FROM memberships AS membership
                WHERE membership.tenant_id = role.tenant_id
                  AND (membership.project_id = role.project_id OR membership.project_id IS NULL)
                  AND NOT EXISTS (SELECT 1 FROM api_keys AS role_token
                      WHERE role_token.actor_user_id = membership.user_id
                        AND role_token.tenant_id = membership.tenant_id
                        AND role_token.role_project_id IS NOT NULL
                        AND role_token.role_project_id <> role.project_id)
                  AND membership.role = role.id
                  AND membership.revoked_at IS NULL) AS member_count,
               (SELECT COUNT(*) FROM api_keys AS api_key
                WHERE api_key.tenant_id = role.tenant_id
                  AND COALESCE(api_key.role_project_id, api_key.project_id) = role.project_id
                  AND api_key.role = role.id
                  AND api_key.revoked_at IS NULL
                  AND api_key.expires_at > now()) AS active_token_count
        FROM project_roles AS role
        WHERE role.tenant_id = $1 AND role.project_id = $2
        ORDER BY role.is_system DESC, role.created_at ASC, role.id ASC
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let items = rows
        .into_iter()
        .map(role_response)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(RoleListResponse { items }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRoleRequest {
    name: String,
    base_role: Role,
    permissions: Vec<String>,
}

async fn create_role(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<RoleResponse>), AppError> {
    actor.require_session_admin()?;
    let project_id = selected_project_id(&actor)?;
    let name = normalize_role_name(request.name)?;
    require_non_admin_base_role(request.base_role)?;
    let permissions = normalize_role_permissions(request.permissions)?;
    let id = format!("role_{}", Uuid::now_v7().simple());
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO project_roles (
            tenant_id, project_id, id, name, base_role, permissions,
            is_system, updated_by_user_id
        ) VALUES ($1, $2, $3, $4, $5, $6, false, $7)
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&id)
    .bind(&name)
    .bind(request.base_role.as_str())
    .bind(&permissions)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await
    .map_err(role_write_error)?;
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: Some(project_id),
            actor_id: actor.actor_id,
            action: "role.created",
            resource_kind: "role",
            resource_id: None,
            metadata: serde_json::json!({
                "role_id": id,
                "name": name,
                "base_role": request.base_role,
                "permissions": permissions,
            }),
        },
    )
    .await?;
    transaction.commit().await?;
    let response = load_role_response(&state.db, actor.tenant_id, project_id, &id).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRoleRequest {
    name: String,
    base_role: Role,
    permissions: Vec<String>,
}

async fn update_role(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(role_id): Path<String>,
    Json(request): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    actor.require_session_admin()?;
    let project_id = selected_project_id(&actor)?;
    validate_role_id(&role_id)?;
    let name = normalize_role_name(request.name)?;
    require_non_admin_base_role(request.base_role)?;
    let permissions = normalize_role_permissions(request.permissions)?;
    let access_token_permissions = access_token_permissions_from(&permissions);
    let mut transaction = state.db.begin().await?;
    let previous = sqlx::query_as::<_, ProjectRoleDefinition>(
        r#"
        SELECT id, name, base_role, permissions, is_system
        FROM project_roles
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&role_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if previous.is_system {
        return Err(AppError::Conflict(
            "the administrator role is managed by the system".to_owned(),
        ));
    }
    sqlx::query(
        r#"
        UPDATE project_roles
        SET name = $4, base_role = $5, permissions = $6,
            updated_by_user_id = $7, updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&role_id)
    .bind(&name)
    .bind(request.base_role.as_str())
    .bind(&permissions)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await
    .map_err(role_write_error)?;
    sqlx::query(
        r#"
        UPDATE memberships
        SET permissions = $4, updated_at = now()
        WHERE tenant_id = $1 AND (project_id = $2 OR project_id IS NULL) AND role = $3
          AND NOT EXISTS (SELECT 1 FROM api_keys AS role_token
              WHERE role_token.actor_user_id = memberships.user_id
                AND role_token.tenant_id = memberships.tenant_id
                AND role_token.role_project_id IS NOT NULL AND role_token.role_project_id <> $2)
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&role_id)
    .bind(&permissions)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE api_keys
        SET permissions = $4
        WHERE tenant_id = $1 AND COALESCE(role_project_id, project_id) = $2 AND role = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&role_id)
    .bind(&access_token_permissions)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: Some(project_id),
            actor_id: actor.actor_id,
            action: "role.updated",
            resource_kind: "role",
            resource_id: None,
            metadata: serde_json::json!({
                "role_id": role_id,
                "previous_name": previous.name,
                "previous_base_role": previous.base_role,
                "previous_permissions": previous.permissions,
                "name": name,
                "base_role": request.base_role,
                "permissions": permissions,
            }),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(
        load_role_response(&state.db, actor.tenant_id, project_id, &role_id).await?,
    ))
}

async fn delete_role(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(role_id): Path<String>,
) -> Result<StatusCode, AppError> {
    actor.require_session_admin()?;
    let project_id = selected_project_id(&actor)?;
    validate_role_id(&role_id)?;
    let mut transaction = state.db.begin().await?;
    let role = sqlx::query_as::<_, ProjectRoleDefinition>(
        r#"
        SELECT id, name, base_role, permissions, is_system
        FROM project_roles
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&role_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if role.is_system {
        return Err(AppError::Conflict(
            "the administrator role cannot be deleted".to_owned(),
        ));
    }
    let member_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM memberships
        WHERE tenant_id = $1 AND (project_id = $2 OR project_id IS NULL) AND role = $3
          AND NOT EXISTS (SELECT 1 FROM api_keys AS role_token
              WHERE role_token.actor_user_id = memberships.user_id
                AND role_token.tenant_id = memberships.tenant_id
                AND role_token.role_project_id IS NOT NULL AND role_token.role_project_id <> $2)
          AND revoked_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&role_id)
    .fetch_one(&mut *transaction)
    .await?;
    let token_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM api_keys
        WHERE tenant_id = $1 AND COALESCE(role_project_id, project_id) = $2 AND role = $3
          AND revoked_at IS NULL AND expires_at > now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&role_id)
    .fetch_one(&mut *transaction)
    .await?;
    if member_count > 0 || token_count > 0 {
        return Err(AppError::Conflict(format!(
            "move {member_count} active member(s) and revoke {token_count} active token(s) before deleting this role"
        )));
    }
    sqlx::query("DELETE FROM project_roles WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(&role_id)
        .execute(&mut *transaction)
        .await?;
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: Some(project_id),
            actor_id: actor.actor_id,
            action: "role.deleted",
            resource_kind: "role",
            resource_id: None,
            metadata: serde_json::json!({ "role_id": role.id, "name": role.name }),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn load_role_response(
    db: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    role_id: &str,
) -> Result<RoleResponse, AppError> {
    let row = sqlx::query_as::<_, ProjectRoleListRow>(
        r#"
        SELECT role.id, role.name, role.base_role, role.permissions, role.is_system,
               (SELECT COUNT(*) FROM memberships AS membership
                WHERE membership.tenant_id = role.tenant_id
                  AND (membership.project_id = role.project_id OR membership.project_id IS NULL)
                  AND NOT EXISTS (SELECT 1 FROM api_keys AS role_token
                      WHERE role_token.actor_user_id = membership.user_id
                        AND role_token.tenant_id = membership.tenant_id
                        AND role_token.role_project_id IS NOT NULL
                        AND role_token.role_project_id <> role.project_id)
                  AND membership.role = role.id
                  AND membership.revoked_at IS NULL) AS member_count,
               (SELECT COUNT(*) FROM api_keys AS api_key
                WHERE api_key.tenant_id = role.tenant_id
                  AND COALESCE(api_key.role_project_id, api_key.project_id) = role.project_id
                  AND api_key.role = role.id
                  AND api_key.revoked_at IS NULL
                  AND api_key.expires_at > now()) AS active_token_count
        FROM project_roles AS role
        WHERE role.tenant_id = $1 AND role.project_id = $2 AND role.id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(role_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    role_response(row)
}

fn selected_project_id(actor: &ActorContext) -> Result<Uuid, AppError> {
    actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))
}

fn normalize_role_name(name: String) -> Result<String, AppError> {
    let name = name.trim();
    if !(1..=100).contains(&name.chars().count()) {
        return Err(AppError::BadRequest(
            "name must contain between 1 and 100 characters".to_owned(),
        ));
    }
    Ok(name.to_owned())
}

fn validate_role_id(role_id: &str) -> Result<(), AppError> {
    let valid = (1..=63).contains(&role_id.len())
        && role_id.bytes().enumerate().all(|(index, value)| {
            value.is_ascii_lowercase()
                || value.is_ascii_digit()
                || (index > 0 && matches!(value, b'_' | b'-'))
        });
    if valid {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

fn require_non_admin_base_role(role: Role) -> Result<(), AppError> {
    if role == Role::Admin {
        Err(AppError::BadRequest(
            "custom roles can only use the manager or operator access scope".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn role_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("a role with this name already exists".to_owned())
    } else {
        AppError::Database(error)
    }
}

async fn logout(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<(HeaderMap, StatusCode), AppError> {
    let session_id = actor.session_id()?;
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        "UPDATE operator_sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: actor.project_id,
            actor_id: actor.actor_id,
            action: "auth.logout",
            resource_kind: "session",
            resource_id: Some(session_id),
            metadata: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok((
        clear_session_cookie_headers(state.config.auth.cookie_secure)?,
        StatusCode::NO_CONTENT,
    ))
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum AccessTokenProjectScope {
    #[default]
    Project,
    All,
}

#[derive(Debug, FromRow)]
struct AccessTokenRow {
    project_id: Option<Uuid>,
    role_project_id: Option<Uuid>,
    id: Uuid,
    name: String,
    role: String,
    role_id: String,
    role_name: String,
    permissions: Vec<String>,
    inbox_scope: Option<Vec<Uuid>>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct AccessTokenResponse {
    project_scope: AccessTokenProjectScope,
    project_id: Option<Uuid>,
    role_project_id: Option<Uuid>,
    id: Uuid,
    name: String,
    role: Role,
    role_id: String,
    role_name: String,
    permissions: Vec<String>,
    inbox_scope: Option<Vec<Uuid>>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<AccessTokenRow> for AccessTokenResponse {
    type Error = AppError;

    fn try_from(row: AccessTokenRow) -> Result<Self, Self::Error> {
        Ok(Self {
            project_scope: if row.project_id.is_none() && row.role_project_id.is_some() {
                AccessTokenProjectScope::All
            } else {
                AccessTokenProjectScope::Project
            },
            project_id: row.project_id,
            role_project_id: row.role_project_id.or(row.project_id),
            id: row.id,
            name: row.name,
            role: Role::from_str(&row.role)?,
            role_id: row.role_id,
            role_name: row.role_name,
            permissions: if row.role_project_id.is_some() {
                access_token_permissions_from(&row.permissions)
            } else {
                row.permissions
            },
            inbox_scope: row.inbox_scope,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            last_used_at: row.last_used_at,
            created_at: row.created_at,
        })
    }
}

#[derive(Debug, Serialize)]
struct AccessTokenListResponse {
    items: Vec<AccessTokenResponse>,
    can_create_all_projects: bool,
}

async fn has_tenant_admin_access(db: &PgPool, actor: &ActorContext) -> Result<bool, AppError> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memberships WHERE tenant_id=$1 AND user_id=$2 AND project_id IS NULL AND role='admin' AND department_id IS NULL AND revoked_at IS NULL)")
        .bind(actor.tenant_id).bind(actor.actor_id).fetch_one(db).await.map_err(AppError::from)
}

async fn list_access_tokens(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<AccessTokenListResponse>, AppError> {
    actor.require_session_admin()?;
    let tenant_admin = has_tenant_admin_access(&state.db, &actor).await?;
    let _ = tenant_admin;
    let can_create_all_projects = false;
    let rows = sqlx::query_as::<_, AccessTokenRow>(
        r#"
        SELECT api_key.id, api_key.name, api_key.project_id, api_key.role_project_id,
               COALESCE(project_role.base_role, 'operator') AS role,
               api_key.role AS role_id,
               COALESCE(project_role.name, api_key.role) AS role_name,
               CASE WHEN api_key.role_project_id IS NOT NULL THEN project_role.permissions ELSE api_key.permissions END AS permissions,
               api_key.inbox_scope, api_key.expires_at,
               api_key.revoked_at, api_key.last_used_at, api_key.created_at
        FROM api_keys AS api_key
        LEFT JOIN project_roles AS project_role
          ON project_role.tenant_id = api_key.tenant_id
         AND project_role.project_id = COALESCE(api_key.role_project_id, api_key.project_id)
         AND project_role.id = api_key.role
        WHERE api_key.tenant_id = $1
          AND ($4::uuid IS NULL OR COALESCE(api_key.project_id, api_key.role_project_id) = $4)
          AND ($3 OR (api_key.project_id IS NOT NULL AND EXISTS (
              SELECT 1 FROM memberships WHERE tenant_id=api_key.tenant_id AND user_id=$2
                AND project_id=api_key.project_id AND role='admin' AND department_id IS NULL AND revoked_at IS NULL
          )))
        ORDER BY api_key.created_at DESC, api_key.id DESC
        LIMIT 500
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(tenant_admin)
    .bind(actor.deployment_project_id())
    .fetch_all(&state.db)
    .await?;
    let items = rows
        .into_iter()
        .map(AccessTokenResponse::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(AccessTokenListResponse {
        items,
        can_create_all_projects,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAccessTokenRequest {
    #[serde(default)]
    project_scope: AccessTokenProjectScope,
    project_id: Option<Uuid>,
    name: String,
    role_id: String,
    expires_in_days: u16,
    inbox_scope: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize)]
struct CreatedAccessTokenResponse {
    project_scope: AccessTokenProjectScope,
    project_id: Option<Uuid>,
    role_project_id: Uuid,
    id: Uuid,
    name: String,
    role: Role,
    role_id: String,
    role_name: String,
    permissions: Vec<String>,
    inbox_scope: Option<Vec<Uuid>>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    secret: String,
}

async fn create_access_token(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<CreateAccessTokenRequest>,
) -> Result<(HeaderMap, Json<CreatedAccessTokenResponse>), AppError> {
    actor.require_session_admin()?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::BadRequest(
            "name must contain between 1 and 100 characters".to_owned(),
        ));
    }
    if !(1..=365).contains(&request.expires_in_days) {
        return Err(AppError::BadRequest(
            "expires_in_days must be between 1 and 365".to_owned(),
        ));
    }
    let project_id = request.project_id.or(actor.project_id).ok_or_else(|| {
        AppError::BadRequest("select a project before creating an access token".to_owned())
    })?;
    crate::projects::require_project_admin_access(&state.db, &actor, project_id).await?;
    let all_projects = matches!(request.project_scope, AccessTokenProjectScope::All);
    if all_projects {
        return Err(AppError::Forbidden);
    }
    if all_projects && request.inbox_scope.is_some() {
        return Err(AppError::BadRequest(
            "all-project tokens cannot be restricted to individual Inboxes".to_owned(),
        ));
    }
    let bound_project_id = (!all_projects).then_some(project_id);
    let id = Uuid::now_v7();
    let secret = format!("tzm_{}", generate_token());
    let now = Utc::now();
    let expires_at = now + ChronoDuration::days(i64::from(request.expires_in_days));
    let mut transaction = state.db.begin().await?;
    let role = lock_project_role(
        &mut transaction,
        actor.tenant_id,
        project_id,
        &request.role_id,
    )
    .await?;
    let base_role = role.base_role()?;
    if base_role == Role::Admin {
        return Err(AppError::BadRequest(
            "the administrator role cannot be assigned to an access token".to_owned(),
        ));
    }
    let inbox_scope = validate_access_token_inbox_scope(
        &state.db,
        actor.tenant_id,
        project_id,
        request.inbox_scope,
    )
    .await?;
    let permissions = access_token_permissions_from(&role.permissions);
    let technical_user_id = Uuid::now_v7();
    let technical_user_email = format!("access-token-{technical_user_id}@internal.workspace");
    sqlx::query(
        r#"
        INSERT INTO users (id, email, display_name, status)
        VALUES ($1, $2, $3, 'active')
        "#,
    )
    .bind(technical_user_id)
    .bind(technical_user_email)
    .bind(name)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role, permissions)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(bound_project_id)
    .bind(technical_user_id)
    .bind(&role.id)
    .bind(&role.permissions)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO api_keys (
            id, tenant_id, project_id, actor_user_id, created_by_user_id,
            name, token_hash, role, permissions, inbox_scope, expires_at, role_project_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(id)
    .bind(actor.tenant_id)
    .bind(bound_project_id)
    .bind(technical_user_id)
    .bind(actor.actor_id)
    .bind(name)
    .bind(hash_token(&secret))
    .bind(&role.id)
    .bind(&permissions)
    .bind(&inbox_scope)
    .bind(expires_at)
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: actor.project_id,
            actor_id: actor.actor_id,
            action: "access_token.created",
            resource_kind: "access_token",
            resource_id: Some(id),
            metadata: serde_json::json!({
                "role_id": role.id,
                "role_name": role.name,
                "base_role": base_role,
                "expires_at": expires_at,
                "inbox_scope": inbox_scope.clone(),
                "project_scope": request.project_scope,
                "project_id": bound_project_id,
                "role_project_id": project_id,
            }),
        },
    )
    .await?;
    transaction.commit().await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        headers,
        Json(CreatedAccessTokenResponse {
            project_scope: request.project_scope,
            project_id: bound_project_id,
            role_project_id: project_id,
            id,
            name: name.to_owned(),
            role: base_role,
            role_id: role.id,
            role_name: role.name,
            permissions,
            inbox_scope,
            expires_at,
            created_at: now,
            secret,
        }),
    ))
}

async fn validate_access_token_inbox_scope(
    db: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_scope: Option<Vec<Uuid>>,
) -> Result<Option<Vec<Uuid>>, AppError> {
    let Some(mut inbox_scope) = inbox_scope else {
        return Ok(None);
    };
    inbox_scope.sort_unstable();
    inbox_scope.dedup();
    if inbox_scope.is_empty() || inbox_scope.len() > 100 {
        return Err(AppError::BadRequest(
            "inbox_scope must contain between 1 and 100 unique Inbox IDs".to_owned(),
        ));
    }
    let inboxes = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT id, project_id
        FROM inboxes
        WHERE tenant_id = $1 AND id = ANY($2)
          AND project_id = $3 AND status = 'active'
        "#,
    )
    .bind(tenant_id)
    .bind(&inbox_scope)
    .bind(project_id)
    .fetch_all(db)
    .await?;
    if inboxes.len() != inbox_scope.len() {
        return Err(AppError::Forbidden);
    }
    Ok(Some(inbox_scope))
}

async fn revoke_access_token(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(token_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    actor.require_session_admin()?;
    let tenant_admin = has_tenant_admin_access(&state.db, &actor).await?;
    let mut transaction = state.db.begin().await?;
    let technical_user_id = sqlx::query_scalar::<_, Option<Uuid>>(
        r#"
        SELECT actor_user_id
        FROM api_keys
        WHERE id = $1
          AND tenant_id = $2
          AND ($5::uuid IS NULL OR COALESCE(project_id, role_project_id) = $5)
          AND ($4 OR (project_id IS NOT NULL AND EXISTS (
              SELECT 1 FROM memberships AS admin_grant WHERE admin_grant.tenant_id=api_keys.tenant_id
                AND admin_grant.user_id=$3 AND admin_grant.project_id=api_keys.project_id AND admin_grant.role='admin'
                AND admin_grant.department_id IS NULL AND admin_grant.revoked_at IS NULL
          )))
        FOR UPDATE
        "#,
    )
    .bind(token_id)
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(tenant_admin)
    .bind(actor.deployment_project_id())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let updated = sqlx::query(
        r#"
        UPDATE api_keys
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE id = $1
          AND tenant_id = $2
          AND ($5::uuid IS NULL OR COALESCE(project_id, role_project_id) = $5)
          AND ($4 OR (project_id IS NOT NULL AND EXISTS (
              SELECT 1 FROM memberships AS admin_grant WHERE admin_grant.tenant_id=api_keys.tenant_id
                AND admin_grant.user_id=$3 AND admin_grant.project_id=api_keys.project_id AND admin_grant.role='admin'
                AND admin_grant.department_id IS NULL AND admin_grant.revoked_at IS NULL
          )))
        "#,
    )
    .bind(token_id)
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(tenant_admin)
    .bind(actor.deployment_project_id())
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    if let Some(technical_user_id) = technical_user_id {
        cleanup_access_token_user(&mut transaction, technical_user_id, false).await?;
    }
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: actor.project_id,
            actor_id: actor.actor_id,
            action: "access_token.revoked",
            resource_kind: "access_token",
            resource_id: Some(token_id),
            metadata: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_access_token(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(token_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    actor.require_session_admin()?;
    let tenant_admin = has_tenant_admin_access(&state.db, &actor).await?;
    let mut transaction = state.db.begin().await?;
    let deleted = sqlx::query_as::<_, (String, String, Option<DateTime<Utc>>, Option<Uuid>)>(
        r#"
        DELETE FROM api_keys
        WHERE id = $1
          AND tenant_id = $2
          AND ($5::uuid IS NULL OR COALESCE(project_id, role_project_id) = $5)
          AND ($4 OR (project_id IS NOT NULL AND EXISTS (
              SELECT 1 FROM memberships AS admin_grant WHERE admin_grant.tenant_id=api_keys.tenant_id
                AND admin_grant.user_id=$3 AND admin_grant.project_id=api_keys.project_id AND admin_grant.role='admin'
                AND admin_grant.department_id IS NULL AND admin_grant.revoked_at IS NULL
          )))
        RETURNING name, role, revoked_at, actor_user_id
        "#,
    )
    .bind(token_id)
    .bind(actor.tenant_id)
    .bind(actor.actor_id)
    .bind(tenant_admin)
    .bind(actor.deployment_project_id())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if let Some(technical_user_id) = deleted.3 {
        cleanup_access_token_user(&mut transaction, technical_user_id, true).await?;
    }
    insert_audit(
        &mut transaction,
        AuditEvent {
            tenant_id: actor.tenant_id,
            project_id: actor.project_id,
            actor_id: actor.actor_id,
            action: "access_token.deleted",
            resource_kind: "access_token",
            resource_id: Some(token_id),
            metadata: serde_json::json!({
                "name": deleted.0,
                "role_id": deleted.1,
                "was_revoked": deleted.2.is_some(),
            }),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn cleanup_access_token_user(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    delete_if_unused: bool,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM team_members WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM operator_sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM memberships WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM operator_profile_avatars WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("DELETE FROM operator_chat_profiles WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;

    let has_references = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM api_keys WHERE actor_user_id = $1 OR created_by_user_id = $1
            UNION ALL SELECT 1 FROM conversation_assignments WHERE user_id = $1 OR assigned_by = $1
            UNION ALL SELECT 1 FROM conversation_participants WHERE user_id = $1
            UNION ALL SELECT 1 FROM messages WHERE author_id = $1
            UNION ALL SELECT 1 FROM ai_provider_connections WHERE created_by = $1
            UNION ALL SELECT 1 FROM ai_profiles WHERE created_by = $1
            UNION ALL SELECT 1 FROM ai_tasks WHERE created_by = $1
            UNION ALL SELECT 1 FROM ai_api_runs WHERE actor_id = $1
            UNION ALL SELECT 1 FROM ai_api_settings WHERE key_created_by = $1
            UNION ALL SELECT 1 FROM knowledge_bases WHERE created_by = $1
            UNION ALL SELECT 1 FROM knowledge_articles WHERE created_by = $1 OR updated_by = $1
            UNION ALL SELECT 1 FROM project_roles WHERE updated_by_user_id = $1
            UNION ALL SELECT 1 FROM operator_profile_avatars WHERE created_by = $1
            UNION ALL SELECT 1 FROM ai_profile_avatars WHERE created_by = $1
            UNION ALL SELECT 1 FROM ai_profile_public_identity_avatars WHERE created_by = $1
            UNION ALL SELECT 1 FROM project_logos WHERE created_by = $1
        )
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await?;
    if delete_if_unused && !has_references {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&mut **transaction)
            .await?;
    } else {
        sqlx::query("UPDATE users SET status = 'disabled', updated_at = now() WHERE id = $1")
            .bind(user_id)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
fn password_session_inbox_scope(project_id: Option<Uuid>) -> Option<Vec<Uuid>> {
    project_id.map_or_else(|| Some(Vec::new()), |_| None)
}

struct AuditEvent<'a> {
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    actor_id: Uuid,
    action: &'a str,
    resource_kind: &'a str,
    resource_id: Option<Uuid>,
    metadata: serde_json::Value,
}

async fn insert_audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event: AuditEvent<'_>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(event.tenant_id)
    .bind(event.project_id)
    .bind(event.actor_id)
    .bind(event.action)
    .bind(event.resource_kind)
    .bind(event.resource_id)
    .bind(event.metadata)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_csrf(parts: &Parts, expected_hash: &[u8]) -> Result<(), AppError> {
    if parts.method == Method::GET
        || parts.method == Method::HEAD
        || parts.method == Method::OPTIONS
    {
        return Ok(());
    }
    let value = parts
        .headers
        .get(CSRF_HEADER)
        .ok_or(AppError::Forbidden)?
        .to_str()
        .map_err(|_| AppError::Forbidden)?;
    let csrf_cookie = cookie_value(parts, csrf_cookie_name(false))
        .or_else(|| cookie_value(parts, csrf_cookie_name(true)))
        .ok_or(AppError::Forbidden)?;
    if value == csrf_cookie && hash_token(value) == expected_hash {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn optional_bearer_token(parts: &Parts) -> Result<Option<&str>, AppError> {
    let Some(value) = parts.headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| AppError::Unauthorized)?;
    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .map(Some)
        .ok_or(AppError::Unauthorized)
}

fn required_bearer_token(parts: &Parts) -> Result<&str, AppError> {
    optional_bearer_token(parts)?.ok_or(AppError::Unauthorized)
}

fn cookie_value<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
    parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|item| item.trim().split_once('='))
        .find_map(|(cookie_name, value)| (cookie_name == name).then_some(value))
        .filter(|value| !value.is_empty())
}

fn session_cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_SESSION_COOKIE
    } else {
        LOCAL_SESSION_COOKIE
    }
}

fn csrf_cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_CSRF_COOKIE
    } else {
        LOCAL_CSRF_COOKIE
    }
}

fn session_cookie_headers(
    secure: bool,
    session_token: &str,
    csrf_token: &str,
    max_age: u64,
) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    append_cookie(
        &mut headers,
        session_cookie_name(secure),
        session_token,
        max_age,
        true,
        secure,
    )?;
    append_cookie(
        &mut headers,
        csrf_cookie_name(secure),
        csrf_token,
        max_age,
        false,
        secure,
    )?;
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(headers)
}

fn clear_session_cookie_headers(secure: bool) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    append_cookie(
        &mut headers,
        session_cookie_name(secure),
        "",
        0,
        true,
        secure,
    )?;
    append_cookie(&mut headers, csrf_cookie_name(secure), "", 0, false, secure)?;
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(headers)
}

fn append_cookie(
    headers: &mut HeaderMap,
    name: &str,
    value: &str,
    max_age: u64,
    http_only: bool,
    secure: bool,
) -> Result<(), AppError> {
    let mut cookie = format!("{name}={value}; Path=/; Max-Age={max_age}; SameSite=Strict");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(AppError::internal)?,
    );
    Ok(())
}

fn chrono_duration(seconds: u64) -> Result<ChronoDuration, AppError> {
    let seconds = i64::try_from(seconds).map_err(AppError::internal)?;
    Ok(ChronoDuration::seconds(seconds))
}

pub(crate) fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub(crate) fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Header name used by the browser session CSRF contract.
pub fn csrf_header_name() -> HeaderName {
    HeaderName::from_static(CSRF_HEADER)
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, Request};
    use uuid::Uuid;

    use super::{
        ActorContext, AuthSource, DUMMY_PASSWORD_HASH, DepartmentContext, LoginRequest,
        OPERATOR_PERMISSIONS, Role, access_token_permissions_from, cookie_value, generate_token,
        hash_token, normalize_role_name, normalize_role_permissions, optional_bearer_token,
        password_session_inbox_scope, require_non_admin_base_role, session_cookie_headers,
        validate_csrf, verify_password,
    };

    #[test]
    fn legacy_sessions_without_departments_use_the_project_as_the_inbox_boundary() {
        assert_eq!(password_session_inbox_scope(Some(Uuid::now_v7())), None);
        assert_eq!(password_session_inbox_scope(None), Some(Vec::new()));
    }

    #[test]
    fn remembered_password_sessions_require_an_explicit_boolean_opt_in() {
        for (remember_me, expected) in [(None, false), (Some(false), false), (Some(true), true)] {
            let mut input =
                serde_json::json!({"email": "operator@example.test", "password": "test"});
            if let Some(value) = remember_me {
                input["remember_me"] = value.into();
            }
            let request: LoginRequest = serde_json::from_value(input).unwrap();
            assert_eq!(request.remember_me, expected);
        }
        let invalid = serde_json::json!({
            "email": "operator@example.test", "password": "test", "remember_me": "true"
        });
        assert!(serde_json::from_value::<LoginRequest>(invalid).is_err());
    }

    #[test]
    fn roles_have_safe_default_permission_presets() {
        assert_eq!(
            Role::Operator.permissions().len(),
            OPERATOR_PERMISSIONS.len()
        );
        assert!(
            Role::Admin
                .permissions()
                .contains(&"access_tokens:manage".to_owned())
        );
        assert!(
            Role::Admin
                .permissions()
                .contains(&"projects:manage".to_owned())
        );
        assert!(
            !access_token_permissions_from(&Role::Admin.permissions())
                .contains(&"access_tokens:manage".to_owned())
        );
        assert!(
            Role::Manager
                .permissions()
                .contains(&"visitor_network:read".to_owned())
        );
        assert!(
            Role::Manager
                .permissions()
                .contains(&"quality:read_all".to_owned())
        );
        assert!(
            access_token_permissions_from(&Role::Admin.permissions())
                .contains(&"visitor_network:read".to_owned())
        );
        assert!(
            !Role::Operator
                .permissions()
                .contains(&"visitor_network:read".to_owned())
        );
        assert!(
            !Role::Operator
                .permissions()
                .contains(&"quality:read_all".to_owned())
        );
    }

    #[test]
    fn reply_template_management_is_granular_and_available_to_admins_and_managers() {
        for role in [Role::Admin, Role::Manager] {
            assert!(
                role.permissions()
                    .contains(&"reply_templates:manage".to_owned())
            );
        }
        assert!(
            !Role::Operator
                .permissions()
                .contains(&"reply_templates:manage".to_owned())
        );
        let custom = normalize_role_permissions(vec!["reply_templates:manage".to_owned()]).unwrap();
        assert_eq!(access_token_permissions_from(&custom), custom);
    }

    #[test]
    fn contact_management_is_available_to_standard_roles_and_requires_read_access() {
        for role in [Role::Admin, Role::Manager, Role::Operator] {
            assert!(role.permissions().contains(&"contacts:manage".to_owned()));
        }
        assert!(normalize_role_permissions(vec!["contacts:manage".to_owned()]).is_err());
        assert!(
            normalize_role_permissions(vec![
                "contacts:read".to_owned(),
                "contacts:manage".to_owned(),
            ])
            .is_ok()
        );
    }

    #[test]
    fn editable_roles_reject_unknown_duplicate_and_incomplete_permissions() {
        assert!(
            normalize_role_permissions(vec![
                "conversations:read".to_owned(),
                "unknown:read".to_owned()
            ],)
            .is_err()
        );
        assert!(
            normalize_role_permissions(vec![
                "contacts:read".to_owned(),
                "contacts:read".to_owned()
            ],)
            .is_err()
        );
        assert!(normalize_role_permissions(vec!["conversations:reply".to_owned()]).is_err());
        assert!(normalize_role_permissions(vec!["channels:manage".to_owned()]).is_err());
        assert!(normalize_role_permissions(vec!["quality:read_all".to_owned()]).is_err());
    }

    #[test]
    fn note_writes_require_read_access_and_standard_roles_can_collaborate() {
        assert!(normalize_role_permissions(vec!["notes:write".to_owned()]).is_err());
        assert!(
            normalize_role_permissions(vec!["notes:read".to_owned(), "notes:write".to_owned()])
                .is_ok()
        );
        for role in [Role::Admin, Role::Manager, Role::Operator] {
            assert!(role.permissions().contains(&"notes:read".to_owned()));
            assert!(role.permissions().contains(&"notes:write".to_owned()));
        }
    }

    #[test]
    fn process_actions_require_explicit_read_permission() {
        for permission in ["processes:edit", "processes:approve"] {
            assert!(normalize_role_permissions(vec![permission.to_owned()]).is_err());
            let permissions = normalize_role_permissions(vec![
                permission.to_owned(),
                "processes:read".to_owned(),
            ])
            .expect("process action with read access");
            assert_eq!(permissions.len(), 2);
        }
        assert!(
            Role::Operator
                .permissions()
                .contains(&"processes:read".to_owned())
        );
        assert!(
            !Role::Operator
                .permissions()
                .contains(&"processes:approve".to_owned())
        );
    }

    #[test]
    fn editable_roles_are_ordered_and_tokens_preserve_visitor_network_permission() {
        let permissions = normalize_role_permissions(vec![
            "ai:manage".to_owned(),
            "visitor_network:read".to_owned(),
            "conversations:read".to_owned(),
            "projects:read".to_owned(),
            "quality:read_all".to_owned(),
            "quality:read".to_owned(),
        ])
        .expect("valid role permissions");
        assert_eq!(
            permissions,
            vec![
                "projects:read",
                "conversations:read",
                "visitor_network:read",
                "quality:read",
                "quality:read_all",
                "ai:manage"
            ]
        );
        assert_eq!(
            access_token_permissions_from(&permissions),
            vec![
                "projects:read",
                "conversations:read",
                "visitor_network:read",
                "quality:read",
                "quality:read_all",
                "ai:manage"
            ]
        );
    }

    #[test]
    fn custom_roles_require_a_name_and_non_admin_scope() {
        assert_eq!(
            normalize_role_name("  Senior support  ".to_owned()).unwrap(),
            "Senior support"
        );
        assert!(normalize_role_name(" ".to_owned()).is_err());
        assert!(require_non_admin_base_role(Role::Admin).is_err());
        assert!(require_non_admin_base_role(Role::Manager).is_ok());
        assert!(require_non_admin_base_role(Role::Operator).is_ok());
    }

    #[test]
    fn project_email_settings_require_unrestricted_project_credentials() {
        let project_id = Uuid::now_v7();
        let mut actor = ActorContext {
            actor_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Some(project_id),
            role: Role::Manager,
            role_id: "manager".to_owned(),
            role_name: "Manager".to_owned(),
            email: None,
            display_name: None,
            chat_display_name: None,
            avatar_url: None,
            permissions: Role::Manager.permissions(),
            is_demo: false,
            demo_login_ip: None,
            demo_read_only_request: false,
            access_all_projects: false,
            deployment_project_id: None,
            inbox_scope: None,
            department: DepartmentContext::default(),
            source: AuthSource::Session {
                session_id: Uuid::now_v7(),
                membership_id: Uuid::nil(),
            },
        };
        assert_eq!(
            crate::email::require_email_management(&actor).unwrap(),
            project_id
        );
        actor.inbox_scope = Some(vec![Uuid::now_v7()]);
        assert!(crate::email::require_email_management(&actor).is_err());
        actor.source = AuthSource::AccessToken {
            api_key_id: Uuid::now_v7(),
        };
        assert!(crate::email::require_email_management(&actor).is_err());
        actor.inbox_scope = None;
        assert_eq!(
            crate::email::require_email_management(&actor).unwrap(),
            project_id
        );
        actor.source = AuthSource::Session {
            session_id: Uuid::now_v7(),
            membership_id: Uuid::nil(),
        };
        actor.permissions.clear();
        assert!(crate::email::require_email_management(&actor).is_err());
        actor.permissions = Role::Manager.permissions();
        actor.project_id = None;
        assert!(crate::email::require_email_management(&actor).is_err());
    }

    #[test]
    fn project_scope_denies_cross_project_resources() {
        let allowed_project = Uuid::now_v7();
        let actor = ActorContext {
            actor_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Some(allowed_project),
            role: Role::Operator,
            role_id: "operator".to_owned(),
            role_name: "Operator".to_owned(),
            email: None,
            display_name: None,
            chat_display_name: None,
            avatar_url: None,
            permissions: Role::Operator.permissions(),
            is_demo: false,
            demo_login_ip: None,
            demo_read_only_request: false,
            access_all_projects: false,
            deployment_project_id: None,
            inbox_scope: None,
            department: DepartmentContext::default(),
            source: AuthSource::AccessToken {
                api_key_id: Uuid::now_v7(),
            },
        };

        assert!(actor.require_project(allowed_project).is_ok());
        assert!(actor.require_project(Uuid::now_v7()).is_err());
    }

    #[test]
    fn legacy_session_without_departments_allows_every_inbox_in_the_selected_project() {
        let allowed_project = Uuid::now_v7();
        let actor = ActorContext {
            actor_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Some(allowed_project),
            role: Role::Operator,
            role_id: "operator".to_owned(),
            role_name: "Operator".to_owned(),
            email: Some("operator@example.com".to_owned()),
            display_name: Some("Operator".to_owned()),
            chat_display_name: Some("Operator".to_owned()),
            avatar_url: None,
            permissions: Role::Operator.permissions(),
            is_demo: false,
            demo_login_ip: None,
            demo_read_only_request: false,
            access_all_projects: false,
            deployment_project_id: None,
            inbox_scope: None,
            department: DepartmentContext::default(),
            source: AuthSource::Session {
                session_id: Uuid::now_v7(),
                membership_id: Uuid::nil(),
            },
        };

        assert!(actor.require_inbox(allowed_project, Uuid::now_v7()).is_ok());
        assert!(actor.require_inbox(allowed_project, Uuid::now_v7()).is_ok());
        assert!(actor.require_inbox(Uuid::now_v7(), Uuid::now_v7()).is_err());
    }

    #[test]
    fn access_token_preserves_its_explicit_inbox_scope() {
        let allowed_project = Uuid::now_v7();
        let allowed_inbox = Uuid::now_v7();
        let actor = ActorContext {
            actor_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Some(allowed_project),
            role: Role::Operator,
            role_id: "operator".to_owned(),
            role_name: "Operator".to_owned(),
            email: None,
            display_name: None,
            chat_display_name: None,
            avatar_url: None,
            permissions: Role::Operator.permissions(),
            is_demo: false,
            demo_login_ip: None,
            demo_read_only_request: false,
            access_all_projects: false,
            deployment_project_id: None,
            inbox_scope: Some(vec![allowed_inbox]),
            department: DepartmentContext::default(),
            source: AuthSource::AccessToken {
                api_key_id: Uuid::now_v7(),
            },
        };

        assert!(actor.require_inbox(allowed_project, allowed_inbox).is_ok());
        assert!(
            actor
                .require_inbox(allowed_project, Uuid::now_v7())
                .is_err()
        );
    }

    #[test]
    fn access_tokens_cannot_administer_roles_or_other_tokens() {
        let actor = ActorContext {
            actor_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Some(Uuid::now_v7()),
            role: Role::Admin,
            role_id: "admin".to_owned(),
            role_name: "Administrator".to_owned(),
            email: None,
            display_name: None,
            chat_display_name: None,
            avatar_url: None,
            permissions: Role::Admin.permissions(),
            is_demo: false,
            demo_login_ip: None,
            demo_read_only_request: false,
            access_all_projects: false,
            deployment_project_id: None,
            inbox_scope: None,
            department: DepartmentContext::default(),
            source: AuthSource::AccessToken {
                api_key_id: Uuid::now_v7(),
            },
        };

        assert!(actor.require_session_admin().is_err());
        assert!(actor.require_password_session().is_err());
    }

    #[test]
    fn department_assignment_cannot_administer_even_with_a_malformed_admin_role() {
        let mut actor = ActorContext {
            actor_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Some(Uuid::now_v7()),
            role: Role::Admin,
            role_id: "admin".to_owned(),
            role_name: "Admin".to_owned(),
            email: None,
            display_name: None,
            chat_display_name: None,
            avatar_url: None,
            permissions: Role::Admin.permissions(),
            is_demo: false,
            demo_login_ip: None,
            demo_read_only_request: false,
            access_all_projects: false,
            deployment_project_id: None,
            inbox_scope: Some(vec![Uuid::now_v7()]),
            department: DepartmentContext {
                id: Some(Uuid::now_v7()),
                restricted: true,
                ..DepartmentContext::default()
            },
            source: AuthSource::Session {
                session_id: Uuid::now_v7(),
                membership_id: Uuid::nil(),
            },
        };
        assert!(actor.require_session_admin().is_err());
        assert!(actor.has_restricted_inbox_scope());
        actor.department.restricted = false;
        assert!(actor.require_session_admin().is_ok());
        assert!(!actor.has_restricted_inbox_scope());
        actor.source = AuthSource::AccessToken {
            api_key_id: Uuid::now_v7(),
        };
        assert!(actor.has_restricted_inbox_scope());
    }

    #[test]
    fn hashes_tokens_deterministically_without_storing_plaintext() {
        assert_eq!(hash_token("secret"), hash_token("secret"));
        assert_ne!(hash_token("secret"), b"secret");
    }

    #[test]
    fn generates_distinct_opaque_tokens() {
        assert_ne!(generate_token(), generate_token());
    }

    #[test]
    fn bearer_auth_does_not_fall_back_from_an_invalid_header() {
        let request = Request::builder()
            .header("authorization", "Basic nope")
            .body(())
            .unwrap();
        let (parts, ()) = request.into_parts();
        assert!(optional_bearer_token(&parts).is_err());
    }

    #[test]
    fn parses_only_the_named_cookie() {
        let request = Request::builder()
            .header("cookie", "other=value; tzomet_session=session-token")
            .body(())
            .unwrap();
        let (parts, ()) = request.into_parts();
        assert_eq!(
            cookie_value(&parts, "tzomet_session"),
            Some("session-token")
        );
    }

    #[test]
    fn csrf_is_required_only_for_unsafe_session_requests() {
        let get_request = Request::builder().method(Method::GET).body(()).unwrap();
        let (get_parts, ()) = get_request.into_parts();
        assert!(validate_csrf(&get_parts, &hash_token("csrf")).is_ok());

        let post_request = Request::builder()
            .method(Method::POST)
            .header("x-csrf-token", "csrf")
            .header("cookie", "tz_csrf=csrf")
            .body(())
            .unwrap();
        let (post_parts, ()) = post_request.into_parts();
        assert!(validate_csrf(&post_parts, &hash_token("csrf")).is_ok());

        let missing_request = Request::builder().method(Method::POST).body(()).unwrap();
        let (missing_parts, ()) = missing_request.into_parts();
        assert!(validate_csrf(&missing_parts, &hash_token("csrf")).is_err());
    }

    #[tokio::test]
    async fn verifies_argon2id_passwords_without_accepting_the_dummy_identity() {
        assert!(
            verify_password(
                "tzomet-local-admin".to_owned(),
                Some(DUMMY_PASSWORD_HASH.to_owned()),
            )
            .await
            .unwrap()
        );
        assert!(
            !verify_password(
                "wrong-password".to_owned(),
                Some(DUMMY_PASSWORD_HASH.to_owned())
            )
            .await
            .unwrap()
        );
        assert!(
            !verify_password("tzomet-local-admin".to_owned(), None)
                .await
                .unwrap()
        );
    }

    #[test]
    fn session_cookie_flags_keep_the_session_secret_http_only() {
        let headers = session_cookie_headers(true, "session", "csrf", 600).unwrap();
        let cookies = headers
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(cookies.len(), 2);
        assert!(cookies[0].starts_with("__Host-tz_session="));
        assert!(cookies[0].contains("HttpOnly"));
        assert!(cookies[0].contains("SameSite=Strict"));
        assert!(cookies[0].contains("Secure"));
        assert!(cookies[1].starts_with("__Host-tz_csrf="));
        assert!(!cookies[1].contains("HttpOnly"));
    }

    #[test]
    fn a_foreign_csrf_cookie_name_is_rejected() {
        for (name, accepted) in [("tz_csrf", true), ("tzomet_csrf", false)] {
            let (parts, ()) = axum::http::Request::builder()
                .method("POST")
                .header("cookie", format!("{name}=csrf"))
                .header("x-csrf-token", "csrf")
                .body(())
                .unwrap()
                .into_parts();
            assert_eq!(
                super::validate_csrf(&parts, &hash_token("csrf")).is_ok(),
                accepted
            );
        }
        let cleared = super::clear_session_cookie_headers(true).unwrap();
        assert!(cleared.get_all("set-cookie").iter().all(|value| {
            let value = value.to_str().unwrap();
            value.starts_with("__Host-tz_") && value.contains("Max-Age=0")
        }));
    }
}
