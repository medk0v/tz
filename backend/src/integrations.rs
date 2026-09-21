//! API integrations with workspace visibility and explicit AI-profile grants.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Duration,
};

use anyhow::Context as _;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{StatusCode, header},
    routing::{get, put},
};
use chrono::{DateTime, Utc};
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction};
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    ai_settings::{decrypt_secret, encrypt_secret, load_secret_encryption_key},
    auth::ActorContext,
    error::AppError,
    provider_reply,
    resource_visibility::{self, Resource, ResourceVisibility},
};

const MAX_INTEGRATION_BODY_LENGTH: usize = 512 * 1_024;
const MAX_ACTIONS: usize = 32;
const MAX_PARAMETER_NAMES: usize = 32;
const MAX_RUNTIME_PARAMETER_BYTES: usize = 512;
const MAX_RUNTIME_RESPONSE_BYTES: usize = 256 * 1_024;
const MAX_RUNTIME_TARGET_URL_BYTES: usize = 16 * 1_024;
const MAX_RUNTIME_CATALOG_ACTIONS: usize = 128;
const MAX_RUNTIME_CATALOG_JSON_BYTES: usize = 128 * 1_024;
const MAX_RUNTIME_INTEGRATIONS: usize = 32;
const INTEGRATION_TOKEN_AAD_PREFIX: &str = "tzomet:api-integration-token:v1";

type StoredTokenMetadata = (Vec<u8>, Vec<u8>, String);

/// Routes for API integration administration within the selected workspace.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/integrations/apis",
            get(list_integrations)
                .post(create_integration)
                .layer(DefaultBodyLimit::max(MAX_INTEGRATION_BODY_LENGTH)),
        )
        .route(
            "/api/v1/integrations/apis/{integration_id}",
            put(update_integration)
                .delete(delete_integration)
                .layer(DefaultBodyLimit::max(MAX_INTEGRATION_BODY_LENGTH)),
        )
}

#[derive(Debug, Serialize)]
struct ApiIntegrationListResponse {
    items: Vec<ApiIntegrationResponse>,
    ai_profiles: Vec<ApiIntegrationAiProfile>,
}

#[derive(Debug, Serialize)]
struct ApiIntegrationResponse {
    visibility: ResourceVisibility,
    id: Uuid,
    name: String,
    key: String,
    description: String,
    base_url: String,
    status: String,
    auth_kind: String,
    token_configured: bool,
    actions: Vec<ApiIntegrationActionResponse>,
    ai_profile_ids: Vec<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ApiIntegrationActionResponse {
    id: Uuid,
    key: String,
    name: String,
    description: String,
    path_template: String,
    parameter_names: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ApiIntegrationAiProfile {
    id: Uuid,
    name: String,
    status: String,
    runtime_available: bool,
}

#[derive(Debug, FromRow)]
struct ApiIntegrationAiProfileRow {
    id: Uuid,
    name: String,
    status: String,
    provider_status: Option<String>,
    provider_base_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiIntegrationInput {
    visibility: Option<ResourceVisibility>,
    name: String,
    key: String,
    description: String,
    base_url: String,
    status: String,
    auth_kind: String,
    token: Option<String>,
    clear_token: bool,
    actions: Vec<ApiIntegrationActionInput>,
    ai_profile_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiIntegrationActionInput {
    key: String,
    name: String,
    description: String,
    path_template: String,
}

struct NormalizedIntegration {
    visibility: Option<ResourceVisibility>,
    name: String,
    key: String,
    description: String,
    base_url: String,
    status: String,
    auth_kind: String,
    token: Option<String>,
    clear_token: bool,
    actions: Vec<NormalizedAction>,
    ai_profile_ids: Vec<Uuid>,
}

struct NormalizedAction {
    key: String,
    name: String,
    description: String,
    path_template: String,
    parameter_names: Vec<String>,
}

#[derive(Debug, FromRow)]
struct IntegrationRow {
    visibility: sqlx::types::Json<ResourceVisibility>,
    id: Uuid,
    name: String,
    key: String,
    description: String,
    base_url: String,
    status: String,
    auth_kind: String,
    token_configured: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct ActionRow {
    id: Uuid,
    integration_id: Uuid,
    key: String,
    name: String,
    description: String,
    path_template: String,
    parameter_names: Vec<String>,
}

#[derive(Debug, FromRow)]
struct ExistingSecretRow {
    integration_key: String,
    encrypted_token: Option<Vec<u8>>,
    token_nonce: Option<Vec<u8>>,
    key_version: Option<String>,
}

/// Non-secret API integration metadata made available to an AI runtime.
#[derive(Debug, Serialize)]
pub(crate) struct RuntimeIntegrationCatalog {
    pub key: String,
    pub name: String,
    pub description: String,
    pub actions: Vec<RuntimeIntegrationActionCatalog>,
}

/// Non-secret action metadata made available to an AI runtime.
#[derive(Debug, Serialize)]
pub(crate) struct RuntimeIntegrationActionCatalog {
    pub key: String,
    pub name: String,
    pub description: String,
    pub parameter_names: Vec<String>,
}

/// Fully scoped internal request to one granted API action.
pub(crate) struct RuntimeActionRequest {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub profile_id: Uuid,
    pub source_id: Uuid,
    pub integration_key: String,
    pub action_key: String,
    pub parameters: BTreeMap<String, String>,
}

/// Bounded upstream response returned to the provider broker.
pub(crate) struct RuntimeActionResponse {
    pub status_code: u16,
    pub content_type: String,
    pub body: String,
}

#[derive(Debug, FromRow)]
struct RuntimeCatalogRow {
    integration_id: Uuid,
    integration_key: String,
    integration_name: String,
    integration_description: String,
    action_key: String,
    action_name: String,
    action_description: String,
    parameter_names: Vec<String>,
}

#[derive(Debug, FromRow)]
struct ProfileCatalogRow {
    profile_id: Uuid,
    integration_id: Uuid,
    integration_key: String,
    integration_name: String,
    integration_description: String,
    action_key: String,
    action_name: String,
    action_description: String,
    parameter_names: Vec<String>,
}

#[derive(Debug, FromRow)]
struct RuntimeActionRow {
    owner_project_id: Uuid,
    integration_id: Uuid,
    action_id: Uuid,
    base_url: String,
    auth_kind: String,
    encrypted_token: Option<Vec<u8>>,
    token_nonce: Option<Vec<u8>>,
    key_version: Option<String>,
    path_template: String,
    parameter_names: Vec<String>,
    provider_base_url: String,
}

async fn list_integrations(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<ApiIntegrationListResponse>, AppError> {
    require_read_management(&actor)?;
    let items = load_integrations(&state, &actor, None).await?;
    let ai_profiles = load_ai_profile_options(&state, &actor).await?;
    Ok(Json(ApiIntegrationListResponse { items, ai_profiles }))
}

async fn create_integration(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<ApiIntegrationInput>,
) -> Result<(StatusCode, Json<ApiIntegrationResponse>), AppError> {
    let project_id = require_write_management(&actor)?;
    let input = normalize_integration(request)?;
    if !input.ai_profile_ids.is_empty() {
        actor.require("ai:manage")?;
    }
    if input.auth_kind != "none" && input.token.is_none() {
        return Err(AppError::BadRequest(
            "token is required for the selected authentication kind".to_owned(),
        ));
    }

    let integration_id = Uuid::now_v7();
    let encrypted = encrypt_integration_token(
        &state,
        actor.tenant_id,
        project_id,
        integration_id,
        input.token.as_deref(),
    )?;
    let mut transaction = state.db.begin().await?;
    validate_ai_profiles(&mut transaction, &actor, &input.ai_profile_ids, &[]).await?;
    validate_profile_assignment_capacity(
        &mut transaction,
        actor.tenant_id,
        integration_id,
        &input.ai_profile_ids,
    )
    .await?;
    sqlx::query(
        r#"
        INSERT INTO api_integrations (
            id, tenant_id, project_id, name, integration_key, description,
            base_url, status, auth_kind, encrypted_token, token_nonce,
            key_version, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
    )
    .bind(integration_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&input.name)
    .bind(&input.key)
    .bind(&input.description)
    .bind(&input.base_url)
    .bind(&input.status)
    .bind(&input.auth_kind)
    .bind(
        encrypted
            .as_ref()
            .map(|secret| secret.ciphertext.as_slice()),
    )
    .bind(encrypted.as_ref().map(|secret| secret.nonce.as_slice()))
    .bind(encrypted.as_ref().map(|secret| secret.key_version.as_str()))
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await
    .map_err(integration_write_error)?;

    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Integration,
        integration_id,
        input.visibility.as_ref(),
        true,
    )
    .await?;

    let action_ids = insert_actions(
        &mut transaction,
        &actor,
        project_id,
        integration_id,
        &input.actions,
    )
    .await?;
    replace_profile_assignments(
        &mut transaction,
        &actor,
        project_id,
        integration_id,
        &input.ai_profile_ids,
    )
    .await?;
    validate_profile_catalog_capacity(&mut transaction, actor.tenant_id, &input.ai_profile_ids)
        .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "api_integration.created",
        integration_id,
        &action_ids,
        &input.ai_profile_ids,
    )
    .await?;
    transaction.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(load_integration(&state, &actor, integration_id).await?),
    ))
}

async fn update_integration(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(integration_id): Path<Uuid>,
    Json(request): Json<ApiIntegrationInput>,
) -> Result<Json<ApiIntegrationResponse>, AppError> {
    require_write_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::Integration,
        integration_id,
    )
    .await?;
    let input = normalize_integration(request)?;
    let mut transaction = state.db.begin().await?;
    let existing = sqlx::query_as::<_, ExistingSecretRow>(
        r#"
        SELECT integration_key, encrypted_token, token_nonce, key_version
        FROM api_integrations
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
          AND resource_visible(visibility, $4, $5)
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .bind(actor.project_id)
    .bind(actor.department_id())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Integration,
        integration_id,
        input.visibility.as_ref(),
        false,
    )
    .await?;

    if existing.integration_key != input.key {
        return Err(AppError::BadRequest(
            "integration key cannot be changed".to_owned(),
        ));
    }
    let existing_profile_ids = load_profile_assignment_ids(
        &mut transaction,
        actor.tenant_id,
        project_id,
        integration_id,
    )
    .await?;
    let assignments_changed =
        profile_assignments_changed(&existing_profile_ids, &input.ai_profile_ids);
    if assignments_changed {
        actor.require("ai:manage")?;
    }

    validate_ai_profiles(
        &mut transaction,
        &actor,
        &input.ai_profile_ids,
        &existing_profile_ids,
    )
    .await?;
    validate_profile_assignment_capacity(
        &mut transaction,
        actor.tenant_id,
        integration_id,
        &input.ai_profile_ids,
    )
    .await?;
    let token_metadata = resolve_updated_token(
        &state,
        actor.tenant_id,
        project_id,
        integration_id,
        &input,
        existing,
    )?;
    sqlx::query(
        r#"
        UPDATE api_integrations
        SET name = $4, description = $5,
            base_url = $6, status = $7, auth_kind = $8,
            encrypted_token = $9, token_nonce = $10, key_version = $11,
            updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.base_url)
    .bind(&input.status)
    .bind(&input.auth_kind)
    .bind(
        token_metadata
            .as_ref()
            .map(|(ciphertext, _, _)| ciphertext.as_slice()),
    )
    .bind(
        token_metadata
            .as_ref()
            .map(|(_, nonce, _)| nonce.as_slice()),
    )
    .bind(
        token_metadata
            .as_ref()
            .map(|(_, _, key_version)| key_version.as_str()),
    )
    .execute(&mut *transaction)
    .await
    .map_err(integration_write_error)?;

    let action_ids = replace_actions(
        &mut transaction,
        &actor,
        project_id,
        integration_id,
        &input.actions,
    )
    .await?;
    if assignments_changed {
        replace_profile_assignments(
            &mut transaction,
            &actor,
            project_id,
            integration_id,
            &input.ai_profile_ids,
        )
        .await?;
    }
    validate_profile_catalog_capacity(&mut transaction, actor.tenant_id, &input.ai_profile_ids)
        .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "api_integration.updated",
        integration_id,
        &action_ids,
        &input.ai_profile_ids,
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_integration(&state, &actor, integration_id).await?,
    ))
}

