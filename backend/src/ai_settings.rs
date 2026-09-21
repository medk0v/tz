//! Language-model connections and AI profiles with optional workspace visibility.

use crate::resource_visibility::{self, Resource, ResourceVisibility};
mod backups;
pub(crate) mod proxy;

use std::collections::{BTreeMap, HashSet};

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::{get, patch, put},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction, types::Json as SqlJson};
use tracing::warn;
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    auth::ActorContext,
    avatars::{MAX_AVATAR_BYTES, ValidatedAvatar},
    error::AppError,
    realtime::RealtimeEvent,
};

const PROVIDER_SECRET_AAD: &[u8] = b"tzomet:ai-provider-api-key:v1";
const MAX_AI_SETTINGS_BODY_LENGTH: usize = 2 * 1_024 * 1_024;
const MAX_PROFILE_CUSTOM_FIELDS: usize = 32;
const MAX_PROFILE_LANGUAGE_LIST_CHARS: usize = 255;
const MAX_PROFILE_PUBLIC_IDENTITIES: usize = 64;
/// Preset templates live in the frontend; a profile saved from one records its key.
const PROFILE_PRESET_KEYS: &[&str] = &[
    "coordinator",
    "sales",
    "marketing",
    "finance",
    "legal",
    "programmer",
    "support",
    "quality",
    "hr",
];
const MAX_PROFILE_SECRETS: usize = 32;
pub(crate) const MODEL_TYPE_CHAT: &str = "chat";
const MODEL_TYPES: [&str; 1] = [MODEL_TYPE_CHAT];

/// Routes for provider connections and AI copilot profile administration.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(backups::router())
        .merge(proxy::router())
        .route(
            "/api/v1/ai/providers",
            get(list_providers).post(create_provider),
        )
        .route("/api/v1/ai/channel-options", get(list_channel_options))
        .route(
            "/api/v1/ai/providers/{provider_id}",
            patch(update_provider).delete(delete_provider),
        )
        .route(
            "/api/v1/ai/profiles",
            get(list_profiles)
                .post(create_profile)
                .layer(DefaultBodyLimit::max(MAX_AI_SETTINGS_BODY_LENGTH)),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}",
            patch(update_profile)
                .delete(delete_profile)
                .layer(DefaultBodyLimit::max(MAX_AI_SETTINGS_BODY_LENGTH)),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/avatar",
            put(upload_profile_avatar)
                .delete(delete_profile_avatar)
                .layer(DefaultBodyLimit::max(MAX_AVATAR_BYTES)),
        )
        .route(
            "/api/v1/ai/profiles/{profile_id}/public-identities/{language}/avatar",
            put(upload_profile_public_identity_avatar)
                .delete(delete_profile_public_identity_avatar)
                .layer(DefaultBodyLimit::max(MAX_AVATAR_BYTES)),
        )
}

#[derive(Debug, FromRow, Serialize)]
struct AiProviderResponse {
    visibility: sqlx::types::Json<ResourceVisibility>,
    id: Uuid,
    name: String,
    provider_kind: String,
    model_type: String,
    base_url: String,
    default_model: String,
    status: String,
    api_key_configured: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct AiProviderListResponse {
    items: Vec<AiProviderResponse>,
}

#[derive(Debug, FromRow, Serialize)]
struct AiChannelOptionResponse {
    id: Uuid,
    inbox_id: Uuid,
    kind: String,
    name: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct AiChannelOptionListResponse {
    items: Vec<AiChannelOptionResponse>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AiProviderRequest {
    visibility: Option<ResourceVisibility>,
    name: String,
    provider_kind: String,
    /// Omitted on create means `chat`; the type cannot change after creation.
    #[serde(default)]
    model_type: Option<String>,
    base_url: String,
    default_model: String,
    status: String,
    api_key: Option<String>,
    #[serde(default)]
    clear_api_key: bool,
}

async fn list_channel_options(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<AiChannelOptionListResponse>, AppError> {
    let project_id = require_management(&actor)?;
    let items = sqlx::query_as::<_, AiChannelOptionResponse>(
        r#"
        SELECT connection.id, connection.inbox_id, connection.kind,
               connection.name, connection.status
        FROM channel_connections AS connection
        JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
        WHERE connection.tenant_id = $1
          AND connection.project_id = $2
          AND connection.deleted_at IS NULL
          AND connection.kind <> 'custom_ai'
          AND inbox.status = 'active'
          AND ($3::uuid[] IS NULL OR connection.inbox_id = ANY($3))
        ORDER BY connection.name, connection.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.inbox_scope())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(AiChannelOptionListResponse { items }))
}

async fn list_providers(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<AiProviderListResponse>, AppError> {
    let project_id = require_management(&actor)?;
    let items = load_providers(&state, &actor, project_id).await?;
    Ok(Json(AiProviderListResponse { items }))
}

async fn create_provider(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<AiProviderRequest>,
) -> Result<(StatusCode, Json<AiProviderResponse>), AppError> {
    let project_id = require_project_wide_management(&actor)?;
    let visibility = request.visibility.clone();
    let input = normalize_provider(request)?;
    let model_type = input.model_type.as_deref().unwrap_or(MODEL_TYPE_CHAT);
    let encrypted = encrypt_optional_api_key(&state, input.api_key.as_deref())?;
    let provider_id = Uuid::now_v7();
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO ai_provider_connections (
            id, tenant_id, project_id, name, provider_kind, base_url,
            default_model, status, encrypted_api_key, api_key_nonce,
            key_version, created_by, model_type
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
    )
    .bind(provider_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&input.name)
    .bind(&input.provider_kind)
    .bind(&input.base_url)
    .bind(&input.default_model)
    .bind(&input.status)
    .bind(
        encrypted
            .as_ref()
            .map(|secret| secret.ciphertext.as_slice()),
    )
    .bind(encrypted.as_ref().map(|secret| secret.nonce.as_slice()))
    .bind(encrypted.as_ref().map(|secret| secret.key_version.as_str()))
    .bind(actor.actor_id)
    .bind(model_type)
    .execute(&mut *transaction)
    .await
    .map_err(ai_write_error)?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Provider,
        provider_id,
        visibility.as_ref(),
        true,
    )
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_provider.created",
        "ai_provider",
        provider_id,
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_provider(&state, &actor, project_id, provider_id).await?),
    ))
}

async fn update_provider(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(provider_id): Path<Uuid>,
    Json(request): Json<AiProviderRequest>,
) -> Result<Json<AiProviderResponse>, AppError> {
    require_project_wide_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Provider, provider_id)
            .await?;
    let visibility = request.visibility.clone();
    let input = normalize_provider(request)?;
    let encrypted = encrypt_optional_api_key(&state, input.api_key.as_deref())?;
    let mut transaction = state.db.begin().await?;
    let (current_base_url, api_key_configured, current_model_type) =
        sqlx::query_as::<_, (String, bool, String)>(
            r#"
        SELECT base_url, encrypted_api_key IS NOT NULL, model_type
        FROM ai_provider_connections
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR UPDATE
        "#,
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(provider_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    if input
        .model_type
        .as_deref()
        .is_some_and(|model_type| model_type != current_model_type)
    {
        return Err(AppError::BadRequest(
            "model_type cannot be changed after the connection is created".to_owned(),
        ));
    }
    validate_provider_api_key_destination(&current_base_url, api_key_configured, &input)?;
    let updated = sqlx::query(
        r#"
        UPDATE ai_provider_connections
        SET name = $4, provider_kind = $5, base_url = $6,
            default_model = $7, status = $8,
            encrypted_api_key = CASE
                WHEN $9 THEN NULL
                WHEN $10::bytea IS NOT NULL THEN $10
                ELSE encrypted_api_key
            END,
            api_key_nonce = CASE
                WHEN $9 THEN NULL
                WHEN $11::bytea IS NOT NULL THEN $11
                ELSE api_key_nonce
            END,
            key_version = CASE
                WHEN $9 THEN NULL
                WHEN $12::text IS NOT NULL THEN $12
                ELSE key_version
            END,
            updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(provider_id)
    .bind(&input.name)
    .bind(&input.provider_kind)
    .bind(&input.base_url)
    .bind(&input.default_model)
    .bind(&input.status)
    .bind(input.clear_api_key)
    .bind(
        encrypted
            .as_ref()
            .map(|secret| secret.ciphertext.as_slice()),
    )
    .bind(encrypted.as_ref().map(|secret| secret.nonce.as_slice()))
    .bind(encrypted.as_ref().map(|secret| secret.key_version.as_str()))
    .execute(&mut *transaction)
    .await
    .map_err(ai_write_error)?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Provider,
        provider_id,
        visibility.as_ref(),
        false,
    )
    .await?;
    let profile_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM ai_profiles
        WHERE tenant_id = $1 AND provider_connection_id = $2
        ORDER BY id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(provider_id)
    .fetch_all(&mut *transaction)
    .await?;
    let mut ai_left_events = Vec::new();
    for profile_id in profile_ids {
        ai_left_events.extend(
            leave_profile_participants_without_runtime_access(
                &mut transaction,
                actor.tenant_id,
                profile_id,
                false,
            )
            .await?,
        );
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_provider.updated",
        "ai_provider",
        provider_id,
    )
    .await?;
    transaction.commit().await?;
    publish_realtime_events_best_effort(&state, &ai_left_events).await;
    Ok(Json(
        load_provider(&state, &actor, project_id, provider_id).await?,
    ))
}

async fn delete_provider(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(provider_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_project_wide_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Provider, provider_id)
            .await?;
    let mut transaction = state.db.begin().await?;
    let deleted = sqlx::query(
        "DELETE FROM ai_provider_connections WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(provider_id)
    .execute(&mut *transaction)
    .await
    .map_err(ai_delete_error)?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_provider.deleted",
        "ai_provider",
        provider_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, FromRow)]
#[allow(clippy::struct_excessive_bools)]
struct AiProfileRow {
    execution_ready: bool,
    visibility: sqlx::types::Json<ResourceVisibility>,
    id: Uuid,
    provider_connection_id: Option<Uuid>,
    name: String,
    description: String,
    preset_key: Option<String>,
    avatar_url: Option<String>,
    status: String,
    mode: String,
    model: Option<String>,
    instructions: String,
    tool_instructions: String,
    blacklist_reply_text: String,
    blacklist_reply_match_language: bool,
    http_allowed_hosts: Vec<String>,
    language: String,
    max_output_tokens: i32,
    max_concurrent_runs: i32,
    auto_join_new_conversations: bool,
    can_resolve_conversations: bool,
    capability_http_get: bool,
    capability_http_post: bool,
    capability_shell: bool,
    telegram_notify_on_new_visitor: bool,
    telegram_notify_on_new_message: bool,
    telegram_notify_on_operator_request: bool,
    custom_fields: SqlJson<BTreeMap<String, String>>,
    secret_keys: Vec<String>,
    knowledge_base_ids: Vec<Uuid>,
    channel_ids: Vec<Uuid>,
    public_identities: SqlJson<Vec<AiProfilePublicIdentity>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)] // Independent persisted settings and derived provider availability.
