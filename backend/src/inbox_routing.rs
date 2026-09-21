//! Inbox-scoped routing configuration and runtime decision hooks.

use std::collections::{HashMap, HashSet};

use anyhow::Context as _;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::HeaderMap,
    routing::get,
};
use chrono::{
    DateTime, Datelike, Days, Duration as ChronoDuration, LocalResult, NaiveDateTime, NaiveTime,
    TimeZone, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgConnection, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    ai_settings::{encrypt_secret, load_secret_encryption_key},
    auth::ActorContext,
    error::AppError,
    operator_presence::OPERATOR_ONLINE_WINDOW_SECONDS,
    provider_reply,
    realtime::RealtimeEvent,
    telegram_notifications::{
        self, RoutingTelegramNotificationKind, RoutingTelegramNotificationPayload,
    },
};

const MAX_ROUTING_BODY_LENGTH: usize = 256 * 1_024;
const MAX_QUEUES: usize = 32;
const MAX_QUEUE_MEMBERS: usize = 500;
const MAX_RULES: usize = 64;
const MAX_WORKING_INTERVALS: usize = 28;
const MAX_TELEGRAM_CHAT_IDS: usize = 32;
const MAX_QUEUE_NAME_CHARS: usize = 120;
const MIN_SLA_SECONDS: i32 = 60;
const MAX_FIRST_RESPONSE_SECONDS: i32 = 604_800;
const MAX_RESOLUTION_SECONDS: i32 = 2_592_000;
const IDEMPOTENCY_TTL_HOURS: i32 = 24;
const MAX_WORKING_SLA_WEEKS: i64 = 520;
const MAX_WORKING_DEADLINE_SCAN_DAYS: u64 = 4_200;

/// Routes for one Inbox routing aggregate.
pub fn router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/inboxes/{inbox_id}/routing",
        get(get_routing_configuration)
            .put(put_routing_configuration)
            .layer(DefaultBodyLimit::max(MAX_ROUTING_BODY_LENGTH)),
    )
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AssignmentStrategy {
    Manual,
    LeastActive,
}

impl AssignmentStrategy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::LeastActive => "least_active",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum QueueStatus {
    Active,
    Disabled,
}

impl QueueStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FallbackAction {
    Ai,
    Queue,
}

impl FallbackAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ai => "ai",
            Self::Queue => "queue",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SlaClock {
    Elapsed,
    WorkingHours,
}