async fn delete_integration(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(integration_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_write_management(&actor)?;
    let project_id = resource_visibility::owner_project(
        &state.db,
        &actor,
        Resource::Integration,
        integration_id,
    )
    .await?;
    let mut transaction = state.db.begin().await?;
    let exists = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM api_integrations
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
          AND resource_visible(visibility, $4, $5)
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .bind(actor.project_id)
    .bind(actor.department_id())
    .fetch_optional(&mut *transaction)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let action_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM api_integration_actions
        WHERE tenant_id = $1 AND project_id = $2 AND integration_id = $3
        ORDER BY position, id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .fetch_all(&mut *transaction)
    .await?;
    let ai_profile_ids = load_profile_assignment_ids(
        &mut transaction,
        actor.tenant_id,
        project_id,
        integration_id,
    )
    .await?;
    if !ai_profile_ids.is_empty() {
        actor.require("ai:manage")?;
    }
    sqlx::query(
        "DELETE FROM api_integrations WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "api_integration.deleted",
        integration_id,
        &action_ids,
        &ai_profile_ids,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn normalize_integration(request: ApiIntegrationInput) -> Result<NormalizedIntegration, AppError> {
    let name = normalize_required_text(request.name, 200, "name")?;
    let key = normalize_key(request.key, "key")?;
    let description = normalize_description(request.description, "description")?;
    let base_url = normalize_base_url(request.base_url)?;
    let status = normalize_choice(request.status, &["active", "disabled"], "status")?;
    let auth_kind = normalize_choice(
        request.auth_kind,
        &["none", "bearer", "x_api_key"],
        "auth_kind",
    )?;
    let token = request.token.map(normalize_token).transpose()?;
    if token.is_some() && request.clear_token {
        return Err(AppError::BadRequest(
            "token and clear_token cannot be used together".to_owned(),
        ));
    }
    if auth_kind == "none" && token.is_some() {
        return Err(AppError::BadRequest(
            "token is not allowed when auth_kind is none".to_owned(),
        ));
    }
    if request.actions.is_empty() || request.actions.len() > MAX_ACTIONS {
        return Err(AppError::BadRequest(format!(
            "actions must contain between 1 and {MAX_ACTIONS} items"
        )));
    }
    let mut seen_action_keys = HashSet::with_capacity(request.actions.len());
    let actions = request
        .actions
        .into_iter()
        .map(|action| normalize_action(action, &mut seen_action_keys))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen_profile_ids = HashSet::with_capacity(request.ai_profile_ids.len());
    if !request
        .ai_profile_ids
        .iter()
        .all(|profile_id| seen_profile_ids.insert(*profile_id))
    {
        return Err(AppError::BadRequest(
            "ai_profile_ids must not contain duplicates".to_owned(),
        ));
    }
    Ok(NormalizedIntegration {
        visibility: request.visibility,
        name,
        key,
        description,
        base_url,
        status,
        auth_kind,
        token,
        clear_token: request.clear_token,
        actions,
        ai_profile_ids: request.ai_profile_ids,
    })
}

fn normalize_action(
    action: ApiIntegrationActionInput,
    seen_keys: &mut HashSet<String>,
) -> Result<NormalizedAction, AppError> {
    let key = normalize_key(action.key, "action key")?;
    if !seen_keys.insert(key.clone()) {
        return Err(AppError::BadRequest(
            "action keys must be unique".to_owned(),
        ));
    }
    let name = normalize_required_text(action.name, 200, "action name")?;
    let description = normalize_description(action.description, "action description")?;
    let (path_template, parameter_names) = normalize_path_template(action.path_template)?;
    Ok(NormalizedAction {
        key,
        name,
        description,
        path_template,
        parameter_names,
    })
}

fn normalize_required_text(
    value: String,
    max_chars: usize,
    field: &str,
) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(AppError::BadRequest(format!(
            "{field} must contain between 1 and {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_description(value: String, field: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.chars().count() > 2_000 {
        return Err(AppError::BadRequest(format!(
            "{field} must contain at most 2000 characters"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_key(value: String, field: &str) -> Result<String, AppError> {
    let value = value.trim();
    let mut characters = value.chars();
    let valid = (2..=64).contains(&value.len())
        && characters
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        });
    if !valid {
        return Err(AppError::BadRequest(format!(
            "{field} must match ^[a-z][a-z0-9_]{{1,63}}$"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_choice(value: String, allowed: &[&str], field: &str) -> Result<String, AppError> {
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(AppError::BadRequest(format!("unsupported {field}")))
    }
}

fn normalize_token(value: String) -> Result<String, AppError> {
    let value = value.trim();
    if value.len() < 8 || value.len() > 8_192 || HeaderValue::from_str(value).is_err() {
        return Err(AppError::BadRequest(
            "token must contain between 8 and 8192 valid HTTP header characters".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn normalize_base_url(value: String) -> Result<String, AppError> {
    if value.trim().chars().count() > 2_000 {
        return Err(AppError::BadRequest(
            "base_url must contain at most 2000 characters".to_owned(),
        ));
    }
    let parsed = Url::parse(value.trim())
        .map_err(|_| AppError::BadRequest("base_url must be a valid HTTPS URL".to_owned()))?;
    if parsed.scheme() != "https"
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.port().is_some_and(|port| port != 443)
        || parsed.port_or_known_default() != Some(443)
    {
        return Err(AppError::BadRequest(
            "base_url must use HTTPS port 443 without credentials, query, or fragment".to_owned(),
        ));
    }
    let normalized = parsed.to_string();
    let normalized = normalized.trim_end_matches('/');
    if normalized.chars().count() > 2_000 {
        return Err(AppError::BadRequest(
            "normalized base_url must contain at most 2000 characters".to_owned(),
        ));
    }
    Ok(normalized.to_owned())
}

fn normalize_path_template(value: String) -> Result<(String, Vec<String>), AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > 2_000
        || !value.starts_with('/')
        || value.contains("//")
        || value.contains("://")
        || value.contains('\\')
        || value.contains('#')
        || value.contains('%')
        || value.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(
            "path_template must be a relative absolute-path without a scheme, host, backslash, percent escape, or fragment"
                .to_owned(),
        ));
    }
    let path = value.split_once('?').map_or(value, |(path, _)| path);
    if path.split('/').any(is_dot_segment) {
        return Err(AppError::BadRequest(
            "path_template must not contain dot segments".to_owned(),
        ));
    }

    let mut parameter_names = Vec::new();
    let mut seen = HashSet::new();
    let mut remaining = value;
    while let Some(open) = remaining.find(['{', '}']) {
        let marker = remaining.as_bytes()[open];
        if marker == b'}' {
            return Err(AppError::BadRequest(
                "path_template contains an unmatched closing brace".to_owned(),
            ));
        }
        let after_open = &remaining[open + 1..];
        let close = after_open.find('}').ok_or_else(|| {
            AppError::BadRequest("path_template contains an unclosed placeholder".to_owned())
        })?;
        let parameter = &after_open[..close];
        if !is_parameter_name(parameter) {
            return Err(AppError::BadRequest(
                "path_template placeholders must use snake_case names".to_owned(),
            ));
        }
        if seen.insert(parameter.to_owned()) {
            parameter_names.push(parameter.to_owned());
        }
        if parameter_names.len() > MAX_PARAMETER_NAMES {
            return Err(AppError::BadRequest(format!(
                "an action may contain at most {MAX_PARAMETER_NAMES} distinct placeholders"
            )));
        }
        remaining = &after_open[close + 1..];
    }
    Ok((value.to_owned(), parameter_names))
}

fn is_dot_segment(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().as_str(),
        "." | ".." | "%2e" | "%2e%2e" | ".%2e" | "%2e."
    )
}

fn is_parameter_name(value: &str) -> bool {
    let mut characters = value.chars();
    (1..=64).contains(&value.len())
        && characters
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn require_read_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    actor.require_password_session()?;
    actor.require("integrations:manage")?;
    let project_id = actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))?;
    if actor.has_restricted_inbox_scope() && actor.department_id().is_none() {
        return Err(AppError::Forbidden);
    }
    Ok(project_id)
}

fn require_write_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    require_read_management(actor)
}

fn integration_token_aad(tenant_id: Uuid, project_id: Uuid, integration_id: Uuid) -> Vec<u8> {
    format!("{INTEGRATION_TOKEN_AAD_PREFIX}:{tenant_id}:{project_id}:{integration_id}").into_bytes()
}

fn encrypt_integration_token(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    integration_id: Uuid,
    token: Option<&str>,
) -> Result<Option<crate::ai_settings::EncryptedSecret>, AppError> {
    token
        .map(|token| {
            let key = load_secret_encryption_key(state)?;
            let aad = integration_token_aad(tenant_id, project_id, integration_id);
            encrypt_secret(
                &key,
                token.as_bytes(),
                state.config.secrets.key_version.clone(),
                &aad,
            )
        })
        .transpose()
}

fn resolve_updated_token(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    integration_id: Uuid,
    input: &NormalizedIntegration,
    existing: ExistingSecretRow,
) -> Result<Option<StoredTokenMetadata>, AppError> {
    if input.auth_kind == "none" {
        return Ok(None);
    }
    if input.clear_token {
        return Err(AppError::BadRequest(
            "clear_token cannot be used with an authenticated integration".to_owned(),
        ));
    }
    if let Some(token) = input.token.as_deref() {
        let encrypted =
            encrypt_integration_token(state, tenant_id, project_id, integration_id, Some(token))?
                .ok_or_else(|| {
                AppError::internal(anyhow::anyhow!(
                    "provided integration token was not encrypted"
                ))
            })?;
        return Ok(Some((
            encrypted.ciphertext,
            encrypted.nonce.to_vec(),
            encrypted.key_version,
        )));
    }
    match (
        existing.encrypted_token,
        existing.token_nonce,
        existing.key_version,
    ) {
        (Some(ciphertext), Some(nonce), Some(key_version)) => {
            Ok(Some((ciphertext, nonce, key_version)))
        }
        (None, None, None) => Err(AppError::BadRequest(
            "token is required for the selected authentication kind".to_owned(),
        )),
        _ => Err(AppError::internal(anyhow::anyhow!(
            "API integration token encryption metadata is incomplete"
        ))),
    }
}

async fn validate_ai_profiles(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    profile_ids: &[Uuid],
    existing_ids: &[Uuid],
) -> Result<(), AppError> {
    if profile_ids.is_empty() {
        return Ok(());
    }
    let found = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM ai_profiles
        WHERE tenant_id = $1 AND id = ANY($2)
          AND (id = ANY($3) OR (resource_visible(visibility, $4, $5)
               AND ($6::uuid IS NULL OR project_id = $6)))
        ORDER BY id
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(profile_ids)
    .bind(existing_ids)
    .bind(actor.project_id)
    .bind(actor.department_id())
    .bind(resource_visibility::owner_limit(actor))
    .fetch_all(&mut **transaction)
    .await?;
    if found.len() != profile_ids.len() {
        return Err(AppError::BadRequest(
            "every new ai_profile_id must be visible in the selected workspace".to_owned(),
        ));
    }
    Ok(())
}

async fn load_profile_assignment_ids(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    integration_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT ai_profile_id
        FROM ai_profile_api_integrations
        WHERE tenant_id = $1 AND project_id = $2 AND integration_id = $3
        ORDER BY ai_profile_id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(AppError::Database)
}

fn profile_assignments_changed(current: &[Uuid], requested: &[Uuid]) -> bool {
    if current.len() != requested.len() {
        return true;
    }
    let requested = requested.iter().copied().collect::<HashSet<_>>();
    current
        .iter()
        .any(|profile_id| !requested.contains(profile_id))
}

async fn validate_profile_assignment_capacity(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    integration_id: Uuid,
    profile_ids: &[Uuid],
) -> Result<(), AppError> {
    if profile_ids.is_empty() {
        return Ok(());
    }
    let full_profile = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT assignment.ai_profile_id
        FROM ai_profile_api_integrations AS assignment
        WHERE assignment.tenant_id = $1
          AND assignment.ai_profile_id = ANY($2)
          AND assignment.integration_id <> $3
        GROUP BY assignment.ai_profile_id
        HAVING COUNT(*) >= 32
        LIMIT 1
        "#,
    )
    .bind(tenant_id)
    .bind(profile_ids)
    .bind(integration_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if full_profile.is_some() {
        return Err(AppError::BadRequest(
            "an AI profile may be assigned at most 32 API integrations".to_owned(),
        ));
    }
    Ok(())
}

async fn validate_profile_catalog_capacity(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    profile_ids: &[Uuid],
) -> Result<(), AppError> {
    if profile_ids.is_empty() {
        return Ok(());
    }
    let rows = sqlx::query_as::<_, ProfileCatalogRow>(
        r#"
        SELECT assignment.ai_profile_id AS profile_id,
               integration.id AS integration_id,
               integration.integration_key,
               integration.name AS integration_name,
               integration.description AS integration_description,
               action.action_key,
               action.name AS action_name,
               action.description AS action_description,
               action.parameter_names
        FROM ai_profile_api_integrations AS assignment
        JOIN api_integrations AS integration
          ON integration.tenant_id = assignment.tenant_id
         AND integration.project_id = assignment.project_id
         AND integration.id = assignment.integration_id
        JOIN api_integration_actions AS action
          ON action.tenant_id = integration.tenant_id
         AND action.project_id = integration.project_id
         AND action.integration_id = integration.id
        WHERE assignment.tenant_id = $1
          AND assignment.ai_profile_id = ANY($2)
        ORDER BY assignment.ai_profile_id, integration.name, integration.id,
                 action.position, action.id
        "#,
    )
    .bind(tenant_id)
    .bind(profile_ids)
    .fetch_all(&mut **transaction)
    .await?;

    let mut catalogs = HashMap::<Uuid, Vec<RuntimeIntegrationCatalog>>::new();
    let mut indexes = HashMap::<(Uuid, Uuid), usize>::new();
    for row in rows {
        let catalog = catalogs.entry(row.profile_id).or_default();
        let index = if let Some(index) = indexes.get(&(row.profile_id, row.integration_id)) {
            *index
        } else {
            let index = catalog.len();
            indexes.insert((row.profile_id, row.integration_id), index);
            catalog.push(RuntimeIntegrationCatalog {
                key: row.integration_key,
                name: row.integration_name,
                description: row.integration_description,
                actions: Vec::new(),
            });
            index
        };
        catalog[index]
            .actions
            .push(RuntimeIntegrationActionCatalog {
                key: row.action_key,
                name: row.action_name,
                description: row.action_description,
                parameter_names: row.parameter_names,
            });
    }
    for catalog in catalogs.values() {
        if let Some(message) =
            runtime_catalog_limit_violation(catalog).map_err(AppError::internal)?
        {
            return Err(AppError::BadRequest(message.to_owned()));
        }
    }
    Ok(())
}

fn runtime_catalog_limit_violation(
    catalog: &[RuntimeIntegrationCatalog],
) -> Result<Option<&'static str>, serde_json::Error> {
    let mut keys = HashSet::with_capacity(catalog.len());
    if catalog
        .iter()
        .any(|integration| !keys.insert(&integration.key))
    {
        return Ok(Some(
            "an AI profile cannot use multiple integrations with the same key",
        ));
    }

    if catalog.len() > MAX_RUNTIME_INTEGRATIONS {
        return Ok(Some(
            "an AI profile may be assigned at most 32 API integrations",
        ));
    }
    let action_count = catalog
        .iter()
        .map(|integration| integration.actions.len())
        .sum::<usize>();
    if action_count > MAX_RUNTIME_CATALOG_ACTIONS {
        return Ok(Some(
            "an AI profile API integration catalog may contain at most 128 actions",
        ));
    }
    if serde_json::to_vec(catalog)?.len() > MAX_RUNTIME_CATALOG_JSON_BYTES {
        return Ok(Some(
            "an AI profile API integration catalog may contain at most 128 KiB of metadata",
        ));
    }
    Ok(None)
}

async fn insert_actions(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    integration_id: Uuid,
    actions: &[NormalizedAction],
) -> Result<Vec<Uuid>, AppError> {
    let mut action_ids = Vec::with_capacity(actions.len());
    for (position, action) in actions.iter().enumerate() {
        let action_id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO api_integration_actions (
                id, tenant_id, project_id, integration_id, action_key,
                name, description, path_template, parameter_names, position
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(action_id)
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(integration_id)
        .bind(&action.key)
        .bind(&action.name)
        .bind(&action.description)
        .bind(&action.path_template)
        .bind(&action.parameter_names)
        .bind(i16::try_from(position).map_err(AppError::internal)?)
        .execute(&mut **transaction)
        .await
        .map_err(integration_write_error)?;
        action_ids.push(action_id);
    }
    Ok(action_ids)
}

async fn replace_actions(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    integration_id: Uuid,
    actions: &[NormalizedAction],
) -> Result<Vec<Uuid>, AppError> {
    let existing = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT id, action_key
        FROM api_integration_actions
        WHERE tenant_id = $1 AND project_id = $2 AND integration_id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut existing_by_key = existing
        .into_iter()
        .map(|(id, key)| (key, id))
        .collect::<HashMap<_, _>>();
    let mut action_ids = Vec::with_capacity(actions.len());
    for (position, action) in actions.iter().enumerate() {
        let action_id = existing_by_key
            .remove(&action.key)
            .unwrap_or_else(Uuid::now_v7);
        sqlx::query(
            r#"
            INSERT INTO api_integration_actions (
                id, tenant_id, project_id, integration_id, action_key,
                name, description, path_template, parameter_names, position
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (tenant_id, integration_id, action_key) DO UPDATE
            SET name = EXCLUDED.name,
                description = EXCLUDED.description,
                path_template = EXCLUDED.path_template,
                parameter_names = EXCLUDED.parameter_names,
                position = EXCLUDED.position,
                updated_at = now()
            "#,
        )
        .bind(action_id)
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(integration_id)
        .bind(&action.key)
        .bind(&action.name)
        .bind(&action.description)
        .bind(&action.path_template)
        .bind(&action.parameter_names)
        .bind(i16::try_from(position).map_err(AppError::internal)?)
        .execute(&mut **transaction)
        .await
        .map_err(integration_write_error)?;
        action_ids.push(action_id);
    }
    let removed_ids = existing_by_key.into_values().collect::<Vec<_>>();
    if !removed_ids.is_empty() {
        sqlx::query(
            r#"
            DELETE FROM api_integration_actions
            WHERE tenant_id = $1 AND project_id = $2
              AND integration_id = $3 AND id = ANY($4)
            "#,
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(integration_id)
        .bind(&removed_ids)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(action_ids)
}

async fn replace_profile_assignments(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    integration_id: Uuid,
    profile_ids: &[Uuid],
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        DELETE FROM ai_profile_api_integrations
        WHERE tenant_id = $1 AND project_id = $2 AND integration_id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(integration_id)
    .execute(&mut **transaction)
    .await?;
    for profile_id in profile_ids {
        sqlx::query(
            r#"
            INSERT INTO ai_profile_api_integrations (
                tenant_id, project_id, ai_profile_id, integration_id, created_by
            ) VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(profile_id)
        .bind(integration_id)
        .bind(actor.actor_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    action: &str,
    integration_id: Uuid,
    action_ids: &[Uuid],
    ai_profile_ids: &[Uuid],
) -> Result<(), AppError> {
    let metadata = json!({
        "action_ids": action_ids,
        "action_count": action_ids.len(),
        "ai_profile_ids": ai_profile_ids,
        "ai_profile_count": ai_profile_ids.len(),
    });
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5, 'api_integration', $6, $7)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(action)
    .bind(integration_id)
    .bind(metadata)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn integration_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict(
            "an API integration with this name, key, or action key already exists".to_owned(),
        )
    } else {
        AppError::Database(error)
    }
}

async fn load_ai_profile_options(
    state: &AppState,
    actor: &ActorContext,
) -> Result<Vec<ApiIntegrationAiProfile>, AppError> {
    let rows = sqlx::query_as::<_, ApiIntegrationAiProfileRow>(
        r#"
        SELECT profile.id, profile.name, profile.status,
               provider.status AS provider_status,
               provider.base_url AS provider_base_url
        FROM ai_profiles AS profile
        LEFT JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
        WHERE profile.tenant_id = $1 AND resource_visible(profile.visibility, $2, $3)
          AND ($4::uuid IS NULL OR profile.project_id = $4)
        ORDER BY profile.name, profile.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .bind(actor.department_id())
    .bind(resource_visibility::owner_limit(actor))
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ApiIntegrationAiProfile {
            runtime_available: row.status == "active"
                && row.provider_status.as_deref() == Some("active")
                && row
                    .provider_base_url
                    .as_deref()
                    .is_some_and(|base_url| state.config.openclaw.handles_provider(base_url)),
            id: row.id,
            name: row.name,
            status: row.status,
        })
        .collect())
}

async fn load_integrations(
    state: &AppState,
    actor: &ActorContext,
    saved_id: Option<Uuid>,
) -> Result<Vec<ApiIntegrationResponse>, AppError> {
    let rows = sqlx::query_as::<_, IntegrationRow>(
        r#"
        SELECT visibility, id, name, integration_key AS key, description, base_url,
               status, auth_kind, encrypted_token IS NOT NULL AS token_configured,
               created_at, updated_at
        FROM api_integrations
        WHERE tenant_id = $1
          AND (($5::uuid IS NOT NULL AND id = $5) OR
               ($5 IS NULL AND resource_visible(visibility, $2, $3)
                AND ($4::uuid IS NULL OR project_id = $4)))
        ORDER BY name, id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .bind(actor.department_id())
    .bind(resource_visibility::owner_limit(actor))
    .bind(saved_id)
    .fetch_all(&state.db)
    .await?;
    let integration_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let action_rows = sqlx::query_as::<_, ActionRow>(
        r#"
        SELECT action.id, action.integration_id, action.action_key AS key,
               action.name, action.description, action.path_template,
               action.parameter_names
        FROM api_integration_actions AS action
        JOIN api_integrations AS integration
          ON integration.tenant_id = action.tenant_id
         AND integration.project_id = action.project_id
         AND integration.id = action.integration_id
        WHERE integration.tenant_id = $1 AND integration.id = ANY($2)
        ORDER BY action.integration_id, action.position, action.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(&integration_ids)
    .fetch_all(&state.db)
    .await?;
    let assignments = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT assignment.integration_id, assignment.ai_profile_id
        FROM ai_profile_api_integrations AS assignment
        JOIN api_integrations AS integration
          ON integration.tenant_id = assignment.tenant_id
         AND integration.project_id = assignment.project_id
         AND integration.id = assignment.integration_id
        WHERE integration.tenant_id = $1 AND integration.id = ANY($2)
        ORDER BY assignment.integration_id, assignment.ai_profile_id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(&integration_ids)
    .fetch_all(&state.db)
    .await?;

    let mut items = rows
        .into_iter()
        .map(|row| ApiIntegrationResponse {
            visibility: row.visibility.0,
            id: row.id,
            name: row.name,
            key: row.key,
            description: row.description,
            base_url: row.base_url,
            status: row.status,
            auth_kind: row.auth_kind,
            token_configured: row.token_configured,
            actions: Vec::new(),
            ai_profile_ids: Vec::new(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect::<Vec<_>>();
    let indexes = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.id, index))
        .collect::<HashMap<_, _>>();
    for action in action_rows {
        if let Some(index) = indexes.get(&action.integration_id) {
            items[*index].actions.push(ApiIntegrationActionResponse {
                id: action.id,
                key: action.key,
                name: action.name,
                description: action.description,
                path_template: action.path_template,
                parameter_names: action.parameter_names,
            });
        }
    }
    for (integration_id, profile_id) in assignments {
        if let Some(index) = indexes.get(&integration_id) {
            items[*index].ai_profile_ids.push(profile_id);
        }
    }
    Ok(items)
}

async fn load_integration(
    state: &AppState,
    actor: &ActorContext,
    integration_id: Uuid,
) -> Result<ApiIntegrationResponse, AppError> {
    load_integrations(state, actor, Some(integration_id))
        .await?
        .into_iter()
        .find(|integration| integration.id == integration_id)
        .ok_or(AppError::NotFound)
}

/// Loads only active integrations explicitly granted to one active AI profile.
pub(crate) async fn load_runtime_catalog(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    profile_id: Uuid,
) -> anyhow::Result<Vec<RuntimeIntegrationCatalog>> {
    load_scoped_runtime_catalog(state, tenant_id, project_id, profile_id, false).await
}

pub(crate) async fn load_test_runtime_catalog(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    profile_id: Uuid,
) -> anyhow::Result<Vec<RuntimeIntegrationCatalog>> {
    load_scoped_runtime_catalog(state, tenant_id, project_id, profile_id, true).await
}

async fn load_scoped_runtime_catalog(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    profile_id: Uuid,
    test_execution: bool,
) -> anyhow::Result<Vec<RuntimeIntegrationCatalog>> {
    let provider_base_url = sqlx::query_scalar::<_, String>(
        r#"
        SELECT provider.base_url
        FROM ai_profiles AS profile
        JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
         AND provider.status = 'active'
        WHERE profile.tenant_id = $1
          AND profile.project_id = $2
          AND profile.id = $3
          AND (profile.status = 'active' OR $4)
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(test_execution)
    .fetch_optional(&state.db)
    .await
    .context("could not verify API integration runtime provider")?;
    if !provider_base_url
        .as_deref()
        .is_some_and(|base_url| state.config.openclaw.handles_provider(base_url))
    {
        return Ok(Vec::new());
    }
    let assignment_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM ai_profiles AS profile
        JOIN ai_profile_api_integrations AS assignment
          ON assignment.tenant_id = profile.tenant_id
         AND assignment.ai_profile_id = profile.id
        WHERE profile.tenant_id = $1
          AND profile.project_id = $2
          AND profile.id = $3
          AND (profile.status = 'active' OR $4)
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(test_execution)
    .fetch_one(&state.db)
    .await
    .context("could not verify API integration runtime catalog capacity")?;
    let maximum_integrations = i64::try_from(MAX_RUNTIME_INTEGRATIONS)
        .context("API integration runtime catalog limit is invalid")?;
    if assignment_count > maximum_integrations {
        anyhow::bail!("AI profile API integration catalog exceeds the 32 item limit");
    }
    let rows = sqlx::query_as::<_, RuntimeCatalogRow>(
        r#"
        SELECT integration.id AS integration_id,
               integration.integration_key,
               integration.name AS integration_name,
               integration.description AS integration_description,
               action.action_key,
               action.name AS action_name,
               action.description AS action_description,
               action.parameter_names
        FROM ai_profiles AS profile
        JOIN ai_profile_api_integrations AS assignment
          ON assignment.tenant_id = profile.tenant_id
         AND assignment.ai_profile_id = profile.id
        JOIN api_integrations AS integration
          ON integration.tenant_id = assignment.tenant_id
         AND integration.project_id = assignment.project_id
         AND integration.id = assignment.integration_id
         AND integration.status = 'active'
        JOIN api_integration_actions AS action
          ON action.tenant_id = integration.tenant_id
         AND action.project_id = integration.project_id
         AND action.integration_id = integration.id
        WHERE profile.tenant_id = $1
          AND profile.project_id = $2
          AND profile.id = $3
          AND (profile.status = 'active' OR $4)
        ORDER BY integration.name, integration.id, action.position, action.id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(test_execution)
    .fetch_all(&state.db)
    .await
    .context("could not load API integration runtime catalog")?;

    let mut catalog = Vec::<RuntimeIntegrationCatalog>::new();
    let mut indexes = HashMap::<Uuid, usize>::new();
    for row in rows {
        let index = if let Some(index) = indexes.get(&row.integration_id) {
            *index
        } else {
            let index = catalog.len();
            indexes.insert(row.integration_id, index);
            catalog.push(RuntimeIntegrationCatalog {
                key: row.integration_key,
                name: row.integration_name,
                description: row.integration_description,
                actions: Vec::new(),
            });
            index
        };
        catalog[index]
            .actions
            .push(RuntimeIntegrationActionCatalog {
                key: row.action_key,
                name: row.action_name,
                description: row.action_description,
                parameter_names: row.parameter_names,
            });
    }
    if let Some(message) = runtime_catalog_limit_violation(&catalog)
        .context("could not measure API integration runtime catalog")?
    {
        anyhow::bail!(message);
    }
    Ok(catalog)
}

/// Executes one active, profile-granted API action as a bounded GET request.
pub(crate) async fn execute_runtime_action(
    state: &AppState,
    request: RuntimeActionRequest,
) -> anyhow::Result<RuntimeActionResponse> {
    execute_scoped_runtime_action(state, request, false).await
}

pub(crate) async fn execute_test_runtime_action(
    state: &AppState,
    request: RuntimeActionRequest,
) -> anyhow::Result<RuntimeActionResponse> {
    execute_scoped_runtime_action(state, request, true).await
}

async fn execute_scoped_runtime_action(
    state: &AppState,
    request: RuntimeActionRequest,
    test_mode: bool,
) -> anyhow::Result<RuntimeActionResponse> {
    let action = sqlx::query_as::<_, RuntimeActionRow>(
        r#"
        SELECT integration.project_id AS owner_project_id, integration.id AS integration_id,
               action.id AS action_id,
               integration.base_url,
               integration.auth_kind,
               integration.encrypted_token,
               integration.token_nonce,
               integration.key_version,
               action.path_template,
               action.parameter_names,
               provider.base_url AS provider_base_url
        FROM ai_profiles AS profile
        JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
         AND provider.status = 'active'
        JOIN ai_profile_api_integrations AS assignment
          ON assignment.tenant_id = profile.tenant_id
         AND assignment.ai_profile_id = profile.id
        JOIN api_integrations AS integration
          ON integration.tenant_id = assignment.tenant_id
         AND integration.project_id = assignment.project_id
         AND integration.id = assignment.integration_id
         AND integration.status = 'active'
        JOIN api_integration_actions AS action
          ON action.tenant_id = integration.tenant_id
         AND action.project_id = integration.project_id
         AND action.integration_id = integration.id
        WHERE profile.tenant_id = $1
          AND profile.project_id = $2
          AND profile.id = $3
          AND (profile.status = 'active' OR $6)
          AND integration.integration_key = $4
          AND action.action_key = $5
        "#,
    )
    .bind(request.tenant_id)
    .bind(request.project_id)
    .bind(request.profile_id)
    .bind(&request.integration_key)
    .bind(&request.action_key)
    .bind(test_mode)
    .fetch_optional(&state.db)
    .await
    .context("could not authorize API integration runtime action")?
    .context("API integration action is not active or granted to this AI profile")?;

    insert_runtime_audit(
        state,
        &request,
        action.integration_id,
        action.action_id,
        "started",
        None,
        None,
    )
    .await?;
    let execution = execute_authorized_runtime_action(state, &request, &action).await;
    match execution {
        Ok(response) => {
            insert_runtime_audit(
                state,
                &request,
                action.integration_id,
                action.action_id,
                "success",
                Some(response.status_code),
                None,
            )
            .await?;
            Ok(response)
        }
        Err(error) => {
            insert_runtime_audit(
                state,
                &request,
                action.integration_id,
                action.action_id,
                "failure",
                None,
                Some("execution_failed"),
            )
            .await
            .context("could not audit failed API integration action")?;
            Err(error)
        }
    }
}

async fn execute_authorized_runtime_action(
    state: &AppState,
    request: &RuntimeActionRequest,
    action: &RuntimeActionRow,
) -> anyhow::Result<RuntimeActionResponse> {
    if !state
        .config
        .openclaw
        .handles_provider(&action.provider_base_url)
    {
        anyhow::bail!("AI profile runtime provider is not available for API integrations");
    }
    let (path_template, parameter_names) = normalize_path_template(action.path_template.clone())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if parameter_names != action.parameter_names {
        anyhow::bail!("stored API integration action parameter metadata is inconsistent");
    }
    validate_runtime_parameters(&parameter_names, &request.parameters)?;
    let expanded_path = expand_path_template(&path_template, &request.parameters)?;
    let base_url = Url::parse(&action.base_url).context("stored API integration URL is invalid")?;
    let target_url = build_runtime_target(&base_url, &expanded_path)?;
    let client = provider_reply::build_task_provider_http_client(&base_url)
        .await
        .context("API integration target is not a permitted public endpoint")?;
    let token = decrypt_integration_token(
        state,
        request.tenant_id,
        action.owner_project_id,
        action.integration_id,
        action.encrypted_token.as_deref(),
        action.token_nonce.as_deref(),
        action.key_version.as_deref(),
    )?;
    let mut outbound = client.get(target_url);
    match action.auth_kind.as_str() {
        "none" if token.is_none() => {}
        "bearer" => {
            let token = token
                .as_deref()
                .context("bearer integration token is not configured")?;
            let value = HeaderValue::from_str(&format!("Bearer {token}"))
                .context("bearer integration token is not a valid HTTP header value")?;
            outbound = outbound.header(header::AUTHORIZATION, value);
        }
        "x_api_key" => {
            let token = token
                .as_deref()
                .context("X-API-Key integration token is not configured")?;
            let value = HeaderValue::from_str(token)
                .context("X-API-Key integration token is not a valid HTTP header value")?;
            outbound = outbound.header("x-api-key", value);
        }
        "none" => anyhow::bail!("unauthenticated integration unexpectedly has a stored token"),
        _ => anyhow::bail!("stored API integration authentication kind is invalid"),
    }

    let mut response = outbound
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .with_context(|| format!("API integration action {} request failed", action.action_id))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RUNTIME_RESPONSE_BYTES as u64)
    {
        anyhow::bail!("API integration response exceeds the 256 KiB limit");
    }
    let status_code = response.status().as_u16();
    if !(100..=599).contains(&status_code) {
        anyhow::bail!("API integration returned an invalid HTTP status code");
    }
    let upstream_content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().map(str::to_owned))
        .transpose()
        .context("API integration response has an invalid Content-Type header")?;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("could not read API integration response")?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RUNTIME_RESPONSE_BYTES {
            anyhow::bail!("API integration response exceeds the 256 KiB limit");
        }
        body.extend_from_slice(&chunk);
    }
    let content_type = normalize_runtime_content_type(upstream_content_type.as_deref())?;
    let body = String::from_utf8(body).context("API integration response is not valid UTF-8")?;
    let body = redact_response_token(body, token.as_deref());
    Ok(RuntimeActionResponse {
        status_code,
        content_type,
        body,
    })
}

fn redact_response_token(mut body: String, token: Option<&str>) -> String {
    let Some(token) = token else {
        return body;
    };
    if let Ok(encoded) = serde_json::to_string(token) {
        let escaped = encoded
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or_default();
        if !escaped.is_empty() && escaped != token {
            body = body.replace(escaped, "[REDACTED]");
        }
    }
    body.replace(token, "[REDACTED]")
}

async fn insert_runtime_audit(
    state: &AppState,
    request: &RuntimeActionRequest,
    integration_id: Uuid,
    action_id: Uuid,
    outcome: &str,
    status_code: Option<u16>,
    error_code: Option<&str>,
) -> anyhow::Result<()> {
    let mut metadata = json!({
        "action_id": action_id,
        "ai_profile_id": request.profile_id,
        "source_id": request.source_id,
        "outcome": outcome,
    });
    if let Some(status_code) = status_code {
        metadata["status_code"] = json!(status_code);
    }
    if let Some(error_code) = error_code {
        metadata["error_code"] = json!(error_code);
    }
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES (
            $1, $2, $3, NULL, 'api_integration.read_by_ai',
            'api_integration', $4, $5
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(request.tenant_id)
    .bind(request.project_id)
    .bind(integration_id)
    .bind(metadata)
    .execute(&state.db)
    .await
    .context("could not write API integration runtime audit")?;
    Ok(())
}

fn decrypt_integration_token(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    integration_id: Uuid,
    ciphertext: Option<&[u8]>,
    nonce: Option<&[u8]>,
    key_version: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let (Some(ciphertext), Some(nonce), Some(key_version)) = (ciphertext, nonce, key_version)
    else {
        if ciphertext.is_none() && nonce.is_none() && key_version.is_none() {
            return Ok(None);
        }
        anyhow::bail!("API integration token encryption metadata is incomplete");
    };
    if key_version != state.config.secrets.key_version {
        anyhow::bail!("API integration token uses an unsupported encryption key version");
    }
    let key = load_secret_encryption_key(state)?;
    let aad = integration_token_aad(tenant_id, project_id, integration_id);
    let plaintext = decrypt_secret(&key, ciphertext, nonce, &aad)?;
    String::from_utf8(plaintext)
        .map(Some)
        .context("API integration token is not valid UTF-8")
}

fn validate_runtime_parameters(
    expected: &[String],
    parameters: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let expected = expected.iter().map(String::as_str).collect::<HashSet<_>>();
    if expected.len() != parameters.len()
        || !parameters.keys().all(|key| expected.contains(key.as_str()))
    {
        anyhow::bail!("API integration action parameters do not match its declared placeholders");
    }
    for value in parameters.values() {
        if value.is_empty()
            || value.len() > MAX_RUNTIME_PARAMETER_BYTES
            || value == "."
            || value == ".."
            || value.contains('\'')
            || !value.bytes().all(is_safe_runtime_parameter_byte)
        {
            anyhow::bail!(
                "API integration parameter values must be bounded safe ASCII identifiers"
            );
        }
    }
    Ok(())
}

fn is_safe_runtime_parameter_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'.' | b'_' | b'~' | b':' | b'@' | b'+' | b' ' | b'-')
}

fn expand_path_template(
    template: &str,
    parameters: &BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let mut expanded = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some(open) = remaining.find('{') {
        expanded.push_str(&remaining[..open]);
        let after_open = &remaining[open + 1..];
        let close = after_open
            .find('}')
            .context("stored API integration path template is invalid")?;
        let name = &after_open[..close];
        let value = parameters
            .get(name)
            .with_context(|| format!("API integration parameter {name} is missing"))?;
        push_percent_encoded(&mut expanded, value);
        remaining = &after_open[close + 1..];
    }
    if remaining.contains('}') {
        anyhow::bail!("stored API integration path template is invalid");
    }
    expanded.push_str(remaining);
    Ok(expanded)
}

fn push_percent_encoded(target: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            target.push(char::from(byte));
        } else {
            target.push('%');
            target.push(char::from(HEX[usize::from(byte >> 4)]));
            target.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

fn build_runtime_target(base_url: &Url, expanded_path: &str) -> anyhow::Result<Url> {
    if base_url.scheme() != "https"
        || base_url.host().is_none()
        || !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
        || base_url.port_or_known_default() != Some(443)
    {
        anyhow::bail!("stored API integration base URL is outside the HTTPS policy");
    }
    if !expanded_path.starts_with('/')
        || expanded_path.starts_with("//")
        || expanded_path.contains('\\')
        || expanded_path.contains('#')
        || expanded_path.chars().any(char::is_control)
    {
        anyhow::bail!("expanded API integration target path is invalid");
    }
    let base = base_url.as_str().trim_end_matches('/');
    if base.len().saturating_add(expanded_path.len()) > MAX_RUNTIME_TARGET_URL_BYTES {
        anyhow::bail!("expanded API integration target exceeds the 16 KiB limit");
    }
    let target = Url::parse(&format!("{base}{expanded_path}"))
        .context("expanded API integration target URL is invalid")?;
    if target.scheme() != base_url.scheme()
        || target.host() != base_url.host()
        || target.port_or_known_default() != base_url.port_or_known_default()
        || target.username() != base_url.username()
        || target.password() != base_url.password()
        || target.fragment().is_some()
        || !path_is_under_base(base_url.path(), target.path())
    {
        anyhow::bail!("expanded API integration target escaped its configured base URL");
    }
    Ok(target)
}

fn path_is_under_base(base_path: &str, target_path: &str) -> bool {
    let base_path = base_path.trim_end_matches('/');
    if base_path.is_empty() {
        return target_path.starts_with('/');
    }
    target_path == base_path
        || target_path
            .strip_prefix(base_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn normalize_runtime_content_type(value: Option<&str>) -> anyhow::Result<String> {
    let Some(value) = value else {
        return Ok("text/plain".to_owned());
    };
    let media_type = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if media_type == "application/json" || media_type.ends_with("+json") {
        Ok("application/json".to_owned())
    } else if media_type.starts_with("text/") {
        Ok("text/plain".to_owned())
    } else {
        anyhow::bail!("API integration response must be JSON or text")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use serde_json::to_value;
    use uuid::Uuid;

    use super::{
        ApiIntegrationActionInput, ApiIntegrationActionResponse, ApiIntegrationInput,
        ApiIntegrationResponse, RuntimeIntegrationActionCatalog, RuntimeIntegrationCatalog,
        build_runtime_target, decrypt_secret, encrypt_secret, expand_path_template,
        integration_token_aad, normalize_base_url, normalize_integration, normalize_path_template,
        profile_assignments_changed, redact_response_token, runtime_catalog_limit_violation,
        validate_runtime_parameters,
    };

    #[test]
    fn normalization_preserves_base_prefix_and_derives_unique_parameters() {
        let normalized = normalize_integration(ApiIntegrationInput {
            visibility: None,
            name: " Orders ".to_owned(),
            key: "orders_api".to_owned(),
            description: " Order lookup ".to_owned(),
            base_url: "https://api.example.com/v1/".to_owned(),
            status: "active".to_owned(),
            auth_kind: "bearer".to_owned(),
            token: Some("secret-token".to_owned()),
            clear_token: false,
            actions: vec![ApiIntegrationActionInput {
                key: "get_order".to_owned(),
                name: "Get order".to_owned(),
                description: "Lookup by identifiers".to_owned(),
                path_template: "/orders/{id}?account={account_id}&again={id}".to_owned(),
            }],
            ai_profile_ids: Vec::new(),
        })
        .unwrap();
        assert_eq!(normalized.name, "Orders");
        assert_eq!(normalized.base_url, "https://api.example.com/v1");
        assert_eq!(normalized.actions[0].parameter_names, ["id", "account_id"]);
        let target = build_runtime_target(
            &url::Url::parse(&normalized.base_url).unwrap(),
            "/orders/42?account=a-1",
        )
        .unwrap();
        assert_eq!(
            target.as_str(),
            "https://api.example.com/v1/orders/42?account=a-1"
        );
        assert_eq!(
            normalize_path_template("/search?q={q}".to_owned())
                .unwrap()
                .1,
            ["q"]
        );
    }

    #[test]
    fn url_and_path_validation_reject_authority_and_traversal() {
        assert!(normalize_base_url("http://api.example.com".to_owned()).is_err());
        assert!(normalize_base_url("https://user@api.example.com".to_owned()).is_err());
        assert!(normalize_base_url("https://api.example.com:8443".to_owned()).is_err());
        assert!(
            normalize_base_url(format!("https://api.example.com/{}", "é".repeat(700))).is_err()
        );
        assert!(normalize_path_template("//evil.example/path".to_owned()).is_err());
        assert!(normalize_path_template("/orders/../secret".to_owned()).is_err());
        assert!(normalize_path_template("/orders/%2e%2e/secret".to_owned()).is_err());
        assert!(normalize_path_template("/orders/%2e%2e%2fadmin".to_owned()).is_err());
        assert!(normalize_path_template("/orders/{Bad}".to_owned()).is_err());
    }

    #[test]
    fn response_serialization_never_contains_write_only_secret_fields() {
        let response = ApiIntegrationResponse {
            visibility: crate::resource_visibility::ResourceVisibility::default(),
            id: Uuid::now_v7(),
            name: "Orders".to_owned(),
            key: "orders_api".to_owned(),
            description: String::new(),
            base_url: "https://api.example.com".to_owned(),
            status: "active".to_owned(),
            auth_kind: "bearer".to_owned(),
            token_configured: true,
            actions: vec![ApiIntegrationActionResponse {
                id: Uuid::now_v7(),
                key: "get_order".to_owned(),
                name: "Get order".to_owned(),
                description: String::new(),
                path_template: "/orders/{id}".to_owned(),
                parameter_names: vec!["id".to_owned()],
            }],
            ai_profile_ids: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let serialized = to_value(response).unwrap();
        assert_eq!(serialized["token_configured"], true);
        assert!(serialized.get("token").is_none());
        assert!(serialized.get("encrypted_token").is_none());
        assert!(serialized.get("token_nonce").is_none());
        assert!(serialized.get("key_version").is_none());
    }

    #[test]
    fn integration_token_encryption_is_bound_to_full_scope_aad() {
        let key = [9_u8; 32];
        let tenant_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let integration_id = Uuid::now_v7();
        let aad = integration_token_aad(tenant_id, project_id, integration_id);
        let encrypted = encrypt_secret(&key, b"secret", "v1".to_owned(), &aad).unwrap();
        let plaintext =
            decrypt_secret(&key, &encrypted.ciphertext, &encrypted.nonce, &aad).unwrap();
        assert_eq!(plaintext, b"secret");
        let wrong_aad = integration_token_aad(tenant_id, Uuid::now_v7(), integration_id);
        assert!(
            decrypt_secret(&key, &encrypted.ciphertext, &encrypted.nonce, &wrong_aad,).is_err()
        );
    }

    #[test]
    fn runtime_parameters_are_exact_safe_identifiers_and_percent_encoded() {
        let expected = vec!["id".to_owned(), "account".to_owned()];
        let parameters = BTreeMap::from([
            ("account".to_owned(), "ACME 1".to_owned()),
            ("id".to_owned(), "abc:42".to_owned()),
        ]);
        validate_runtime_parameters(&expected, &parameters).unwrap();
        let expanded = expand_path_template("/orders/{id}?account={account}", &parameters).unwrap();
        assert_eq!(expanded, "/orders/abc%3A42?account=ACME%201");
        let target = build_runtime_target(
            &url::Url::parse("https://api.example.com/v1").unwrap(),
            &expanded,
        )
        .unwrap();
        assert_eq!(
            target.as_str(),
            "https://api.example.com/v1/orders/abc%3A42?account=ACME%201"
        );
        assert!(
            build_runtime_target(
                &url::Url::parse("https://api.example.com/v1").unwrap(),
                &format!("/orders/{}", "a".repeat(16 * 1_024)),
            )
            .is_err()
        );

        for unsafe_value in [
            "",
            "ACME'1",
            "../admin",
            "foo/../../admin",
            "%2e%2e%2fadmin",
        ] {
            let unsafe_parameters = BTreeMap::from([
                ("account".to_owned(), unsafe_value.to_owned()),
                ("id".to_owned(), "abc".to_owned()),
            ]);
            assert!(validate_runtime_parameters(&expected, &unsafe_parameters).is_err());
        }
    }

    #[test]
    fn profile_grant_permission_is_required_only_when_the_set_changes() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        assert!(!profile_assignments_changed(
            &[first, second],
            &[second, first]
        ));
        assert!(profile_assignments_changed(&[first], &[second]));
        assert!(profile_assignments_changed(&[first], &[]));
    }

    #[test]
    fn configured_token_is_redacted_from_plain_and_json_escaped_responses() {
        assert_eq!(
            redact_response_token(
                "secret-token and secret-token".to_owned(),
                Some("secret-token")
            ),
            "[REDACTED] and [REDACTED]"
        );
        let redacted = redact_response_token(
            r#"{"token":"secret\"token"}"#.to_owned(),
            Some("secret\"token"),
        );
        assert_eq!(redacted, r#"{"token":"[REDACTED]"}"#);
    }

    #[test]
    fn runtime_catalog_limits_total_actions_and_serialized_metadata() {
        let catalog = vec![RuntimeIntegrationCatalog {
            key: "catalog_api".to_owned(),
            name: "Catalog".to_owned(),
            description: String::new(),
            actions: (0..129)
                .map(|index| RuntimeIntegrationActionCatalog {
                    key: format!("action_{index}"),
                    name: "Action".to_owned(),
                    description: String::new(),
                    parameter_names: Vec::new(),
                })
                .collect(),
        }];
        assert_eq!(
            runtime_catalog_limit_violation(&catalog).unwrap(),
            Some("an AI profile API integration catalog may contain at most 128 actions")
        );

        let oversized = vec![RuntimeIntegrationCatalog {
            key: "catalog_api".to_owned(),
            name: "Catalog".to_owned(),
            description: "x".repeat(129 * 1_024),
            actions: vec![RuntimeIntegrationActionCatalog {
                key: "get_item".to_owned(),
                name: "Get item".to_owned(),
                description: String::new(),
                parameter_names: Vec::new(),
            }],
        }];
        assert_eq!(
            runtime_catalog_limit_violation(&oversized).unwrap(),
            Some("an AI profile API integration catalog may contain at most 128 KiB of metadata")
        );
    }
}