struct AiProfileResponse {
    execution_ready: bool,
    visibility: ResourceVisibility,
    id: Uuid,
    provider_connection_id: Option<Uuid>,
    name: String,
    description: String,
    preset_key: Option<String>,
    avatar_url: Option<String>,
    status: String,
    mode: String,
    model: Option<String>,
    instructions: String,
    tool_instructions: String,
    blacklist_reply_text: String,
    blacklist_reply_match_language: bool,
    http_allowed_hosts: Vec<String>,
    language: String,
    max_output_tokens: i32,
    max_concurrent_runs: i32,
    auto_join_new_conversations: bool,
    can_resolve_conversations: bool,
    capabilities: AiProfileCapabilities,
    telegram_notifications: TelegramNotificationSettings,
    custom_fields: Vec<AiProfileCustomField>,
    secrets: Vec<AiProfileSecretResponse>,
    knowledge_base_ids: Vec<Uuid>,
    channel_ids: Vec<Uuid>,
    public_identities: Vec<AiProfilePublicIdentity>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AiProfileRow> for AiProfileResponse {
    fn from(row: AiProfileRow) -> Self {
        Self {
            execution_ready: row.execution_ready,
            visibility: row.visibility.0,
            id: row.id,
            provider_connection_id: row.provider_connection_id,
            name: row.name,
            description: row.description,
            preset_key: row.preset_key,
            avatar_url: row.avatar_url,
            status: row.status,
            mode: row.mode,
            model: row.model,
            instructions: row.instructions,
            tool_instructions: row.tool_instructions,
            blacklist_reply_text: row.blacklist_reply_text,
            blacklist_reply_match_language: row.blacklist_reply_match_language,
            http_allowed_hosts: row.http_allowed_hosts,
            language: row.language,
            max_output_tokens: row.max_output_tokens,
            max_concurrent_runs: row.max_concurrent_runs,
            auto_join_new_conversations: row.auto_join_new_conversations,
            can_resolve_conversations: row.can_resolve_conversations,
            capabilities: AiProfileCapabilities {
                http_get: row.capability_http_get,
                http_post: row.capability_http_post,
                shell: row.capability_shell,
            },
            telegram_notifications: TelegramNotificationSettings {
                new_visitor: row.telegram_notify_on_new_visitor,
                new_message: row.telegram_notify_on_new_message,
                operator_request: row.telegram_notify_on_operator_request,
            },
            custom_fields: row
                .custom_fields
                .0
                .into_iter()
                .map(|(key, value)| AiProfileCustomField { key, value })
                .collect(),
            secrets: row
                .secret_keys
                .into_iter()
                .map(|key| AiProfileSecretResponse {
                    key,
                    configured: true,
                })
                .collect(),
            knowledge_base_ids: row.knowledge_base_ids,
            channel_ids: row.channel_ids,
            public_identities: row.public_identities.0,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct AiProfileListResponse {
    items: Vec<AiProfileResponse>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AiProfileCustomField {
    key: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct AiProfileSecretResponse {
    key: String,
    configured: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AiProfilePublicIdentity {
    language: String,
    display_name: String,
    avatar_url: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AiProfilePublicIdentityInput {
    language: String,
    display_name: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AiProfileSecretInput {
    key: String,
    value: Option<String>,
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TelegramNotificationSettings {
    #[serde(default)]
    new_visitor: bool,
    #[serde(default)]
    new_message: bool,
    #[serde(default = "default_true")]
    operator_request: bool,
}

impl Default for TelegramNotificationSettings {
    fn default() -> Self {
        Self {
            new_visitor: false,
            new_message: false,
            operator_request: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
struct AiProfileCapabilities {
    http_get: bool,
    http_post: bool,
    shell: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AiProfileRequest {
    visibility: Option<ResourceVisibility>,
    name: String,
    description: Option<String>,
    status: String,
    provider_connection_id: Option<Uuid>,
    model: Option<String>,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    tool_instructions: Option<String>,
    blacklist_reply_text: Option<String>,
    blacklist_reply_match_language: Option<bool>,
    http_allowed_hosts: Option<Vec<String>>,
    language: String,
    max_output_tokens: i32,
    max_concurrent_runs: Option<i32>,
    #[serde(default)]
    auto_join_new_conversations: bool,
    #[serde(default)]
    can_resolve_conversations: bool,
    #[serde(default)]
    capabilities: AiProfileCapabilities,
    #[serde(default)]
    telegram_notifications: TelegramNotificationSettings,
    #[serde(default)]
    custom_fields: Option<Vec<AiProfileCustomField>>,
    #[serde(default)]
    secrets: Option<Vec<AiProfileSecretInput>>,
    #[serde(default)]
    knowledge_base_ids: Vec<Uuid>,
    #[serde(default)]
    channel_ids: Vec<Uuid>,
    public_identities: Vec<AiProfilePublicIdentityInput>,
    #[serde(default)]
    preset_key: Option<String>,
}

async fn list_profiles(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<AiProfileListResponse>, AppError> {
    let project_id = require_management(&actor)?;
    let items = load_profiles(&state, &actor, project_id, None).await?;
    Ok(Json(AiProfileListResponse { items }))
}

async fn create_profile(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<AiProfileRequest>,
) -> Result<(StatusCode, Json<AiProfileResponse>), AppError> {
    let project_id = require_management(&actor)?;
    let visibility = request.visibility.clone();
    let preset_key = normalize_profile_preset_key(request.preset_key.as_deref())?;
    let input = normalize_profile(request)?;
    let profile_id = Uuid::now_v7();
    let custom_fields = input
        .custom_fields
        .clone()
        .unwrap_or_else(|| SqlJson(BTreeMap::new()));
    let mut transaction = state.db.begin().await?;
    validate_profile_references(&mut transaction, &actor, project_id, &input, None).await?;
    sqlx::query(
        r#"
        INSERT INTO ai_profiles (
            id, tenant_id, project_id, provider_connection_id, name, status,
            model, instructions, language, max_output_tokens,
            auto_join_new_conversations, can_resolve_conversations,
            capability_http_get, capability_http_post, capability_shell,
            telegram_notify_on_new_visitor, telegram_notify_on_new_message,
            telegram_notify_on_operator_request, custom_fields, created_by, tool_instructions, http_allowed_hosts,
            blacklist_reply_text, blacklist_reply_match_language, description,
            preset_key, max_concurrent_runs
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25,
            $26, $27
        )
        "#,
    )
    .bind(profile_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(input.provider_connection_id)
    .bind(&input.name)
    .bind(&input.status)
    .bind(&input.model)
    .bind(&input.instructions)
    .bind(&input.language)
    .bind(input.max_output_tokens)
    .bind(input.auto_join_new_conversations)
    .bind(input.can_resolve_conversations)
    .bind(input.capabilities.http_get)
    .bind(input.capabilities.http_post)
    .bind(input.capabilities.shell)
    .bind(input.telegram_notifications.new_visitor)
    .bind(input.telegram_notifications.new_message)
    .bind(input.telegram_notifications.operator_request)
    .bind(custom_fields)
    .bind(actor.actor_id)
    .bind(input.tool_instructions.as_deref().unwrap_or_default())
    .bind(input.http_allowed_hosts.as_deref().unwrap_or_default())
    .bind(input.blacklist_reply_text.as_deref().unwrap_or_default())
    .bind(input.blacklist_reply_match_language.unwrap_or(true))
    .bind(input.description.as_deref().unwrap_or_default())
    .bind(preset_key)
    .bind(input.max_concurrent_runs.unwrap_or(2))
    .execute(&mut *transaction)
    .await
    .map_err(ai_write_error)?;
    let ai_left_events =
        replace_profile_assignments(&mut transaction, actor.tenant_id, profile_id, &input).await?;
    replace_profile_public_identities(
        &mut transaction,
        actor.tenant_id,
        profile_id,
        &input.public_identities,
    )
    .await?;
    replace_profile_secrets(
        &state,
        &mut transaction,
        actor.tenant_id,
        profile_id,
        input.secrets.as_deref(),
    )
    .await?;
    validate_profile_credential_keys(&mut transaction, actor.tenant_id, profile_id).await?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Profile,
        profile_id,
        visibility.as_ref(),
        true,
    )
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.created",
        "ai_profile",
        profile_id,
    )
    .await?;
    transaction.commit().await?;
    publish_realtime_events_best_effort(&state, &ai_left_events).await;
    Ok((
        StatusCode::CREATED,
        Json(load_profile(&state, &actor, project_id, profile_id).await?),
    ))
}

async fn update_profile(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile_id): Path<Uuid>,
    Json(request): Json<AiProfileRequest>,
) -> Result<Json<AiProfileResponse>, AppError> {
    require_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Profile, profile_id)
            .await?;
    if request.preset_key.is_some() {
        return Err(AppError::BadRequest(
            "preset_key can only be set when creating a profile".to_owned(),
        ));
    }
    let visibility = request.visibility.clone();
    let input = normalize_profile(request)?;
    let mut transaction = state.db.begin().await?;
    lock_profile_for_management_scope(&mut transaction, &actor, project_id, profile_id).await?;
    require_project_wide_for_autonomous_profile(&mut transaction, &actor, project_id, profile_id)
        .await?;
    validate_profile_references(
        &mut transaction,
        &actor,
        project_id,
        &input,
        Some(profile_id),
    )
    .await?;
    let updated = sqlx::query(
        r#"
        UPDATE ai_profiles
        SET provider_connection_id = $4, name = $5, status = $6,
            model = $7, instructions = $8, language = $9,
            max_output_tokens = $10,
            auto_join_new_conversations = $11,
            can_resolve_conversations = $12,
            capability_http_get = $13,
            capability_http_post = $14,
            capability_shell = $15,
            telegram_notify_on_new_visitor = $16,
            telegram_notify_on_new_message = $17,
            telegram_notify_on_operator_request = $18,
            custom_fields = COALESCE($19::jsonb, custom_fields),
            tool_instructions = COALESCE($20::text, tool_instructions),
            http_allowed_hosts = COALESCE($21::text[], http_allowed_hosts),
            blacklist_reply_text = COALESCE($22::text, blacklist_reply_text),
            blacklist_reply_match_language = COALESCE($23::boolean, blacklist_reply_match_language),
            description = COALESCE($24::text, description),
            max_concurrent_runs = COALESCE($25, max_concurrent_runs),
            updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(input.provider_connection_id)
    .bind(&input.name)
    .bind(&input.status)
    .bind(&input.model)
    .bind(&input.instructions)
    .bind(&input.language)
    .bind(input.max_output_tokens)
    .bind(input.auto_join_new_conversations)
    .bind(input.can_resolve_conversations)
    .bind(input.capabilities.http_get)
    .bind(input.capabilities.http_post)
    .bind(input.capabilities.shell)
    .bind(input.telegram_notifications.new_visitor)
    .bind(input.telegram_notifications.new_message)
    .bind(input.telegram_notifications.operator_request)
    .bind(input.custom_fields.clone())
    .bind(&input.tool_instructions)
    .bind(&input.http_allowed_hosts)
    .bind(&input.blacklist_reply_text)
    .bind(input.blacklist_reply_match_language)
    .bind(&input.description)
    .bind(input.max_concurrent_runs)
    .execute(&mut *transaction)
    .await
    .map_err(ai_write_error)?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    let ai_left_events =
        replace_profile_assignments(&mut transaction, actor.tenant_id, profile_id, &input).await?;
    replace_profile_public_identities(
        &mut transaction,
        actor.tenant_id,
        profile_id,
        &input.public_identities,
    )
    .await?;
    replace_profile_secrets(
        &state,
        &mut transaction,
        actor.tenant_id,
        profile_id,
        input.secrets.as_deref(),
    )
    .await?;
    validate_profile_credential_keys(&mut transaction, actor.tenant_id, profile_id).await?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Profile,
        profile_id,
        visibility.as_ref(),
        false,
    )
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.updated",
        "ai_profile",
        profile_id,
    )
    .await?;
    transaction.commit().await?;
    publish_realtime_events_best_effort(&state, &ai_left_events).await;
    Ok(Json(
        load_profile(&state, &actor, project_id, profile_id).await?,
    ))
}

async fn upload_profile_avatar(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile_id): Path<Uuid>,
    bytes: Bytes,
) -> Result<Json<AiProfileResponse>, AppError> {
    require_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Profile, profile_id)
            .await?;
    let avatar = ValidatedAvatar::from_bytes(bytes)?;
    let mut transaction = state.db.begin().await?;
    lock_profile_for_management_scope(&mut transaction, &actor, project_id, profile_id).await?;
    let stored = sqlx::query(
        r#"
        INSERT INTO ai_profile_avatars (
            tenant_id, ai_profile_id, public_id, media_type, content,
            sha256, width, height, created_by
        )
        SELECT profile.tenant_id, profile.id, $4, $5, $6, $7, $8, $9, $10
        FROM ai_profiles AS profile
        WHERE profile.tenant_id = $1 AND profile.project_id = $2 AND profile.id = $3
        ON CONFLICT (tenant_id, ai_profile_id) DO UPDATE
        SET public_id = EXCLUDED.public_id,
            media_type = EXCLUDED.media_type,
            content = EXCLUDED.content,
            sha256 = EXCLUDED.sha256,
            width = EXCLUDED.width,
            height = EXCLUDED.height,
            created_by = EXCLUDED.created_by,
            updated_at = now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(avatar.public_id)
    .bind(avatar.media_type)
    .bind(avatar.content)
    .bind(avatar.sha256)
    .bind(avatar.width)
    .bind(avatar.height)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await?;
    if stored.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.avatar_updated",
        "ai_profile",
        profile_id,
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_profile(&state, &actor, project_id, profile_id).await?,
    ))
}

async fn delete_profile_avatar(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile_id): Path<Uuid>,
) -> Result<Json<AiProfileResponse>, AppError> {
    require_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Profile, profile_id)
            .await?;
    let mut transaction = state.db.begin().await?;
    lock_profile_for_management_scope(&mut transaction, &actor, project_id, profile_id).await?;
    sqlx::query("DELETE FROM ai_profile_avatars WHERE tenant_id = $1 AND ai_profile_id = $2")
        .bind(actor.tenant_id)
        .bind(profile_id)
        .execute(&mut *transaction)
        .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.avatar_deleted",
        "ai_profile",
        profile_id,
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_profile(&state, &actor, project_id, profile_id).await?,
    ))
}

async fn upload_profile_public_identity_avatar(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile_id, language)): Path<(Uuid, String)>,
    bytes: Bytes,
) -> Result<Json<AiProfileResponse>, AppError> {
    require_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Profile, profile_id)
            .await?;
    let language = normalize_public_identity_language(&language)?;
    let avatar = ValidatedAvatar::from_bytes(bytes)?;
    let mut transaction = state.db.begin().await?;
    lock_profile_for_management_scope(&mut transaction, &actor, project_id, profile_id).await?;
    let stored = sqlx::query(
        r#"
        INSERT INTO ai_profile_public_identity_avatars (
            tenant_id, ai_profile_id, language, public_id, media_type, content,
            sha256, width, height, created_by
        )
        SELECT identity.tenant_id, identity.ai_profile_id, identity.language,
               $5, $6, $7, $8, $9, $10, $11
        FROM ai_profile_public_identities AS identity
        JOIN ai_profiles AS profile
          ON profile.tenant_id = identity.tenant_id
         AND profile.id = identity.ai_profile_id
        WHERE identity.tenant_id = $1
          AND profile.project_id = $2
          AND identity.ai_profile_id = $3
          AND identity.language = $4
        ON CONFLICT (tenant_id, ai_profile_id, language) DO UPDATE
        SET public_id = EXCLUDED.public_id,
            media_type = EXCLUDED.media_type,
            content = EXCLUDED.content,
            sha256 = EXCLUDED.sha256,
            width = EXCLUDED.width,
            height = EXCLUDED.height,
            created_by = EXCLUDED.created_by,
            updated_at = now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(&language)
    .bind(avatar.public_id)
    .bind(avatar.media_type)
    .bind(avatar.content)
    .bind(avatar.sha256)
    .bind(avatar.width)
    .bind(avatar.height)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await?;
    if stored.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.public_identity_avatar_updated",
        "ai_profile",
        profile_id,
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_profile(&state, &actor, project_id, profile_id).await?,
    ))
}