impl SlaClock {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Elapsed => "elapsed",
            Self::WorkingHours => "working_hours",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoutingQueueInput {
    id: Uuid,
    name: String,
    status: QueueStatus,
    #[serde(default)]
    member_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoutingRuleInput {
    id: Uuid,
    position: u16,
    enabled: bool,
    channel_id: Option<Uuid>,
    language: Option<String>,
    queue_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkingIntervalInput {
    id: Uuid,
    weekday: u8,
    starts_at: String,
    ends_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CoverageInput {
    timezone: String,
    #[serde(default)]
    weekly_intervals: Vec<WorkingIntervalInput>,
    outside_hours_action: FallbackAction,
    no_operator_action: FallbackAction,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SlaInput {
    clock: SlaClock,
    first_response_seconds: Option<i32>,
    resolution_seconds: Option<i32>,
    unassigned_warning_seconds: Option<i32>,
    escalation_queue_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
struct TelegramInput {
    new_visitor: bool,
    new_message: bool,
    operator_request: bool,
    unassigned_warning: bool,
    sla_breach: bool,
    #[serde(default)]
    chat_ids: Vec<String>,
}

impl TelegramInput {
    const fn any_enabled(&self) -> bool {
        self.new_visitor
            || self.new_message
            || self.operator_request
            || self.unassigned_warning
            || self.sla_breach
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoutingConfigurationInput {
    enabled: bool,
    assignment_strategy: AssignmentStrategy,
    default_queue_id: Option<Uuid>,
    #[serde(default)]
    queues: Vec<RoutingQueueInput>,
    #[serde(default)]
    rules: Vec<RoutingRuleInput>,
    coverage: CoverageInput,
    sla: SlaInput,
    telegram: TelegramInput,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum TelegramBotTokenWrite {
    Preserve,
    Replace { value: String },
    Clear,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoutingPutRequest {
    expected_version: i64,
    configuration: RoutingConfigurationInput,
    telegram_bot_token: TelegramBotTokenWrite,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RoutingQueueResponse {
    id: Uuid,
    name: String,
    status: QueueStatus,
    member_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RoutingRuleResponse {
    id: Uuid,
    position: u16,
    enabled: bool,
    channel_id: Option<Uuid>,
    language: Option<String>,
    queue_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WorkingIntervalResponse {
    id: Uuid,
    weekday: u8,
    starts_at: String,
    ends_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CoverageResponse {
    timezone: String,
    weekly_intervals: Vec<WorkingIntervalResponse>,
    outside_hours_action: FallbackAction,
    no_operator_action: FallbackAction,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SlaResponse {
    clock: SlaClock,
    first_response_seconds: Option<i32>,
    resolution_seconds: Option<i32>,
    unassigned_warning_seconds: Option<i32>,
    escalation_queue_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct TelegramResponse {
    new_visitor: bool,
    new_message: bool,
    operator_request: bool,
    unassigned_warning: bool,
    sla_breach: bool,
    chat_ids: Vec<String>,
    bot_token_configured: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RoutingConfigurationResponse {
    inbox_id: Uuid,
    project_id: Uuid,
    configured: bool,
    version: i64,
    enabled: bool,
    assignment_strategy: AssignmentStrategy,
    default_queue_id: Option<Uuid>,
    queues: Vec<RoutingQueueResponse>,
    rules: Vec<RoutingRuleResponse>,
    coverage: CoverageResponse,
    sla: SlaResponse,
    telegram: TelegramResponse,
}

#[derive(Debug, FromRow, Serialize)]
struct ChannelOptionResponse {
    id: Uuid,
    name: String,
    kind: String,
    status: String,
}

#[derive(Debug, FromRow, Serialize)]
struct OperatorOptionResponse {
    id: Uuid,
    display_name: String,
    email: String,
    avatar_url: Option<String>,
    online: bool,
}

#[derive(Serialize)]
struct RoutingEnvelopeResponse {
    configuration: RoutingConfigurationResponse,
    channel_options: Vec<ChannelOptionResponse>,
    operator_options: Vec<OperatorOptionResponse>,
}

#[derive(Debug, FromRow)]
#[allow(clippy::struct_excessive_bools)]
struct PolicyRow {
    version: i64,
    enabled: bool,
    assignment_strategy: String,
    default_queue_id: Option<Uuid>,
    timezone: String,
    outside_hours_action: String,
    no_operator_action: String,
    sla_clock: String,
    first_response_seconds: Option<i32>,
    resolution_seconds: Option<i32>,
    unassigned_warning_seconds: Option<i32>,
    escalation_queue_id: Option<Uuid>,
    telegram_new_visitor: bool,
    telegram_new_message: bool,
    telegram_operator_request: bool,
    telegram_unassigned_warning: bool,
    telegram_sla_breach: bool,
    telegram_chat_ids: Vec<String>,
    bot_token_configured: bool,
}

#[derive(Debug, FromRow)]
struct QueueRow {
    id: Uuid,
    name: String,
    status: String,
    member_ids: Vec<Uuid>,
}

#[derive(Debug, FromRow)]
struct RuleRow {
    id: Uuid,
    position: i16,
    enabled: bool,
    channel_connection_id: Option<Uuid>,
    language: Option<String>,
    queue_id: Uuid,
}

#[derive(Debug, FromRow)]
struct IntervalRow {
    id: Uuid,
    weekday: i16,
    starts_at: NaiveTime,
    ends_at: NaiveTime,
}

async fn get_routing_configuration(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
) -> Result<Json<RoutingEnvelopeResponse>, AppError> {
    let project_id = require_routing_scope(&actor)?;
    let mut transaction = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await?;
    require_existing_inbox(&mut transaction, &actor, project_id, inbox_id).await?;
    let configuration =
        load_configuration(&mut transaction, actor.tenant_id, project_id, inbox_id).await?;
    let envelope = load_envelope(
        &mut transaction,
        actor.tenant_id,
        project_id,
        inbox_id,
        configuration,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(envelope))
}

async fn put_routing_configuration(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<RoutingPutRequest>,
) -> Result<Json<RoutingEnvelopeResponse>, AppError> {
    let project_id = require_routing_scope(&actor)?;
    let idempotency_key = parse_idempotency_key(&headers)?;
    if !matches!(request.telegram_bot_token, TelegramBotTokenWrite::Preserve) {
        actor.require_password_session()?;
    }
    let request_hash =
        Sha256::digest(serde_json::to_vec(&request).map_err(AppError::internal)?).to_vec();
    let request = normalize_request(request)?;

    let mut transaction = state.db.begin().await?;
    lock_existing_inbox(&mut transaction, &actor, project_id, inbox_id).await?;

    if let Some(configuration) = begin_idempotent_write(
        &mut transaction,
        actor.tenant_id,
        project_id,
        inbox_id,
        idempotency_key,
        &request_hash,
    )
    .await?
    {
        transaction.commit().await?;
        return Ok(Json(
            load_envelope_from_pool(&state, actor.tenant_id, project_id, inbox_id, configuration)
                .await?,
        ));
    }

    let current_version = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT version
        FROM inbox_routing_policies
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_optional(&mut *transaction)
    .await?
    .unwrap_or(0);
    if request.expected_version != current_version {
        return Err(AppError::Conflict(
            "routing configuration has changed; reload it and try again".to_owned(),
        ));
    }

    let token_configured = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM inbox_telegram_credentials
            WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_one(&mut *transaction)
    .await?;
    validate_post_write_telegram(&request, token_configured)?;
    validate_scoped_references(
        &mut transaction,
        actor.tenant_id,
        project_id,
        inbox_id,
        &request.configuration,
    )
    .await?;
    reconcile_open_cycles_before_replace(
        &mut transaction,
        actor.tenant_id,
        project_id,
        inbox_id,
        &request.configuration,
    )
    .await?;

    let next_version = current_version + 1;
    replace_configuration(
        &state,
        &mut transaction,
        &actor,
        project_id,
        inbox_id,
        next_version,
        &request,
    )
    .await?;
    insert_routing_audit(
        &mut transaction,
        &actor,
        project_id,
        inbox_id,
        next_version,
        &request,
    )
    .await?;

    let configuration =
        load_configuration(&mut transaction, actor.tenant_id, project_id, inbox_id).await?;
    complete_idempotent_write(
        &mut transaction,
        actor.tenant_id,
        project_id,
        inbox_id,
        idempotency_key,
        &configuration,
    )
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_envelope_from_pool(&state, actor.tenant_id, project_id, inbox_id, configuration)
            .await?,
    ))
}

fn require_routing_scope(actor: &ActorContext) -> Result<Uuid, AppError> {
    actor.require("routing:manage")?;
    actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))
}

async fn require_existing_inbox(
    connection: &mut PgConnection,
    actor: &ActorContext,
    project_id: Uuid,
    inbox_id: Uuid,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM inboxes
            WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_one(&mut *connection)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    actor.require_inbox(project_id, inbox_id)
}

async fn lock_existing_inbox(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    inbox_id: Uuid,
) -> Result<(), AppError> {
    let found = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM inboxes
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if found.is_none() {
        return Err(AppError::NotFound);
    }
    actor.require_inbox(project_id, inbox_id)
}

fn parse_idempotency_key(headers: &HeaderMap) -> Result<Uuid, AppError> {
    let value = headers
        .get("idempotency-key")
        .ok_or_else(|| AppError::BadRequest("Idempotency-Key header is required".to_owned()))?
        .to_str()
        .map_err(|_| AppError::BadRequest("Idempotency-Key header is invalid".to_owned()))?;
    let key = Uuid::parse_str(value)
        .map_err(|_| AppError::BadRequest("Idempotency-Key must be a UUID".to_owned()))?;
    if key.is_nil() {
        return Err(AppError::BadRequest(
            "Idempotency-Key must not be nil".to_owned(),
        ));
    }
    Ok(key)
}

async fn begin_idempotent_write(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    idempotency_key: Uuid,
    request_hash: &[u8],
) -> Result<Option<RoutingConfigurationResponse>, AppError> {
    sqlx::query(
        r#"
        DELETE FROM inbox_routing_idempotency
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND idempotency_key = $4 AND expires_at <= now()
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(idempotency_key)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO inbox_routing_idempotency (
            id, tenant_id, project_id, inbox_id, idempotency_key,
            request_hash, expires_at
        ) VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(hours => $7))
        ON CONFLICT (tenant_id, project_id, inbox_id, idempotency_key)
        DO NOTHING
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(idempotency_key)
    .bind(request_hash)
    .bind(IDEMPOTENCY_TTL_HOURS)
    .execute(&mut **transaction)
    .await?;
    let row = sqlx::query_as::<_, (Vec<u8>, Option<serde_json::Value>)>(
        r#"
        SELECT request_hash, response_body
        FROM inbox_routing_idempotency
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND idempotency_key = $4
        FOR UPDATE
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(idempotency_key)
    .fetch_one(&mut **transaction)
    .await?;
    if row.0 != request_hash {
        return Err(AppError::Conflict(
            "Idempotency-Key was already used for a different routing request".to_owned(),
        ));
    }
    row.1
        .map(|value| serde_json::from_value(value).map_err(AppError::internal))
        .transpose()
}

async fn complete_idempotent_write(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    idempotency_key: Uuid,
    configuration: &RoutingConfigurationResponse,
) -> Result<(), AppError> {
    let response_body = serde_json::to_value(configuration).map_err(AppError::internal)?;
    let result = sqlx::query(
        r#"
        UPDATE inbox_routing_idempotency
        SET response_body = $5, completed_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND idempotency_key = $4 AND response_body IS NULL
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(idempotency_key)
    .bind(response_body)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::internal(anyhow::anyhow!(
            "routing idempotency result could not be completed"
        )));
    }
    Ok(())
}

async fn load_configuration(
    connection: &mut PgConnection,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
) -> Result<RoutingConfigurationResponse, AppError> {
    let policy = sqlx::query_as::<_, PolicyRow>(
        r#"
        SELECT policy.version, policy.enabled, policy.assignment_strategy,
               policy.default_queue_id, policy.timezone,
               policy.outside_hours_action, policy.no_operator_action,
               policy.sla_clock, policy.first_response_seconds,
               policy.resolution_seconds, policy.unassigned_warning_seconds,
               policy.escalation_queue_id, policy.telegram_new_visitor,
               policy.telegram_new_message, policy.telegram_operator_request,
               policy.telegram_unassigned_warning, policy.telegram_sla_breach,
               policy.telegram_chat_ids,
               credential.inbox_id IS NOT NULL AS bot_token_configured
        FROM inbox_routing_policies AS policy
        LEFT JOIN inbox_telegram_credentials AS credential
          ON credential.tenant_id = policy.tenant_id
         AND credential.project_id = policy.project_id
         AND credential.inbox_id = policy.inbox_id
        WHERE policy.tenant_id = $1 AND policy.project_id = $2
          AND policy.inbox_id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(policy) = policy else {
        return Ok(default_configuration(project_id, inbox_id));
    };

    let queues = sqlx::query_as::<_, QueueRow>(
        r#"
        SELECT queue.id, queue.name, queue.status,
               COALESCE(
                   array_agg(member.user_id ORDER BY member.user_id)
                       FILTER (WHERE member.user_id IS NOT NULL),
                   ARRAY[]::uuid[]
               ) AS member_ids
        FROM inbox_routing_queues AS queue
        LEFT JOIN inbox_routing_queue_members AS member
          ON member.tenant_id = queue.tenant_id
         AND member.project_id = queue.project_id
         AND member.inbox_id = queue.inbox_id
         AND member.queue_id = queue.id
        WHERE queue.tenant_id = $1 AND queue.project_id = $2
          AND queue.inbox_id = $3
        GROUP BY queue.id, queue.name, queue.status
        ORDER BY queue.name, queue.id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_all(&mut *connection)
    .await?;
    let rules = sqlx::query_as::<_, RuleRow>(
        r#"
        SELECT id, position, enabled, channel_connection_id, language, queue_id
        FROM inbox_routing_rules
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        ORDER BY position, id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_all(&mut *connection)
    .await?;
    let intervals = sqlx::query_as::<_, IntervalRow>(
        r#"
        SELECT id, weekday, starts_at, ends_at
        FROM inbox_working_intervals
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        ORDER BY weekday, starts_at, id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_all(&mut *connection)
    .await?;

    Ok(RoutingConfigurationResponse {
        inbox_id,
        project_id,
        configured: true,
        version: policy.version,
        enabled: policy.enabled,
        assignment_strategy: assignment_strategy_from_db(&policy.assignment_strategy)?,
        default_queue_id: policy.default_queue_id,
        queues: queues
            .into_iter()
            .map(|queue| {
                Ok(RoutingQueueResponse {
                    id: queue.id,
                    name: queue.name,
                    status: queue_status_from_db(&queue.status)?,
                    member_ids: queue.member_ids,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?,
        rules: rules
            .into_iter()
            .map(|rule| {
                Ok(RoutingRuleResponse {
                    id: rule.id,
                    position: u16::try_from(rule.position).map_err(AppError::internal)?,
                    enabled: rule.enabled,
                    channel_id: rule.channel_connection_id,
                    language: rule.language,
                    queue_id: rule.queue_id,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?,
        coverage: CoverageResponse {
            timezone: policy.timezone,
            weekly_intervals: intervals
                .into_iter()
                .map(|interval| {
                    Ok(WorkingIntervalResponse {
                        id: interval.id,
                        weekday: u8::try_from(interval.weekday).map_err(AppError::internal)?,
                        starts_at: interval.starts_at.format("%H:%M").to_string(),
                        ends_at: interval.ends_at.format("%H:%M").to_string(),
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?,
            outside_hours_action: fallback_action_from_db(&policy.outside_hours_action)?,
            no_operator_action: fallback_action_from_db(&policy.no_operator_action)?,
        },
        sla: SlaResponse {
            clock: sla_clock_from_db(&policy.sla_clock)?,
            first_response_seconds: policy.first_response_seconds,
            resolution_seconds: policy.resolution_seconds,
            unassigned_warning_seconds: policy.unassigned_warning_seconds,
            escalation_queue_id: policy.escalation_queue_id,
        },
        telegram: TelegramResponse {
            new_visitor: policy.telegram_new_visitor,
            new_message: policy.telegram_new_message,
            operator_request: policy.telegram_operator_request,
            unassigned_warning: policy.telegram_unassigned_warning,
            sla_breach: policy.telegram_sla_breach,
            chat_ids: policy.telegram_chat_ids,
            bot_token_configured: policy.bot_token_configured,
        },
    })
}

fn default_configuration(project_id: Uuid, inbox_id: Uuid) -> RoutingConfigurationResponse {
    RoutingConfigurationResponse {
        inbox_id,
        project_id,
        configured: false,
        version: 0,
        enabled: false,
        assignment_strategy: AssignmentStrategy::Manual,
        default_queue_id: None,
        queues: Vec::new(),
        rules: Vec::new(),
        coverage: CoverageResponse {
            timezone: "UTC".to_owned(),
            weekly_intervals: Vec::new(),
            outside_hours_action: FallbackAction::Ai,
            no_operator_action: FallbackAction::Ai,
        },
        sla: SlaResponse {
            clock: SlaClock::Elapsed,
            first_response_seconds: None,
            resolution_seconds: None,
            unassigned_warning_seconds: None,
            escalation_queue_id: None,
        },
        telegram: TelegramResponse {
            new_visitor: false,
            new_message: false,
            operator_request: false,
            unassigned_warning: false,
            sla_breach: false,
            chat_ids: Vec::new(),
            bot_token_configured: false,
        },
    }
}

async fn load_envelope_from_pool(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    configuration: RoutingConfigurationResponse,
) -> Result<RoutingEnvelopeResponse, AppError> {
    let mut transaction = state.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await?;
    let envelope = load_envelope(
        &mut transaction,
        tenant_id,
        project_id,
        inbox_id,
        configuration,
    )
    .await?;
    transaction.commit().await?;
    Ok(envelope)
}

async fn load_envelope(
    connection: &mut PgConnection,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    configuration: RoutingConfigurationResponse,
) -> Result<RoutingEnvelopeResponse, AppError> {
    let channel_options = sqlx::query_as::<_, ChannelOptionResponse>(
        r#"
        SELECT id, name, kind, status
        FROM channel_connections
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND deleted_at IS NULL
          AND kind <> 'custom_ai'
        ORDER BY name, id
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_all(&mut *connection)
    .await?;
    let operator_options = sqlx::query_as::<_, OperatorOptionResponse>(
        r#"
        WITH eligible AS (
            SELECT DISTINCT ON (app_user.id)
                   app_user.id,
                   COALESCE(profile.display_name, app_user.display_name) AS display_name,
                   app_user.email,
                   COALESCE(
                       '/public/v1/avatars/' || stored_avatar.public_id::text,
                       profile.avatar_url
                   ) AS avatar_url
            FROM memberships AS membership
            JOIN users AS app_user
              ON app_user.id = membership.user_id
             AND app_user.status = 'active'
            JOIN project_roles AS project_role
              ON project_role.tenant_id = membership.tenant_id
             AND project_role.project_id = COALESCE((
                 SELECT role_token.role_project_id FROM api_keys AS role_token
                 WHERE role_token.tenant_id = membership.tenant_id
                   AND role_token.actor_user_id = membership.user_id
                   AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
                 LIMIT 1
             ), $2)
             AND project_role.id = membership.role
             AND NOT EXISTS (
                 SELECT 1 FROM api_keys AS role_token
                 JOIN projects AS role_source ON role_source.tenant_id = role_token.tenant_id
                   AND role_source.id = role_token.role_project_id
                 WHERE role_token.tenant_id = membership.tenant_id
                   AND role_token.actor_user_id = membership.user_id
                   AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
                   AND (role_token.revoked_at IS NOT NULL OR role_token.expires_at <= now()
                        OR role_source.status <> 'active')
             )
             AND 'conversations:reply' = ANY(project_role.permissions)
            LEFT JOIN operator_chat_profiles AS profile
              ON profile.tenant_id = membership.tenant_id
             AND profile.project_id = $2
             AND profile.user_id = app_user.id
            LEFT JOIN operator_profile_avatars AS stored_avatar
              ON stored_avatar.tenant_id = membership.tenant_id
             AND stored_avatar.project_id = $2
             AND stored_avatar.user_id = app_user.id
            WHERE membership.tenant_id = $1
              AND (membership.project_id = $2 OR membership.project_id IS NULL)
              AND membership.revoked_at IS NULL
              AND (membership.department_id IS NULL OR membership.department_id = (
                  SELECT department_id FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND id = $3
              ))
            ORDER BY app_user.id, (membership.project_id IS NULL)
        )
        SELECT eligible.id, eligible.display_name, eligible.email,
               eligible.avatar_url,
               EXISTS (
                   SELECT 1
                   FROM operator_sessions AS session
                   LEFT JOIN operator_session_projects AS workspace
                     ON workspace.tenant_id = session.tenant_id
                    AND workspace.session_id = session.id
                    AND workspace.project_id = $2
                   JOIN memberships AS session_membership
                     ON session_membership.tenant_id = session.tenant_id
                    AND session_membership.id = COALESCE(workspace.membership_id, session.membership_id)
                    AND session_membership.user_id = session.user_id
                    AND session_membership.revoked_at IS NULL
                   JOIN project_roles AS session_role
                     ON session_role.tenant_id = session_membership.tenant_id
                    AND session_role.project_id = $2
                    AND session_role.id = session_membership.role
                    AND 'conversations:reply' = ANY(session_role.permissions)
                   WHERE session.tenant_id = $1
                     AND (workspace.session_id IS NOT NULL OR session.project_id = $2)
                     AND (session_membership.project_id IS NULL OR session_membership.project_id = $2)
                     AND (session_membership.department_id IS NULL OR session_membership.department_id = (
                         SELECT department_id FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND id = $3
                     ))
                     AND session.user_id = eligible.id
                     AND session.revoked_at IS NULL
                     AND session.idle_expires_at > now()
                     AND session.absolute_expires_at > now()
                     AND CASE WHEN workspace.session_id IS NULL THEN session.presence_last_seen_at
                              ELSE workspace.presence_last_seen_at END >= now() - interval '60 seconds'
               ) AS online
        FROM eligible
        ORDER BY eligible.display_name, eligible.email, eligible.id
        LIMIT 500
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_all(&mut *connection)
    .await?;
    Ok(RoutingEnvelopeResponse {
        configuration,
        channel_options,
        operator_options,
    })
}

fn assignment_strategy_from_db(value: &str) -> Result<AssignmentStrategy, AppError> {
    match value {
        "manual" => Ok(AssignmentStrategy::Manual),
        "least_active" => Ok(AssignmentStrategy::LeastActive),
        _ => Err(AppError::internal(anyhow::anyhow!(
            "unsupported routing assignment strategy"
        ))),
    }
}

fn queue_status_from_db(value: &str) -> Result<QueueStatus, AppError> {
    match value {
        "active" => Ok(QueueStatus::Active),
        "disabled" => Ok(QueueStatus::Disabled),
        _ => Err(AppError::internal(anyhow::anyhow!(
            "unsupported routing queue status"
        ))),
    }
}

fn fallback_action_from_db(value: &str) -> Result<FallbackAction, AppError> {
    match value {
        "ai" => Ok(FallbackAction::Ai),
        "queue" => Ok(FallbackAction::Queue),
        _ => Err(AppError::internal(anyhow::anyhow!(
            "unsupported routing fallback action"
        ))),
    }
}

fn sla_clock_from_db(value: &str) -> Result<SlaClock, AppError> {
    match value {
        "elapsed" => Ok(SlaClock::Elapsed),
        "working_hours" => Ok(SlaClock::WorkingHours),
        _ => Err(AppError::internal(anyhow::anyhow!(
            "unsupported routing SLA clock"
        ))),
    }
}

fn normalize_request(mut request: RoutingPutRequest) -> Result<RoutingPutRequest, AppError> {
    if request.expected_version < 0 {
        return Err(AppError::BadRequest(
            "expected_version cannot be negative".to_owned(),
        ));
    }
    let configuration = &mut request.configuration;
    if configuration.queues.len() > MAX_QUEUES {
        return Err(AppError::BadRequest(format!(
            "routing configuration cannot contain more than {MAX_QUEUES} queues"
        )));
    }
    if configuration.rules.len() > MAX_RULES {
        return Err(AppError::BadRequest(format!(
            "routing configuration cannot contain more than {MAX_RULES} rules"
        )));
    }

    let mut queue_ids = HashSet::with_capacity(configuration.queues.len());
    let mut queue_names = HashSet::with_capacity(configuration.queues.len());
    for queue in &mut configuration.queues {
        require_non_nil_uuid(queue.id, "queue id")?;
        if !queue_ids.insert(queue.id) {
            return Err(AppError::BadRequest("queue ids must be unique".to_owned()));
        }
        queue.name =
            normalize_required_text(queue.name.clone(), MAX_QUEUE_NAME_CHARS, "queue name")?;
        if !queue_names.insert(queue.name.to_lowercase()) {
            return Err(AppError::BadRequest(
                "queue names must be unique".to_owned(),
            ));
        }
        if queue.member_ids.len() > MAX_QUEUE_MEMBERS {
            return Err(AppError::BadRequest(format!(
                "a queue cannot contain more than {MAX_QUEUE_MEMBERS} operators"
            )));
        }
        sort_and_validate_uuids(&mut queue.member_ids, "queue member ids")?;
    }
    let queue_statuses = configuration
        .queues
        .iter()
        .map(|queue| (queue.id, queue.status))
        .collect::<HashMap<_, _>>();
    validate_active_queue_reference(
        configuration.default_queue_id,
        &queue_statuses,
        "default_queue_id",
    )?;
    if matches!(
        configuration.assignment_strategy,
        AssignmentStrategy::LeastActive
    ) && configuration.default_queue_id.is_none()
    {
        return Err(AppError::BadRequest(
            "default_queue_id is required for least_active assignment".to_owned(),
        ));
    }
    if (matches!(
        configuration.coverage.outside_hours_action,
        FallbackAction::Queue
    ) || matches!(
        configuration.coverage.no_operator_action,
        FallbackAction::Queue
    )) && configuration.default_queue_id.is_none()
    {
        return Err(AppError::BadRequest(
            "default_queue_id is required for queue fallback actions".to_owned(),
        ));
    }
    validate_active_queue_reference(
        configuration.sla.escalation_queue_id,
        &queue_statuses,
        "sla.escalation_queue_id",
    )?;

    configuration.rules.sort_by_key(|rule| rule.position);
    let mut rule_ids = HashSet::with_capacity(configuration.rules.len());
    for (index, rule) in configuration.rules.iter_mut().enumerate() {
        require_non_nil_uuid(rule.id, "rule id")?;
        require_non_nil_uuid(rule.queue_id, "rule queue id")?;
        if !rule_ids.insert(rule.id) {
            return Err(AppError::BadRequest("rule ids must be unique".to_owned()));
        }
        let expected_position = u16::try_from(index + 1).map_err(AppError::internal)?;
        if rule.position != expected_position {
            return Err(AppError::BadRequest(
                "rule positions must be contiguous and start at 1".to_owned(),
            ));
        }
        if let Some(channel_id) = rule.channel_id {
            require_non_nil_uuid(channel_id, "rule channel id")?;
        }
        rule.language = normalize_language(rule.language.take())?;
        if rule.channel_id.is_none() && rule.language.is_none() {
            return Err(AppError::BadRequest(
                "each routing rule must match a channel, a language, or both".to_owned(),
            ));
        }
        let Some(status) = queue_statuses.get(&rule.queue_id) else {
            return Err(AppError::BadRequest(
                "every routing rule must reference a queue in this configuration".to_owned(),
            ));
        };
        if rule.enabled && *status != QueueStatus::Active {
            return Err(AppError::BadRequest(
                "enabled routing rules must reference active queues".to_owned(),
            ));
        }
    }

    let parsed_timezone = configuration
        .coverage
        .timezone
        .trim()
        .parse::<Tz>()
        .map_err(|_| {
            AppError::BadRequest("coverage.timezone must be an IANA timezone".to_owned())
        })?;
    configuration.coverage.timezone = parsed_timezone.to_string();
    normalize_intervals(&mut configuration.coverage.weekly_intervals)?;
    validate_sla(&configuration.sla, &configuration.coverage)?;
    normalize_chat_ids(&mut configuration.telegram.chat_ids)?;
    if configuration.telegram.any_enabled() && configuration.telegram.chat_ids.is_empty() {
        return Err(AppError::BadRequest(
            "telegram.chat_ids is required when a Telegram event is enabled".to_owned(),
        ));
    }

    if let TelegramBotTokenWrite::Replace { value } = &mut request.telegram_bot_token {
        *value = value.trim().to_owned();
        if !valid_bot_token(value) {
            return Err(AppError::BadRequest(
                "Telegram bot token has an invalid format".to_owned(),
            ));
        }
    }
    Ok(request)
}

fn normalize_required_text(
    value: String,
    max_chars: usize,
    field: &str,
) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars || value.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(format!(
            "{field} must contain between 1 and {max_chars} printable characters"
        )));
    }
    Ok(value.to_owned())
}

fn require_non_nil_uuid(value: Uuid, field: &str) -> Result<(), AppError> {
    if value.is_nil() {
        Err(AppError::BadRequest(format!("{field} must not be nil")))
    } else {
        Ok(())
    }
}

fn sort_and_validate_uuids(values: &mut [Uuid], field: &str) -> Result<(), AppError> {
    if values.iter().any(Uuid::is_nil) {
        return Err(AppError::BadRequest(format!(
            "{field} must not contain nil UUIDs"
        )));
    }
    values.sort_unstable();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(AppError::BadRequest(format!(
            "{field} must not contain duplicates"
        )));
    }
    Ok(())
}

fn validate_active_queue_reference(
    queue_id: Option<Uuid>,
    queue_statuses: &HashMap<Uuid, QueueStatus>,
    field: &str,
) -> Result<(), AppError> {
    let Some(queue_id) = queue_id else {
        return Ok(());
    };
    require_non_nil_uuid(queue_id, field)?;
    if queue_statuses.get(&queue_id) != Some(&QueueStatus::Active) {
        return Err(AppError::BadRequest(format!(
            "{field} must reference an active queue in this configuration"
        )));
    }
    Ok(())
}

fn normalize_language(value: Option<String>) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().replace('_', "-").to_ascii_lowercase();
    if value.is_empty() {
        return Ok(None);
    }
    let mut segments = value.split('-');
    let Some(primary) = segments.next() else {
        return Ok(None);
    };
    let primary_valid =
        (2..=8).contains(&primary.len()) && primary.bytes().all(|byte| byte.is_ascii_lowercase());
    let rest_valid = segments.all(|segment| {
        (1..=8).contains(&segment.len()) && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())
    });
    if value.len() > 35 || !primary_valid || !rest_valid {
        return Err(AppError::BadRequest(
            "rule language must be a normalized BCP 47 language tag".to_owned(),
        ));
    }
    Ok(Some(value))
}

fn normalize_intervals(intervals: &mut [WorkingIntervalInput]) -> Result<(), AppError> {
    if intervals.len() > MAX_WORKING_INTERVALS {
        return Err(AppError::BadRequest(format!(
            "coverage cannot contain more than {MAX_WORKING_INTERVALS} weekly intervals"
        )));
    }
    let mut ids = HashSet::with_capacity(intervals.len());
    let mut by_weekday = HashMap::<u8, Vec<(NaiveTime, NaiveTime)>>::new();
    for interval in intervals.iter_mut() {
        require_non_nil_uuid(interval.id, "working interval id")?;
        if !ids.insert(interval.id) {
            return Err(AppError::BadRequest(
                "working interval ids must be unique".to_owned(),
            ));
        }
        if !(1..=7).contains(&interval.weekday) {
            return Err(AppError::BadRequest(
                "working interval weekday must be between 1 and 7".to_owned(),
            ));
        }
        let starts_at = parse_local_time(&interval.starts_at)?;
        let ends_at = parse_local_time(&interval.ends_at)?;
        if starts_at >= ends_at {
            return Err(AppError::BadRequest(
                "working intervals must end after they start; split overnight intervals".to_owned(),
            ));
        }
        interval.starts_at = starts_at.format("%H:%M").to_string();
        interval.ends_at = ends_at.format("%H:%M").to_string();
        by_weekday
            .entry(interval.weekday)
            .or_default()
            .push((starts_at, ends_at));
    }
    for day_intervals in by_weekday.values_mut() {
        day_intervals.sort_unstable_by_key(|interval| interval.0);
        if day_intervals.windows(2).any(|pair| pair[1].0 < pair[0].1) {
            return Err(AppError::BadRequest(
                "working intervals on the same weekday must not overlap".to_owned(),
            ));
        }
    }
    intervals.sort_by_key(|interval| (interval.weekday, interval.starts_at.clone(), interval.id));
    Ok(())
}

fn parse_local_time(value: &str) -> Result<NaiveTime, AppError> {
    NaiveTime::parse_from_str(value.trim(), "%H:%M").map_err(|_| {
        AppError::BadRequest("working interval times must use HH:MM format".to_owned())
    })
}

fn validate_sla(sla: &SlaInput, coverage: &CoverageInput) -> Result<(), AppError> {
    validate_optional_seconds(
        sla.first_response_seconds,
        MAX_FIRST_RESPONSE_SECONDS,
        "sla.first_response_seconds",
    )?;
    validate_optional_seconds(
        sla.unassigned_warning_seconds,
        MAX_FIRST_RESPONSE_SECONDS,
        "sla.unassigned_warning_seconds",
    )?;
    validate_optional_seconds(
        sla.resolution_seconds,
        MAX_RESOLUTION_SECONDS,
        "sla.resolution_seconds",
    )?;
    if let (Some(warning), Some(first_response)) =
        (sla.unassigned_warning_seconds, sla.first_response_seconds)
        && warning >= first_response
    {
        return Err(AppError::BadRequest(
            "sla.unassigned_warning_seconds must be less than first_response_seconds".to_owned(),
        ));
    }
    if let (Some(first_response), Some(resolution)) =
        (sla.first_response_seconds, sla.resolution_seconds)
        && first_response >= resolution
    {
        return Err(AppError::BadRequest(
            "sla.resolution_seconds must be greater than first_response_seconds".to_owned(),
        ));
    }
    if matches!(sla.clock, SlaClock::WorkingHours)
        && (sla.first_response_seconds.is_some()
            || sla.resolution_seconds.is_some()
            || sla.unassigned_warning_seconds.is_some())
    {
        if coverage.weekly_intervals.is_empty() {
            return Err(AppError::BadRequest(
                "working_hours SLA requires at least one weekly interval".to_owned(),
            ));
        }
        let weekly_seconds = coverage.weekly_intervals.iter().try_fold(
            0_i64,
            |total, interval| -> Result<i64, AppError> {
                let starts_at = parse_local_time(&interval.starts_at)?;
                let ends_at = parse_local_time(&interval.ends_at)?;
                Ok(total + (ends_at - starts_at).num_seconds())
            },
        )?;
        let feasible_seconds = weekly_seconds.saturating_mul(MAX_WORKING_SLA_WEEKS);
        for (value, field) in [
            (
                sla.unassigned_warning_seconds,
                "sla.unassigned_warning_seconds",
            ),
            (sla.first_response_seconds, "sla.first_response_seconds"),
            (sla.resolution_seconds, "sla.resolution_seconds"),
        ] {
            if value.is_some_and(|seconds| i64::from(seconds) > feasible_seconds) {
                return Err(AppError::BadRequest(format!(
                    "{field} exceeds 520 weeks of the configured working schedule"
                )));
            }
        }
    }
    Ok(())
}

fn validate_optional_seconds(
    value: Option<i32>,
    maximum: i32,
    field: &str,
) -> Result<(), AppError> {
    if value.is_some_and(|seconds| !(MIN_SLA_SECONDS..=maximum).contains(&seconds)) {
        return Err(AppError::BadRequest(format!(
            "{field} must be between {MIN_SLA_SECONDS} and {maximum} seconds"
        )));
    }
    Ok(())
}

fn normalize_chat_ids(chat_ids: &mut [String]) -> Result<(), AppError> {
    if chat_ids.len() > MAX_TELEGRAM_CHAT_IDS {
        return Err(AppError::BadRequest(format!(
            "telegram.chat_ids cannot contain more than {MAX_TELEGRAM_CHAT_IDS} recipients"
        )));
    }
    for chat_id in chat_ids.iter_mut() {
        *chat_id = chat_id.trim().to_owned();
        if !valid_chat_id(chat_id) {
            return Err(AppError::BadRequest(
                "telegram.chat_ids contains an invalid chat id".to_owned(),
            ));
        }
    }
    chat_ids.sort();
    if chat_ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(AppError::BadRequest(
            "telegram.chat_ids must not contain duplicates".to_owned(),
        ));
    }
    Ok(())
}

fn valid_bot_token(value: &str) -> bool {
    if value.len() > 256 {
        return false;
    }
    let Some((bot_id, secret)) = value.split_once(':') else {
        return false;
    };
    !bot_id.is_empty()
        && bot_id.bytes().all(|byte| byte.is_ascii_digit())
        && secret.len() >= 20
        && secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_chat_id(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty() && digits.len() <= 20 && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_post_write_telegram(
    request: &RoutingPutRequest,
    currently_configured: bool,
) -> Result<(), AppError> {
    let configured_after_write = match request.telegram_bot_token {
        TelegramBotTokenWrite::Preserve => currently_configured,
        TelegramBotTokenWrite::Replace { .. } => true,
        TelegramBotTokenWrite::Clear => false,
    };
    if request.configuration.telegram.any_enabled() && !configured_after_write {
        return Err(AppError::BadRequest(
            "a Telegram bot token is required when a Telegram event is enabled".to_owned(),
        ));
    }
    Ok(())
}

async fn validate_scoped_references(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    configuration: &RoutingConfigurationInput,
) -> Result<(), AppError> {
    let channel_ids = configuration
        .rules
        .iter()
        .filter_map(|rule| rule.channel_id)
        .collect::<HashSet<_>>();
    if !channel_ids.is_empty() {
        let channel_ids = channel_ids.into_iter().collect::<Vec<_>>();
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM channel_connections
            WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
              AND deleted_at IS NULL AND id = ANY($4)
              AND kind <> 'custom_ai'
            "#,
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(inbox_id)
        .bind(&channel_ids)
        .fetch_one(&mut **transaction)
        .await?;
        let expected = i64::try_from(channel_ids.len()).map_err(AppError::internal)?;
        if count != expected {
            return Err(AppError::BadRequest(
                "routing rule channels must belong to this Inbox".to_owned(),
            ));
        }
    }

    let member_ids = configuration
        .queues
        .iter()
        .flat_map(|queue| queue.member_ids.iter().copied())
        .collect::<HashSet<_>>();
    if !member_ids.is_empty() {
        let member_ids = member_ids.into_iter().collect::<Vec<_>>();
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(DISTINCT membership.user_id)
            FROM memberships AS membership
            JOIN users AS app_user
              ON app_user.id = membership.user_id
             AND app_user.status = 'active'
            JOIN project_roles AS project_role
              ON project_role.tenant_id = membership.tenant_id
             AND project_role.project_id = COALESCE((
                 SELECT role_token.role_project_id FROM api_keys AS role_token
                 WHERE role_token.tenant_id = membership.tenant_id
                   AND role_token.actor_user_id = membership.user_id
                   AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
                 LIMIT 1
             ), $2)
             AND project_role.id = membership.role
             AND NOT EXISTS (
                 SELECT 1 FROM api_keys AS role_token
                 JOIN projects AS role_source ON role_source.tenant_id = role_token.tenant_id
                   AND role_source.id = role_token.role_project_id
                 WHERE role_token.tenant_id = membership.tenant_id
                   AND role_token.actor_user_id = membership.user_id
                   AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
                   AND (role_token.revoked_at IS NOT NULL OR role_token.expires_at <= now()
                        OR role_source.status <> 'active')
             )
             AND 'conversations:reply' = ANY(project_role.permissions)
            WHERE membership.tenant_id = $1
              AND (membership.project_id = $2 OR membership.project_id IS NULL)
              AND membership.revoked_at IS NULL
              AND membership.user_id = ANY($3)
              AND (membership.department_id IS NULL OR membership.department_id = (
                  SELECT department_id FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND id = $4
              ))
            "#,
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(&member_ids)
        .bind(inbox_id)
        .fetch_one(&mut **transaction)
        .await?;
        let expected = i64::try_from(member_ids.len()).map_err(AppError::internal)?;
        if count != expected {
            return Err(AppError::BadRequest(
                "queue operators must be active project members allowed to reply".to_owned(),
            ));
        }
    }
    Ok(())
}

fn active_submitted_queue_ids(configuration: &RoutingConfigurationInput) -> Vec<Uuid> {
    configuration
        .queues
        .iter()
        .filter(|queue| queue.status == QueueStatus::Active)
        .map(|queue| queue.id)
        .collect()
}

async fn reconcile_open_cycles_before_replace(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    configuration: &RoutingConfigurationInput,
) -> Result<(), AppError> {
    if !configuration.enabled {
        sqlx::query(
            r#"
            UPDATE conversation_routing_cycles
            SET closed_at = now(), updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
              AND closed_at IS NULL
            "#,
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(inbox_id)
        .execute(&mut **transaction)
        .await?;
        return Ok(());
    }

    let active_queue_ids = active_submitted_queue_ids(configuration);
    let invalid_queue_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT queue_id
        FROM conversation_routing_cycles
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND closed_at IS NULL AND queue_id IS NOT NULL
          AND NOT (queue_id = ANY($4))
        ORDER BY started_at, conversation_id, cycle_number
        LIMIT 1
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(&active_queue_ids)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(queue_id) = invalid_queue_id {
        return Err(AppError::Conflict(format!(
            "open routing cycles still use queue {queue_id}; resolve those conversations or keep the queue active"
        )));
    }
    Ok(())
}

async fn replace_configuration(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    inbox_id: Uuid,
    next_version: i64,
    request: &RoutingPutRequest,
) -> Result<(), AppError> {
    let tenant_id = actor.tenant_id;
    let configuration = &request.configuration;

    sqlx::query(
        r#"
        UPDATE inbox_routing_policies
        SET assignment_strategy = 'manual', default_queue_id = NULL,
            outside_hours_action = 'ai', no_operator_action = 'ai',
            escalation_queue_id = NULL
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM inbox_routing_rules WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM inbox_working_intervals WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        UPDATE inbox_routing_queues
        SET name = '__routing_tmp_' || id::text, updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .execute(&mut **transaction)
    .await?;
    let queue_ids = configuration
        .queues
        .iter()
        .map(|queue| queue.id)
        .collect::<Vec<_>>();
    sqlx::query(
        r#"
        DELETE FROM inbox_routing_queues
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND NOT (id = ANY($4))
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(&queue_ids)
    .execute(&mut **transaction)
    .await?;
    for queue in &configuration.queues {
        sqlx::query(
            r#"
            INSERT INTO inbox_routing_queues (
                id, tenant_id, project_id, inbox_id, name, status,
                created_by, updated_by
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
            ON CONFLICT (tenant_id, project_id, inbox_id, id)
            DO UPDATE SET name = EXCLUDED.name, status = EXCLUDED.status,
                          updated_by = EXCLUDED.updated_by, updated_at = now()
            "#,
        )
        .bind(queue.id)
        .bind(tenant_id)
        .bind(project_id)
        .bind(inbox_id)
        .bind(&queue.name)
        .bind(queue.status.as_str())
        .bind(actor.actor_id)
        .execute(&mut **transaction)
        .await
        .map_err(routing_write_error)?;
    }

    sqlx::query(
        r#"
        INSERT INTO inbox_routing_policies (
            tenant_id, project_id, inbox_id, enabled, version,
            assignment_strategy, default_queue_id, timezone,
            outside_hours_action, no_operator_action, sla_clock,
            first_response_seconds, resolution_seconds,
            unassigned_warning_seconds, escalation_queue_id,
            telegram_new_visitor, telegram_new_message,
            telegram_operator_request, telegram_unassigned_warning,
            telegram_sla_breach, telegram_chat_ids, created_by, updated_by
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
            $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $22
        )
        ON CONFLICT (tenant_id, project_id, inbox_id)
        DO UPDATE SET enabled = EXCLUDED.enabled,
                      version = EXCLUDED.version,
                      assignment_strategy = EXCLUDED.assignment_strategy,
                      default_queue_id = EXCLUDED.default_queue_id,
                      timezone = EXCLUDED.timezone,
                      outside_hours_action = EXCLUDED.outside_hours_action,
                      no_operator_action = EXCLUDED.no_operator_action,
                      sla_clock = EXCLUDED.sla_clock,
                      first_response_seconds = EXCLUDED.first_response_seconds,
                      resolution_seconds = EXCLUDED.resolution_seconds,
                      unassigned_warning_seconds = EXCLUDED.unassigned_warning_seconds,
                      escalation_queue_id = EXCLUDED.escalation_queue_id,
                      telegram_new_visitor = EXCLUDED.telegram_new_visitor,
                      telegram_new_message = EXCLUDED.telegram_new_message,
                      telegram_operator_request = EXCLUDED.telegram_operator_request,
                      telegram_unassigned_warning = EXCLUDED.telegram_unassigned_warning,
                      telegram_sla_breach = EXCLUDED.telegram_sla_breach,
                      telegram_chat_ids = EXCLUDED.telegram_chat_ids,
                      updated_by = EXCLUDED.updated_by,
                      updated_at = now()
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(configuration.enabled)
    .bind(next_version)
    .bind(configuration.assignment_strategy.as_str())
    .bind(configuration.default_queue_id)
    .bind(&configuration.coverage.timezone)
    .bind(configuration.coverage.outside_hours_action.as_str())
    .bind(configuration.coverage.no_operator_action.as_str())
    .bind(configuration.sla.clock.as_str())
    .bind(configuration.sla.first_response_seconds)
    .bind(configuration.sla.resolution_seconds)
    .bind(configuration.sla.unassigned_warning_seconds)
    .bind(configuration.sla.escalation_queue_id)
    .bind(configuration.telegram.new_visitor)
    .bind(configuration.telegram.new_message)
    .bind(configuration.telegram.operator_request)
    .bind(configuration.telegram.unassigned_warning)
    .bind(configuration.telegram.sla_breach)
    .bind(&configuration.telegram.chat_ids)
    .bind(actor.actor_id)
    .execute(&mut **transaction)
    .await
    .map_err(routing_write_error)?;

    for queue in &configuration.queues {
        sqlx::query(
            r#"
            DELETE FROM inbox_routing_queue_members
            WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
              AND queue_id = $4 AND NOT (user_id = ANY($5))
            "#,
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(inbox_id)
        .bind(queue.id)
        .bind(&queue.member_ids)
        .execute(&mut **transaction)
        .await?;
        for member_id in &queue.member_ids {
            sqlx::query(
                r#"
                INSERT INTO inbox_routing_queue_members (
                    tenant_id, project_id, inbox_id, queue_id, user_id, created_by
                ) VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (tenant_id, project_id, inbox_id, queue_id, user_id)
                DO NOTHING
                "#,
            )
            .bind(tenant_id)
            .bind(project_id)
            .bind(inbox_id)
            .bind(queue.id)
            .bind(member_id)
            .bind(actor.actor_id)
            .execute(&mut **transaction)
            .await
            .map_err(routing_write_error)?;
        }
    }

    for rule in &configuration.rules {
        let position = i16::try_from(rule.position).map_err(AppError::internal)?;
        sqlx::query(
            r#"
            INSERT INTO inbox_routing_rules (
                id, tenant_id, project_id, inbox_id, position, enabled,
                channel_connection_id, language, queue_id, created_by, updated_by
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
            "#,
        )
        .bind(rule.id)
        .bind(tenant_id)
        .bind(project_id)
        .bind(inbox_id)
        .bind(position)
        .bind(rule.enabled)
        .bind(rule.channel_id)
        .bind(&rule.language)
        .bind(rule.queue_id)
        .bind(actor.actor_id)
        .execute(&mut **transaction)
        .await
        .map_err(routing_write_error)?;
    }

    for interval in &configuration.coverage.weekly_intervals {
        let starts_at =
            NaiveTime::parse_from_str(&interval.starts_at, "%H:%M").map_err(AppError::internal)?;
        let ends_at =
            NaiveTime::parse_from_str(&interval.ends_at, "%H:%M").map_err(AppError::internal)?;
        sqlx::query(
            r#"
            INSERT INTO inbox_working_intervals (
                id, tenant_id, project_id, inbox_id, weekday, starts_at, ends_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(interval.id)
        .bind(tenant_id)
        .bind(project_id)
        .bind(inbox_id)
        .bind(i16::from(interval.weekday))
        .bind(starts_at)
        .bind(ends_at)
        .execute(&mut **transaction)
        .await
        .map_err(routing_write_error)?;
    }

    match &request.telegram_bot_token {
        TelegramBotTokenWrite::Preserve => {}
        TelegramBotTokenWrite::Clear => {
            sqlx::query(
                r#"
                DELETE FROM inbox_telegram_credentials
                WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
                "#,
            )
            .bind(tenant_id)
            .bind(project_id)
            .bind(inbox_id)
            .execute(&mut **transaction)
            .await?;
        }
        TelegramBotTokenWrite::Replace { value } => {
            let key = load_secret_encryption_key(state)?;
            let aad =
                telegram_notifications::routing_bot_token_aad(tenant_id, project_id, inbox_id);
            let encrypted = encrypt_secret(
                &key,
                value.as_bytes(),
                state.config.secrets.key_version.clone(),
                &aad,
            )?;
            sqlx::query(
                r#"
                INSERT INTO inbox_telegram_credentials (
                    tenant_id, project_id, inbox_id, encrypted_bot_token,
                    bot_token_nonce, key_version, created_by, updated_by
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
                ON CONFLICT (tenant_id, project_id, inbox_id)
                DO UPDATE SET encrypted_bot_token = EXCLUDED.encrypted_bot_token,
                              bot_token_nonce = EXCLUDED.bot_token_nonce,
                              key_version = EXCLUDED.key_version,
                              updated_by = EXCLUDED.updated_by,
                              updated_at = now()
                "#,
            )
            .bind(tenant_id)
            .bind(project_id)
            .bind(inbox_id)
            .bind(encrypted.ciphertext)
            .bind(encrypted.nonce.as_slice())
            .bind(encrypted.key_version)
            .bind(actor.actor_id)
            .execute(&mut **transaction)
            .await
            .map_err(routing_write_error)?;
        }
    }
    Ok(())
}

async fn insert_routing_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    inbox_id: Uuid,
    version: i64,
    request: &RoutingPutRequest,
) -> Result<(), AppError> {
    let token_action = match request.telegram_bot_token {
        TelegramBotTokenWrite::Preserve => "preserve",
        TelegramBotTokenWrite::Replace { .. } => "replace",
        TelegramBotTokenWrite::Clear => "clear",
    };
    let telegram_event_count = [
        request.configuration.telegram.new_visitor,
        request.configuration.telegram.new_message,
        request.configuration.telegram.operator_request,
        request.configuration.telegram.unassigned_warning,
        request.configuration.telegram.sla_breach,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count();
    let metadata = json!({
        "version": version,
        "enabled": request.configuration.enabled,
        "queue_count": request.configuration.queues.len(),
        "rule_count": request.configuration.rules.len(),
        "working_interval_count": request.configuration.coverage.weekly_intervals.len(),
        "telegram_event_count": telegram_event_count,
        "telegram_bot_token_action": token_action,
    });
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, 'inbox_routing.updated',
                  'inbox_routing', $5, $6)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(inbox_id)
    .bind(metadata)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn routing_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("routing queue, rule, or interval identifiers must be unique".to_owned())
    } else if error.as_database_error().is_some_and(|database_error| {
        database_error.is_foreign_key_violation() || database_error.is_check_violation()
    }) {
        AppError::BadRequest("routing configuration contains an invalid reference".to_owned())
    } else {
        AppError::Database(error)
    }
}

#[derive(Debug, FromRow)]
struct RuntimePolicyRow {
    version: i64,
    assignment_strategy: String,
    default_queue_id: Option<Uuid>,
    timezone: String,
    outside_hours_action: String,
    no_operator_action: String,
    sla_clock: String,
    first_response_seconds: Option<i32>,
    resolution_seconds: Option<i32>,
    unassigned_warning_seconds: Option<i32>,
    telegram_new_message: bool,
    telegram_chat_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, FromRow)]
struct RuntimeIntervalRow {
    weekday: i16,
    starts_at: NaiveTime,
    ends_at: NaiveTime,
}

#[derive(Debug, FromRow)]
struct RuntimeRuleRow {
    id: Uuid,
    channel_connection_id: Option<Uuid>,
    language: Option<String>,
    queue_id: Uuid,
}

#[derive(Debug, FromRow)]
struct RuntimeConversationRow {
    contact_id: Uuid,
}

#[derive(Clone, Copy, Debug, FromRow)]
struct RuntimeCycleRow {
    queue_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct DueSlaCycleRow {
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    conversation_id: Uuid,
    cycle_number: i32,
    queue_id: Option<Uuid>,
    warning_due_at: Option<DateTime<Utc>>,
    first_response_due_at: Option<DateTime<Utc>>,
    resolution_due_at: Option<DateTime<Utc>>,
    first_response_at: Option<DateTime<Utc>>,
    warning_triggered_at: Option<DateTime<Utc>>,
    breached_at: Option<DateTime<Utc>>,
    resolution_breached_at: Option<DateTime<Utc>>,
    contact_id: Uuid,
    conversation_status: String,
    policy_enabled: bool,
    escalation_queue_id: Option<Uuid>,
    telegram_unassigned_warning: bool,
    telegram_sla_breach: bool,
    telegram_chat_ids: Vec<String>,
    claimed_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct CurrentSlaPolicyRow {
    enabled: bool,
    escalation_queue_id: Option<Uuid>,
    telegram_unassigned_warning: bool,
    telegram_sla_breach: bool,
    telegram_chat_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlaDueKind {
    UnassignedWarning,
    FirstResponse,
    Resolution,
}

/// Scope and source metadata for one newly persisted inbound message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundContext {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub inbox_id: Uuid,
    pub conversation_id: Uuid,
    pub channel_connection_id: Uuid,
    pub widget_language: Option<String>,
}

/// Routing outcome consumed by the conversation message flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InboundDecision {
    pub routing_configured: bool,
    pub allow_ai: bool,
    pub operator_assigned: bool,
}

/// Enqueues a routed visitor alert and reports whether this Inbox owns Telegram delivery.
///
/// # Errors
///
/// Returns an application error when routing state cannot be inspected or the alert cannot be
/// persisted.
#[allow(clippy::too_many_arguments)]
pub async fn on_new_visitor(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_connection_id: Uuid,
    widget_name: &str,
    _page: &str,
) -> Result<bool, AppError> {
    let routing = sqlx::query_as::<_, (bool, Vec<String>)>(
        r#"
        SELECT telegram_new_visitor, telegram_chat_ids
        FROM inbox_routing_policies
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND enabled
        FOR SHARE
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((notify, chat_ids)) = routing else {
        return Ok(false);
    };
    if notify {
        let text = format!(
            "Новый посетитель Inbox.\nВиджет: {}\nКанал: {channel_connection_id}",
            safe_alert_value(widget_name, 200),
        );
        enqueue_routing_alerts(
            transaction,
            tenant_id,
            project_id,
            inbox_id,
            &chat_ids,
            RoutingTelegramNotificationKind::NewVisitor,
            &text,
        )
        .await?;
    }
    Ok(true)
}

/// Applies Inbox routing to a newly persisted inbound message.
///
/// # Errors
///
/// Returns an application error when routing state cannot be inspected or changed.
pub async fn on_inbound_message(
    _state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    context: &InboundContext,
) -> Result<InboundDecision, AppError> {
    let Some(policy) = load_runtime_policy(transaction, context).await? else {
        return Ok(InboundDecision {
            routing_configured: false,
            allow_ai: true,
            operator_assigned: false,
        });
    };
    let now = database_now(transaction).await?;
    let timezone = policy.timezone.parse::<Tz>().map_err(|_| {
        AppError::internal(anyhow::anyhow!("stored Inbox routing timezone is invalid"))
    })?;
    let intervals = load_runtime_intervals(transaction, context).await?;
    let working_now = is_within_working_hours(now, timezone, &intervals);
    let conversation = sqlx::query_as::<_, RuntimeConversationRow>(
        r#"
        SELECT contact_id
        FROM conversations
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3 AND id = $4
        FOR UPDATE
        "#,
    )
    .bind(context.tenant_id)
    .bind(context.project_id)
    .bind(context.inbox_id)
    .bind(context.conversation_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    let existing_cycle = sqlx::query_as::<_, RuntimeCycleRow>(
        r#"
        SELECT queue_id
        FROM conversation_routing_cycles
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND conversation_id = $4 AND closed_at IS NULL
        ORDER BY cycle_number DESC
        LIMIT 1
        "#,
    )
    .bind(context.tenant_id)
    .bind(context.project_id)
    .bind(context.inbox_id)
    .bind(context.conversation_id)
    .fetch_optional(&mut **transaction)
    .await?;

    let cycle = if let Some(cycle) = existing_cycle {
        cycle
    } else {
        let rules = load_runtime_rules(transaction, context).await?;
        let widget_language = context
            .widget_language
            .as_ref()
            .and_then(|language| normalize_language(Some(language.clone())).ok().flatten());
        let matched_rule = rules.iter().find(|rule| {
            runtime_rule_matches(
                rule,
                context.channel_connection_id,
                widget_language.as_deref(),
            )
        });
        let queue_id = matched_rule
            .map(|rule| rule.queue_id)
            .or(policy.default_queue_id);
        let warning_due_at = sla_deadline(
            now,
            policy.unassigned_warning_seconds,
            &policy.sla_clock,
            timezone,
            &intervals,
        )?;
        let first_response_due_at = sla_deadline(
            now,
            policy.first_response_seconds,
            &policy.sla_clock,
            timezone,
            &intervals,
        )?;
        let resolution_due_at = sla_deadline(
            now,
            policy.resolution_seconds,
            &policy.sla_clock,
            timezone,
            &intervals,
        )?;
        sqlx::query_as::<_, RuntimeCycleRow>(
            r#"
            INSERT INTO conversation_routing_cycles (
                tenant_id, project_id, inbox_id, conversation_id, cycle_number,
                policy_version, rule_id, queue_id, started_at, warning_due_at,
                first_response_due_at, resolution_due_at
            ) VALUES (
                $1, $2, $3, $4,
                (
                    SELECT COALESCE(MAX(existing.cycle_number), -1) + 1
                    FROM conversation_routing_cycles AS existing
                    WHERE existing.tenant_id = $1
                      AND existing.project_id = $2
                      AND existing.inbox_id = $3
                      AND existing.conversation_id = $4
                ),
                $5, $6, $7, $8, $9, $10, $11
            )
            RETURNING queue_id
            "#,
        )
        .bind(context.tenant_id)
        .bind(context.project_id)
        .bind(context.inbox_id)
        .bind(context.conversation_id)
        .bind(policy.version)
        .bind(matched_rule.map(|rule| rule.id))
        .bind(queue_id)
        .bind(now)
        .bind(warning_due_at)
        .bind(first_response_due_at)
        .bind(resolution_due_at)
        .fetch_one(&mut **transaction)
        .await?
    };

    let current_operator =
        active_operator_id(transaction, context.tenant_id, context.conversation_id).await?;
    let mut operator_assigned = current_operator.is_some();
    let mut online_operator_available = operator_assigned;
    if !operator_assigned
        && working_now
        && let Some(queue_id) = cycle.queue_id
    {
        let candidate = select_online_queue_operator(
            transaction,
            context.tenant_id,
            context.project_id,
            context.inbox_id,
            queue_id,
            None,
        )
        .await?;
        online_operator_available = candidate.is_some();
        if policy.assignment_strategy == "least_active"
            && let Some(user_id) = candidate
        {
            assign_operator(
                transaction,
                context.tenant_id,
                context.project_id,
                context.inbox_id,
                conversation.contact_id,
                context.conversation_id,
                queue_id,
                user_id,
                now,
                "initial_route",
            )
            .await?;
            operator_assigned = true;
        }
    }

    let allow_ai = if operator_assigned {
        false
    } else if !working_now {
        policy.outside_hours_action == "ai"
    } else if cycle.queue_id.is_none() {
        true
    } else {
        !online_operator_available && policy.no_operator_action == "ai"
    };

    if policy.telegram_new_message {
        let destination = if operator_assigned {
            "оператор"
        } else if allow_ai {
            "ИИ"
        } else {
            "очередь"
        };
        let text = format!(
            "Новое сообщение в Inbox.\nЧат: {}\nМаршрут: {destination}",
            context.conversation_id
        );
        enqueue_routing_alerts(
            transaction,
            context.tenant_id,
            context.project_id,
            context.inbox_id,
            &policy.telegram_chat_ids,
            RoutingTelegramNotificationKind::NewMessage,
            &text,
        )
        .await?;
    }

    Ok(InboundDecision {
        routing_configured: true,
        allow_ai,
        operator_assigned,
    })
}

/// Records a customer-visible outbound response for the active SLA cycle.
///
/// # Errors
///
/// Returns an application error when the SLA cycle cannot be updated.
pub async fn on_outbound_message(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    conversation_id: Uuid,
    sender_kind: &str,
) -> Result<(), AppError> {
    if !matches!(sender_kind, "operator" | "ai") {
        return Ok(());
    }
    sqlx::query(
        r#"
        UPDATE conversation_routing_cycles
        SET first_response_at = now(), updated_at = now()
        WHERE tenant_id = $1 AND conversation_id = $2
          AND first_response_at IS NULL AND closed_at IS NULL
        "#,
    )
    .bind(tenant_id)
    .bind(conversation_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Closes pending routing deadlines after a conversation is resolved.
///
/// # Errors
///
/// Returns an application error when pending deadlines cannot be closed.
pub async fn on_conversation_resolved(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE conversation_routing_cycles
        SET closed_at = now(), updated_at = now()
        WHERE tenant_id = $1 AND conversation_id = $2 AND closed_at IS NULL
        "#,
    )
    .bind(tenant_id)
    .bind(conversation_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Processes at most one due Inbox routing SLA cycle.
///
/// # Errors
///
/// Returns an error when a due SLA cycle cannot be claimed or processed.
pub async fn process_sla_once(state: &AppState, worker_id: Uuid) -> anyhow::Result<bool> {
    let mut transaction = state
        .db
        .begin()
        .await
        .context("failed to start an Inbox SLA transaction")?;
    let job = claim_due_sla_cycle(&mut transaction).await?;
    let Some(mut job) = job else {
        transaction.commit().await?;
        return Ok(false);
    };
    if !refresh_sla_job_policy(&mut transaction, &mut job).await? {
        transaction.commit().await?;
        return Ok(true);
    }
    if job.conversation_status == "resolved" {
        on_conversation_resolved(&mut transaction, job.tenant_id, job.conversation_id).await?;
        transaction.commit().await?;
        return Ok(true);
    }
    let Some(due_kind) = next_due_kind(&job) else {
        transaction.commit().await?;
        return Ok(true);
    };

    let mut assigned_user_id = None;
    let should_notify = match due_kind {
        SlaDueKind::UnassignedWarning => {
            sqlx::query(
                r#"
                UPDATE conversation_routing_cycles
                SET warning_triggered_at = $6, updated_at = $6
                WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
                  AND conversation_id = $4 AND cycle_number = $5
                  AND warning_triggered_at IS NULL AND first_response_at IS NULL
                  AND closed_at IS NULL
                "#,
            )
            .bind(job.tenant_id)
            .bind(job.project_id)
            .bind(job.inbox_id)
            .bind(job.conversation_id)
            .bind(job.cycle_number)
            .bind(job.claimed_at)
            .execute(&mut *transaction)
            .await?;
            active_operator_id(&mut transaction, job.tenant_id, job.conversation_id)
                .await?
                .is_none()
                && job.policy_enabled
                && job.telegram_unassigned_warning
        }
        SlaDueKind::FirstResponse => {
            sqlx::query(
                r#"
                UPDATE conversation_routing_cycles
                SET breached_at = $6,
                    queue_id = COALESCE($7, queue_id),
                    updated_at = $6
                WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
                  AND conversation_id = $4 AND cycle_number = $5
                  AND breached_at IS NULL AND first_response_at IS NULL
                  AND closed_at IS NULL
                "#,
            )
            .bind(job.tenant_id)
            .bind(job.project_id)
            .bind(job.inbox_id)
            .bind(job.conversation_id)
            .bind(job.cycle_number)
            .bind(job.claimed_at)
            .bind(job.escalation_queue_id)
            .execute(&mut *transaction)
            .await?;
            if job.policy_enabled
                && let Some(queue_id) = job.escalation_queue_id
            {
                assigned_user_id =
                    reassign_for_sla_breach(&mut transaction, &job, queue_id, job.claimed_at)
                        .await?;
            }
            job.policy_enabled && job.telegram_sla_breach
        }
        SlaDueKind::Resolution => {
            sqlx::query(
                r#"
                UPDATE conversation_routing_cycles
                SET resolution_breached_at = $6, updated_at = $6
                WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
                  AND conversation_id = $4 AND cycle_number = $5
                  AND resolution_breached_at IS NULL AND closed_at IS NULL
                "#,
            )
            .bind(job.tenant_id)
            .bind(job.project_id)
            .bind(job.inbox_id)
            .bind(job.conversation_id)
            .bind(job.cycle_number)
            .bind(job.claimed_at)
            .execute(&mut *transaction)
            .await?;
            job.policy_enabled && job.telegram_sla_breach
        }
    };

    if should_notify {
        let text = match due_kind {
            SlaDueKind::UnassignedWarning => format!(
                "Диалог остаётся без оператора.\nЧат: {}",
                job.conversation_id
            ),
            SlaDueKind::FirstResponse => {
                format!("Нарушен SLA первого ответа.\nЧат: {}", job.conversation_id)
            }
            SlaDueKind::Resolution => {
                format!("Нарушен SLA решения.\nЧат: {}", job.conversation_id)
            }
        };
        let kind = if matches!(due_kind, SlaDueKind::UnassignedWarning) {
            RoutingTelegramNotificationKind::UnassignedWarning
        } else {
            RoutingTelegramNotificationKind::SlaBreach
        };
        enqueue_routing_alerts(
            &mut transaction,
            job.tenant_id,
            job.project_id,
            job.inbox_id,
            &job.telegram_chat_ids,
            kind,
            &text,
        )
        .await?;
    }
    insert_runtime_audit(
        &mut transaction,
        &job,
        due_kind,
        worker_id,
        assigned_user_id,
    )
    .await?;
    transaction
        .commit()
        .await
        .context("failed to commit an Inbox SLA transaction")?;
    Ok(true)
}

fn safe_alert_value(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(max_chars)
        .collect()
}

async fn enqueue_routing_alerts(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    chat_ids: &[String],
    kind: RoutingTelegramNotificationKind,
    text: &str,
) -> Result<(), AppError> {
    for chat_id in chat_ids {
        telegram_notifications::enqueue_routing(
            transaction,
            RoutingTelegramNotificationPayload {
                tenant_id,
                project_id,
                inbox_id,
                chat_id: chat_id.clone(),
                kind,
                text: text.to_owned(),
            },
        )
        .await
        .map_err(AppError::internal)?;
    }
    Ok(())
}

async fn load_runtime_policy(
    transaction: &mut Transaction<'_, Postgres>,
    context: &InboundContext,
) -> Result<Option<RuntimePolicyRow>, AppError> {
    sqlx::query_as::<_, RuntimePolicyRow>(
        r#"
        SELECT version, assignment_strategy, default_queue_id, timezone,
               outside_hours_action, no_operator_action, sla_clock,
               first_response_seconds, resolution_seconds,
               unassigned_warning_seconds, telegram_new_message,
               telegram_chat_ids
        FROM inbox_routing_policies
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND enabled
        FOR SHARE
        "#,
    )
    .bind(context.tenant_id)
    .bind(context.project_id)
    .bind(context.inbox_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(AppError::from)
}

async fn database_now(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<DateTime<Utc>, AppError> {
    sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut **transaction)
        .await
        .map_err(AppError::from)
}

async fn load_runtime_intervals(
    transaction: &mut Transaction<'_, Postgres>,
    context: &InboundContext,
) -> Result<Vec<RuntimeIntervalRow>, AppError> {
    sqlx::query_as::<_, RuntimeIntervalRow>(
        r#"
        SELECT weekday, starts_at, ends_at
        FROM inbox_working_intervals
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        ORDER BY weekday, starts_at, id
        "#,
    )
    .bind(context.tenant_id)
    .bind(context.project_id)
    .bind(context.inbox_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(AppError::from)
}

fn is_within_working_hours(
    now: DateTime<Utc>,
    timezone: Tz,
    intervals: &[RuntimeIntervalRow],
) -> bool {
    if intervals.is_empty() {
        return true;
    }
    let local = now.with_timezone(&timezone);
    let weekday = i16::try_from(local.weekday().number_from_monday()).unwrap_or_default();
    intervals.iter().any(|interval| {
        interval.weekday == weekday
            && interval.starts_at <= local.time()
            && local.time() < interval.ends_at
    })
}

async fn load_runtime_rules(
    transaction: &mut Transaction<'_, Postgres>,
    context: &InboundContext,
) -> Result<Vec<RuntimeRuleRow>, AppError> {
    sqlx::query_as::<_, RuntimeRuleRow>(
        r#"
        SELECT rule.id, rule.channel_connection_id, rule.language, rule.queue_id
        FROM inbox_routing_rules AS rule
        JOIN inbox_routing_queues AS queue
          ON queue.tenant_id = rule.tenant_id
         AND queue.project_id = rule.project_id
         AND queue.inbox_id = rule.inbox_id
         AND queue.id = rule.queue_id
         AND queue.status = 'active'
        WHERE rule.tenant_id = $1 AND rule.project_id = $2 AND rule.inbox_id = $3
          AND rule.enabled
        ORDER BY rule.position, rule.id
        "#,
    )
    .bind(context.tenant_id)
    .bind(context.project_id)
    .bind(context.inbox_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(AppError::from)
}

fn runtime_rule_matches(
    rule: &RuntimeRuleRow,
    channel_connection_id: Uuid,
    widget_language: Option<&str>,
) -> bool {
    if rule
        .channel_connection_id
        .is_some_and(|channel_id| channel_id != channel_connection_id)
    {
        return false;
    }
    let Some(rule_language) = rule.language.as_deref() else {
        return true;
    };
    let Some(widget_language) = widget_language else {
        return false;
    };
    language_tags_compatible(rule_language, widget_language)
}

fn language_tags_compatible(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let left_primary = left.split('-').next();
    let right_primary = right.split('-').next();
    left_primary == right_primary && (!left.contains('-') || !right.contains('-'))
}

fn sla_deadline(
    started_at: DateTime<Utc>,
    seconds: Option<i32>,
    clock: &str,
    timezone: Tz,
    intervals: &[RuntimeIntervalRow],
) -> Result<Option<DateTime<Utc>>, AppError> {
    let Some(seconds) = seconds else {
        return Ok(None);
    };
    if clock == "working_hours" {
        return working_hours_deadline(started_at, i64::from(seconds), timezone, intervals)
            .map(Some)
            .ok_or_else(|| {
                AppError::internal(anyhow::anyhow!(
                    "could not calculate a working-hours Inbox SLA deadline"
                ))
            });
    }
    started_at
        .checked_add_signed(ChronoDuration::seconds(i64::from(seconds)))
        .map(Some)
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("Inbox SLA deadline overflowed")))
}

fn working_hours_deadline(
    started_at: DateTime<Utc>,
    mut remaining_seconds: i64,
    timezone: Tz,
    intervals: &[RuntimeIntervalRow],
) -> Option<DateTime<Utc>> {
    let mut date = started_at.with_timezone(&timezone).date_naive();
    for day_offset in 0..=MAX_WORKING_DEADLINE_SCAN_DAYS {
        if day_offset > 0 {
            date = date.checked_add_days(Days::new(1))?;
        }
        let weekday = i16::try_from(date.weekday().number_from_monday()).ok()?;
        for interval in intervals
            .iter()
            .filter(|interval| interval.weekday == weekday)
        {
            let Some(start) = resolve_local_boundary(
                timezone,
                NaiveDateTime::new(date, interval.starts_at),
                false,
            ) else {
                continue;
            };
            let Some(end) =
                resolve_local_boundary(timezone, NaiveDateTime::new(date, interval.ends_at), true)
            else {
                continue;
            };
            if end <= started_at || end <= start {
                continue;
            }
            let segment_start = start.max(started_at);
            if segment_start >= end {
                continue;
            }
            let available_seconds = (end - segment_start).num_seconds();
            if remaining_seconds <= available_seconds {
                return segment_start
                    .checked_add_signed(ChronoDuration::seconds(remaining_seconds));
            }
            remaining_seconds -= available_seconds;
        }
    }
    None
}

fn resolve_local_boundary(
    timezone: Tz,
    mut local: NaiveDateTime,
    prefer_late: bool,
) -> Option<DateTime<Utc>> {
    for _ in 0..=180 {
        match timezone.from_local_datetime(&local) {
            LocalResult::Single(value) => return Some(value.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, second) => {
                let value = if prefer_late {
                    first.max(second)
                } else {
                    first.min(second)
                };
                return Some(value.with_timezone(&Utc));
            }
            LocalResult::None => {
                local = local.checked_add_signed(ChronoDuration::minutes(1))?;
            }
        }
    }
    None
}

async fn active_operator_id(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    conversation_id: Uuid,
) -> Result<Option<Uuid>, AppError> {
    let assignment = sqlx::query_as::<_, (Uuid, bool)>(
        r#"
        SELECT assignment.user_id,
               EXISTS (
                   SELECT 1 FROM memberships AS membership
                   JOIN users AS app_user ON app_user.id = membership.user_id AND app_user.status = 'active'
                   JOIN project_roles AS project_role
                     ON project_role.tenant_id = membership.tenant_id
                    AND project_role.project_id = COALESCE((
                 SELECT role_token.role_project_id FROM api_keys AS role_token
                 WHERE role_token.tenant_id = membership.tenant_id
                   AND role_token.actor_user_id = membership.user_id
                   AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
                 LIMIT 1
             ), conversation.project_id)
                    AND project_role.id = membership.role
                    AND NOT EXISTS (
                        SELECT 1 FROM api_keys AS role_token
                        JOIN projects AS role_source ON role_source.tenant_id = role_token.tenant_id
                          AND role_source.id = role_token.role_project_id
                        WHERE role_token.tenant_id = membership.tenant_id
                          AND role_token.actor_user_id = membership.user_id
                          AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
                          AND (role_token.revoked_at IS NOT NULL OR role_token.expires_at <= now()
                               OR role_source.status <> 'active')
                    )
                    AND 'conversations:reply' = ANY(project_role.permissions)
                   WHERE membership.tenant_id = conversation.tenant_id
                     AND membership.user_id = assignment.user_id
                     AND membership.revoked_at IS NULL
                     AND (membership.project_id IS NULL OR membership.project_id = conversation.project_id)
                     AND (membership.department_id IS NULL OR membership.department_id = inbox.department_id)
               ) AS still_allowed
        FROM conversation_assignments AS assignment
        JOIN conversations AS conversation
          ON conversation.tenant_id = assignment.tenant_id AND conversation.id = assignment.conversation_id
        JOIN inboxes AS inbox
          ON inbox.tenant_id = conversation.tenant_id AND inbox.project_id = conversation.project_id
         AND inbox.id = conversation.inbox_id
        WHERE assignment.tenant_id = $1 AND assignment.conversation_id = $2
          AND assignment.user_id IS NOT NULL AND assignment.unassigned_at IS NULL
        ORDER BY assignment.assigned_at DESC, assignment.id DESC
        LIMIT 1
        "#,
    )
    .bind(tenant_id)
    .bind(conversation_id)
    .fetch_optional(&mut **transaction)
    .await?;
    match assignment {
        Some((user_id, true)) => Ok(Some(user_id)),
        Some((user_id, false)) => {
            crate::projects::release_department_assignment(
                transaction,
                tenant_id,
                conversation_id,
                user_id,
            )
            .await?;
            Ok(None)
        }
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
async fn select_online_queue_operator(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    queue_id: Uuid,
    excluded_user_id: Option<Uuid>,
) -> Result<Option<Uuid>, AppError> {
    let queue_lock_key = format!("{tenant_id}:{project_id}:{inbox_id}:{queue_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(queue_lock_key)
        .execute(&mut **transaction)
        .await?;
    sqlx::query_scalar(
        r#"
        SELECT member.user_id
        FROM inbox_routing_queue_members AS member
        JOIN inbox_routing_queues AS queue
          ON queue.tenant_id = member.tenant_id
         AND queue.project_id = member.project_id
         AND queue.inbox_id = member.inbox_id
         AND queue.id = member.queue_id
         AND queue.status = 'active'
        JOIN users AS app_user
          ON app_user.id = member.user_id AND app_user.status = 'active'
        WHERE member.tenant_id = $1
          AND member.project_id = $2
          AND member.inbox_id = $3
          AND member.queue_id = $4
          AND ($5::uuid IS NULL OR member.user_id <> $5)
          AND EXISTS (
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
              JOIN project_roles AS project_role
                ON project_role.tenant_id = membership.tenant_id
               AND project_role.project_id = $2
               AND project_role.id = membership.role
               AND 'conversations:reply' = ANY(project_role.permissions)
              WHERE session.tenant_id = member.tenant_id
                AND (workspace.session_id IS NOT NULL OR session.project_id = $2)
                AND (membership.project_id IS NULL OR membership.project_id = $2)
                AND (membership.department_id IS NULL OR membership.department_id = (
                    SELECT department_id FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND id = $3
                ))
                AND session.user_id = member.user_id
                AND session.revoked_at IS NULL
                AND session.idle_expires_at > now()
                AND session.absolute_expires_at > now()
                AND CASE WHEN workspace.session_id IS NULL THEN session.presence_last_seen_at
                         ELSE workspace.presence_last_seen_at END >=
                    now() - ($6::bigint * interval '1 second')
          )
        ORDER BY (
            SELECT COUNT(*)
            FROM conversation_assignments AS assignment
            JOIN conversations AS conversation
              ON conversation.tenant_id = assignment.tenant_id
             AND conversation.id = assignment.conversation_id
            WHERE assignment.tenant_id = member.tenant_id
              AND assignment.user_id = member.user_id
              AND assignment.unassigned_at IS NULL
              AND conversation.project_id = $2
              AND conversation.inbox_id = $3
              AND conversation.status <> 'resolved'
        ), member.last_assigned_at NULLS FIRST, member.user_id
        LIMIT 1
        FOR UPDATE OF member
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(queue_id)
    .bind(excluded_user_id)
    .bind(OPERATOR_ONLINE_WINDOW_SECONDS)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(AppError::from)
}

#[allow(clippy::too_many_arguments)]
async fn assign_operator(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    contact_id: Uuid,
    conversation_id: Uuid,
    queue_id: Uuid,
    user_id: Uuid,
    assigned_at: DateTime<Utc>,
    reason: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO conversation_assignments (
            id, tenant_id, conversation_id, user_id, assigned_by, assigned_at
        ) VALUES ($1, $2, $3, $4, NULL, $5)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(conversation_id)
    .bind(user_id)
    .bind(assigned_at)
    .execute(&mut **transaction)
    .await?;
    provider_reply::cancel_pending(transaction, tenant_id, conversation_id).await?;
    sqlx::query(
        r#"
        INSERT INTO conversation_participants (
            id, tenant_id, conversation_id, participant_kind, user_id, joined_at
        ) VALUES ($1, $2, $3, 'operator', $4, $5)
        ON CONFLICT (tenant_id, conversation_id, user_id)
            WHERE participant_kind = 'operator' AND left_at IS NULL
        DO NOTHING
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(conversation_id)
    .bind(user_id)
    .bind(assigned_at)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE conversation_participants
        SET left_at = $3
        WHERE tenant_id = $1 AND conversation_id = $2
          AND participant_kind = 'ai' AND left_at IS NULL
        "#,
    )
    .bind(tenant_id)
    .bind(conversation_id)
    .bind(assigned_at)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE conversations
        SET status = CASE WHEN status = 'new' THEN 'open' ELSE status END,
            updated_at = $3, version = version + 1
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(conversation_id)
    .bind(assigned_at)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE inbox_routing_queue_members
        SET last_assigned_at = $6
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND queue_id = $4 AND user_id = $5
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(queue_id)
    .bind(user_id)
    .bind(assigned_at)
    .execute(&mut **transaction)
    .await?;

    insert_runtime_realtime_outbox(
        transaction,
        &RealtimeEvent {
            event_id: Uuid::now_v7(),
            tenant_id,
            project_id,
            inbox_id,
            contact_id: Some(contact_id),
            event_type: "conversation.operator_joined".to_owned(),
            aggregate_id: conversation_id,
            sequence: None,
            occurred_at: assigned_at,
            data: json!({ "conversation_id": conversation_id }),
        },
    )
    .await?;
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata, occurred_at
        ) VALUES (
            $1, $2, $3, NULL, 'inbox_routing.operator_assigned',
            'conversation', $4, $5, $6
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(project_id)
    .bind(conversation_id)
    .bind(json!({
        "queue_id": queue_id,
        "user_id": user_id,
        "reason": reason,
    }))
    .bind(assigned_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_runtime_realtime_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    event: &RealtimeEvent,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload,
            status, available_at, created_at
        ) VALUES (
            $1, $2, 'realtime', $3, $4, $5,
            'pending', now(), now()
        )
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

async fn reassign_for_sla_breach(
    transaction: &mut Transaction<'_, Postgres>,
    job: &DueSlaCycleRow,
    queue_id: Uuid,
    assigned_at: DateTime<Utc>,
) -> Result<Option<Uuid>, AppError> {
    let current_user_id =
        active_operator_id(transaction, job.tenant_id, job.conversation_id).await?;
    let candidate = select_online_queue_operator(
        transaction,
        job.tenant_id,
        job.project_id,
        job.inbox_id,
        queue_id,
        current_user_id,
    )
    .await?;
    let Some(user_id) = candidate else {
        return Ok(None);
    };
    if let Some(current_user_id) = current_user_id {
        sqlx::query(
            r#"
            UPDATE conversation_assignments
            SET unassigned_at = $4
            WHERE tenant_id = $1 AND conversation_id = $2
              AND user_id = $3 AND unassigned_at IS NULL
            "#,
        )
        .bind(job.tenant_id)
        .bind(job.conversation_id)
        .bind(current_user_id)
        .bind(assigned_at)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE conversation_participants
            SET left_at = $4
            WHERE tenant_id = $1 AND conversation_id = $2
              AND participant_kind = 'operator' AND user_id = $3 AND left_at IS NULL
            "#,
        )
        .bind(job.tenant_id)
        .bind(job.conversation_id)
        .bind(current_user_id)
        .bind(assigned_at)
        .execute(&mut **transaction)
        .await?;
    }
    assign_operator(
        transaction,
        job.tenant_id,
        job.project_id,
        job.inbox_id,
        job.contact_id,
        job.conversation_id,
        queue_id,
        user_id,
        assigned_at,
        "first_response_sla_breach",
    )
    .await?;
    Ok(Some(user_id))
}

async fn claim_due_sla_cycle(
    transaction: &mut Transaction<'_, Postgres>,
) -> anyhow::Result<Option<DueSlaCycleRow>> {
    sqlx::query_as::<_, DueSlaCycleRow>(
        r#"
        WITH candidate AS (
            SELECT cycle.tenant_id, cycle.project_id, cycle.inbox_id,
                   cycle.conversation_id, cycle.cycle_number, cycle.queue_id,
                   cycle.warning_due_at, cycle.first_response_due_at,
                   cycle.resolution_due_at, cycle.first_response_at,
                   cycle.warning_triggered_at, cycle.breached_at,
                   cycle.resolution_breached_at,
                   conversation.contact_id,
                   conversation.status AS conversation_status,
                   now() AS claimed_at
            FROM conversation_routing_cycles AS cycle
            JOIN conversations AS conversation
              ON conversation.tenant_id = cycle.tenant_id
             AND conversation.project_id = cycle.project_id
             AND conversation.inbox_id = cycle.inbox_id
             AND conversation.id = cycle.conversation_id
            WHERE cycle.closed_at IS NULL
              AND (
                  (cycle.warning_due_at <= now()
                   AND cycle.warning_triggered_at IS NULL
                   AND cycle.first_response_at IS NULL)
                  OR (cycle.first_response_due_at <= now()
                      AND cycle.breached_at IS NULL
                      AND cycle.first_response_at IS NULL)
                  OR (cycle.resolution_due_at <= now()
                      AND cycle.resolution_breached_at IS NULL)
              )
            ORDER BY LEAST(
                COALESCE(
                    CASE WHEN cycle.warning_triggered_at IS NULL
                              AND cycle.first_response_at IS NULL
                         THEN cycle.warning_due_at END,
                    'infinity'::timestamptz
                ),
                COALESCE(
                    CASE WHEN cycle.breached_at IS NULL
                              AND cycle.first_response_at IS NULL
                         THEN cycle.first_response_due_at END,
                    'infinity'::timestamptz
                ),
                COALESCE(
                    CASE WHEN cycle.resolution_breached_at IS NULL
                         THEN cycle.resolution_due_at END,
                    'infinity'::timestamptz
                )
            ), cycle.conversation_id, cycle.cycle_number
            LIMIT 1
            FOR UPDATE OF conversation SKIP LOCKED
        )
        SELECT candidate.tenant_id, candidate.project_id, candidate.inbox_id,
               candidate.conversation_id, candidate.cycle_number, candidate.queue_id,
               candidate.warning_due_at, candidate.first_response_due_at,
               candidate.resolution_due_at, candidate.first_response_at,
               candidate.warning_triggered_at, candidate.breached_at,
               candidate.resolution_breached_at, candidate.contact_id,
               candidate.conversation_status,
               COALESCE(policy.enabled, false) AS policy_enabled,
               policy.escalation_queue_id,
               COALESCE(policy.telegram_unassigned_warning, false)
                   AS telegram_unassigned_warning,
               COALESCE(policy.telegram_sla_breach, false) AS telegram_sla_breach,
               COALESCE(policy.telegram_chat_ids, ARRAY[]::text[]) AS telegram_chat_ids,
               candidate.claimed_at
        FROM candidate
        LEFT JOIN inbox_routing_policies AS policy
          ON policy.tenant_id = candidate.tenant_id
         AND policy.project_id = candidate.project_id
         AND policy.inbox_id = candidate.inbox_id
        "#,
    )
    .fetch_optional(&mut **transaction)
    .await
    .context("failed to claim an Inbox SLA deadline")
}

async fn refresh_sla_job_policy(
    transaction: &mut Transaction<'_, Postgres>,
    job: &mut DueSlaCycleRow,
) -> anyhow::Result<bool> {
    let policy = sqlx::query_as::<_, CurrentSlaPolicyRow>(
        r#"
        SELECT enabled, escalation_queue_id, telegram_unassigned_warning,
               telegram_sla_breach, telegram_chat_ids
        FROM inbox_routing_policies
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
        FOR SHARE
        "#,
    )
    .bind(job.tenant_id)
    .bind(job.project_id)
    .bind(job.inbox_id)
    .fetch_optional(&mut **transaction)
    .await
    .context("failed to refresh Inbox SLA policy")?;

    if let Some(policy) = policy {
        job.policy_enabled = policy.enabled;
        job.escalation_queue_id = policy.escalation_queue_id;
        job.telegram_unassigned_warning = policy.telegram_unassigned_warning;
        job.telegram_sla_breach = policy.telegram_sla_breach;
        job.telegram_chat_ids = policy.telegram_chat_ids;
    } else {
        job.policy_enabled = false;
        job.escalation_queue_id = None;
        job.telegram_unassigned_warning = false;
        job.telegram_sla_breach = false;
        job.telegram_chat_ids.clear();
    }

    sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM conversation_routing_cycles
            WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
              AND conversation_id = $4 AND cycle_number = $5
              AND closed_at IS NULL
        )
        "#,
    )
    .bind(job.tenant_id)
    .bind(job.project_id)
    .bind(job.inbox_id)
    .bind(job.conversation_id)
    .bind(job.cycle_number)
    .fetch_one(&mut **transaction)
    .await
    .context("failed to recheck an Inbox SLA cycle")
}

fn next_due_kind(job: &DueSlaCycleRow) -> Option<SlaDueKind> {
    let mut due = Vec::with_capacity(3);
    if job.warning_triggered_at.is_none()
        && job.first_response_at.is_none()
        && job.warning_due_at.is_some_and(|at| at <= job.claimed_at)
    {
        due.push((job.warning_due_at?, 0_u8, SlaDueKind::UnassignedWarning));
    }
    if job.breached_at.is_none()
        && job.first_response_at.is_none()
        && job
            .first_response_due_at
            .is_some_and(|at| at <= job.claimed_at)
    {
        due.push((job.first_response_due_at?, 1_u8, SlaDueKind::FirstResponse));
    }
    if job.resolution_breached_at.is_none()
        && job.resolution_due_at.is_some_and(|at| at <= job.claimed_at)
    {
        due.push((job.resolution_due_at?, 2_u8, SlaDueKind::Resolution));
    }
    due.into_iter()
        .min_by_key(|(due_at, priority, _)| (*due_at, *priority))
        .map(|(_, _, kind)| kind)
}

async fn insert_runtime_audit(
    transaction: &mut Transaction<'_, Postgres>,
    job: &DueSlaCycleRow,
    due_kind: SlaDueKind,
    worker_id: Uuid,
    assigned_user_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let action = match due_kind {
        SlaDueKind::UnassignedWarning => "inbox_routing.unassigned_warning",
        SlaDueKind::FirstResponse => "inbox_routing.first_response_breached",
        SlaDueKind::Resolution => "inbox_routing.resolution_breached",
    };
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata, occurred_at
        ) VALUES ($1, $2, $3, NULL, $4, 'conversation', $5, $6, $7)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(job.tenant_id)
    .bind(job.project_id)
    .bind(action)
    .bind(job.conversation_id)
    .bind(json!({
        "cycle_number": job.cycle_number,
        "queue_id": job.queue_id,
        "assigned_user_id": assigned_user_id,
        "worker_id": worker_id,
    }))
    .bind(job.claimed_at)
    .execute(&mut **transaction)
    .await
    .context("failed to audit an Inbox SLA event")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AssignmentStrategy, CoverageInput, FallbackAction, RoutingConfigurationInput,
        RoutingPutRequest, RoutingQueueInput, RoutingRuleInput, RuntimeIntervalRow, RuntimeRuleRow,
        SlaClock, SlaDueKind, SlaInput, TelegramBotTokenWrite, TelegramInput, WorkingIntervalInput,
        active_submitted_queue_ids, is_within_working_hours, next_due_kind, normalize_language,
        normalize_request, runtime_rule_matches, valid_bot_token, valid_chat_id,
        validate_post_write_telegram, working_hours_deadline,
    };
    use chrono::{Duration, NaiveTime, TimeZone, Utc};
    use chrono_tz::Tz;
    use uuid::Uuid;

    fn valid_request() -> RoutingPutRequest {
        let queue_id = Uuid::now_v7();
        RoutingPutRequest {
            expected_version: 0,
            configuration: RoutingConfigurationInput {
                enabled: true,
                assignment_strategy: AssignmentStrategy::LeastActive,
                default_queue_id: Some(queue_id),
                queues: vec![RoutingQueueInput {
                    id: queue_id,
                    name: " Support ".to_owned(),
                    status: super::QueueStatus::Active,
                    member_ids: Vec::new(),
                }],
                rules: vec![RoutingRuleInput {
                    id: Uuid::now_v7(),
                    position: 1,
                    enabled: true,
                    channel_id: Some(Uuid::now_v7()),
                    language: Some("RU_ru".to_owned()),
                    queue_id,
                }],
                coverage: CoverageInput {
                    timezone: " Europe/Istanbul ".to_owned(),
                    weekly_intervals: vec![WorkingIntervalInput {
                        id: Uuid::now_v7(),
                        weekday: 1,
                        starts_at: "09:00".to_owned(),
                        ends_at: "18:00".to_owned(),
                    }],
                    outside_hours_action: FallbackAction::Queue,
                    no_operator_action: FallbackAction::Ai,
                },
                sla: SlaInput {
                    clock: SlaClock::WorkingHours,
                    first_response_seconds: Some(900),
                    resolution_seconds: Some(86_400),
                    unassigned_warning_seconds: Some(300),
                    escalation_queue_id: Some(queue_id),
                },
                telegram: TelegramInput {
                    new_visitor: false,
                    new_message: false,
                    operator_request: false,
                    unassigned_warning: false,
                    sla_breach: false,
                    chat_ids: vec![" -1001234567890 ".to_owned()],
                },
            },
            telegram_bot_token: TelegramBotTokenWrite::Preserve,
        }
    }

    #[test]
    fn normalizes_complete_routing_configuration() {
        let request = normalize_request(valid_request()).unwrap();
        assert_eq!(request.configuration.queues[0].name, "Support");
        assert_eq!(
            request.configuration.rules[0].language.as_deref(),
            Some("ru-ru")
        );
        assert_eq!(request.configuration.coverage.timezone, "Europe/Istanbul");
        assert_eq!(request.configuration.telegram.chat_ids, ["-1001234567890"]);
    }

    #[test]
    fn only_active_submitted_queues_can_back_open_cycles() {
        let mut request = valid_request();
        let active_queue_id = request.configuration.queues[0].id;
        request.configuration.queues.push(RoutingQueueInput {
            id: Uuid::now_v7(),
            name: "Disabled".to_owned(),
            status: super::QueueStatus::Disabled,
            member_ids: Vec::new(),
        });

        assert_eq!(
            active_submitted_queue_ids(&request.configuration),
            [active_queue_id]
        );
    }

    #[test]
    fn rejects_overlapping_or_overnight_working_intervals() {
        let mut overlapping = valid_request();
        overlapping
            .configuration
            .coverage
            .weekly_intervals
            .push(WorkingIntervalInput {
                id: Uuid::now_v7(),
                weekday: 1,
                starts_at: "17:00".to_owned(),
                ends_at: "19:00".to_owned(),
            });
        assert!(normalize_request(overlapping).is_err());

        let mut overnight = valid_request();
        overnight.configuration.coverage.weekly_intervals[0].starts_at = "22:00".to_owned();
        overnight.configuration.coverage.weekly_intervals[0].ends_at = "06:00".to_owned();
        assert!(normalize_request(overnight).is_err());
    }

    #[test]
    fn enforces_sla_threshold_order_and_working_hours_coverage() {
        let mut bad_warning = valid_request();
        bad_warning.configuration.sla.unassigned_warning_seconds = Some(900);
        assert!(normalize_request(bad_warning).is_err());

        let mut bad_resolution = valid_request();
        bad_resolution.configuration.sla.resolution_seconds = Some(600);
        assert!(normalize_request(bad_resolution).is_err());

        let mut no_coverage = valid_request();
        no_coverage.configuration.coverage.weekly_intervals.clear();
        assert!(normalize_request(no_coverage).is_err());

        let mut infeasible_sparse_schedule = valid_request();
        infeasible_sparse_schedule
            .configuration
            .coverage
            .weekly_intervals[0]
            .ends_at = "09:01".to_owned();
        infeasible_sparse_schedule
            .configuration
            .sla
            .resolution_seconds = Some(2_592_000);
        assert!(normalize_request(infeasible_sparse_schedule).is_err());
    }

    #[test]
    fn validates_telegram_values_without_echoing_the_token() {
        assert!(valid_bot_token("123456:abcdefghijklmnopqrst"));
        assert!(!valid_bot_token("invalid"));
        assert!(valid_chat_id("-1001234567890"));
        assert!(!valid_chat_id("support-room"));

        let mut request = valid_request();
        request.configuration.telegram.sla_breach = true;
        assert!(validate_post_write_telegram(&request, false).is_err());
        request.telegram_bot_token = TelegramBotTokenWrite::Replace {
            value: "123456:abcdefghijklmnopqrst".to_owned(),
        };
        assert!(validate_post_write_telegram(&request, false).is_ok());
    }

    #[test]
    fn normalizes_regional_language_tags_and_rejects_malformed_values() {
        assert_eq!(
            normalize_language(Some(" PT_br ".to_owned())).unwrap(),
            Some("pt-br".to_owned())
        );
        assert!(normalize_language(Some("../../ru".to_owned())).is_err());
        assert!(normalize_language(Some("r".to_owned())).is_err());
    }

    #[test]
    fn empty_coverage_means_always_open() {
        let now = Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0).unwrap();
        assert!(is_within_working_hours(now, Tz::UTC, &[]));
    }

    #[test]
    fn working_hours_deadline_pauses_until_the_next_interval() {
        let intervals = [
            RuntimeIntervalRow {
                weekday: 1,
                starts_at: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                ends_at: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            },
            RuntimeIntervalRow {
                weekday: 2,
                starts_at: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                ends_at: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            },
        ];
        let started_at = Utc.with_ymd_and_hms(2026, 8, 31, 16, 30, 0).unwrap();

        assert_eq!(
            working_hours_deadline(started_at, 3_600, Tz::UTC, &intervals),
            Some(Utc.with_ymd_and_hms(2026, 9, 1, 9, 30, 0).unwrap())
        );
    }

    #[test]
    fn runtime_scan_covers_the_maximum_accepted_sparse_schedule() {
        let intervals = [RuntimeIntervalRow {
            weekday: 1,
            starts_at: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            ends_at: NaiveTime::from_hms_opt(9, 1, 0).unwrap(),
        }];
        let started_at = Utc.with_ymd_and_hms(2026, 8, 31, 9, 1, 0).unwrap();

        assert!(working_hours_deadline(started_at, 60 * 520, Tz::UTC, &intervals).is_some());
    }

    #[test]
    fn runtime_language_rules_match_primary_tags_without_crossing_regions() {
        let rule = RuntimeRuleRow {
            id: Uuid::now_v7(),
            channel_connection_id: None,
            language: Some("en".to_owned()),
            queue_id: Uuid::now_v7(),
        };
        assert!(runtime_rule_matches(&rule, Uuid::now_v7(), Some("en-us")));

        let regional_rule = RuntimeRuleRow {
            language: Some("en-gb".to_owned()),
            ..rule
        };
        assert!(!runtime_rule_matches(
            &regional_rule,
            Uuid::now_v7(),
            Some("en-us")
        ));
    }

    #[test]
    fn sla_worker_selects_the_earliest_unprocessed_deadline() {
        let claimed_at = Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0).unwrap();
        let job = super::DueSlaCycleRow {
            tenant_id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            inbox_id: Uuid::now_v7(),
            conversation_id: Uuid::now_v7(),
            cycle_number: 0,
            queue_id: None,
            warning_due_at: Some(claimed_at - Duration::minutes(2)),
            first_response_due_at: Some(claimed_at - Duration::minutes(1)),
            resolution_due_at: None,
            first_response_at: None,
            warning_triggered_at: None,
            breached_at: None,
            resolution_breached_at: None,
            contact_id: Uuid::now_v7(),
            conversation_status: "open".to_owned(),
            policy_enabled: true,
            escalation_queue_id: None,
            telegram_unassigned_warning: true,
            telegram_sla_breach: true,
            telegram_chat_ids: Vec::new(),
            claimed_at,
        };
        assert_eq!(next_due_kind(&job), Some(SlaDueKind::UnassignedWarning));
    }
}
