//! Persisted custom channel drafts for AI agents and external APIs. This module
//! does not execute instructions, access websites, poll messages, or send replies.
//! The configured destination describes future behavior and grants no permission
//! to create conversations, tasks, or custom results. An authenticated source
//! executor must be chosen before these channels can become a live transport.

use crate::resource_visibility::{self, Resource, ResourceVisibility};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction};
use url::Url;
use uuid::Uuid;

use crate::{AppState, ai_settings, auth::ActorContext, error::AppError, projects};

/// Routes for saving and managing custom AI channel configuration.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/custom-ai-channels",
            get(list_channels).post(create_channel),
        )
        .route(
            "/api/v1/custom-ai-channels/{channel_id}",
            put(update_channel).delete(delete_channel),
        )
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ConfigurationMode {
    Instructions,
    Agent,
}

impl ConfigurationMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Instructions => "instructions",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SourceKind {
    Website,
    Api,
}

impl SourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Website => "website",
            Self::Api => "api",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ConnectionType {
    #[default]
    AiAgent,
    ExternalApi,
}

impl ConnectionType {
    fn as_str(self) -> &'static str {
        match self {
            Self::AiAgent => "ai_agent",
            Self::ExternalApi => "external_api",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Destination {
    #[default]
    Conversations,
    Tasks,
    Custom,
}

impl Destination {
    fn as_str(self) -> &'static str {
        match self {
            Self::Conversations => "conversations",
            Self::Tasks => "tasks",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelRequest {
    visibility: Option<ResourceVisibility>,
    name: String,
    #[serde(default = "default_icon")]
    icon: String,
    inbox_id: Uuid,
    #[serde(default)]
    connection_type: ConnectionType,
    #[serde(default)]
    destination: Destination,
    mode: ConfigurationMode,
    #[serde(default)]
    source_url: String,
    source_kind: SourceKind,
    #[serde(default)]
    instructions: String,
    ai_profile_id: Option<Uuid>,
}

impl ChannelRequest {
    fn normalize(mut self) -> Result<Self, AppError> {
        self.name = self.name.trim().to_owned();
        if !(1..=200).contains(&self.name.chars().count()) {
            return Err(AppError::BadRequest(
                "name must contain between 1 and 200 characters".to_owned(),
            ));
        }
        if !CHANNEL_ICONS.contains(&self.icon.as_str()) {
            return Err(AppError::BadRequest("unsupported channel icon".to_owned()));
        }
        self.instructions = self.instructions.trim().to_owned();
        if self.instructions.chars().count() > 50_000 {
            return Err(AppError::BadRequest(
                "instructions must not exceed 50000 characters".to_owned(),
            ));
        }
        if self.connection_type == ConnectionType::ExternalApi {
            if self.ai_profile_id.is_some() {
                return Err(AppError::BadRequest(
                    "ai_profile_id must be null for an external API connection".to_owned(),
                ));
            }
            self.mode = ConfigurationMode::Instructions;
            self.source_kind = SourceKind::Api;
        } else {
            match self.mode {
                ConfigurationMode::Instructions if self.ai_profile_id.is_some() => {
                    return Err(AppError::BadRequest(
                        "ai_profile_id must be null in instructions mode".to_owned(),
                    ));
                }
                ConfigurationMode::Agent if self.ai_profile_id.is_none() => {
                    return Err(AppError::BadRequest(
                        "select an AI agent for agent mode".to_owned(),
                    ));
                }
                ConfigurationMode::Instructions | ConfigurationMode::Agent => {}
            }
        }
        let needs_instructions = self.destination == Destination::Custom
            || (self.connection_type == ConnectionType::AiAgent
                && self.mode == ConfigurationMode::Instructions);
        if needs_instructions && self.instructions.chars().count() < 10 {
            return Err(AppError::BadRequest(
                "instructions must describe the desired channel action (at least 10 characters)"
                    .to_owned(),
            ));
        }
        self.source_url = normalize_source_url(&self.source_url)?;
        Ok(self)
    }
}

const CHANNEL_ICONS: &[&str] = &[
    "bot",
    "globe",
    "message-circle",
    "mail",
    "briefcase",
    "users",
    "headset",
    "send",
    "linkedin",
    "phone",
    "workflow",
    "layers",
];

fn default_icon() -> String {
    "bot".to_owned()
}

fn normalize_source_url(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.chars().count() > 4096 || value.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "source_url must be an HTTPS URL of at most 4096 characters".to_owned(),
        ));
    }
    let url = Url::parse(value)
        .map_err(|_| AppError::BadRequest("source_url must be a valid HTTPS URL".to_owned()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::BadRequest(
            "source_url must use HTTPS and must not contain username or password credentials"
                .to_owned(),
        ));
    }
    let normalized = url.to_string();
    if normalized.chars().count() > 4096 {
        return Err(AppError::BadRequest(
            "source_url must not exceed 4096 characters after URL normalization".to_owned(),
        ));
    }
    Ok(normalized)
}

#[derive(Debug, FromRow, Serialize)]
struct ChannelResponse {
    inbox_name: String,
    visibility: sqlx::types::Json<ResourceVisibility>,
    id: Uuid,
    #[serde(skip)]
    project_id: Uuid,
    name: String,
    icon: String,
    inbox_id: Uuid,
    connection_type: String,
    destination: String,
    mode: String,
    source_url: String,
    source_kind: String,
    instructions: String,
    ai_profile_id: Option<Uuid>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ChannelListResponse {
    items: Vec<ChannelResponse>,
}

const SELECT_CHANNEL: &str = r#"
    SELECT (SELECT name FROM inboxes WHERE tenant_id=connection.tenant_id AND id=connection.inbox_id) AS inbox_name, connection.visibility, connection.id, connection.project_id, connection.name,
           connection.inbox_id, config.icon, config.connection_type, config.destination,
           config.mode, config.source_url,
           config.source_kind, config.instructions, config.ai_profile_id,
           connection.status, connection.created_at, connection.updated_at
    FROM channel_connections AS connection
    JOIN custom_ai_channel_configs AS config
      ON config.tenant_id = connection.tenant_id
     AND config.project_id = connection.project_id
     AND config.inbox_id = connection.inbox_id
     AND config.channel_connection_id = connection.id
    WHERE connection.tenant_id = $1
      AND connection.kind = 'custom_ai'
      AND connection.deleted_at IS NULL
"#;

async fn list_channels(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<ChannelListResponse>, AppError> {
    actor.require("channels:read")?;
    let query = format!(
        "{SELECT_CHANNEL} AND resource_visible(connection.visibility,$2,$3) AND ($4::uuid IS NULL OR connection.project_id=$4) \
         ORDER BY connection.name, connection.id"
    );
    let mut items = sqlx::query_as::<_, ChannelResponse>(&query)
        .bind(actor.tenant_id)
        .bind(actor.project_id)
        .bind(actor.department_id())
        .bind(resource_visibility::owner_limit(&actor))
        .fetch_all(&state.db)
        .await?;
    if !actor.is_password_session() {
        items.retain(|item| actor.require_inbox(item.project_id, item.inbox_id).is_ok());
    }
    Ok(Json(ChannelListResponse { items }))
}

fn require_management(actor: &ActorContext) -> Result<(), AppError> {
    actor.require("channels:manage")?;
    if actor.is_demo() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn validate_references(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    input: &ChannelRequest,
    existing: Option<&ChannelResponse>,
) -> Result<Uuid, AppError> {
    let (project_id, inbox_status) = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT project_id, status FROM inboxes WHERE tenant_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(actor.tenant_id)
    .bind(input.inbox_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if existing.is_none_or(|channel| channel.inbox_id != input.inbox_id) {
        actor.require_inbox(project_id, input.inbox_id)?;
    }
    if inbox_status != "active" {
        return Err(AppError::Conflict(
            "custom AI channels require an active Inbox".to_owned(),
        ));
    }
    if let Some(profile_id) = input.ai_profile_id {
        ai_settings::require_management(actor)?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_profiles WHERE tenant_id=$1 AND id=$2 AND resource_visible(visibility,$3,$4))")
            .bind(actor.tenant_id).bind(profile_id).bind(actor.project_id).bind(actor.department_id())
            .fetch_one(&mut **transaction).await?;
        if !exists {
            return Err(AppError::NotFound);
        }
    }
    Ok(project_id)
}

async fn load_channel(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    channel_id: Uuid,
) -> Result<ChannelResponse, AppError> {
    let query = format!("{SELECT_CHANNEL} AND connection.id = $2 FOR UPDATE OF connection");
    let channel = sqlx::query_as::<_, ChannelResponse>(&query)
        .bind(actor.tenant_id)
        .bind(channel_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(channel)
}

async fn create_channel(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<ChannelRequest>,
) -> Result<(StatusCode, Json<ChannelResponse>), AppError> {
    require_management(&actor)?;
    let input = request.normalize()?;
    let mut transaction = state.db.begin().await?;
    let project_id = validate_references(&mut transaction, &actor, &input, None).await?;
    let channel_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO channel_connections (
            id, tenant_id, project_id, inbox_id, public_id, kind, name, status
        ) VALUES ($1, $2, $3, $4, $5, 'custom_ai', $6, 'draft')
        "#,
    )
    .bind(channel_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(input.inbox_id)
    .bind(Uuid::now_v7())
    .bind(&input.name)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO custom_ai_channel_configs (
            channel_connection_id, tenant_id, project_id, inbox_id, mode,
            source_url, source_kind, instructions, ai_profile_id, icon,
            connection_type, destination
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(channel_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(input.inbox_id)
    .bind(input.mode.as_str())
    .bind(&input.source_url)
    .bind(input.source_kind.as_str())
    .bind(&input.instructions)
    .bind(input.ai_profile_id)
    .bind(&input.icon)
    .bind(input.connection_type.as_str())
    .bind(input.destination.as_str())
    .execute(&mut *transaction)
    .await?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Channel,
        channel_id,
        input.visibility.as_ref(),
        true,
    )
    .await?;
    projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "custom_ai_channel.created",
        "channel_connection",
        Some(channel_id),
        json!({
            "mode": input.mode.as_str(), "source_kind": input.source_kind.as_str(),
            "connection_type": input.connection_type.as_str(), "destination": input.destination.as_str()
        }),
    )
    .await?;
    let response = load_channel(&mut transaction, &actor, channel_id).await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn update_channel(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(channel_id): Path<Uuid>,
    Json(request): Json<ChannelRequest>,
) -> Result<Json<ChannelResponse>, AppError> {
    require_management(&actor)?;
    resource_visibility::owner_project(&state.db, &actor, Resource::Channel, channel_id).await?;
    let input = request.normalize()?;
    let mut transaction = state.db.begin().await?;
    let existing = load_channel(&mut transaction, &actor, channel_id).await?;
    let project_id = validate_references(&mut transaction, &actor, &input, Some(&existing)).await?;
    if existing.project_id != project_id {
        return Err(AppError::BadRequest(
            "a custom AI channel cannot be moved to a different project".to_owned(),
        ));
    }
    sqlx::query(
        r#"
        UPDATE channel_connections
        SET name = $3, inbox_id = $4, status = 'draft', updated_at = now()
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .bind(&input.name)
    .bind(input.inbox_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE custom_ai_channel_configs
        SET mode = $3, source_url = $4, source_kind = $5,
            instructions = $6, ai_profile_id = $7, icon = $8,
            connection_type = $9, destination = $10
        WHERE tenant_id = $1 AND channel_connection_id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .bind(input.mode.as_str())
    .bind(&input.source_url)
    .bind(input.source_kind.as_str())
    .bind(&input.instructions)
    .bind(input.ai_profile_id)
    .bind(&input.icon)
    .bind(input.connection_type.as_str())
    .bind(input.destination.as_str())
    .execute(&mut *transaction)
    .await?;
    resource_visibility::save(
        &mut transaction,
        &actor,
        Resource::Channel,
        channel_id,
        input.visibility.as_ref(),
        false,
    )
    .await?;
    projects::insert_audit(
        &mut transaction,
        &actor,
        Some(project_id),
        "custom_ai_channel.updated",
        "channel_connection",
        Some(channel_id),
        json!({
            "mode": input.mode.as_str(), "source_kind": input.source_kind.as_str(),
            "connection_type": input.connection_type.as_str(), "destination": input.destination.as_str()
        }),
    )
    .await?;
    let response = load_channel(&mut transaction, &actor, channel_id).await?;
    transaction.commit().await?;
    Ok(Json(response))
}

async fn delete_channel(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(channel_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_management(&actor)?;
    resource_visibility::owner_project(&state.db, &actor, Resource::Channel, channel_id).await?;
    let mut transaction = state.db.begin().await?;
    let existing = load_channel(&mut transaction, &actor, channel_id).await?;
    sqlx::query(
        r#"
        UPDATE channel_connections
        SET status = 'disabled', deleted_at = now(), updated_at = now()
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .execute(&mut *transaction)
    .await?;
    projects::insert_audit(
        &mut transaction,
        &actor,
        Some(existing.project_id),
        "custom_ai_channel.deleted",
        "channel_connection",
        Some(channel_id),
        json!({}),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::{
        CHANNEL_ICONS, ChannelRequest, ConfigurationMode, ConnectionType, Destination, SourceKind,
        normalize_source_url,
    };
    use uuid::Uuid;

    fn request() -> ChannelRequest {
        ChannelRequest {
            visibility: None,
            name: " Custom channel ".to_owned(),
            icon: "bot".to_owned(),
            inbox_id: Uuid::now_v7(),
            connection_type: ConnectionType::AiAgent,
            destination: Destination::Conversations,
            mode: ConfigurationMode::Instructions,
            source_url: " https://example.com/messages ".to_owned(),
            source_kind: SourceKind::Website,
            instructions: " Read new customer messages and prepare replies. ".to_owned(),
            ai_profile_id: None,
        }
    }

    #[test]
    fn accepts_catalog_icons_and_defaults_missing_icon() {
        for icon in CHANNEL_ICONS {
            let mut input = request();
            input.icon = (*icon).to_owned();
            assert_eq!(input.normalize().unwrap().icon, *icon);
        }
        let input = serde_json::json!({
            "name": "Channel", "inbox_id": Uuid::now_v7(),
            "mode": "instructions", "source_url": "https://example.com",
            "source_kind": "website", "instructions": "Read and answer messages"
        });
        assert_eq!(
            serde_json::from_value::<ChannelRequest>(input)
                .unwrap()
                .normalize()
                .unwrap()
                .icon,
            "bot"
        );
        for invalid in ["", "unknown", "<svg>", "https://example.com/icon.svg"] {
            let mut input = request();
            input.icon = invalid.to_owned();
            assert!(input.normalize().is_err());
        }
    }

    #[test]
    fn source_url_requires_https_without_credentials() {
        for invalid in [
            "http://example.com",
            "javascript:alert(1)",
            "file:///tmp/messages",
            "https://alice:secret@example.com",
            "https://alice@example.com",
            "https://example.com\n/secret",
            "https://",
        ] {
            assert!(normalize_source_url(invalid).is_err(), "accepted {invalid}");
        }
        assert_eq!(
            normalize_source_url(" https://EXAMPLE.com/messages#/inbox ").unwrap(),
            "https://example.com/messages#/inbox"
        );
        assert_eq!(normalize_source_url("  ").unwrap(), "");
    }

    #[test]
    fn configuration_modes_require_their_own_input() {
        let input = request().normalize().unwrap();
        assert_eq!(input.name, "Custom channel");
        assert_eq!(input.source_url, "https://example.com/messages");
        let mut short = request();
        short.instructions = "read".to_owned();
        assert!(short.normalize().is_err());
        let mut ambiguous = request();
        ambiguous.ai_profile_id = Some(Uuid::now_v7());
        assert!(ambiguous.normalize().is_err());
        let mut agent = request();
        agent.mode = ConfigurationMode::Agent;
        assert!(agent.normalize().is_err());
        let mut valid_agent = request();
        valid_agent.mode = ConfigurationMode::Agent;
        valid_agent.ai_profile_id = Some(Uuid::now_v7());
        valid_agent.instructions.clear();
        assert!(valid_agent.normalize().is_ok());
    }

    #[test]
    fn legacy_configuration_defaults_to_ai_conversations() {
        let input = serde_json::json!({
            "name": "Channel", "inbox_id": Uuid::now_v7(),
            "mode": "instructions", "source_kind": "website",
            "instructions": "Read and answer messages"
        });
        let normalized = serde_json::from_value::<ChannelRequest>(input)
            .unwrap()
            .normalize()
            .unwrap();
        assert_eq!(normalized.connection_type, ConnectionType::AiAgent);
        assert_eq!(normalized.destination, Destination::Conversations);
        assert_eq!(normalized.source_url, "");
    }

    #[test]
    fn external_api_needs_no_agent_or_instructions_for_standard_destinations() {
        for destination in [Destination::Conversations, Destination::Tasks] {
            let mut input = request();
            input.connection_type = ConnectionType::ExternalApi;
            input.destination = destination;
            input.mode = ConfigurationMode::Agent;
            input.instructions.clear();
            input.source_url.clear();
            let normalized = input.normalize().unwrap();
            assert_eq!(normalized.mode, ConfigurationMode::Instructions);
            assert_eq!(normalized.source_kind.as_str(), "api");
            assert_eq!(normalized.ai_profile_id, None);
            assert_eq!(normalized.instructions, "");
        }
        let mut input = request();
        input.connection_type = ConnectionType::ExternalApi;
        input.ai_profile_id = Some(Uuid::now_v7());
        assert!(input.normalize().is_err());
    }

    #[test]
    fn custom_destination_requires_instructions_for_both_connection_types() {
        for connection_type in [ConnectionType::AiAgent, ConnectionType::ExternalApi] {
            let mut input = request();
            input.connection_type = connection_type;
            input.destination = Destination::Custom;
            input.instructions = "  do it  ".to_owned();
            if connection_type == ConnectionType::AiAgent {
                input.mode = ConfigurationMode::Agent;
                input.ai_profile_id = Some(Uuid::now_v7());
            }
            assert!(input.normalize().is_err());
            let mut input = request();
            input.connection_type = connection_type;
            input.destination = Destination::Custom;
            assert!(input.normalize().is_ok());
        }
    }

    #[test]
    fn rejects_unknown_connection_types_and_destinations() {
        for (field, value) in [
            ("connection_type", "webhook"),
            ("connection_type", ""),
            ("destination", "admin"),
            ("destination", ""),
        ] {
            let mut input = serde_json::json!({
                "name": "Channel", "inbox_id": Uuid::now_v7(),
                "mode": "instructions", "source_kind": "website",
                "instructions": "Read and answer messages"
            });
            input[field] = serde_json::json!(value);
            assert!(serde_json::from_value::<ChannelRequest>(input).is_err());
        }
    }

    #[test]
    fn rejects_oversized_input_and_transport_flags() {
        let mut oversized = request();
        oversized.instructions = "a".repeat(50_001);
        assert!(oversized.normalize().is_err());
        let mut oversized = request();
        oversized.name = "a".repeat(201);
        assert!(oversized.normalize().is_err());
        assert!(
            normalize_source_url(&format!("https://example.com/{}", "a".repeat(4096))).is_err()
        );
        let input = serde_json::json!({
            "name": "Channel", "inbox_id": Uuid::now_v7(),
            "mode": "instructions", "source_url": "https://example.com",
            "source_kind": "website", "instructions": "Read and answer messages",
            "status": "active"
        });
        assert!(serde_json::from_value::<ChannelRequest>(input).is_err());
    }
}