async fn delete_profile_public_identity_avatar(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((profile_id, language)): Path<(Uuid, String)>,
) -> Result<Json<AiProfileResponse>, AppError> {
    require_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Profile, profile_id)
            .await?;
    let language = normalize_public_identity_language(&language)?;
    let mut transaction = state.db.begin().await?;
    lock_profile_for_management_scope(&mut transaction, &actor, project_id, profile_id).await?;
    let identity_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM ai_profile_public_identities AS identity
            JOIN ai_profiles AS profile
              ON profile.tenant_id = identity.tenant_id
             AND profile.id = identity.ai_profile_id
            WHERE identity.tenant_id = $1
              AND profile.project_id = $2
              AND identity.ai_profile_id = $3
              AND identity.language = $4
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .bind(&language)
    .fetch_one(&mut *transaction)
    .await?;
    if !identity_exists {
        return Err(AppError::NotFound);
    }
    sqlx::query(
        r#"
        DELETE FROM ai_profile_public_identity_avatars
        WHERE tenant_id = $1 AND ai_profile_id = $2 AND language = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(profile_id)
    .bind(&language)
    .execute(&mut *transaction)
    .await?;
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.public_identity_avatar_deleted",
        "ai_profile",
        profile_id,
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_profile(&state, &actor, project_id, profile_id).await?,
    ))
}

async fn delete_profile(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(profile_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_management(&actor)?;
    let project_id =
        resource_visibility::owner_project(&state.db, &actor, Resource::Profile, profile_id)
            .await?;
    let mut transaction = state.db.begin().await?;
    lock_profile_for_management_scope(&mut transaction, &actor, project_id, profile_id).await?;
    require_project_wide_for_autonomous_profile(&mut transaction, &actor, project_id, profile_id)
        .await?;
    let assigned_to_position: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM company_positions WHERE tenant_id=$1 AND project_id=$2 AND ai_profile_id=$3)",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .fetch_one(&mut *transaction)
    .await?;
    if assigned_to_position {
        return Err(AppError::Conflict(
            "AI profile cannot be deleted while assigned to a company position".to_owned(),
        ));
    }
    let assigned_to_task = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM ai_task_agents AS assignment
            JOIN ai_tasks AS task
              ON task.tenant_id = assignment.tenant_id
             AND task.id = assignment.task_id
            WHERE assignment.tenant_id = $1
              AND assignment.ai_profile_id = $2
              AND task.project_id = $3
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(profile_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await?;
    if assigned_to_task {
        return Err(AppError::BadRequest(
            "AI profile cannot be deleted while it is assigned to a scheduled task".to_owned(),
        ));
    }
    let ai_left_events = leave_profile_participants_without_runtime_access(
        &mut transaction,
        actor.tenant_id,
        profile_id,
        true,
    )
    .await?;
    let deleted =
        sqlx::query("DELETE FROM ai_profiles WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
            .bind(actor.tenant_id)
            .bind(project_id)
            .bind(profile_id)
            .execute(&mut *transaction)
            .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    insert_audit(
        &mut transaction,
        &actor,
        project_id,
        "ai_profile.deleted",
        "ai_profile",
        profile_id,
    )
    .await?;
    transaction.commit().await?;
    publish_realtime_events_best_effort(&state, &ai_left_events).await;
    Ok(StatusCode::NO_CONTENT)
}

struct NormalizedProvider {
    name: String,
    provider_kind: String,
    model_type: Option<String>,
    base_url: String,
    default_model: String,
    status: String,
    api_key: Option<String>,
    clear_api_key: bool,
}

fn normalize_provider(request: AiProviderRequest) -> Result<NormalizedProvider, AppError> {
    let api_key = request
        .api_key
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if request.clear_api_key && api_key.is_some() {
        return Err(AppError::BadRequest(
            "api_key and clear_api_key cannot be used together".to_owned(),
        ));
    }
    if api_key
        .as_ref()
        .is_some_and(|value| value.chars().count() > 4_096)
    {
        return Err(AppError::BadRequest(
            "API key must not exceed 4096 characters".to_owned(),
        ));
    }
    let provider_kind = validate_value(
        request.provider_kind,
        &["openai", "anthropic", "openai_compatible"],
        "provider kind",
    )?;
    let default_model = normalize_required_text(request.default_model, 200, "default model")?;
    let model_type = request
        .model_type
        .map(|value| validate_value(value, &MODEL_TYPES, "model type"))
        .transpose()?;
    Ok(NormalizedProvider {
        name: normalize_required_text(request.name, 200, "connection name")?,
        provider_kind,
        model_type,
        base_url: normalize_provider_url(request.base_url)?,
        default_model,
        status: validate_value(request.status, &["active", "disabled"], "status")?,
        api_key,
        clear_api_key: request.clear_api_key,
    })
}

fn normalize_provider_url(value: String) -> Result<String, AppError> {
    let mut parsed = Url::parse(value.trim())
        .map_err(|_| AppError::BadRequest("provider base URL must be a valid URL".to_owned()))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(AppError::BadRequest(
            "provider base URL must use http or https and contain no credentials, query, or fragment"
                .to_owned(),
        ));
    }
    let normalized_path = parsed.path().trim_end_matches('/').to_owned();
    parsed.set_path(&normalized_path);
    Ok(parsed.to_string().trim_end_matches('/').to_owned())
}

fn validate_provider_api_key_destination(
    current_base_url: &str,
    api_key_configured: bool,
    input: &NormalizedProvider,
) -> Result<(), AppError> {
    if api_key_configured
        && input.api_key.is_none()
        && !input.clear_api_key
        && normalize_provider_url(current_base_url.to_owned())? != input.base_url
    {
        return Err(AppError::BadRequest(
            "changing a provider base URL requires a new API key or clear_api_key".to_owned(),
        ));
    }
    Ok(())
}

struct NormalizedProfile {
    name: String,
    description: Option<String>,
    status: String,
    provider_connection_id: Option<Uuid>,
    model: Option<String>,
    instructions: String,
    tool_instructions: Option<String>,
    blacklist_reply_text: Option<String>,
    blacklist_reply_match_language: Option<bool>,
    http_allowed_hosts: Option<Vec<String>>,
    language: String,
    max_output_tokens: i32,
    max_concurrent_runs: Option<i32>,
    auto_join_new_conversations: bool,
    can_resolve_conversations: bool,
    capabilities: AiProfileCapabilities,
    telegram_notifications: TelegramNotificationSettings,
    custom_fields: Option<SqlJson<BTreeMap<String, String>>>,
    secrets: Option<Vec<NormalizedProfileSecret>>,
    knowledge_base_ids: Vec<Uuid>,
    channel_ids: Vec<Uuid>,
    public_identities: Vec<NormalizedProfilePublicIdentity>,
}

struct NormalizedProfileSecret {
    key: String,
    value: Option<String>,
}

struct NormalizedProfilePublicIdentity {
    language: String,
    display_name: String,
}

fn normalize_http_allowed_hosts(values: Vec<String>) -> Result<Vec<String>, AppError> {
    if values.len() > 64 {
        return Err(AppError::BadRequest(
            "http_allowed_hosts must contain at most 64 domains".to_owned(),
        ));
    }
    let mut hosts = Vec::new();
    for value in values {
        let host = value.trim().to_ascii_lowercase();
        let valid = host == "*"
            || (!host.is_empty()
                && host.len() <= 253
                && host.contains('.')
                && !host.ends_with('.')
                && host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
                && matches!(url::Host::parse(&host), Ok(url::Host::Domain(ref domain)) if domain == &host));
        if !valid {
            return Err(AppError::BadRequest("http_allowed_hosts must contain domain names without URLs, paths, ports, or IP addresses".to_owned()));
        }
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    if hosts.iter().any(|host| host == "*") {
        return Ok(vec!["*".to_owned()]);
    }
    Ok(hosts)
}

fn normalize_profile_preset_key(value: Option<&str>) -> Result<Option<&'static str>, AppError> {
    value
        .map(|value| {
            PROFILE_PRESET_KEYS
                .iter()
                .copied()
                .find(|key| *key == value)
                .ok_or_else(|| AppError::BadRequest("unknown preset_key".to_owned()))
        })
        .transpose()
}

fn normalize_profile(request: AiProfileRequest) -> Result<NormalizedProfile, AppError> {
    if request
        .max_concurrent_runs
        .is_some_and(|limit| !(1..=16).contains(&limit))
    {
        return Err(AppError::BadRequest(
            "max_concurrent_runs must be between 1 and 16".to_owned(),
        ));
    }
    if !(64..=32_000).contains(&request.max_output_tokens) {
        return Err(AppError::BadRequest(
            "max_output_tokens must be between 64 and 32000".to_owned(),
        ));
    }
    let status = validate_value(request.status, &["draft", "active", "disabled"], "status")?;
    let instructions = normalize_optional_text(request.instructions, 50_000, "instructions")?;
    if status == "active" && (request.provider_connection_id.is_none() || instructions.is_empty()) {
        return Err(AppError::BadRequest(
            "an active profile requires a provider connection and instructions".to_owned(),
        ));
    }
    let tool_instructions = request
        .tool_instructions
        .map(|value| normalize_optional_text(value, 50_000, "tool_instructions"))
        .transpose()?;
    let blacklist_reply_text = request
        .blacklist_reply_text
        .map(|value| normalize_optional_text(value, 4_000, "blacklist_reply_text"))
        .transpose()?;
    let custom_fields = normalize_profile_custom_fields(request.custom_fields)?;
    let secrets = normalize_profile_secrets(request.secrets)?;
    if let (Some(custom_fields), Some(secrets)) = (&custom_fields, &secrets) {
        let field_keys = custom_fields
            .0
            .keys()
            .map(|key| key.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        if secrets
            .iter()
            .any(|secret| field_keys.contains(&secret.key.to_ascii_lowercase()))
        {
            return Err(AppError::BadRequest(
                "custom field and secret keys must be distinct".to_owned(),
            ));
        }
    }
    let language = normalize_profile_languages(request.language)?;
    let public_identities =
        normalize_profile_public_identities(request.public_identities, &language)?;
    Ok(NormalizedProfile {
        name: normalize_required_text(request.name, 200, "profile name")?,
        description: request
            .description
            .map(|value| normalize_optional_text(value, 1000, "description"))
            .transpose()?,
        status,
        provider_connection_id: request.provider_connection_id,
        model: normalize_optional_value(request.model, 200, "model")?,
        instructions,
        tool_instructions,
        blacklist_reply_text,
        blacklist_reply_match_language: request.blacklist_reply_match_language,
        http_allowed_hosts: request
            .http_allowed_hosts
            .map(normalize_http_allowed_hosts)
            .transpose()?,
        language,
        max_output_tokens: request.max_output_tokens,
        max_concurrent_runs: request.max_concurrent_runs,
        auto_join_new_conversations: request.auto_join_new_conversations,
        can_resolve_conversations: request.can_resolve_conversations,
        capabilities: request.capabilities,
        telegram_notifications: request.telegram_notifications,
        custom_fields,
        secrets,
        knowledge_base_ids: unique_ids(request.knowledge_base_ids),
        channel_ids: unique_ids(request.channel_ids),
        public_identities,
    })
}

fn normalize_profile_public_identities(
    values: Vec<AiProfilePublicIdentityInput>,
    profile_languages: &str,
) -> Result<Vec<NormalizedProfilePublicIdentity>, AppError> {
    if values.len() > MAX_PROFILE_PUBLIC_IDENTITIES {
        return Err(AppError::BadRequest(format!(
            "an AI profile cannot contain more than {MAX_PROFILE_PUBLIC_IDENTITIES} public identities"
        )));
    }
    let supported_languages = profile_languages
        .split(',')
        .map(|language| language.trim().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut normalized = Vec::with_capacity(values.len());
    let mut seen = HashSet::new();
    for identity in values {
        let language = normalize_public_identity_language(&identity.language)?;
        if !supported_languages.contains(&language) {
            return Err(AppError::BadRequest(
                "public identity languages must be included in the profile reply languages"
                    .to_owned(),
            ));
        }
        if !seen.insert(language.clone()) {
            return Err(AppError::BadRequest(
                "public identity languages must be unique".to_owned(),
            ));
        }
        normalized.push(NormalizedProfilePublicIdentity {
            language,
            display_name: normalize_required_text(
                identity.display_name,
                200,
                "public display name",
            )?,
        });
    }
    if seen != supported_languages {
        return Err(AppError::BadRequest(
            "public identities must define one client-facing name for every profile reply language"
                .to_owned(),
        ));
    }
    Ok(normalized)
}

fn normalize_public_identity_language(value: &str) -> Result<String, AppError> {
    let language = value.trim();
    if !is_valid_language_tag(language) {
        return Err(AppError::BadRequest(
            "public identity language must be a valid language tag".to_owned(),
        ));
    }
    Ok(language.to_ascii_lowercase())
}

fn normalize_profile_custom_fields(
    values: Option<Vec<AiProfileCustomField>>,
) -> Result<Option<SqlJson<BTreeMap<String, String>>>, AppError> {
    let Some(values) = values else {
        return Ok(None);
    };
    if values.len() > MAX_PROFILE_CUSTOM_FIELDS {
        return Err(AppError::BadRequest(format!(
            "an AI profile cannot contain more than {MAX_PROFILE_CUSTOM_FIELDS} custom fields"
        )));
    }
    let mut normalized = BTreeMap::new();
    let mut normalized_keys = HashSet::new();
    for field in values {
        let key = normalize_profile_credential_key(field.key)?;
        if field.value.chars().count() > 4_096 || field.value.chars().any(char::is_control) {
            return Err(AppError::BadRequest(
                "custom field values must not exceed 4096 printable characters".to_owned(),
            ));
        }
        if !normalized_keys.insert(key.to_ascii_lowercase()) {
            return Err(AppError::BadRequest(
                "custom field keys must be unique ignoring case".to_owned(),
            ));
        }
        normalized.insert(key, field.value);
    }
    Ok(Some(SqlJson(normalized)))
}

fn normalize_profile_secrets(
    values: Option<Vec<AiProfileSecretInput>>,
) -> Result<Option<Vec<NormalizedProfileSecret>>, AppError> {
    let Some(values) = values else {
        return Ok(None);
    };
    if values.len() > MAX_PROFILE_SECRETS {
        return Err(AppError::BadRequest(format!(
            "an AI profile cannot contain more than {MAX_PROFILE_SECRETS} secrets"
        )));
    }
    let mut normalized = Vec::with_capacity(values.len());
    let mut normalized_keys = HashSet::new();
    for secret in values {
        let key = normalize_profile_credential_key(secret.key)?;
        if !normalized_keys.insert(key.to_ascii_lowercase()) {
            return Err(AppError::BadRequest(
                "secret keys must be unique ignoring case".to_owned(),
            ));
        }
        if secret
            .value
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.chars().count() > 8_192)
        {
            return Err(AppError::BadRequest(
                "secret values must contain between 1 and 8192 characters".to_owned(),
            ));
        }
        normalized.push(NormalizedProfileSecret {
            key,
            value: secret.value,
        });
    }
    Ok(Some(normalized))
}

fn normalize_profile_credential_key(value: String) -> Result<String, AppError> {
    let key = value.trim();
    let mut characters = key.chars();
    let valid_first = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
    let valid_rest = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'));
    if key.chars().count() > 80 || !valid_first || !valid_rest {
        return Err(AppError::BadRequest(
            "credential keys must use 1-80 ASCII letters, numbers, dots, dashes, or underscores"
                .to_owned(),
        ));
    }
    Ok(key.to_owned())
}

fn unique_ids(values: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(*value))
        .collect()
}

async fn validate_profile_references(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    input: &NormalizedProfile,
    existing_profile: Option<Uuid>,
) -> Result<(), AppError> {
    validate_scoped_profile_channel_selection(
        if actor.is_password_session() {
            None
        } else {
            actor.inbox_scope()
        },
        &input.channel_ids,
    )?;

    if let Some(provider_id) = input.provider_connection_id {
        let provider_status = sqlx::query_scalar::<_, String>(
            r#"
            SELECT provider.status
            FROM ai_provider_connections AS provider
            WHERE provider.tenant_id = $1 AND provider.id = $3
              AND provider.model_type = 'chat'
              AND (resource_visible(provider.visibility, $2, $4) OR EXISTS (
                  SELECT 1 FROM ai_profiles AS profile
                  WHERE profile.tenant_id = provider.tenant_id AND profile.id = $5
                    AND profile.provider_connection_id = provider.id
              ))
              AND ($6::uuid IS NULL OR provider.project_id = $6)
            FOR SHARE
            "#,
        )
        .bind(actor.tenant_id)
        .bind(actor.project_id)
        .bind(provider_id)
        .bind(actor.department_id())
        .bind(existing_profile)
        .bind(resource_visibility::owner_limit(actor))
        .fetch_optional(&mut **transaction)
        .await?;
        let Some(provider_status) = provider_status else {
            return Err(AppError::BadRequest(
                "provider connection must be available in the current workspace and be enabled for an active profile"
                    .to_owned(),
            ));
        };
        if input.status == "active" && provider_status != "active" {
            return Err(AppError::BadRequest(
                "provider connection must be available in the current workspace and be enabled for an active profile"
                    .to_owned(),
            ));
        }
    }

    let knowledge_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM knowledge_bases base
        WHERE tenant_id = $1 AND base.project_id = $6
          AND (resource_visible(visibility,$2,$4) OR EXISTS (
          SELECT 1 FROM ai_profile_knowledge_bases assignment WHERE assignment.tenant_id=base.tenant_id
            AND assignment.knowledge_base_id=base.id AND assignment.ai_profile_id=$5)) AND status = 'active'
          AND id = ANY($3)
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .bind(&input.knowledge_base_ids)
    .bind(actor.department_id())
    .bind(existing_profile)
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    let channels = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT connection.id, connection.inbox_id
        FROM channel_connections AS connection
        JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
        WHERE connection.tenant_id = $1
          AND connection.project_id = $2
          AND connection.deleted_at IS NULL
          AND connection.kind <> 'custom_ai'
          AND connection.id = ANY($3)
          AND inbox.status = 'active'
        FOR SHARE OF connection, inbox
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(&input.channel_ids)
    .fetch_all(&mut **transaction)
    .await?;
    let expected_knowledge = i64::try_from(input.knowledge_base_ids.len())
        .map_err(|_| AppError::BadRequest("too many knowledge bases".to_owned()))?;
    if knowledge_count != expected_knowledge || channels.len() != input.channel_ids.len() {
        return Err(AppError::BadRequest(
            "profile assignments must belong to the active project".to_owned(),
        ));
    }
    for (channel_id, inbox_id) in channels {
        if actor.require_inbox(project_id, inbox_id).is_err() {
            let existing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_profile_channel_connections WHERE tenant_id=$1 AND ai_profile_id=$2 AND channel_connection_id=$3)")
                .bind(actor.tenant_id).bind(existing_profile).bind(channel_id).fetch_one(&mut **transaction).await?;
            if !actor.is_password_session() || !existing {
                return Err(AppError::Forbidden);
            }
        }
    }
    Ok(())
}

pub(crate) async fn require_project_wide_for_autonomous_profile(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    profile_id: Uuid,
) -> Result<(), AppError> {
    if actor.inbox_scope().is_none() {
        return Ok(());
    }
    let has_autonomous_usage = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM ai_task_agents AS assignment
            JOIN ai_tasks AS task
              ON task.tenant_id = assignment.tenant_id
             AND task.id = assignment.task_id
            WHERE assignment.tenant_id = $1
              AND assignment.ai_profile_id = $2
              AND task.project_id = $3
        ) OR EXISTS (
            SELECT 1 FROM ai_api_endpoints
            WHERE tenant_id = $1 AND ai_profile_id = $2 AND project_id = $3
              AND deleted_at IS NULL
        ) OR EXISTS (
            SELECT 1 FROM ai_api_settings
            WHERE tenant_id = $1 AND ai_profile_id = $2 AND project_id = $3
              AND (enabled OR key_id IS NOT NULL)
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(profile_id)
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    crate::ai_tasks::require_project_wide_task_dependency(actor, has_autonomous_usage)
}

fn validate_scoped_profile_channel_selection(
    inbox_scope: Option<&[Uuid]>,
    channel_ids: &[Uuid],
) -> Result<(), AppError> {
    if inbox_scope.is_some() && channel_ids.is_empty() {
        return Err(AppError::BadRequest(
            "inbox-scoped AI profiles must have at least one assigned channel".to_owned(),
        ));
    }
    Ok(())
}

async fn replace_profile_assignments(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    profile_id: Uuid,
    input: &NormalizedProfile,
) -> Result<Vec<RealtimeEvent>, AppError> {
    sqlx::query(
        "DELETE FROM ai_profile_knowledge_bases WHERE tenant_id = $1 AND ai_profile_id = $2",
    )
    .bind(tenant_id)
    .bind(profile_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO ai_profile_knowledge_bases (
            tenant_id, ai_profile_id, knowledge_base_id
        )
        SELECT $1, $2, value FROM unnest($3::uuid[]) AS value
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .bind(&input.knowledge_base_ids)
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM ai_profile_channel_connections
        WHERE tenant_id = $1 AND ai_profile_id = $2
          AND channel_connection_id <> ALL($3::uuid[])
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .bind(&input.channel_ids)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO ai_profile_channel_connections (
            tenant_id, ai_profile_id, channel_connection_id
        )
        SELECT $1, $2, value FROM unnest($3::uuid[]) AS value
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .bind(&input.channel_ids)
    .execute(&mut **transaction)
    .await?;

    sqlx::query("DELETE FROM inbox_ai_profiles WHERE tenant_id = $1 AND ai_profile_id = $2")
        .bind(tenant_id)
        .bind(profile_id)
        .execute(&mut **transaction)
        .await?;
    leave_profile_participants_without_runtime_access(
        transaction,
        tenant_id,
        profile_id,
        input.status != "active",
    )
    .await
}

pub(crate) async fn leave_profile_participants_without_runtime_access(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    profile_id: Uuid,
    force: bool,
) -> Result<Vec<RealtimeEvent>, AppError> {
    let occurred_at = Utc::now();
    let conversations = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid)>(
        r#"
        WITH left_participants AS (
            UPDATE conversation_participants AS participant
            SET left_at = $4
            FROM conversations AS conversation
            WHERE participant.tenant_id = $1
              AND participant.ai_profile_id = $2
              AND participant.participant_kind = 'ai'
              AND participant.left_at IS NULL
              AND conversation.tenant_id = participant.tenant_id
              AND conversation.id = participant.conversation_id
              AND (
                  $3
                  OR NOT EXISTS (
                      SELECT 1
                      FROM ai_profile_channel_connections AS assignment
                      JOIN channel_connections AS connection
                        ON connection.tenant_id = assignment.tenant_id
                       AND connection.id = assignment.channel_connection_id
                      JOIN inboxes AS inbox
                        ON inbox.tenant_id = connection.tenant_id
                       AND inbox.project_id = connection.project_id
                       AND inbox.id = connection.inbox_id
                      JOIN ai_profiles AS profile
                        ON profile.tenant_id = assignment.tenant_id
                       AND profile.id = assignment.ai_profile_id
                       AND profile.project_id = connection.project_id
                      JOIN ai_provider_connections AS provider
                        ON provider.tenant_id = profile.tenant_id
                       AND provider.id = profile.provider_connection_id
                      WHERE assignment.tenant_id = participant.tenant_id
                        AND assignment.ai_profile_id = participant.ai_profile_id
                        AND assignment.channel_connection_id = conversation.channel_connection_id
                        AND connection.inbox_id = conversation.inbox_id
                        AND connection.status = 'active'
                        AND connection.deleted_at IS NULL
                        AND inbox.status = 'active'
                        AND profile.status = 'active'
                        AND provider.status = 'active'
                        AND provider.provider_kind IN ('openai', 'openai_compatible')
                        AND conversation.widget_language IS NOT NULL
                        AND EXISTS (
                            SELECT 1
                            FROM unnest(string_to_array(profile.language, ','))
                                AS configured_language(value)
                            WHERE lower(btrim(configured_language.value))
                                    = lower(replace(conversation.widget_language, '_', '-'))
                               OR (
                                   split_part(
                                       lower(btrim(configured_language.value)), '-', 1
                                   ) = split_part(
                                       lower(replace(conversation.widget_language, '_', '-')),
                                       '-', 1
                                   )
                                   AND (
                                       position('-' IN btrim(configured_language.value)) = 0
                                       OR position(
                                           '-' IN lower(replace(
                                               conversation.widget_language, '_', '-'
                                           ))
                                       ) = 0
                                   )
                               )
                        )
                  )
              )
            RETURNING participant.conversation_id
        )
        SELECT conversation.id, conversation.project_id,
               conversation.inbox_id, conversation.contact_id
        FROM left_participants
        JOIN conversations AS conversation
          ON conversation.tenant_id = $1
         AND conversation.id = left_participants.conversation_id
        ORDER BY conversation.id
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .bind(force)
    .bind(occurred_at)
    .fetch_all(&mut **transaction)
    .await?;

    if conversations.is_empty() {
        return Ok(Vec::new());
    }

    let conversation_ids = conversations
        .iter()
        .map(|(conversation_id, _, _, _)| *conversation_id)
        .collect::<Vec<_>>();
    sqlx::query(
        r#"
        UPDATE outbox_events
        SET status = 'completed', completed_at = $3,
            locked_at = NULL, locked_by = NULL,
            last_error = 'cancelled because AI channel access was revoked'
        WHERE tenant_id = $1
          AND aggregate_type = 'provider_reply'
          AND aggregate_id = ANY($2::uuid[])
          AND status = 'pending'
        "#,
    )
    .bind(tenant_id)
    .bind(&conversation_ids)
    .bind(occurred_at)
    .execute(&mut **transaction)
    .await?;

    let events = conversations
        .into_iter()
        .map(
            |(conversation_id, project_id, inbox_id, contact_id)| RealtimeEvent {
                event_id: Uuid::now_v7(),
                tenant_id,
                project_id,
                inbox_id,
                contact_id: Some(contact_id),
                event_type: "conversation.ai_left".to_owned(),
                aggregate_id: conversation_id,
                sequence: None,
                occurred_at,
                data: json!({
                    "conversation_id": conversation_id,
                    "ai_profile_id": profile_id,
                    "reason": "profile_channel_access_revoked",
                }),
            },
        )
        .collect::<Vec<_>>();
    for event in &events {
        insert_realtime_outbox(transaction, event).await?;
    }
    Ok(events)
}

async fn insert_realtime_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    event: &RealtimeEvent,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload,
            status, available_at, created_at
        ) VALUES ($1, $2, 'realtime', $3, $4, $5, 'pending', now(), now())
        "#,
    )
    .bind(event.event_id)
    .bind(event.tenant_id)
    .bind(event.aggregate_id)
    .bind(&event.event_type)
    .bind(serde_json::to_value(event).map_err(AppError::internal)?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn publish_realtime_events_best_effort(
    state: &AppState,
    events: &[RealtimeEvent],
) {
    for event in events {
        if let Err(error) = state.publish(event).await {
            warn!(?error, event_id = %event.event_id, "realtime publish deferred to outbox");
        }
    }
}

async fn replace_profile_public_identities(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    profile_id: Uuid,
    identities: &[NormalizedProfilePublicIdentity],
) -> Result<(), AppError> {
    let languages = identities
        .iter()
        .map(|identity| identity.language.clone())
        .collect::<Vec<_>>();
    let display_names = identities
        .iter()
        .map(|identity| identity.display_name.clone())
        .collect::<Vec<_>>();
    sqlx::query(
        r#"
        INSERT INTO ai_profile_public_identities (
            tenant_id, ai_profile_id, language, display_name
        )
        SELECT $1, $2, identity.language, identity.display_name
        FROM unnest($3::text[], $4::text[]) AS identity(language, display_name)
        ON CONFLICT (tenant_id, ai_profile_id, language) DO UPDATE
        SET display_name = EXCLUDED.display_name,
            updated_at = now()
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .bind(&languages)
    .bind(&display_names)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        DELETE FROM ai_profile_public_identities
        WHERE tenant_id = $1 AND ai_profile_id = $2
          AND language <> ALL($3::text[])
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .bind(&languages)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn replace_profile_secrets(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    profile_id: Uuid,
    secrets: Option<&[NormalizedProfileSecret]>,
) -> Result<(), AppError> {
    let Some(secrets) = secrets else {
        return Ok(());
    };
    let keys = secrets
        .iter()
        .map(|secret| secret.key.clone())
        .collect::<Vec<_>>();
    let preserved_keys = secrets
        .iter()
        .filter(|secret| secret.value.is_none())
        .map(|secret| secret.key.clone())
        .collect::<Vec<_>>();
    if !preserved_keys.is_empty() {
        let existing_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM ai_profile_secrets
            WHERE tenant_id = $1 AND ai_profile_id = $2
              AND secret_key = ANY($3)
            "#,
        )
        .bind(tenant_id)
        .bind(profile_id)
        .bind(&preserved_keys)
        .fetch_one(&mut **transaction)
        .await?;
        let expected_count = i64::try_from(preserved_keys.len())
            .map_err(|_| AppError::BadRequest("too many secrets".to_owned()))?;
        if existing_count != expected_count {
            return Err(AppError::BadRequest(
                "a value is required for every new secret".to_owned(),
            ));
        }
    }

    let new_secret_count = secrets
        .iter()
        .filter(|secret| secret.value.is_some())
        .count();
    let encryption_key = (new_secret_count > 0)
        .then(|| load_secret_encryption_key(state))
        .transpose()?;
    let mut encrypted = Vec::with_capacity(new_secret_count);
    if let Some(encryption_key) = encryption_key {
        for secret in secrets {
            let Some(value) = secret.value.as_ref() else {
                continue;
            };
            let aad = profile_secret_aad(tenant_id, profile_id, &secret.key);
            encrypted.push((
                secret.key.clone(),
                encrypt_secret(
                    &encryption_key,
                    value.as_bytes(),
                    state.config.secrets.key_version.clone(),
                    &aad,
                )?,
            ));
        }
    }

    sqlx::query(
        r#"
        DELETE FROM ai_profile_secrets
        WHERE tenant_id = $1 AND ai_profile_id = $2
          AND NOT (secret_key = ANY($3))
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .bind(&keys)
    .execute(&mut **transaction)
    .await?;

    for (secret_key, secret) in encrypted {
        sqlx::query(
            r#"
            INSERT INTO ai_profile_secrets (
                tenant_id, ai_profile_id, secret_key, encrypted_value, nonce, key_version
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (tenant_id, ai_profile_id, secret_key)
            DO UPDATE SET encrypted_value = EXCLUDED.encrypted_value,
                          nonce = EXCLUDED.nonce,
                          key_version = EXCLUDED.key_version,
                          updated_at = now()
            "#,
        )
        .bind(tenant_id)
        .bind(profile_id)
        .bind(secret_key)
        .bind(secret.ciphertext)
        .bind(secret.nonce.as_slice())
        .bind(secret.key_version)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn validate_profile_credential_keys(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    profile_id: Uuid,
) -> Result<(), AppError> {
    let custom_fields = sqlx::query_scalar::<_, SqlJson<BTreeMap<String, String>>>(
        "SELECT custom_fields FROM ai_profiles WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(profile_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let secret_keys = sqlx::query_scalar::<_, Vec<String>>(
        r#"
        SELECT COALESCE(array_agg(secret_key ORDER BY secret_key), ARRAY[]::text[])
        FROM ai_profile_secrets
        WHERE tenant_id = $1 AND ai_profile_id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(profile_id)
    .fetch_one(&mut **transaction)
    .await?;
    let mut seen = HashSet::new();
    let unique = custom_fields
        .0
        .keys()
        .chain(secret_keys.iter())
        .all(|key| seen.insert(key.to_ascii_lowercase()));
    if !unique {
        return Err(AppError::BadRequest(
            "custom field and secret keys must be unique ignoring case".to_owned(),
        ));
    }
    Ok(())
}

async fn load_providers(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<Vec<AiProviderResponse>, AppError> {
    sqlx::query_as::<_, AiProviderResponse>(
        r#"
        SELECT visibility, id, name, provider_kind, model_type, base_url, default_model, status,
               encrypted_api_key IS NOT NULL AS api_key_configured,
               created_at, updated_at
        FROM ai_provider_connections
        WHERE tenant_id = $1 AND resource_visible(visibility, $2, $3)
          AND ($4::uuid IS NULL OR project_id = $4)
        ORDER BY name, id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.department_id())
    .bind(resource_visibility::owner_limit(actor))
    .fetch_all(&state.db)
    .await
    .map_err(AppError::Database)
}

async fn load_provider(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    provider_id: Uuid,
) -> Result<AiProviderResponse, AppError> {
    // The caller already authorized this resource; visibility may change on save.
    sqlx::query_as::<_, AiProviderResponse>(
        r#"
        SELECT visibility, id, name, provider_kind, model_type, base_url, default_model, status,
               encrypted_api_key IS NOT NULL AS api_key_configured,
               created_at, updated_at
        FROM ai_provider_connections
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(provider_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn load_profiles(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    selected_id: Option<Uuid>,
) -> Result<Vec<AiProfileResponse>, AppError> {
    let rows = sqlx::query_as::<_, AiProfileRow>(
        r#"
        SELECT (profile.status='active' AND EXISTS(SELECT 1 FROM ai_provider_connections provider
                 WHERE provider.tenant_id=profile.tenant_id
                   AND provider.id=profile.provider_connection_id AND provider.status='active'
                   AND provider.provider_kind IN ('openai','openai_compatible'))) AS execution_ready,
               profile.visibility, profile.id, profile.provider_connection_id, profile.name, profile.description, profile.preset_key,
               (
                   SELECT '/public/v1/avatars/' || avatar.public_id::text
                   FROM ai_profile_avatars AS avatar
                   WHERE avatar.tenant_id = profile.tenant_id
                     AND avatar.ai_profile_id = profile.id
               ) AS avatar_url,
               profile.status, profile.mode, profile.model, profile.instructions, profile.tool_instructions, profile.http_allowed_hosts,
               profile.blacklist_reply_text, profile.blacklist_reply_match_language,
               profile.language, profile.max_output_tokens, profile.max_concurrent_runs,
               profile.auto_join_new_conversations, profile.can_resolve_conversations,
               profile.capability_http_get, profile.capability_http_post,
               profile.capability_shell,
               profile.telegram_notify_on_new_visitor,
               profile.telegram_notify_on_new_message,
               profile.telegram_notify_on_operator_request,
               profile.custom_fields,
               COALESCE(
                   (
                       SELECT array_agg(secret.secret_key ORDER BY secret.secret_key)
                       FROM ai_profile_secrets AS secret
                       WHERE secret.tenant_id = profile.tenant_id
                         AND secret.ai_profile_id = profile.id
                   ),
                   ARRAY[]::text[]
               ) AS secret_keys,
               COALESCE(
                   (
                       SELECT array_agg(knowledge.knowledge_base_id ORDER BY knowledge.knowledge_base_id)
                       FROM ai_profile_knowledge_bases AS knowledge
                       JOIN knowledge_bases AS base
                         ON base.tenant_id = knowledge.tenant_id
                        AND base.id = knowledge.knowledge_base_id
                        AND base.project_id = profile.project_id
                       WHERE knowledge.tenant_id = profile.tenant_id
                         AND knowledge.ai_profile_id = profile.id
                   ),
                   ARRAY[]::uuid[]
               ) AS knowledge_base_ids,
               COALESCE(
                   (
                       SELECT array_agg(assignment.channel_connection_id ORDER BY assignment.channel_connection_id)
                       FROM ai_profile_channel_connections AS assignment
                       JOIN channel_connections AS connection
                         ON connection.tenant_id = assignment.tenant_id
                        AND connection.id = assignment.channel_connection_id
                       WHERE assignment.tenant_id = profile.tenant_id
                         AND assignment.ai_profile_id = profile.id
                         AND connection.project_id = profile.project_id
                         AND connection.deleted_at IS NULL
                   ),
                   ARRAY[]::uuid[]
               ) AS channel_ids,
               COALESCE(
                   (
                       SELECT jsonb_agg(
                           jsonb_build_object(
                               'language', identity.language,
                               'display_name', identity.display_name,
                               'avatar_url', CASE
                                   WHEN avatar.public_id IS NULL THEN NULL
                                   ELSE '/public/v1/avatars/' || avatar.public_id::text
                               END
                           )
                           ORDER BY identity.language
                       )
                       FROM ai_profile_public_identities AS identity
                       LEFT JOIN ai_profile_public_identity_avatars AS avatar
                         ON avatar.tenant_id = identity.tenant_id
                        AND avatar.ai_profile_id = identity.ai_profile_id
                        AND avatar.language = identity.language
                       WHERE identity.tenant_id = profile.tenant_id
                         AND identity.ai_profile_id = profile.id
                   ),
                   '[]'::jsonb
               ) AS public_identities,
               profile.created_at, profile.updated_at
        FROM ai_profiles AS profile
        WHERE profile.tenant_id = $1
          AND (($6::uuid IS NOT NULL AND profile.id=$6 AND profile.project_id=$2)
            OR ($6 IS NULL AND resource_visible(profile.visibility,$2,$4)
              AND ($5::uuid IS NULL OR profile.project_id=$5)))
          AND (
              $3::uuid[] IS NULL
              OR (
                  SELECT COUNT(*) > 0
                     AND COUNT(*) FILTER (
                             WHERE connection.id IS NOT NULL
                               AND inbox.id IS NOT NULL
                               AND connection.inbox_id = ANY($3::uuid[])
                         ) = COUNT(*)
                  FROM ai_profile_channel_connections AS scoped_assignment
                  LEFT JOIN channel_connections AS connection
                    ON connection.tenant_id = scoped_assignment.tenant_id
                   AND connection.id = scoped_assignment.channel_connection_id
                   AND connection.project_id = profile.project_id
                   AND connection.deleted_at IS NULL
                  LEFT JOIN inboxes AS inbox
                    ON inbox.tenant_id = connection.tenant_id
                   AND inbox.project_id = connection.project_id
                   AND inbox.id = connection.inbox_id
                   AND inbox.status = 'active'
                  WHERE scoped_assignment.tenant_id = profile.tenant_id
                    AND scoped_assignment.ai_profile_id = profile.id
              )
          )
        ORDER BY profile.name, profile.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(if actor.is_password_session() { None } else { actor.inbox_scope() })
    .bind(actor.department_id())
    .bind(resource_visibility::owner_limit(actor))
    .bind(selected_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(AiProfileResponse::from).collect())
}

pub(crate) async fn lock_profile_for_management_scope(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    profile_id: Uuid,
) -> Result<(), AppError> {
    let locked_profile = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM ai_profiles
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(profile_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if locked_profile.is_none() {
        return Err(AppError::NotFound);
    }

    let Some(inbox_scope) = actor
        .inbox_scope()
        .filter(|_| actor.has_restricted_inbox_scope() && !actor.is_password_session())
    else {
        return Ok(());
    };
    let (total_assignments, visible_assignments) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT COUNT(*),
               COUNT(*) FILTER (
                   WHERE connection.id IS NOT NULL
                     AND inbox.id IS NOT NULL
                     AND connection.inbox_id = ANY($4::uuid[])
               )
        FROM ai_profile_channel_connections AS assignment
        LEFT JOIN channel_connections AS connection
          ON connection.tenant_id = assignment.tenant_id
         AND connection.id = assignment.channel_connection_id
         AND connection.project_id = $3
         AND connection.deleted_at IS NULL
        LEFT JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
         AND inbox.status = 'active'
        WHERE assignment.tenant_id = $1
          AND assignment.ai_profile_id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(profile_id)
    .bind(project_id)
    .bind(inbox_scope)
    .fetch_one(&mut **transaction)
    .await?;
    if !has_complete_profile_assignment_scope(total_assignments, visible_assignments) {
        return Err(AppError::NotFound);
    }
    Ok(())
}

fn has_complete_profile_assignment_scope(total_assignments: i64, visible_assignments: i64) -> bool {
    total_assignments > 0 && visible_assignments == total_assignments
}

async fn load_profile(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    profile_id: Uuid,
) -> Result<AiProfileResponse, AppError> {
    load_profiles(state, actor, project_id, Some(profile_id))
        .await?
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or(AppError::NotFound)
}

pub(crate) struct EncryptedSecret {
    pub(crate) ciphertext: Vec<u8>,
    pub(crate) nonce: [u8; 12],
    pub(crate) key_version: String,
}

fn encrypt_optional_api_key(
    state: &AppState,
    api_key: Option<&str>,
) -> Result<Option<EncryptedSecret>, AppError> {
    api_key
        .map(|value| {
            let key = load_secret_encryption_key(state)?;
            encrypt_secret(
                &key,
                value.as_bytes(),
                state.config.secrets.key_version.clone(),
                PROVIDER_SECRET_AAD,
            )
        })
        .transpose()
}

pub(crate) fn decrypt_provider_api_key(
    state: &AppState,
    ciphertext: Option<&[u8]>,
    nonce: Option<&[u8]>,
    key_version: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let (Some(ciphertext), Some(nonce), Some(key_version)) = (ciphertext, nonce, key_version)
    else {
        if ciphertext.is_none() && nonce.is_none() && key_version.is_none() {
            return Ok(None);
        }
        anyhow::bail!("provider API key encryption metadata is incomplete");
    };
    if key_version != state.config.secrets.key_version {
        anyhow::bail!("provider API key uses an unsupported encryption key version");
    }
    let key = load_secret_encryption_key(state)?;
    let plaintext = decrypt_secret(&key, ciphertext, nonce, PROVIDER_SECRET_AAD)?;
    String::from_utf8(plaintext)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("provider API key is not valid UTF-8"))
}

pub(crate) fn decrypt_profile_secret(
    state: &AppState,
    tenant_id: Uuid,
    profile_id: Uuid,
    secret_key: Option<&str>,
    ciphertext: Option<&[u8]>,
    nonce: Option<&[u8]>,
    key_version: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let (Some(secret_key), Some(ciphertext), Some(nonce), Some(key_version)) =
        (secret_key, ciphertext, nonce, key_version)
    else {
        if secret_key.is_none() && ciphertext.is_none() && nonce.is_none() && key_version.is_none()
        {
            return Ok(None);
        }
        anyhow::bail!("profile secret encryption metadata is incomplete");
    };
    if key_version != state.config.secrets.key_version {
        anyhow::bail!("profile secret uses an unsupported encryption key version");
    }
    let key = load_secret_encryption_key(state)?;
    let aad = profile_secret_aad(tenant_id, profile_id, secret_key);
    let plaintext = decrypt_secret(&key, ciphertext, nonce, &aad)?;
    String::from_utf8(plaintext)
        .map(Some)
        .map_err(|_| anyhow::anyhow!("profile secret is not valid UTF-8"))
}

pub(crate) fn load_secret_encryption_key(state: &AppState) -> Result<[u8; 32], AppError> {
    let encoded_key = std::env::var(&state.config.secrets.encryption_key_env)
        .map_err(|error| AppError::internal(anyhow::Error::new(error)))?;
    let bytes = STANDARD
        .decode(encoded_key.trim())
        .map_err(AppError::internal)?;
    bytes.try_into().map_err(|_| {
        AppError::internal(anyhow::anyhow!(
            "the configured secret encryption key must contain exactly 32 bytes"
        ))
    })
}

fn profile_secret_aad(tenant_id: Uuid, profile_id: Uuid, secret_key: &str) -> Vec<u8> {
    format!("tzomet:ai-profile-secret:v1:{tenant_id}:{profile_id}:{secret_key}").into_bytes()
}

pub(crate) fn encrypt_secret(
    key: &[u8; 32],
    plaintext: &[u8],
    key_version: String,
    aad: &[u8],
) -> Result<EncryptedSecret, AppError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(AppError::internal)?;
    let mut nonce = [0_u8; 12];
    rand::rng().fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| AppError::internal(anyhow::anyhow!("could not encrypt secret")))?;
    Ok(EncryptedSecret {
        ciphertext,
        nonce,
        key_version,
    })
}

pub(crate) fn decrypt_secret(
    key: &[u8; 32],
    ciphertext: &[u8],
    nonce: &[u8],
    aad: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let nonce: [u8; 12] = nonce
        .try_into()
        .map_err(|_| anyhow::anyhow!("encrypted secret nonce must contain exactly 12 bytes"))?;
    let cipher = Aes256Gcm::new_from_slice(key)?;
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("could not decrypt secret"))
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

fn normalize_optional_text(
    value: String,
    max_chars: usize,
    field: &str,
) -> Result<String, AppError> {
    let value = value.trim();
    if value.chars().count() > max_chars {
        return Err(AppError::BadRequest(format!(
            "{field} must not exceed {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_optional_value(
    value: Option<String>,
    max_chars: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    value
        .map(|value| normalize_required_text(value, max_chars, field))
        .transpose()
}

fn normalize_profile_languages(value: String) -> Result<String, AppError> {
    let mut languages = Vec::new();
    let mut seen = HashSet::new();

    for language in value.split(',').map(str::trim) {
        if !is_valid_language_tag(language) {
            return Err(AppError::BadRequest(
                "languages must be a comma-separated list of valid language tags".to_owned(),
            ));
        }
        if seen.insert(language.to_ascii_lowercase()) {
            languages.push(language);
        }
    }

    let normalized = languages.join(", ");
    if normalized.chars().count() > MAX_PROFILE_LANGUAGE_LIST_CHARS {
        return Err(AppError::BadRequest(format!(
            "languages must not exceed {MAX_PROFILE_LANGUAGE_LIST_CHARS} characters"
        )));
    }
    Ok(normalized)
}

fn is_valid_language_tag(value: &str) -> bool {
    (2..=35).contains(&value.len())
        && value.split('-').all(|part| {
            (1..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

fn validate_value(value: String, allowed: &[&str], field: &str) -> Result<String, AppError> {
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(AppError::BadRequest(format!("unsupported {field}")))
    }
}

pub(crate) fn require_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    actor.require("ai:manage")?;
    actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))
}

fn require_project_wide_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    let project_id = require_management(actor)?;
    if actor.has_restricted_inbox_scope() {
        return Err(AppError::Forbidden);
    }
    Ok(project_id)
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    action: &str,
    resource_kind: &str,
    resource_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, '{}'::jsonb)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(action)
    .bind(resource_kind)
    .bind(resource_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn ai_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(|error| error.constraint() == Some("ai_profiles_project_preset_unique_idx"))
    {
        AppError::Conflict("an agent from this preset already exists in the project".to_owned())
    } else if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("an AI setting with this name already exists".to_owned())
    } else {
        AppError::Database(error)
    }
}

fn ai_delete_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation)
    {
        AppError::Conflict("the provider is still assigned to an AI profile".to_owned())
    } else {
        AppError::Database(error)
    }
}

#[cfg(test)]
mod tests {
    use aes_gcm::{
        Aes256Gcm, KeyInit, Nonce,
        aead::{Aead, Payload},
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use sha2::{Digest, Sha256};
    use sqlx::PgPool;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::{AppState, Config};

    use super::{
        AiChannelOptionResponse, AiProfileCapabilities, AiProfileCustomField,
        AiProfilePublicIdentityInput, AiProfileRequest, AiProfileSecretInput,
        AiProfileSecretResponse, AiProviderRequest, PROVIDER_SECRET_AAD,
        TelegramNotificationSettings, decrypt_secret, encrypt_secret,
        has_complete_profile_assignment_scope, normalize_http_allowed_hosts, normalize_profile,
        normalize_profile_custom_fields, normalize_profile_languages,
        normalize_profile_public_identities, normalize_profile_secrets, normalize_provider,
        profile_secret_aad, validate_provider_api_key_destination,
        validate_scoped_profile_channel_selection,
    };

    #[test]
    fn exposes_only_assignment_fields_in_ai_channel_options() {
        let id = Uuid::now_v7();
        let inbox_id = Uuid::now_v7();
        let response = serde_json::to_value(AiChannelOptionResponse {
            id,
            inbox_id,
            kind: "widget".to_owned(),
            name: "Website".to_owned(),
            status: "active".to_owned(),
        })
        .unwrap();

        assert_eq!(
            response,
            serde_json::json!({
                "id": id,
                "inbox_id": inbox_id,
                "kind": "widget",
                "name": "Website",
                "status": "active",
            })
        );
    }

    #[test]
    fn inbox_scoped_profiles_require_a_nonempty_channel_selection() {
        let inbox_id = Uuid::now_v7();
        let channel_id = Uuid::now_v7();

        assert!(validate_scoped_profile_channel_selection(None, &[]).is_ok());
        assert!(
            validate_scoped_profile_channel_selection(Some(&[inbox_id]), &[channel_id]).is_ok()
        );
        assert!(validate_scoped_profile_channel_selection(Some(&[inbox_id]), &[]).is_err());
    }

    #[test]
    fn inbox_scoped_profiles_require_every_assignment_to_be_visible() {
        assert!(!has_complete_profile_assignment_scope(0, 0));
        assert!(has_complete_profile_assignment_scope(1, 1));
        assert!(has_complete_profile_assignment_scope(3, 3));
        assert!(!has_complete_profile_assignment_scope(3, 2));
        assert!(!has_complete_profile_assignment_scope(1, 0));
    }

    #[test]
    fn encrypts_provider_keys_with_authenticated_random_nonces() {
        let key = [7_u8; 32];
        let first = encrypt_secret(
            &key,
            b"provider-secret",
            "v1".to_owned(),
            PROVIDER_SECRET_AAD,
        )
        .unwrap();
        let second = encrypt_secret(
            &key,
            b"provider-secret",
            "v1".to_owned(),
            PROVIDER_SECRET_AAD,
        )
        .unwrap();
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.ciphertext, b"provider-secret");

        let decrypted =
            decrypt_secret(&key, &first.ciphertext, &first.nonce, PROVIDER_SECRET_AAD).unwrap();
        assert_eq!(decrypted, b"provider-secret");
    }

    #[test]
    fn stored_provider_keys_cannot_follow_a_changed_credential_destination() {
        for destination in [
            "https://attacker.example/v1",
            "http://models.example/v1",
            "https://models.example:8443/v1",
            "https://models.example/another-account",
        ] {
            let input = normalize_provider(AiProviderRequest {
                visibility: None,
                name: "Primary".to_owned(),
                provider_kind: "openai_compatible".to_owned(),
                model_type: None,
                base_url: destination.to_owned(),
                default_model: "support-model".to_owned(),
                status: "active".to_owned(),
                api_key: None,
                clear_api_key: false,
            })
            .unwrap();
            assert!(
                validate_provider_api_key_destination("https://models.example/v1", true, &input)
                    .is_err(),
                "stored credentials must not move to {destination}"
            );
            assert!(
                validate_provider_api_key_destination("https://models.example/v1", false, &input)
                    .is_ok()
            );
        }
    }

    #[test]
    fn provider_updates_allow_same_destination_or_explicit_credential_replacement() {
        for (destination, api_key, clear_api_key) in [
            ("https://MODELS.example:443/v1/", None, false),
            ("https://new-provider.example/v1", Some("new-key"), false),
            ("https://new-provider.example/v1", None, true),
        ] {
            let input = normalize_provider(AiProviderRequest {
                visibility: None,
                name: "Renamed".to_owned(),
                provider_kind: "openai_compatible".to_owned(),
                model_type: None,
                base_url: destination.to_owned(),
                default_model: "updated-model".to_owned(),
                status: "active".to_owned(),
                api_key: api_key.map(str::to_owned),
                clear_api_key,
            })
            .unwrap();
            assert!(
                validate_provider_api_key_destination("https://models.example/v1", true, &input)
                    .is_ok()
            );
        }
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn provider_update_keeps_stored_credentials_bound_to_the_original_url(db: PgPool) {
        let tenant_id = Uuid::from_u128(1);
        let project_id = Uuid::from_u128(2);
        let user_id = Uuid::from_u128(3);
        let provider_id = Uuid::from_u128(4);
        sqlx::raw_sql(&format!(
            r#"
            INSERT INTO tenants (id, name) VALUES ('{tenant_id}', 'Provider test');
            INSERT INTO users (id, email, display_name)
                VALUES ('{user_id}', 'provider@example.test', 'Provider manager');
            INSERT INTO projects (id, tenant_id, name, slug)
                VALUES ('{project_id}', '{tenant_id}', 'Provider test', 'provider-test');
            INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
                VALUES ('{tenant_id}', '{project_id}', 'manager', 'Manager', 'manager',
                    ARRAY['ai:manage']);
            INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
                VALUES ('{membership_id}', '{tenant_id}', '{project_id}', '{user_id}', 'manager');
            INSERT INTO ai_provider_connections (
                id, tenant_id, project_id, name, provider_kind, base_url,
                default_model, created_by, encrypted_api_key, api_key_nonce, key_version
            ) VALUES (
                '{provider_id}', '{tenant_id}', '{project_id}', 'Provider', 'openai_compatible',
                'https://models.example/v1', 'model', '{user_id}',
                decode(repeat('07', 32), 'hex'), decode(repeat('08', 12), 'hex'), 'v1'
            );
            "#,
            membership_id = Uuid::from_u128(5),
        ))
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, tenant_id, project_id, actor_user_id, name, token_hash,
                permissions, role, expires_at
            ) VALUES ($1, $2, $3, $4, 'Provider manager', $5,
                      ARRAY['ai:manage'], 'manager', now() + interval '1 hour')
            "#,
        )
        .bind(Uuid::from_u128(6))
        .bind(tenant_id)
        .bind(project_id)
        .bind(user_id)
        .bind(Sha256::digest(b"provider-manager").to_vec())
        .execute(&db)
        .await
        .unwrap();

        let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
        config.pg.migrate_on_start = false;
        config.redis.url = None;
        config.attachments.enabled = false;
        let mut state = AppState::build(config).await.unwrap();
        state.db = db.clone();
        let app = super::router().with_state(state);

        for (destination, clear_api_key, expected_status) in [
            (
                "https://attacker.example/v1",
                false,
                StatusCode::BAD_REQUEST,
            ),
            ("https://MODELS.example:443/v1/", false, StatusCode::OK),
            ("https://new-provider.example/v1", true, StatusCode::OK),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::patch(format!("/api/v1/ai/providers/{provider_id}"))
                        .header("authorization", "Bearer provider-manager")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "name": "Updated provider",
                                "provider_kind": "openai_compatible",
                                "base_url": destination,
                                "default_model": "updated-model",
                                "status": "active",
                                "clear_api_key": clear_api_key
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected_status, "{destination}");
            let stored =
                sqlx::query_as::<_, (String, Option<Vec<u8>>, Option<Vec<u8>>, Option<String>)>(
                    r#"
                SELECT base_url, encrypted_api_key, api_key_nonce, key_version
                FROM ai_provider_connections WHERE id = $1
                "#,
                )
                .bind(provider_id)
                .fetch_one(&db)
                .await
                .unwrap();
            if clear_api_key {
                assert_eq!(stored, (destination.to_owned(), None, None, None));
            } else {
                assert_eq!(
                    stored,
                    (
                        "https://models.example/v1".to_owned(),
                        Some(vec![7; 32]),
                        Some(vec![8; 12]),
                        Some("v1".to_owned()),
                    )
                );
            }
        }
    }

    #[test]
    fn validates_provider_urls_and_profile_capabilities() {
        assert!(
            normalize_provider(AiProviderRequest {
                visibility: None,
                name: "Primary".to_owned(),
                provider_kind: "openai_compatible".to_owned(),
                model_type: None,
                base_url: "https://models.example/v1/".to_owned(),
                default_model: "support-model".to_owned(),
                status: "active".to_owned(),
                api_key: None,
                clear_api_key: false,
            })
            .is_ok()
        );
        assert!(
            normalize_provider(AiProviderRequest {
                visibility: None,
                name: "Removed provider type".to_owned(),
                provider_kind: "letta".to_owned(),
                model_type: None,
                base_url: "http://127.0.0.1:4500/v1".to_owned(),
                default_model: "agent-local-123e4567-e89b-42d3-a456-426614174000".to_owned(),
                status: "active".to_owned(),
                api_key: None,
                clear_api_key: false,
            })
            .is_err()
        );
        let profile = normalize_profile(AiProfileRequest {
            visibility: None,
            name: "Support copilot".to_owned(),
            description: None,
            status: "draft".to_owned(),
            provider_connection_id: None,
            model: None,
            instructions: String::new(),
            tool_instructions: None,
            blacklist_reply_text: None,
            blacklist_reply_match_language: None,
            http_allowed_hosts: None,
            language: "ru".to_owned(),
            max_output_tokens: 800,
            max_concurrent_runs: None,
            auto_join_new_conversations: true,
            can_resolve_conversations: true,
            capabilities: AiProfileCapabilities {
                http_get: true,
                http_post: true,
                shell: true,
            },
            telegram_notifications: TelegramNotificationSettings {
                new_visitor: true,
                new_message: true,
                operator_request: false,
            },
            custom_fields: None,
            secrets: None,
            knowledge_base_ids: Vec::new(),
            channel_ids: Vec::new(),
            public_identities: vec![AiProfilePublicIdentityInput {
                language: "ru".to_owned(),
                display_name: "Екатерина".to_owned(),
            }],
            preset_key: None,
        })
        .expect("valid profile should be normalized");
        assert!(profile.auto_join_new_conversations);
        assert!(profile.can_resolve_conversations);
        assert_eq!(
            profile.capabilities,
            AiProfileCapabilities {
                http_get: true,
                http_post: true,
                shell: true,
            }
        );
        assert!(profile.telegram_notifications.new_visitor);
        assert!(profile.telegram_notifications.new_message);
        assert!(!profile.telegram_notifications.operator_request);
    }

    #[test]
    fn defaults_omitted_profile_capabilities_and_rejects_unknown_fields() {
        let request = serde_json::from_value::<AiProfileRequest>(serde_json::json!({
            "name": "Legacy profile",
            "status": "draft",
            "provider_connection_id": null,
            "model": null,
            "language": "en",
            "max_output_tokens": 800,
            "public_identities": [{
                "language": "en",
                "display_name": "Support"
            }]
        }))
        .expect("legacy profile requests should remain valid");
        assert_eq!(request.capabilities, AiProfileCapabilities::default());
        assert!(request.tool_instructions.is_none());
        assert!(request.blacklist_reply_text.is_none());
        assert!(request.blacklist_reply_match_language.is_none());
        assert!(request.http_allowed_hosts.is_none());
        assert_eq!(
            serde_json::to_value(request.capabilities).unwrap(),
            serde_json::json!({
                "http_get": false,
                "http_post": false,
                "shell": false,
            })
        );

        let partial = serde_json::from_value::<AiProfileCapabilities>(serde_json::json!({
            "http_post": true
        }))
        .expect("omitted capability flags should default to false");
        assert_eq!(
            partial,
            AiProfileCapabilities {
                http_get: false,
                http_post: true,
                shell: false,
            }
        );
        assert!(
            serde_json::from_value::<AiProfileCapabilities>(serde_json::json!({
                "http_get": true,
                "unsupported": true
            }))
            .is_err()
        );
    }

    #[test]
    fn http_domain_policy_normalizes_exact_hosts_and_explicit_public_access() {
        assert_eq!(
            normalize_http_allowed_hosts(vec![
                " API.Example.com ".into(),
                "api.example.com".into(),
                "support.example.org".into()
            ])
            .unwrap(),
            vec!["api.example.com", "support.example.org"]
        );
        assert!(normalize_http_allowed_hosts(Vec::new()).unwrap().is_empty());
        assert_eq!(
            normalize_http_allowed_hosts(vec!["api.example.com".into(), "*".into()]).unwrap(),
            vec!["*"]
        );
        for host in [
            "",
            "localhost",
            "https://api.example.com",
            "api.example.com/path",
            "api.example.com:443",
            "*.example.com",
            "api.example.com.",
            "a..example.com",
            "-a.example.com",
            "127.0.0.1",
            "127.1",
            "0x7f.1",
            "[::1]",
        ] {
            assert!(
                normalize_http_allowed_hosts(vec![host.into()]).is_err(),
                "{host}"
            );
        }
        assert!(normalize_http_allowed_hosts(vec!["api.example.com".into(); 65]).is_err());
    }

    #[test]
    fn tool_descriptions_can_be_cleared_without_granting_capabilities() {
        let mut payload = serde_json::json!({
            "name": "Support", "status": "draft", "provider_connection_id": null,
            "model": null, "language": "en", "max_output_tokens": 800,
            "tool_instructions": "   ",
            "public_identities": [{"language":"en","display_name":"Support"}]
        });
        let normalized =
            normalize_profile(serde_json::from_value(payload.clone()).unwrap()).unwrap();
        assert_eq!(normalized.tool_instructions.as_deref(), Some(""));
        assert_eq!(normalized.capabilities, AiProfileCapabilities::default());
        payload["tool_instructions"] = serde_json::json!("x".repeat(50_001));
        assert!(normalize_profile(serde_json::from_value(payload).unwrap()).is_err());
    }

    #[test]
    fn blacklist_reply_normalizes_text_and_preserves_omitted_settings() {
        let mut payload = serde_json::json!({
            "name": "Support", "status": "draft", "language": "en", "max_output_tokens": 800,
            "public_identities": [{"language":"en","display_name":"Support"}]
        });
        let normalized =
            normalize_profile(serde_json::from_value(payload.clone()).unwrap()).unwrap();
        assert!(normalized.blacklist_reply_text.is_none());
        assert!(normalized.blacklist_reply_match_language.is_none());

        payload["blacklist_reply_text"] =
            serde_json::json!("  Blocked for spam.\nContact support.  ");
        payload["blacklist_reply_match_language"] = serde_json::json!(false);
        let normalized =
            normalize_profile(serde_json::from_value(payload.clone()).unwrap()).unwrap();
        assert_eq!(
            normalized.blacklist_reply_text.as_deref(),
            Some("Blocked for spam.\nContact support.")
        );
        assert_eq!(normalized.blacklist_reply_match_language, Some(false));

        payload["blacklist_reply_text"] = serde_json::json!("   ");
        let normalized =
            normalize_profile(serde_json::from_value(payload.clone()).unwrap()).unwrap();
        assert_eq!(normalized.blacklist_reply_text.as_deref(), Some(""));

        payload["blacklist_reply_text"] = serde_json::json!("я".repeat(4_000));
        assert!(normalize_profile(serde_json::from_value(payload.clone()).unwrap()).is_ok());
        payload["blacklist_reply_text"] = serde_json::json!("я".repeat(4_001));
        assert!(normalize_profile(serde_json::from_value(payload).unwrap()).is_err());
    }

    #[test]
    fn normalizes_comma_separated_profile_languages() {
        assert_eq!(
            normalize_profile_languages(" ru, en-US, RU, de ".to_owned())
                .expect("valid language list should be normalized"),
            "ru, en-US, de"
        );
        assert_eq!(
            normalize_profile_languages("pt-BR".to_owned())
                .expect("a single language should remain supported"),
            "pt-BR"
        );

        for invalid in ["", "ru,", "ru,,en", "r", "ru, en!"] {
            assert!(normalize_profile_languages(invalid.to_owned()).is_err());
        }

        let too_long = (0..50)
            .map(|index| format!("en-{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(normalize_profile_languages(too_long).is_err());
    }

    #[test]
    fn normalizes_public_identities_within_reply_languages() {
        let identities = normalize_profile_public_identities(
            vec![
                AiProfilePublicIdentityInput {
                    language: "RU".to_owned(),
                    display_name: " Екатерина ".to_owned(),
                },
                AiProfilePublicIdentityInput {
                    language: "en-US".to_owned(),
                    display_name: "Kate".to_owned(),
                },
            ],
            "ru, en-US",
        )
        .expect("public identities should be valid");
        assert_eq!(identities[0].language, "ru");
        assert_eq!(identities[0].display_name, "Екатерина");
        assert_eq!(identities[1].language, "en-us");

        assert!(
            normalize_profile_public_identities(
                vec![AiProfilePublicIdentityInput {
                    language: "de".to_owned(),
                    display_name: "Katharina".to_owned(),
                }],
                "ru, en",
            )
            .is_err()
        );
        assert!(
            normalize_profile_public_identities(
                vec![
                    AiProfilePublicIdentityInput {
                        language: "ru".to_owned(),
                        display_name: "Екатерина".to_owned(),
                    },
                    AiProfilePublicIdentityInput {
                        language: "RU".to_owned(),
                        display_name: "Катя".to_owned(),
                    },
                ],
                "ru",
            )
            .is_err()
        );
        assert!(normalize_profile_public_identities(Vec::new(), "ru").is_err());
        assert!(
            normalize_profile_public_identities(
                vec![AiProfilePublicIdentityInput {
                    language: "ru".to_owned(),
                    display_name: "Екатерина".to_owned(),
                }],
                "ru, en",
            )
            .is_err()
        );
    }

    #[test]
    fn validates_custom_credentials_without_serializing_secret_values() {
        assert!(
            normalize_profile_custom_fields(Some(vec![AiProfileCustomField {
                key: "DB_HOST".to_owned(),
                value: "db.internal".to_owned(),
            }]))
            .is_ok()
        );
        assert!(
            normalize_profile_custom_fields(Some(vec![
                AiProfileCustomField {
                    key: "DB_HOST".to_owned(),
                    value: "one".to_owned(),
                },
                AiProfileCustomField {
                    key: "db_host".to_owned(),
                    value: "two".to_owned(),
                },
            ]))
            .is_err()
        );
        assert!(
            normalize_profile_secrets(Some(vec![AiProfileSecretInput {
                key: "DB_PASSWORD".to_owned(),
                value: Some("not-returned".to_owned()),
            }]))
            .is_ok()
        );
        assert!(
            normalize_profile(AiProfileRequest {
                visibility: None,
                name: "Colliding credentials".to_owned(),
                description: None,
                status: "draft".to_owned(),
                provider_connection_id: None,
                model: None,
                instructions: String::new(),
                tool_instructions: None,
                blacklist_reply_text: None,
                blacklist_reply_match_language: None,
                http_allowed_hosts: None,
                language: "en".to_owned(),
                max_output_tokens: 800,
                max_concurrent_runs: None,
                auto_join_new_conversations: false,
                can_resolve_conversations: false,
                capabilities: AiProfileCapabilities::default(),
                telegram_notifications: TelegramNotificationSettings::default(),
                custom_fields: Some(vec![AiProfileCustomField {
                    key: "DB_PASSWORD".to_owned(),
                    value: "visible".to_owned(),
                }]),
                secrets: Some(vec![AiProfileSecretInput {
                    key: "db_password".to_owned(),
                    value: Some("secret".to_owned()),
                }]),
                knowledge_base_ids: Vec::new(),
                channel_ids: Vec::new(),
                public_identities: vec![AiProfilePublicIdentityInput {
                    language: "en".to_owned(),
                    display_name: "Support".to_owned(),
                }],
                preset_key: None,
            })
            .is_err()
        );

        let response = serde_json::to_string(&AiProfileSecretResponse {
            key: "DB_PASSWORD".to_owned(),
            configured: true,
        })
        .unwrap();
        assert_eq!(response, r#"{"key":"DB_PASSWORD","configured":true}"#);
        assert!(!response.contains("not-returned"));
    }

    #[test]
    fn binds_profile_secret_ciphertext_to_its_profile_and_key() {
        let key = [9_u8; 32];
        let tenant_id = Uuid::now_v7();
        let profile_id = Uuid::now_v7();
        let aad = profile_secret_aad(tenant_id, profile_id, "DB_PASSWORD");
        let wrong_aad = profile_secret_aad(tenant_id, profile_id, "OTHER_PASSWORD");
        let encrypted = encrypt_secret(&key, b"database-secret", "v1".to_owned(), &aad).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();

        assert!(
            cipher
                .decrypt(
                    Nonce::from_slice(&encrypted.nonce),
                    Payload {
                        msg: &encrypted.ciphertext,
                        aad: &aad,
                    },
                )
                .is_ok()
        );
        assert!(
            cipher
                .decrypt(
                    Nonce::from_slice(&encrypted.nonce),
                    Payload {
                        msg: &encrypted.ciphertext,
                        aad: &wrong_aad,
                    },
                )
                .is_err()
        );
    }
}
