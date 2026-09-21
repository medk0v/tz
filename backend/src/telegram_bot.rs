//! Telegram Bot API channel setup, inbound webhooks, and outbound delivery.

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, TimeDelta, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    AppState,
    ai_settings::{decrypt_secret, encrypt_secret, load_secret_encryption_key},
    auth::ActorContext,
    contact_blacklist::BlacklistReply,
    conversations,
    error::AppError,
    realtime::RealtimeEvent,
    telegram_notifications::valid_bot_token,
};

const MAX_WEBHOOK_BYTES: usize = 256 * 1024;
const WEBHOOK_SECRET_BYTES: usize = 32;
const CHANNEL_SECRET_AAD_PREFIX: &str = "tzomet:telegram-bot-channel:v1";
const WEBHOOK_CHECK_INTERVAL_SECONDS: i64 = 30;
const WEBHOOK_FAILURE_GRACE_SECONDS: i64 = 60;
const WEBHOOK_RECOVERY_INTERVAL_SECONDS: i64 = 5 * 60;
const RECEIVER_LEASE_SECONDS: i64 = 45;
const GET_UPDATES_TIMEOUT_SECONDS: i64 = 10;
const RATING_INVITATION_TTL_DAYS: i64 = 30;
const MAX_RATING_COMMENT_CHARS: usize = 2_000;
pub(crate) const OUTBOX_AGGREGATE_TYPE: &str = "telegram_bot";
pub(crate) const SETUP_EVENT_TYPE: &str = "telegram.bot.setup_requested";
pub(crate) const DELIVERY_EVENT_TYPE: &str = "telegram.message.delivery_requested";
pub(crate) const UNREGISTER_EVENT_TYPE: &str = "telegram.bot.unregister_requested";
const RATING_PROMPT_EVENT_TYPE: &str = "telegram.rating.prompt_requested";
const RATING_COMMENT_PROMPT_EVENT_TYPE: &str = "telegram.rating.comment_prompt_requested";
const RATING_THANKS_EVENT_TYPE: &str = "telegram.rating.thanks_requested";
const REQUEST_ACKNOWLEDGEMENT_EVENT_TYPE: &str = "telegram.request_acknowledgement.requested";
const RATING_CALLBACK_PREFIX: &str = "tz_rating:";
const RATING_SKIP_CALLBACK: &str = "tz_rating:skip";
const RECEIVER_ALLOWED_UPDATES: [&str; 2] = ["message", "callback_query"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/channels/telegram-bots", post(create_channel))
        .route("/telegram/v1/webhooks/{public_id}", post(receive_webhook))
}

#[derive(Debug, Deserialize, Serialize)]
struct TelegramBotSecrets {
    bot_token: String,
    webhook_secret: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateTelegramBotChannelRequest {
    inbox_id: Uuid,
    name: String,
    bot_token: String,
}

#[derive(Debug, Serialize)]
struct TelegramBotChannelResponse {
    id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    public_id: Uuid,
    kind: &'static str,
    name: String,
    status: &'static str,
    blacklist_reply: BlacklistReply,
    telegram_bot_username: Option<String>,
    telegram_bot_token_configured: bool,
    telegram_webhook_url: String,
    updated_at: chrono::DateTime<Utc>,
}

async fn create_channel(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<CreateTelegramBotChannelRequest>,
) -> Result<(StatusCode, Json<TelegramBotChannelResponse>), AppError> {
    actor.require("channels:manage")?;
    actor.require_password_session()?;
    let name = normalize_required_text(request.name, 200, "name")?;
    let bot_token = request.bot_token.trim().to_owned();
    if !valid_bot_token(&bot_token) {
        return Err(AppError::BadRequest(
            "bot_token is not a valid Telegram bot token".to_owned(),
        ));
    }
    let webhook_base_url = state
        .config
        .telegram
        .webhook_base_url
        .as_deref()
        .ok_or_else(|| {
            AppError::ServiceUnavailable(
                "Telegram webhook delivery is not configured on this deployment".to_owned(),
            )
        })?;

    let mut transaction = state.db.begin().await?;
    let (project_id, inbox_status) = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT project_id, status FROM inboxes WHERE tenant_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(actor.tenant_id)
    .bind(request.inbox_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(project_id, request.inbox_id)?;
    if inbox_status != "active" {
        return Err(AppError::Conflict(
            "Telegram bots cannot be connected to a disabled Inbox".to_owned(),
        ));
    }

    let channel_id = Uuid::now_v7();
    let public_id = Uuid::now_v7();
    let webhook_url = webhook_url(webhook_base_url, public_id)?;
    let webhook_secret = generate_webhook_secret();
    let encrypted_payload = encrypt_channel_secrets(
        &state,
        actor.tenant_id,
        project_id,
        request.inbox_id,
        channel_id,
        &TelegramBotSecrets {
            bot_token,
            webhook_secret,
        },
    )?;
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO channel_connections (
            id, tenant_id, project_id, inbox_id, public_id, kind, name, status, config,
            created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, $5, 'telegram_bot', $6, 'connecting',
            jsonb_build_object('webhook_url', $7::text), $8, $8
        )
        "#,
    )
    .bind(channel_id)
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(request.inbox_id)
    .bind(public_id)
    .bind(&name)
    .bind(&webhook_url)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO channel_secrets (
            id, tenant_id, channel_connection_id, encrypted_payload, key_version,
            created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $6)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(channel_id)
    .bind(encrypted_payload)
    .bind(&state.config.secrets.key_version)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO telegram_bot_receivers (tenant_id, channel_connection_id)
        VALUES ($1, $2)
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .execute(&mut *transaction)
    .await?;
    insert_channel_outbox(
        &mut transaction,
        actor.tenant_id,
        channel_id,
        SETUP_EVENT_TYPE,
        json!({ "channel_connection_id": channel_id }),
    )
    .await?;
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata, occurred_at
        ) VALUES (
            $1, $2, $3, $4, 'channel.telegram_bot.created',
            'channel', $5, jsonb_build_object('inbox_id', $6::uuid, 'name', $7::text), $8
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(channel_id)
    .bind(request.inbox_id)
    .bind(&name)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(TelegramBotChannelResponse {
            id: channel_id,
            project_id,
            inbox_id: request.inbox_id,
            public_id,
            kind: "telegram_bot",
            name,
            status: "connecting",
            blacklist_reply: BlacklistReply::default(),
            telegram_bot_username: None,
            telegram_bot_token_configured: true,
            telegram_webhook_url: webhook_url,
            updated_at: now,
        }),
    ))
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
    callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    #[serde(rename = "from")]
    sender: Option<TelegramUser>,
    chat: TelegramChat,
    text: Option<String>,
    caption: Option<String>,
    reply_to_message: Option<TelegramReplyMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramReplyMessage {
    message_id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramCallbackQuery {
    id: String,
    #[serde(rename = "from")]
    sender: TelegramUser,
    message: Option<TelegramCallbackMessage>,
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramCallbackMessage {
    message_id: i64,
    chat: TelegramChat,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    #[serde(default)]
    is_bot: bool,
    first_name: String,
    last_name: Option<String>,
    username: Option<String>,
    language_code: Option<String>,
}

#[derive(Debug, FromRow)]
struct WebhookChannel {
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_connection_id: Uuid,
    encrypted_payload: Vec<u8>,
    key_version: String,
}

#[derive(Debug, FromRow)]
struct ReceiverClaim {
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_connection_id: Uuid,
    encrypted_payload: Vec<u8>,
    key_version: String,
    webhook_url: String,
    receive_mode: String,
    polling_offset: Option<i64>,
    webhook_failure_started_at: Option<DateTime<Utc>>,
    next_webhook_retry_at: Option<DateTime<Utc>>,
}

impl ReceiverClaim {
    fn webhook_channel(&self) -> WebhookChannel {
        WebhookChannel {
            tenant_id: self.tenant_id,
            project_id: self.project_id,
            inbox_id: self.inbox_id,
            channel_connection_id: self.channel_connection_id,
            encrypted_payload: self.encrypted_payload.clone(),
            key_version: self.key_version.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TelegramWebhookInfo {
    url: String,
    pending_update_count: i64,
    last_error_date: Option<i64>,
    allowed_updates: Option<Vec<String>>,
}

async fn receive_webhook(
    State(state): State<AppState>,
    Path(public_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    if body.len() > MAX_WEBHOOK_BYTES {
        return Err(AppError::PayloadTooLarge(
            "Telegram webhook payload is too large".to_owned(),
        ));
    }
    let channel = sqlx::query_as::<_, WebhookChannel>(
        r#"
        SELECT connection.tenant_id, connection.project_id, connection.inbox_id,
               connection.id AS channel_connection_id,
               secret.encrypted_payload, secret.key_version
        FROM channel_connections AS connection
        JOIN channel_secrets AS secret
          ON secret.tenant_id = connection.tenant_id
         AND secret.channel_connection_id = connection.id
        JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
        WHERE connection.public_id = $1
          AND connection.kind = 'telegram_bot'
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
          AND inbox.status = 'active'
        "#,
    )
    .bind(public_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let secrets = decrypt_channel_secrets(&state, &channel)?;
    let supplied_secret = exactly_one_header(&headers, "x-telegram-bot-api-secret-token")
        .ok_or(AppError::NotFound)?;
    if !constant_time_equal(
        supplied_secret.as_bytes(),
        secrets.webhook_secret.as_bytes(),
    ) {
        return Err(AppError::NotFound);
    }
    let update = serde_json::from_slice::<Value>(&body)
        .map_err(|_| AppError::BadRequest("Telegram webhook payload is invalid".to_owned()))?;
    process_inbound_update(&state, &channel, update).await
}

async fn process_inbound_update(
    state: &AppState,
    channel: &WebhookChannel,
    value: Value,
) -> Result<StatusCode, AppError> {
    let canonical_payload = serde_json::to_vec(&value).map_err(AppError::internal)?;
    let update = serde_json::from_value::<TelegramUpdate>(value)
        .map_err(|_| AppError::BadRequest("Telegram update payload is invalid".to_owned()))?;
    let payload_hash = Sha256::digest(&canonical_payload).to_vec();
    let inbound_event_id =
        begin_inbound_event(state, channel, update.update_id, &payload_hash).await?;
    let Some(inbound_event_id) = inbound_event_id else {
        return Ok(StatusCode::OK);
    };

    if let Some(callback_query) = update.callback_query {
        if let Some(message) = callback_query.message.as_ref()
            && message.chat.kind == "private"
            && !callback_query.sender.is_bot
            && telegram_contact_is_blocked(state, channel, callback_query.sender.id).await?
        {
            let (contact_id, conversation_id) = ensure_contact_and_conversation(
                state,
                channel,
                &callback_query.sender,
                message.chat.id,
            )
            .await?;
            conversations::create_channel_contact_message(
                state,
                channel.tenant_id,
                channel.project_id,
                channel.inbox_id,
                channel.channel_connection_id,
                contact_id,
                conversation_id,
                inbound_event_id,
                "[Telegram button interaction]".to_owned(),
                callback_query.sender.language_code,
            )
            .await?;
        } else {
            process_rating_callback(state, channel, &callback_query).await?;
        }
        mark_inbound_processed(state, channel.tenant_id, inbound_event_id).await?;
        return Ok(StatusCode::OK);
    }

    let Some(message) = update.message else {
        mark_inbound_processed(state, channel.tenant_id, inbound_event_id).await?;
        return Ok(StatusCode::OK);
    };
    let Some(sender) = message.sender else {
        mark_inbound_processed(state, channel.tenant_id, inbound_event_id).await?;
        return Ok(StatusCode::OK);
    };
    if message.chat.kind != "private" || sender.is_bot {
        mark_inbound_processed(state, channel.tenant_id, inbound_event_id).await?;
        return Ok(StatusCode::OK);
    }
    let is_blocked = telegram_contact_is_blocked(state, channel, sender.id).await?;
    let Some(text) = inbound_message_body(message.text, message.caption, is_blocked) else {
        mark_inbound_processed(state, channel.tenant_id, inbound_event_id).await?;
        return Ok(StatusCode::OK);
    };

    if !is_blocked
        && process_rating_comment(
            state,
            channel,
            &sender,
            message.chat.id,
            message
                .reply_to_message
                .as_ref()
                .map(|reply| reply.message_id),
            &text,
        )
        .await?
    {
        mark_inbound_processed(state, channel.tenant_id, inbound_event_id).await?;
        return Ok(StatusCode::OK);
    }

    let (contact_id, conversation_id) =
        ensure_contact_and_conversation(state, channel, &sender, message.chat.id).await?;
    conversations::create_channel_contact_message(
        state,
        channel.tenant_id,
        channel.project_id,
        channel.inbox_id,
        channel.channel_connection_id,
        contact_id,
        conversation_id,
        inbound_event_id,
        text,
        sender.language_code,
    )
    .await?;
    mark_inbound_processed(state, channel.tenant_id, inbound_event_id).await?;
    Ok(StatusCode::OK)
}

fn inbound_message_body(
    text: Option<String>,
    caption: Option<String>,
    is_blocked: bool,
) -> Option<String> {
    if is_blocked {
        Some(
            text.or(caption)
                .filter(|body| !body.trim().is_empty())
                .unwrap_or_else(|| "[Telegram media or service message]".to_owned()),
        )
    } else {
        text
    }
}

async fn telegram_contact_is_blocked(
    state: &AppState,
    channel: &WebhookChannel,
    telegram_user_id: i64,
) -> Result<bool, AppError> {
    let is_blocked = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT contact.is_blocked
        FROM contact_identities AS identity
        JOIN contacts AS contact
          ON contact.tenant_id = identity.tenant_id
         AND contact.id = identity.contact_id
        WHERE identity.tenant_id = $1
          AND contact.project_id = $2
          AND identity.channel_kind = 'telegram_bot'
          AND identity.external_id = $3
        "#,
    )
    .bind(channel.tenant_id)
    .bind(channel.project_id)
    .bind(format!(
        "{}:{telegram_user_id}",
        channel.channel_connection_id
    ))
    .fetch_optional(&state.db)
    .await?;
    Ok(is_blocked.unwrap_or(false))
}

pub(crate) async fn enqueue_rating_invitation(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    conversation_id: Uuid,
    resolution_id: Uuid,
) -> Result<(), AppError> {
    let target = sqlx::query_as::<_, (Uuid, Uuid, Option<String>, Option<String>, String)>(
        r#"
        SELECT conversation.channel_connection_id, conversation.contact_id,
               identity.metadata ->> 'telegram_user_id',
               identity.metadata ->> 'chat_id',
               COALESCE(NULLIF(btrim(conversation.widget_language), ''), 'en')
        FROM conversations AS conversation
        JOIN channel_connections AS connection
          ON connection.tenant_id = conversation.tenant_id
         AND connection.id = conversation.channel_connection_id
        JOIN contact_identities AS identity
          ON identity.tenant_id = conversation.tenant_id
         AND identity.contact_id = conversation.contact_id
         AND identity.channel_kind = 'telegram_bot'
         AND identity.external_id = concat(
             conversation.channel_connection_id::text, ':',
             identity.metadata ->> 'telegram_user_id'
         )
        WHERE conversation.tenant_id = $1
          AND conversation.project_id = $2
          AND conversation.inbox_id = $3
          AND conversation.id = $4
          AND connection.kind = 'telegram_bot'
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM contacts AS contact
              WHERE contact.tenant_id = conversation.tenant_id
                AND contact.id = conversation.contact_id
                AND contact.is_blocked
          )
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(conversation_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((channel_connection_id, contact_id, Some(telegram_user_id), Some(chat_id), language)) =
        target
    else {
        return Ok(());
    };
    if telegram_user_id.parse::<i64>().is_err() || chat_id.parse::<i64>().is_err() {
        return Ok(());
    }

    let invitation_id = Uuid::now_v7();
    let inserted = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO support_rating_telegram_invitations (
            id, tenant_id, project_id, inbox_id, resolution_id,
            channel_connection_id, contact_id, telegram_user_id, chat_id,
            language, expires_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, lower($10),
            now() + ($11 * interval '1 day')
        )
        ON CONFLICT (tenant_id, resolution_id) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(invitation_id)
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(resolution_id)
    .bind(channel_connection_id)
    .bind(contact_id)
    .bind(telegram_user_id)
    .bind(chat_id)
    .bind(language)
    .bind(RATING_INVITATION_TTL_DAYS)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(invitation_id) = inserted {
        insert_channel_outbox(
            transaction,
            tenant_id,
            invitation_id,
            RATING_PROMPT_EVENT_TYPE,
            json!({ "invitation_id": invitation_id }),
        )
        .await?;
    }
    Ok(())
}

async fn process_rating_callback(
    state: &AppState,
    channel: &WebhookChannel,
    callback: &TelegramCallbackQuery,
) -> Result<(), AppError> {
    let Some(data) = callback.data.as_deref() else {
        return Ok(());
    };
    let Some(message) = &callback.message else {
        return Ok(());
    };
    if message.chat.kind != "private" || callback.sender.is_bot {
        return Ok(());
    }
    if data == RATING_SKIP_CALLBACK {
        return skip_rating_comment(state, channel, callback, message).await;
    }
    let Some(rating) = parse_rating_callback(data) else {
        return Ok(());
    };

    let mut transaction = state.db.begin().await?;
    let invitation = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid, String)>(
        r#"
        SELECT id, project_id, inbox_id, resolution_id, language
        FROM support_rating_telegram_invitations
        WHERE tenant_id = $1 AND channel_connection_id = $2
          AND telegram_user_id = $3 AND chat_id = $4
          AND prompt_message_id = $5 AND status = 'pending'
          AND expires_at > now()
        FOR UPDATE
        "#,
    )
    .bind(channel.tenant_id)
    .bind(channel.channel_connection_id)
    .bind(callback.sender.id.to_string())
    .bind(message.chat.id.to_string())
    .bind(message.message_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some((invitation_id, project_id, inbox_id, resolution_id, language)) = invitation else {
        transaction.commit().await?;
        answer_rating_callback(state, channel, &callback.id, rating_copy("en").unavailable).await?;
        return Ok(());
    };

    let support_rating_id = Uuid::now_v7();
    let inserted = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO support_ratings (
            id, tenant_id, project_id, inbox_id, resolution_id, rating
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (tenant_id, resolution_id) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(support_rating_id)
    .bind(channel.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(resolution_id)
    .bind(rating)
    .fetch_optional(&mut *transaction)
    .await?;
    if inserted.is_none() {
        sqlx::query(
            r#"
            UPDATE support_rating_telegram_invitations
            SET status = 'completed', completed_at = now(), updated_at = now()
            WHERE tenant_id = $1 AND id = $2
            "#,
        )
        .bind(channel.tenant_id)
        .bind(invitation_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        answer_rating_callback(
            state,
            channel,
            &callback.id,
            rating_copy(&language).unavailable,
        )
        .await?;
        return Ok(());
    }

    sqlx::query(
        r#"
        UPDATE support_rating_telegram_invitations
        SET support_rating_id = $3, status = 'rating_recorded', updated_at = now()
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(channel.tenant_id)
    .bind(invitation_id)
    .bind(support_rating_id)
    .execute(&mut *transaction)
    .await?;
    insert_channel_outbox(
        &mut transaction,
        channel.tenant_id,
        invitation_id,
        RATING_COMMENT_PROMPT_EVENT_TYPE,
        json!({ "invitation_id": invitation_id }),
    )
    .await?;
    transaction.commit().await?;
    answer_rating_callback(
        state,
        channel,
        &callback.id,
        rating_copy(&language).recorded,
    )
    .await
}

async fn skip_rating_comment(
    state: &AppState,
    channel: &WebhookChannel,
    callback: &TelegramCallbackQuery,
    message: &TelegramCallbackMessage,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await?;
    let invitation = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT id, language
        FROM support_rating_telegram_invitations
        WHERE tenant_id = $1 AND channel_connection_id = $2
          AND telegram_user_id = $3 AND chat_id = $4
          AND comment_prompt_message_id = $5 AND status = 'awaiting_comment'
          AND expires_at > now()
        FOR UPDATE
        "#,
    )
    .bind(channel.tenant_id)
    .bind(channel.channel_connection_id)
    .bind(callback.sender.id.to_string())
    .bind(message.chat.id.to_string())
    .bind(message.message_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some((invitation_id, language)) = invitation else {
        transaction.commit().await?;
        answer_rating_callback(state, channel, &callback.id, rating_copy("en").unavailable).await?;
        return Ok(());
    };
    complete_rating_invitation(&mut transaction, channel.tenant_id, invitation_id).await?;
    transaction.commit().await?;
    answer_rating_callback(
        state,
        channel,
        &callback.id,
        rating_copy(&language).recorded,
    )
    .await
}

async fn process_rating_comment(
    state: &AppState,
    channel: &WebhookChannel,
    sender: &TelegramUser,
    chat_id: i64,
    reply_to_message_id: Option<i64>,
    text: &str,
) -> Result<bool, AppError> {
    let mut transaction = state.db.begin().await?;
    let invitation = sqlx::query_as::<_, (Uuid, Uuid, String)>(
        r#"
        SELECT id, support_rating_id, language
        FROM support_rating_telegram_invitations
        WHERE tenant_id = $1 AND channel_connection_id = $2
          AND telegram_user_id = $3 AND chat_id = $4
          AND ($5::bigint IS NULL OR comment_prompt_message_id = $5)
          AND status = 'awaiting_comment'
          AND expires_at > now() AND support_rating_id IS NOT NULL
        ORDER BY updated_at DESC, id DESC
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(channel.tenant_id)
    .bind(channel.channel_connection_id)
    .bind(sender.id.to_string())
    .bind(chat_id.to_string())
    .bind(reply_to_message_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some((invitation_id, support_rating_id, language)) = invitation else {
        transaction.commit().await?;
        return Ok(false);
    };
    let comment = text.trim();
    if comment.is_empty() || comment.chars().count() > MAX_RATING_COMMENT_CHARS {
        transaction.commit().await?;
        send_rating_text(
            state,
            channel,
            chat_id,
            rating_copy(&language).invalid_comment,
            None,
        )
        .await?;
        return Ok(true);
    }
    sqlx::query("UPDATE support_ratings SET comment = $3 WHERE tenant_id = $1 AND id = $2")
        .bind(channel.tenant_id)
        .bind(support_rating_id)
        .bind(comment)
        .execute(&mut *transaction)
        .await?;
    complete_rating_invitation(&mut transaction, channel.tenant_id, invitation_id).await?;
    transaction.commit().await?;
    Ok(true)
}

async fn complete_rating_invitation(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    invitation_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE support_rating_telegram_invitations
        SET status = 'completed', completed_at = now(), updated_at = now()
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(invitation_id)
    .execute(&mut **transaction)
    .await?;
    insert_channel_outbox(
        transaction,
        tenant_id,
        invitation_id,
        RATING_THANKS_EVENT_TYPE,
        json!({ "invitation_id": invitation_id }),
    )
    .await
}

async fn answer_rating_callback(
    state: &AppState,
    channel: &WebhookChannel,
    callback_query_id: &str,
    text: &str,
) -> Result<(), AppError> {
    let secrets = decrypt_channel_secrets(state, channel)?;
    let _: bool = telegram_api(
        state,
        &secrets.bot_token,
        "answerCallbackQuery",
        json!({ "callback_query_id": callback_query_id, "text": text }),
    )
    .await
    .map_err(AppError::internal)?;
    Ok(())
}

fn parse_rating_callback(value: &str) -> Option<i16> {
    value
        .strip_prefix(RATING_CALLBACK_PREFIX)?
        .parse::<i16>()
        .ok()
        .filter(|rating| (1..=5).contains(rating))
}

struct RatingCopy {
    prompt: &'static str,
    comment_prompt: &'static str,
    skip: &'static str,
    thanks: &'static str,
    recorded: &'static str,
    unavailable: &'static str,
    invalid_comment: &'static str,
}

fn rating_copy(language: &str) -> RatingCopy {
    if language.split('-').next() == Some("ru") {
        RatingCopy {
            prompt: "Спасибо за обращение! Оцените, пожалуйста, качество поддержки.",
            comment_prompt: "Если хотите, ответьте на это сообщение комментарием. Это необязательно.",
            skip: "Без комментария",
            thanks: "Спасибо! Ваш отзыв поможет нам стать лучше.",
            recorded: "Оценка сохранена",
            unavailable: "Эта оценка уже сохранена или больше недоступна",
            invalid_comment: "Комментарий должен содержать от 1 до 2000 символов. Ответьте на предыдущее сообщение ещё раз.",
        }
    } else {
        RatingCopy {
            prompt: "Thanks for chatting with us. Please rate the support you received.",
            comment_prompt: "If you would like, reply to this message with a comment. This is optional.",
            skip: "No comment",
            thanks: "Thank you! Your feedback helps us improve.",
            recorded: "Rating saved",
            unavailable: "This rating was already saved or is no longer available",
            invalid_comment: "A comment must contain between 1 and 2000 characters. Reply to the previous message again.",
        }
    }
}

async fn begin_inbound_event(
    state: &AppState,
    channel: &WebhookChannel,
    update_id: i64,
    payload_hash: &[u8],
) -> Result<Option<Uuid>, AppError> {
    let event_id = Uuid::now_v7();
    let inserted = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO inbound_events (
            id, tenant_id, channel_connection_id, provider_event_id, payload_hash
        ) VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (tenant_id, channel_connection_id, provider_event_id) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(event_id)
    .bind(channel.tenant_id)
    .bind(channel.channel_connection_id)
    .bind(update_id.to_string())
    .bind(payload_hash)
    .fetch_optional(&state.db)
    .await?;
    if inserted.is_some() {
        return Ok(inserted);
    }
    let existing = sqlx::query_as::<_, (Uuid, Vec<u8>, Option<chrono::DateTime<Utc>>)>(
        r#"
        SELECT id, payload_hash, processed_at
        FROM inbound_events
        WHERE tenant_id = $1 AND channel_connection_id = $2 AND provider_event_id = $3
        "#,
    )
    .bind(channel.tenant_id)
    .bind(channel.channel_connection_id)
    .bind(update_id.to_string())
    .fetch_one(&state.db)
    .await?;
    if existing.1 != payload_hash {
        return Err(AppError::Conflict(
            "Telegram update id was reused with a different payload".to_owned(),
        ));
    }
    Ok(existing.2.is_none().then_some(existing.0))
}

async fn mark_inbound_processed(
    state: &AppState,
    tenant_id: Uuid,
    event_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE inbound_events SET processed_at = COALESCE(processed_at, now()) WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(event_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn ensure_contact_and_conversation(
    state: &AppState,
    channel: &WebhookChannel,
    sender: &TelegramUser,
    chat_id: i64,
) -> Result<(Uuid, Uuid), AppError> {
    let external_id = format!("{}:{}", channel.channel_connection_id, sender.id);
    let display_name = telegram_display_name(sender);
    let metadata = json!({
        "telegram_user_id": sender.id.to_string(),
        "chat_id": chat_id.to_string(),
        "username": sender.username,
        "language_code": sender.language_code,
    });
    let mut transaction = state.db.begin().await?;
    let advisory_key = format!("{}:telegram_bot:{external_id}", channel.tenant_id);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(advisory_key)
        .execute(&mut *transaction)
        .await?;
    let existing_contact = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT identity.contact_id, contact.project_id
        FROM contact_identities AS identity
        JOIN contacts AS contact
          ON contact.tenant_id = identity.tenant_id
         AND contact.id = identity.contact_id
        WHERE identity.tenant_id = $1
          AND identity.channel_kind = 'telegram_bot'
          AND identity.external_id = $2
        "#,
    )
    .bind(channel.tenant_id)
    .bind(&external_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let contact_id = if let Some((contact_id, project_id)) = existing_contact {
        if project_id != channel.project_id {
            return Err(AppError::internal(anyhow::anyhow!(
                "Telegram identity resolved to a different project"
            )));
        }
        sqlx::query(
            "UPDATE contacts SET display_name = $3, updated_at = now() WHERE tenant_id = $1 AND id = $2",
        )
        .bind(channel.tenant_id)
        .bind(contact_id)
        .bind(&display_name)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE contact_identities
            SET metadata = $3, updated_at = now()
            WHERE tenant_id = $1 AND contact_id = $2
              AND channel_kind = 'telegram_bot' AND external_id = $4
            "#,
        )
        .bind(channel.tenant_id)
        .bind(contact_id)
        .bind(&metadata)
        .bind(&external_id)
        .execute(&mut *transaction)
        .await?;
        contact_id
    } else {
        let contact_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO contacts (id, tenant_id, project_id, display_name) VALUES ($1, $2, $3, $4)",
        )
        .bind(contact_id)
        .bind(channel.tenant_id)
        .bind(channel.project_id)
        .bind(&display_name)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO contact_identities (
                id, tenant_id, contact_id, channel_kind, external_id, metadata
            ) VALUES ($1, $2, $3, 'telegram_bot', $4, $5)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(channel.tenant_id)
        .bind(contact_id)
        .bind(&external_id)
        .bind(&metadata)
        .execute(&mut *transaction)
        .await?;
        contact_id
    };

    if let Some(conversation_id) = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM conversations
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
          AND channel_connection_id = $4 AND contact_id = $5
          AND status <> 'resolved'
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(channel.tenant_id)
    .bind(channel.project_id)
    .bind(channel.inbox_id)
    .bind(channel.channel_connection_id)
    .bind(contact_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        transaction.commit().await?;
        return Ok((contact_id, conversation_id));
    }

    let conversation_id = Uuid::now_v7();
    let now = Utc::now();
    sqlx::query(
        r#"
        INSERT INTO conversations (
            id, tenant_id, project_id, inbox_id, channel_connection_id,
            contact_id, status, widget_language, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, 'new', $7, $8, $8)
        "#,
    )
    .bind(conversation_id)
    .bind(channel.tenant_id)
    .bind(channel.project_id)
    .bind(channel.inbox_id)
    .bind(channel.channel_connection_id)
    .bind(contact_id)
    .bind(sender.language_code.as_deref())
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO conversation_participants (
            id, tenant_id, conversation_id, participant_kind, contact_id
        ) VALUES ($1, $2, $3, 'contact', $4)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(channel.tenant_id)
    .bind(conversation_id)
    .bind(contact_id)
    .execute(&mut *transaction)
    .await?;
    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: channel.tenant_id,
        project_id: channel.project_id,
        inbox_id: channel.inbox_id,
        contact_id: Some(contact_id),
        event_type: "conversation.created".to_owned(),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: now,
        data: json!({ "conversation_id": conversation_id }),
    };
    conversations::insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    conversations::publish_best_effort(state, &event).await;
    Ok((contact_id, conversation_id))
}

pub(crate) async fn enqueue_outbound_if_needed(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    channel_connection_id: Uuid,
    message_id: Uuid,
) -> Result<bool, AppError> {
    let delivery_target = sqlx::query_as::<_, (String, Option<String>)>(
        r#"
        SELECT connection.kind, identity.metadata ->> 'chat_id'
        FROM messages AS message
        JOIN conversations AS conversation
          ON conversation.tenant_id = message.tenant_id
         AND conversation.id = message.conversation_id
        JOIN channel_connections AS connection
          ON connection.tenant_id = conversation.tenant_id
         AND connection.id = conversation.channel_connection_id
        LEFT JOIN contact_identities AS identity
          ON identity.tenant_id = conversation.tenant_id
         AND identity.contact_id = conversation.contact_id
         AND identity.channel_kind = 'telegram_bot'
         AND identity.external_id = concat(
             connection.id::text, ':', identity.metadata ->> 'telegram_user_id'
         )
        WHERE message.tenant_id = $1 AND message.id = $2
          AND connection.id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(message_id)
    .bind(channel_connection_id)
    .fetch_one(&mut **transaction)
    .await?;
    if delivery_target.0 != "telegram_bot" {
        return Ok(false);
    }
    let chat_id = delivery_target.1.ok_or_else(|| {
        AppError::internal(anyhow::anyhow!(
            "Telegram conversation has no delivery target"
        ))
    })?;
    if chat_id.parse::<i64>().is_err() {
        return Err(AppError::internal(anyhow::anyhow!(
            "Telegram conversation has an invalid delivery target"
        )));
    }
    sqlx::query(
        r#"
        INSERT INTO message_deliveries (
            id, tenant_id, message_id, channel_connection_id, status
        ) VALUES ($1, $2, $3, $4, 'queued')
        ON CONFLICT (tenant_id, message_id, channel_connection_id) DO NOTHING
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(message_id)
    .bind(channel_connection_id)
    .execute(&mut **transaction)
    .await?;
    insert_channel_outbox(
        transaction,
        tenant_id,
        message_id,
        DELIVERY_EVENT_TYPE,
        json!({ "message_id": message_id }),
    )
    .await?;
    Ok(true)
}

pub(crate) async fn enqueue_request_acknowledgement(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    message_id: Uuid,
) -> Result<(), AppError> {
    insert_channel_outbox(
        transaction,
        tenant_id,
        message_id,
        REQUEST_ACKNOWLEDGEMENT_EVENT_TYPE,
        json!({ "message_id": message_id }),
    )
    .await
}

pub(crate) async fn enqueue_unregister(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    channel_id: Uuid,
) -> Result<(), AppError> {
    insert_channel_outbox(
        transaction,
        tenant_id,
        channel_id,
        UNREGISTER_EVENT_TYPE,
        json!({ "channel_connection_id": channel_id }),
    )
    .await
}

async fn insert_channel_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    aggregate_id: Uuid,
    event_type: &str,
    payload: Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload
        ) VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(OUTBOX_AGGREGATE_TYPE)
    .bind(aggregate_id)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn deliver_outbox(
    state: &AppState,
    event_type: &str,
    aggregate_id: Uuid,
) -> Result<()> {
    match event_type {
        SETUP_EVENT_TYPE => setup_webhook(state, aggregate_id).await,
        DELIVERY_EVENT_TYPE => deliver_message(state, aggregate_id).await,
        UNREGISTER_EVENT_TYPE => unregister_webhook(state, aggregate_id).await,
        RATING_PROMPT_EVENT_TYPE => deliver_rating_prompt(state, aggregate_id).await,
        RATING_COMMENT_PROMPT_EVENT_TYPE => {
            deliver_rating_comment_prompt(state, aggregate_id).await
        }
        RATING_THANKS_EVENT_TYPE => deliver_rating_thanks(state, aggregate_id).await,
        REQUEST_ACKNOWLEDGEMENT_EVENT_TYPE => {
            deliver_request_acknowledgement(state, aggregate_id).await
        }
        _ => anyhow::bail!("unsupported Telegram bot outbox event"),
    }
}

/// Checks webhook health or drains one bot through the emergency polling fallback.
///
/// Returns `true` when a receiver was claimed and inspected.
///
/// # Errors
///
/// Returns an error when the receiver cannot be claimed, Telegram cannot be
/// reached, an update cannot be processed, or the receiver state cannot be saved.
pub async fn maintain_receiver_once(state: &AppState, worker_id: Uuid) -> Result<bool> {
    let Some(claim) = claim_receiver(state, worker_id).await? else {
        return Ok(false);
    };
    let result = maintain_claimed_receiver(state, worker_id, &claim).await;
    if result.is_err() {
        if claim.receive_mode == "polling" {
            release_failed_polling_claim(state, worker_id, &claim).await?;
        } else {
            release_receiver_claim(state, worker_id, &claim, None).await?;
        }
    }
    result.map(|()| true)
}

async fn claim_receiver(state: &AppState, worker_id: Uuid) -> Result<Option<ReceiverClaim>> {
    let mut transaction = state.db.begin().await?;
    let claim = sqlx::query_as::<_, ReceiverClaim>(
        r#"
        SELECT receiver.tenant_id, connection.project_id, connection.inbox_id,
               receiver.channel_connection_id, secret.encrypted_payload, secret.key_version,
               connection.config ->> 'webhook_url' AS webhook_url,
               receiver.receive_mode, receiver.polling_offset,
               receiver.webhook_failure_started_at, receiver.next_webhook_retry_at
        FROM telegram_bot_receivers AS receiver
        JOIN channel_connections AS connection
          ON connection.tenant_id = receiver.tenant_id
         AND connection.id = receiver.channel_connection_id
        JOIN channel_secrets AS secret
          ON secret.tenant_id = receiver.tenant_id
         AND secret.channel_connection_id = receiver.channel_connection_id
        JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
        WHERE connection.kind = 'telegram_bot'
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
          AND inbox.status = 'active'
          AND (receiver.lease_expires_at IS NULL OR receiver.lease_expires_at <= now())
          AND (
              receiver.receive_mode = 'polling'
              OR receiver.last_webhook_check_at IS NULL
              OR receiver.last_webhook_check_at <= now() - ($1 * interval '1 second')
          )
        ORDER BY (receiver.receive_mode = 'polling') DESC,
                 receiver.last_webhook_check_at NULLS FIRST,
                 receiver.updated_at
        FOR UPDATE OF receiver SKIP LOCKED
        LIMIT 1
        "#,
    )
    .bind(WEBHOOK_CHECK_INTERVAL_SECONDS)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(claim) = claim else {
        transaction.commit().await?;
        return Ok(None);
    };
    sqlx::query(
        r#"
        UPDATE telegram_bot_receivers
        SET lease_owner = $3,
            lease_expires_at = now() + ($4 * interval '1 second'),
            updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2
        "#,
    )
    .bind(claim.tenant_id)
    .bind(claim.channel_connection_id)
    .bind(worker_id)
    .bind(RECEIVER_LEASE_SECONDS)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Some(claim))
}

async fn maintain_claimed_receiver(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
) -> Result<()> {
    let secrets = decrypt_stored_secrets(
        state,
        claim.tenant_id,
        claim.project_id,
        claim.inbox_id,
        claim.channel_connection_id,
        &claim.encrypted_payload,
        &claim.key_version,
    )?;
    match claim.receive_mode.as_str() {
        "webhook" => maintain_webhook_receiver(state, worker_id, claim, &secrets).await,
        "polling" => maintain_polling_receiver(state, worker_id, claim, &secrets).await,
        _ => anyhow::bail!("Telegram receiver mode is invalid"),
    }
}

async fn maintain_webhook_receiver(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
    secrets: &TelegramBotSecrets,
) -> Result<()> {
    let info: TelegramWebhookInfo =
        telegram_api(state, &secrets.bot_token, "getWebhookInfo", json!({})).await?;
    if info.url != claim.webhook_url || !webhook_receives_required_updates(&info) {
        register_webhook(state, secrets, &claim.webhook_url, false).await?;
        return reset_receiver_to_webhook(state, worker_id, claim).await;
    }

    let now = Utc::now();
    if !webhook_delivery_is_failing(&info, now) {
        return reset_receiver_to_webhook(state, worker_id, claim).await;
    }
    let Some(failure_started_at) = claim.webhook_failure_started_at else {
        return record_webhook_failure(state, worker_id, claim).await;
    };
    if now - failure_started_at < TimeDelta::seconds(WEBHOOK_FAILURE_GRACE_SECONDS) {
        return release_receiver_claim(state, worker_id, claim, Some(failure_started_at)).await;
    }

    let _: bool = telegram_api(
        state,
        &secrets.bot_token,
        "deleteWebhook",
        json!({ "drop_pending_updates": false }),
    )
    .await?;
    enter_polling_mode(state, worker_id, claim).await
}

async fn maintain_polling_receiver(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
    secrets: &TelegramBotSecrets,
) -> Result<()> {
    let mut payload = json!({
        "limit": 100,
        "timeout": GET_UPDATES_TIMEOUT_SECONDS,
        "allowed_updates": RECEIVER_ALLOWED_UPDATES,
    });
    if let Some(offset) = claim.polling_offset {
        payload["offset"] = json!(offset);
    }
    let updates: Vec<Value> =
        match telegram_api(state, &secrets.bot_token, "getUpdates", payload).await {
            Ok(updates) => updates,
            Err(error) => {
                let info: TelegramWebhookInfo =
                    telegram_api(state, &secrets.bot_token, "getWebhookInfo", json!({})).await?;
                if info.url == claim.webhook_url {
                    reset_receiver_to_webhook(state, worker_id, claim).await?;
                    return Ok(());
                }
                return Err(error);
            }
        };
    let mut next_offset = claim.polling_offset;
    let webhook_channel = claim.webhook_channel();
    for update in updates {
        let update_id = update
            .get("update_id")
            .and_then(Value::as_i64)
            .context("Telegram polling update has no valid update_id")?;
        process_inbound_update(state, &webhook_channel, update)
            .await
            .map_err(anyhow::Error::new)?;
        next_offset = Some(advance_polling_offset(next_offset, update_id)?);
    }

    if next_offset != claim.polling_offset {
        return release_polling_claim(state, worker_id, claim, next_offset).await;
    }
    if claim
        .next_webhook_retry_at
        .is_some_and(|retry_at| retry_at <= Utc::now())
    {
        register_webhook(state, secrets, &claim.webhook_url, false).await?;
        return reset_receiver_to_webhook(state, worker_id, claim).await;
    }
    release_polling_claim(state, worker_id, claim, next_offset).await
}

fn webhook_delivery_is_failing(info: &TelegramWebhookInfo, now: DateTime<Utc>) -> bool {
    if info.pending_update_count <= 0 {
        return false;
    }
    info.last_error_date
        .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
        .is_some_and(|last_error_at| {
            last_error_at <= now
                && now - last_error_at <= TimeDelta::seconds(WEBHOOK_FAILURE_GRACE_SECONDS * 2)
        })
}

fn webhook_receives_required_updates(info: &TelegramWebhookInfo) -> bool {
    info.allowed_updates.as_ref().is_none_or(|allowed_updates| {
        allowed_updates.is_empty()
            || RECEIVER_ALLOWED_UPDATES
                .iter()
                .all(|required| allowed_updates.iter().any(|allowed| allowed == required))
    })
}

fn advance_polling_offset(current: Option<i64>, update_id: i64) -> Result<i64> {
    let candidate = update_id
        .checked_add(1)
        .context("Telegram update_id cannot be advanced")?;
    Ok(current.map_or(candidate, |offset| offset.max(candidate)))
}

async fn record_webhook_failure(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
) -> Result<()> {
    warn!(
        channel_connection_id = %claim.channel_connection_id,
        "Telegram webhook delivery is failing; polling fallback grace period started"
    );
    release_receiver_claim(state, worker_id, claim, Some(Utc::now())).await
}

async fn enter_polling_mode(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
) -> Result<()> {
    let updated = sqlx::query(
        r#"
        UPDATE telegram_bot_receivers
        SET receive_mode = 'polling', polling_offset = NULL,
            webhook_failure_started_at = NULL,
            next_webhook_retry_at = now() + ($4 * interval '1 second'),
            last_webhook_check_at = now(), lease_owner = NULL, lease_expires_at = NULL,
            updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2 AND lease_owner = $3
        "#,
    )
    .bind(claim.tenant_id)
    .bind(claim.channel_connection_id)
    .bind(worker_id)
    .bind(WEBHOOK_RECOVERY_INTERVAL_SECONDS)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() > 0 {
        warn!(
            channel_connection_id = %claim.channel_connection_id,
            "Telegram receiver switched to polling fallback"
        );
    }
    Ok(())
}

async fn reset_receiver_to_webhook(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
) -> Result<()> {
    let updated = sqlx::query(
        r#"
        UPDATE telegram_bot_receivers
        SET receive_mode = 'webhook', polling_offset = NULL,
            webhook_failure_started_at = NULL, next_webhook_retry_at = NULL,
            last_webhook_check_at = now(), lease_owner = NULL, lease_expires_at = NULL,
            updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2
          AND (lease_owner = $3 OR lease_owner IS NULL)
        "#,
    )
    .bind(claim.tenant_id)
    .bind(claim.channel_connection_id)
    .bind(worker_id)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() > 0
        && (claim.receive_mode == "polling" || claim.webhook_failure_started_at.is_some())
    {
        info!(
            channel_connection_id = %claim.channel_connection_id,
            "Telegram receiver restored webhook delivery"
        );
    }
    Ok(())
}

async fn release_receiver_claim(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
    failure_started_at: Option<DateTime<Utc>>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE telegram_bot_receivers
        SET webhook_failure_started_at = $4, last_webhook_check_at = now(),
            lease_owner = NULL, lease_expires_at = NULL, updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2 AND lease_owner = $3
        "#,
    )
    .bind(claim.tenant_id)
    .bind(claim.channel_connection_id)
    .bind(worker_id)
    .bind(failure_started_at)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn release_polling_claim(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
    next_offset: Option<i64>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE telegram_bot_receivers
        SET polling_offset = $4, lease_owner = NULL, lease_expires_at = NULL,
            updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2 AND lease_owner = $3
        "#,
    )
    .bind(claim.tenant_id)
    .bind(claim.channel_connection_id)
    .bind(worker_id)
    .bind(next_offset)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn release_failed_polling_claim(
    state: &AppState,
    worker_id: Uuid,
    claim: &ReceiverClaim,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE telegram_bot_receivers
        SET next_webhook_retry_at = now() + ($4 * interval '1 second'),
            lease_owner = NULL, lease_expires_at = NULL, updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2 AND lease_owner = $3
        "#,
    )
    .bind(claim.tenant_id)
    .bind(claim.channel_connection_id)
    .bind(worker_id)
    .bind(WEBHOOK_RECOVERY_INTERVAL_SECONDS)
    .execute(&state.db)
    .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct TelegramApiEnvelope<T> {
    ok: bool,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct TelegramBotIdentity {
    id: i64,
    username: Option<String>,
}

async fn setup_webhook(state: &AppState, channel_id: Uuid) -> Result<()> {
    let channel = load_delivery_channel(state, channel_id, false).await?;
    if channel.deleted_at.is_some() {
        return Ok(());
    }
    let secrets = decrypt_delivery_secrets(state, &channel)?;
    let identity: TelegramBotIdentity =
        telegram_api(state, &secrets.bot_token, "getMe", json!({})).await?;
    let username = identity
        .username
        .filter(|value| !value.trim().is_empty())
        .context("Telegram bot has no username")?;
    let claimed = sqlx::query(
        r#"
        UPDATE channel_connections
        SET config = jsonb_set(
                jsonb_set(config, '{bot_user_id}', to_jsonb($2::text), true),
                '{bot_username}', to_jsonb($3::text), true
            ),
            updated_at = now()
        WHERE tenant_id = $1 AND id = $4 AND kind = 'telegram_bot'
          AND deleted_at IS NULL
        "#,
    )
    .bind(channel.tenant_id)
    .bind(identity.id.to_string())
    .bind(&username)
    .bind(channel_id)
    .execute(&state.db)
    .await
    .context("failed to reserve Telegram bot for this channel")?;
    if claimed.rows_affected() == 0 {
        return Ok(());
    }
    let webhook_url = channel
        .webhook_url
        .clone()
        .context("Telegram webhook URL is missing")?;
    register_webhook(state, &secrets, &webhook_url, true).await?;
    let activated = sqlx::query(
        r#"
        UPDATE channel_connections
        SET status = 'active', updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND kind = 'telegram_bot'
          AND deleted_at IS NULL AND config ->> 'bot_user_id' = $3
        "#,
    )
    .bind(channel.tenant_id)
    .bind(channel_id)
    .bind(identity.id.to_string())
    .execute(&state.db)
    .await
    .context("failed to activate Telegram bot channel")?;
    if activated.rows_affected() == 0 {
        let _: bool = telegram_api(
            state,
            &secrets.bot_token,
            "deleteWebhook",
            json!({ "drop_pending_updates": false }),
        )
        .await?;
    } else {
        mark_receiver_webhook_active(state, channel.tenant_id, channel_id).await?;
    }
    Ok(())
}

async fn register_webhook(
    state: &AppState,
    secrets: &TelegramBotSecrets,
    webhook_url: &str,
    drop_pending_updates: bool,
) -> Result<()> {
    let _: bool = telegram_api(
        state,
        &secrets.bot_token,
        "setWebhook",
        json!({
            "url": webhook_url,
            "secret_token": secrets.webhook_secret,
            "allowed_updates": RECEIVER_ALLOWED_UPDATES,
            "drop_pending_updates": drop_pending_updates,
        }),
    )
    .await?;
    Ok(())
}

async fn mark_receiver_webhook_active(
    state: &AppState,
    tenant_id: Uuid,
    channel_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE telegram_bot_receivers
        SET receive_mode = 'webhook', polling_offset = NULL,
            webhook_failure_started_at = NULL, next_webhook_retry_at = NULL,
            last_webhook_check_at = now(), lease_owner = NULL, lease_expires_at = NULL,
            updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(channel_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
struct DeliveryChannel {
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_connection_id: Uuid,
    encrypted_payload: Vec<u8>,
    key_version: String,
    webhook_url: Option<String>,
    deleted_at: Option<chrono::DateTime<Utc>>,
}

async fn load_delivery_channel(
    state: &AppState,
    channel_id: Uuid,
    require_active: bool,
) -> Result<DeliveryChannel> {
    sqlx::query_as::<_, DeliveryChannel>(
        r#"
        SELECT connection.tenant_id, connection.project_id, connection.inbox_id,
               connection.id AS channel_connection_id,
               secret.encrypted_payload, secret.key_version,
               connection.config ->> 'webhook_url' AS webhook_url,
               connection.deleted_at
        FROM channel_connections AS connection
        JOIN channel_secrets AS secret
          ON secret.tenant_id = connection.tenant_id
         AND secret.channel_connection_id = connection.id
        WHERE connection.id = $1 AND connection.kind = 'telegram_bot'
          AND (NOT $2 OR (connection.status = 'active' AND connection.deleted_at IS NULL))
        "#,
    )
    .bind(channel_id)
    .bind(require_active)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram bot channel is unavailable")
}

async fn deliver_message(state: &AppState, message_id: Uuid) -> Result<()> {
    let message = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid, Uuid, String, String, bool)>(
        r#"
        SELECT message.tenant_id, message.project_id, message.inbox_id,
               conversation.channel_connection_id, conversation.contact_id,
               message.body, identity.metadata ->> 'chat_id' AS chat_id,
               contact.is_blocked AND (
                   message.author_kind = 'ai'
                   OR EXISTS (
                       SELECT 1 FROM outbox_events AS provider_job
                       WHERE provider_job.tenant_id = message.tenant_id
                         AND provider_job.id = message.client_message_id
                         AND provider_job.aggregate_type = 'provider_reply'
                   )
               ) AS suppress_automatic_reply
        FROM messages AS message
        JOIN conversations AS conversation
          ON conversation.tenant_id = message.tenant_id
         AND conversation.id = message.conversation_id
        JOIN contacts AS contact
          ON contact.tenant_id = conversation.tenant_id
         AND contact.id = conversation.contact_id
        JOIN contact_identities AS identity
          ON identity.tenant_id = conversation.tenant_id
         AND identity.contact_id = conversation.contact_id
         AND identity.channel_kind = 'telegram_bot'
         AND identity.external_id = concat(
             conversation.channel_connection_id::text, ':',
             identity.metadata ->> 'telegram_user_id'
         )
        WHERE message.id = $1 AND message.direction = 'outbound' AND message.kind = 'text'
        "#,
    )
    .bind(message_id)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram outbound message is unavailable")?;
    if message.7 {
        mark_message_failed(state, message_id, "Contact is blocked").await?;
        return Ok(());
    }
    let channel = load_delivery_channel(state, message.3, true).await?;
    let secrets = decrypt_delivery_secrets(state, &channel)?;
    let sent: TelegramSentMessage = telegram_api(
        state,
        &secrets.bot_token,
        "sendMessage",
        json!({ "chat_id": message.6, "text": message.5 }),
    )
    .await?;

    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        UPDATE message_deliveries
        SET status = 'sent', provider_message_id = $3, last_error = NULL, updated_at = now()
        WHERE tenant_id = $1 AND message_id = $2 AND channel_connection_id = $4
        "#,
    )
    .bind(message.0)
    .bind(message_id)
    .bind(sent.message_id.to_string())
    .bind(message.3)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE messages SET status = 'sent' WHERE tenant_id = $1 AND id = $2 AND status NOT IN ('delivered', 'read')",
    )
    .bind(message.0)
    .bind(message_id)
    .execute(&mut *transaction)
    .await?;
    let event = message_status_event(
        message.0, message.1, message.2, message.4, message_id, "sent",
    );
    conversations::insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    conversations::publish_best_effort(state, &event).await;
    Ok(())
}

async fn deliver_request_acknowledgement(state: &AppState, message_id: Uuid) -> Result<()> {
    let target = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, bool)>(
        r#"
        SELECT message.tenant_id, conversation.channel_connection_id,
               identity.metadata ->> 'chat_id' AS chat_id,
               identity.metadata ->> 'language_code' AS language_code,
               contact.is_blocked
        FROM messages AS message
        JOIN conversations AS conversation
          ON conversation.tenant_id = message.tenant_id
         AND conversation.id = message.conversation_id
        JOIN contacts AS contact
          ON contact.tenant_id = conversation.tenant_id
         AND contact.id = conversation.contact_id
        JOIN contact_identities AS identity
          ON identity.tenant_id = conversation.tenant_id
         AND identity.contact_id = conversation.contact_id
         AND identity.channel_kind = 'telegram_bot'
         AND identity.external_id = concat(
             conversation.channel_connection_id::text, ':',
             identity.metadata ->> 'telegram_user_id'
         )
        WHERE message.id = $1
          AND message.direction = 'inbound'
          AND message.author_kind = 'contact'
        "#,
    )
    .bind(message_id)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram request acknowledgement target is unavailable")?;
    if target.4 {
        return Ok(());
    }
    let channel = load_delivery_channel(state, target.1, true).await?;
    let secrets = decrypt_delivery_secrets(state, &channel)?;
    let _: TelegramSentMessage = telegram_api(
        state,
        &secrets.bot_token,
        "sendMessage",
        json!({
            "chat_id": target.2,
            "text": request_acknowledgement_text(target.3.as_deref()),
        }),
    )
    .await?;
    Ok(())
}

fn request_acknowledgement_text(language: Option<&str>) -> &'static str {
    let primary_language = language
        .unwrap_or("en")
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase();
    match primary_language.as_str() {
        "ru" => {
            "Ваш запрос получен. Пожалуйста, ожидайте свободного оператора — вам обязательно ответят."
        }
        "zh" => "您的请求已收到。请等待有空的客服人员，我们一定会回复您。",
        "hi" => {
            "आपका अनुरोध प्राप्त हो गया है। कृपया उपलब्ध ऑपरेटर की प्रतीक्षा करें — आपको निश्चित रूप से उत्तर मिलेगा।"
        }
        "ro" => {
            "Am primit solicitarea dvs. Vă rugăm să așteptați un operator disponibil — veți primi cu siguranță un răspuns."
        }
        _ => {
            "We've received your request. Please wait for an available operator — you will definitely receive a reply."
        }
    }
}

async fn deliver_rating_prompt(state: &AppState, invitation_id: Uuid) -> Result<()> {
    if rating_contact_is_blocked(state, invitation_id).await? {
        return Ok(());
    }
    let invitation = sqlx::query_as::<_, (Uuid, String, String, String, Option<i64>)>(
        r#"
        SELECT channel_connection_id, chat_id, language, status, prompt_message_id
        FROM support_rating_telegram_invitations
        WHERE id = $1
        "#,
    )
    .bind(invitation_id)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram rating invitation is unavailable")?;
    if invitation.3 != "pending" || invitation.4.is_some() {
        return Ok(());
    }
    let copy = rating_copy(&invitation.2);
    let keyboard = json!({
        "inline_keyboard": [[
            { "text": "1", "callback_data": format!("{RATING_CALLBACK_PREFIX}1") },
            { "text": "2", "callback_data": format!("{RATING_CALLBACK_PREFIX}2") },
            { "text": "3", "callback_data": format!("{RATING_CALLBACK_PREFIX}3") },
            { "text": "4", "callback_data": format!("{RATING_CALLBACK_PREFIX}4") },
            { "text": "5", "callback_data": format!("{RATING_CALLBACK_PREFIX}5") }
        ]]
    });
    let sent = send_delivery_rating_text(
        state,
        invitation.0,
        &invitation.1,
        copy.prompt,
        Some(keyboard),
    )
    .await?;
    sqlx::query(
        r#"
        UPDATE support_rating_telegram_invitations
        SET prompt_message_id = $2, sent_at = now(), updated_at = now()
        WHERE id = $1 AND status = 'pending' AND prompt_message_id IS NULL
        "#,
    )
    .bind(invitation_id)
    .bind(sent.message_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn deliver_rating_comment_prompt(state: &AppState, invitation_id: Uuid) -> Result<()> {
    if rating_contact_is_blocked(state, invitation_id).await? {
        return Ok(());
    }
    let invitation = sqlx::query_as::<_, (Uuid, String, String, String, Option<i64>)>(
        r#"
        SELECT channel_connection_id, chat_id, language, status, comment_prompt_message_id
        FROM support_rating_telegram_invitations
        WHERE id = $1
        "#,
    )
    .bind(invitation_id)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram rating invitation is unavailable")?;
    if invitation.3 != "rating_recorded" || invitation.4.is_some() {
        return Ok(());
    }
    let copy = rating_copy(&invitation.2);
    let keyboard = json!({
        "inline_keyboard": [[
            { "text": copy.skip, "callback_data": RATING_SKIP_CALLBACK }
        ]]
    });
    let sent = send_delivery_rating_text(
        state,
        invitation.0,
        &invitation.1,
        copy.comment_prompt,
        Some(keyboard),
    )
    .await?;
    sqlx::query(
        r#"
        UPDATE support_rating_telegram_invitations
        SET comment_prompt_message_id = $2, status = 'awaiting_comment', updated_at = now()
        WHERE id = $1 AND status = 'rating_recorded'
          AND comment_prompt_message_id IS NULL
        "#,
    )
    .bind(invitation_id)
    .bind(sent.message_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn deliver_rating_thanks(state: &AppState, invitation_id: Uuid) -> Result<()> {
    if rating_contact_is_blocked(state, invitation_id).await? {
        return Ok(());
    }
    let invitation = sqlx::query_as::<_, (Uuid, String, String, String)>(
        r#"
        SELECT channel_connection_id, chat_id, language, status
        FROM support_rating_telegram_invitations
        WHERE id = $1
        "#,
    )
    .bind(invitation_id)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram rating invitation is unavailable")?;
    if invitation.3 != "completed" {
        return Ok(());
    }
    send_delivery_rating_text(
        state,
        invitation.0,
        &invitation.1,
        rating_copy(&invitation.2).thanks,
        None,
    )
    .await?;
    Ok(())
}

async fn rating_contact_is_blocked(state: &AppState, invitation_id: Uuid) -> Result<bool> {
    sqlx::query_scalar::<_, bool>(
        r#"
        SELECT contact.is_blocked
        FROM support_rating_telegram_invitations AS invitation
        JOIN contacts AS contact
          ON contact.tenant_id = invitation.tenant_id
         AND contact.id = invitation.contact_id
        WHERE invitation.id = $1
        "#,
    )
    .bind(invitation_id)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram rating contact is unavailable")
}

async fn send_delivery_rating_text(
    state: &AppState,
    channel_id: Uuid,
    chat_id: &str,
    text: &str,
    reply_markup: Option<Value>,
) -> Result<TelegramSentMessage> {
    let channel = load_delivery_channel(state, channel_id, true).await?;
    let secrets = decrypt_delivery_secrets(state, &channel)?;
    send_telegram_text(state, &secrets.bot_token, chat_id, text, reply_markup).await
}

async fn send_rating_text(
    state: &AppState,
    channel: &WebhookChannel,
    chat_id: i64,
    text: &str,
    reply_markup: Option<Value>,
) -> Result<TelegramSentMessage, AppError> {
    let secrets = decrypt_channel_secrets(state, channel)?;
    send_telegram_text(
        state,
        &secrets.bot_token,
        &chat_id.to_string(),
        text,
        reply_markup,
    )
    .await
    .map_err(AppError::internal)
}

async fn send_telegram_text(
    state: &AppState,
    token: &str,
    chat_id: &str,
    text: &str,
    reply_markup: Option<Value>,
) -> Result<TelegramSentMessage> {
    let mut payload = json!({ "chat_id": chat_id, "text": text });
    if let Some(reply_markup) = reply_markup {
        payload["reply_markup"] = reply_markup;
    }
    telegram_api(state, token, "sendMessage", payload).await
}

#[derive(Debug, Deserialize)]
struct TelegramSentMessage {
    message_id: i64,
}

async fn unregister_webhook(state: &AppState, channel_id: Uuid) -> Result<()> {
    let channel = load_delivery_channel(state, channel_id, false).await?;
    let secrets = decrypt_delivery_secrets(state, &channel)?;
    let _: bool = telegram_api(
        state,
        &secrets.bot_token,
        "deleteWebhook",
        json!({ "drop_pending_updates": false }),
    )
    .await?;
    purge_channel_credentials(state, channel_id).await
}

async fn purge_channel_credentials(state: &AppState, channel_id: Uuid) -> Result<()> {
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        UPDATE channel_connections
        SET config = config - 'bot_user_id' - 'bot_username', updated_at = now()
        WHERE id = $1 AND kind = 'telegram_bot' AND deleted_at IS NOT NULL
        "#,
    )
    .bind(channel_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM telegram_bot_receivers WHERE channel_connection_id = $1")
        .bind(channel_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        r#"
        DELETE FROM channel_secrets AS secret
        USING channel_connections AS connection
        WHERE connection.id = $1 AND connection.kind = 'telegram_bot'
          AND connection.deleted_at IS NOT NULL
          AND secret.tenant_id = connection.tenant_id
          AND secret.channel_connection_id = connection.id
        "#,
    )
    .bind(channel_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn telegram_api<T: for<'de> Deserialize<'de>>(
    state: &AppState,
    token: &str,
    method: &str,
    payload: Value,
) -> Result<T> {
    let endpoint = format!("https://api.telegram.org/bot{token}/{method}");
    let response = state
        .provider_http
        .post(endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Telegram API request failed"))?;
    if !response.status().is_success() {
        anyhow::bail!("Telegram API rejected the request");
    }
    let response = response
        .json::<TelegramApiEnvelope<T>>()
        .await
        .map_err(|_| anyhow::anyhow!("Telegram API returned an invalid response"))?;
    if !response.ok {
        anyhow::bail!("Telegram API rejected the request");
    }
    response
        .result
        .context("Telegram API response has no result")
}

pub(crate) async fn mark_terminal_failure(
    state: &AppState,
    event_type: &str,
    aggregate_id: Uuid,
) -> Result<()> {
    if event_type == SETUP_EVENT_TYPE {
        sqlx::query(
            "UPDATE channel_connections SET status = 'degraded', updated_at = now() WHERE id = $1 AND kind = 'telegram_bot' AND deleted_at IS NULL",
        )
        .bind(aggregate_id)
        .execute(&state.db)
        .await?;
        return Ok(());
    }
    if event_type == UNREGISTER_EVENT_TYPE {
        purge_channel_credentials(state, aggregate_id).await?;
        return Ok(());
    }
    if event_type != DELIVERY_EVENT_TYPE {
        return Ok(());
    }
    mark_message_failed(state, aggregate_id, "Telegram delivery failed").await
}

async fn mark_message_failed(state: &AppState, aggregate_id: Uuid, last_error: &str) -> Result<()> {
    let context = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid)>(
        r#"
        SELECT message.tenant_id, message.project_id, message.inbox_id, conversation.contact_id
        FROM messages AS message
        JOIN conversations AS conversation
          ON conversation.tenant_id = message.tenant_id
         AND conversation.id = message.conversation_id
        WHERE message.id = $1
        "#,
    )
    .bind(aggregate_id)
    .fetch_optional(&state.db)
    .await?
    .context("Telegram failed message is unavailable")?;
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        "UPDATE message_deliveries SET status = 'failed', last_error = $3, updated_at = now() WHERE tenant_id = $1 AND message_id = $2",
    )
    .bind(context.0)
    .bind(aggregate_id)
    .bind(last_error)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE messages SET status = 'failed' WHERE tenant_id = $1 AND id = $2 AND status NOT IN ('delivered', 'read')",
    )
    .bind(context.0)
    .bind(aggregate_id)
    .execute(&mut *transaction)
    .await?;
    let event = message_status_event(
        context.0,
        context.1,
        context.2,
        context.3,
        aggregate_id,
        "failed",
    );
    conversations::insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    conversations::publish_best_effort(state, &event).await;
    Ok(())
}

fn message_status_event(
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    contact_id: Uuid,
    message_id: Uuid,
    status: &str,
) -> RealtimeEvent {
    RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id,
        project_id,
        inbox_id,
        contact_id: Some(contact_id),
        event_type: "message.status_updated".to_owned(),
        aggregate_id: message_id,
        sequence: None,
        occurred_at: Utc::now(),
        data: json!({ "message_id": message_id, "status": status }),
    }
}

fn encrypt_channel_secrets(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_id: Uuid,
    secrets: &TelegramBotSecrets,
) -> Result<Vec<u8>, AppError> {
    let plaintext = serde_json::to_vec(secrets).map_err(AppError::internal)?;
    let key = load_secret_encryption_key(state)?;
    let encrypted = encrypt_secret(
        &key,
        &plaintext,
        state.config.secrets.key_version.clone(),
        &channel_secret_aad(tenant_id, project_id, inbox_id, channel_id),
    )?;
    let mut payload = encrypted.nonce.to_vec();
    payload.extend(encrypted.ciphertext);
    Ok(payload)
}

fn decrypt_channel_secrets(
    state: &AppState,
    channel: &WebhookChannel,
) -> Result<TelegramBotSecrets, AppError> {
    decrypt_stored_secrets(
        state,
        channel.tenant_id,
        channel.project_id,
        channel.inbox_id,
        channel.channel_connection_id,
        &channel.encrypted_payload,
        &channel.key_version,
    )
    .map_err(AppError::internal)
}

fn decrypt_delivery_secrets(
    state: &AppState,
    channel: &DeliveryChannel,
) -> Result<TelegramBotSecrets> {
    decrypt_stored_secrets(
        state,
        channel.tenant_id,
        channel.project_id,
        channel.inbox_id,
        channel.channel_connection_id,
        &channel.encrypted_payload,
        &channel.key_version,
    )
}

fn decrypt_stored_secrets(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_id: Uuid,
    encrypted_payload: &[u8],
    key_version: &str,
) -> Result<TelegramBotSecrets> {
    if key_version != state.config.secrets.key_version || encrypted_payload.len() <= 12 {
        anyhow::bail!("Telegram bot credential metadata is invalid");
    }
    let key = load_secret_encryption_key(state).map_err(anyhow::Error::new)?;
    let plaintext = decrypt_secret(
        &key,
        &encrypted_payload[12..],
        &encrypted_payload[..12],
        &channel_secret_aad(tenant_id, project_id, inbox_id, channel_id),
    )?;
    serde_json::from_slice(&plaintext).context("Telegram bot credential is invalid")
}

fn channel_secret_aad(
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_id: Uuid,
) -> Vec<u8> {
    format!("{CHANNEL_SECRET_AAD_PREFIX}:{tenant_id}:{project_id}:{inbox_id}:{channel_id}")
        .into_bytes()
}

fn webhook_url(base_url: &str, public_id: Uuid) -> Result<String, AppError> {
    let mut url = url::Url::parse(base_url.trim()).map_err(AppError::internal)?;
    url.set_path(&format!("/telegram/v1/webhooks/{public_id}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn generate_webhook_secret() -> String {
    let mut bytes = [0_u8; WEBHOOK_SECRET_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn exactly_one_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn telegram_display_name(user: &TelegramUser) -> String {
    let full_name = user.last_name.as_ref().map_or_else(
        || user.first_name.trim().to_owned(),
        |last_name| format!("{} {}", user.first_name.trim(), last_name.trim()),
    );
    if full_name.trim().is_empty() {
        user.username.as_deref().map_or_else(
            || format!("Telegram {}", user.id),
            |username| format!("@{username}"),
        )
    } else {
        full_name.trim().to_owned()
    }
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

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, Utc};
    use sqlx::PgPool;

    use super::{
        ReceiverClaim, TelegramMessage, TelegramUser, TelegramWebhookInfo, WebhookChannel,
        advance_polling_offset, claim_receiver, constant_time_equal, enter_polling_mode,
        inbound_message_body, parse_rating_callback, process_inbound_update, rating_copy,
        request_acknowledgement_text, reset_receiver_to_webhook, telegram_display_name,
        webhook_delivery_is_failing, webhook_receives_required_updates, webhook_url,
    };
    use crate::{AppState, Config};
    use uuid::Uuid;

    #[test]
    fn builds_public_webhook_url() {
        let public_id = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        assert_eq!(
            webhook_url(" https://support.example/ ", public_id).unwrap(),
            "https://support.example/telegram/v1/webhooks/00000000-0000-4000-8000-000000000001"
        );
    }

    #[test]
    fn compares_webhook_secrets_and_formats_contacts() {
        assert!(constant_time_equal(b"same", b"same"));
        assert!(!constant_time_equal(b"same", b"other"));
        let user = TelegramUser {
            id: 42,
            is_bot: false,
            first_name: " Anna ".to_owned(),
            last_name: Some(" Ivanova ".to_owned()),
            username: Some("anna".to_owned()),
            language_code: Some("ru".to_owned()),
        };
        assert_eq!(telegram_display_name(&user), "Anna Ivanova");
    }

    #[test]
    fn blocked_contacts_keep_non_text_messages_in_the_reply_flow() {
        for media in ["photo", "voice", "sticker", "video"] {
            let payload = serde_json::json!({
                "chat": {"id": 42, "type": "private"},
                (media): {}
            });
            let message: TelegramMessage = serde_json::from_value(payload).unwrap();
            assert!(inbound_message_body(message.text, message.caption, true).is_some());
        }
        assert_eq!(
            inbound_message_body(None, Some("caption".to_owned()), false),
            None
        );
        assert_eq!(
            inbound_message_body(None, Some("caption".to_owned()), true),
            Some("caption".to_owned())
        );
        assert_eq!(
            inbound_message_body(Some("hello".to_owned()), None, false),
            Some("hello".to_owned())
        );
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn blocked_telegram_messages_and_rating_buttons_get_one_reply(db: PgPool) {
        let tenant_id = Uuid::from_u128(101);
        let project_id = Uuid::from_u128(102);
        let inbox_id = Uuid::from_u128(103);
        let channel_connection_id = Uuid::from_u128(104);
        let contact_id = Uuid::from_u128(105);
        sqlx::raw_sql(&format!(
            r#"
            INSERT INTO tenants (id, name) VALUES ('{tenant_id}', 'Blacklist test');
            INSERT INTO projects (id, tenant_id, name, slug)
                VALUES ('{project_id}', '{tenant_id}', 'Blacklist test', 'blacklist-test');
            INSERT INTO inboxes (id, tenant_id, project_id, name)
                VALUES ('{inbox_id}', '{tenant_id}', '{project_id}', 'Support');
            INSERT INTO channel_connections
                (id, tenant_id, project_id, inbox_id, public_id, kind, name, status)
                VALUES ('{channel_connection_id}', '{tenant_id}', '{project_id}',
                        '{inbox_id}', '{channel_connection_id}', 'telegram_bot', 'Bot', 'active');
            INSERT INTO contacts (id, tenant_id, project_id, is_blocked)
                VALUES ('{contact_id}', '{tenant_id}', '{project_id}', true);
            INSERT INTO contact_identities
                (id, tenant_id, contact_id, channel_kind, external_id, metadata)
                VALUES ('{contact_id}', '{tenant_id}', '{contact_id}', 'telegram_bot',
                        '{channel_connection_id}:42', '{{"telegram_user_id":"42","chat_id":"42"}}');
            "#,
        ))
        .execute(&db)
        .await
        .unwrap();
        let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
        config.pg.url = std::env::var("DATABASE_URL").unwrap();
        config.pg.migrate_on_start = false;
        config.redis.url = None;
        config.attachments.enabled = false;
        let mut state = AppState::build(config).await.unwrap();
        state.db = db;
        let channel = WebhookChannel {
            tenant_id,
            project_id,
            inbox_id,
            channel_connection_id,
            encrypted_payload: Vec::new(),
            key_version: "v1".to_owned(),
        };
        let sender = serde_json::json!({"id":42, "first_name":"Visitor", "language_code":"ru"});
        let chat = serde_json::json!({"id":42, "type":"private"});
        let updates = [
            serde_json::json!({"update_id":1,"message":{"from":sender,"chat":chat,"text":"hello"}}),
            serde_json::json!({"update_id":2,"message":{"from":sender,"chat":chat,"photo":[]}}),
            serde_json::json!({"update_id":3,"message":{"from":sender,"chat":chat,"voice":{}}}),
            serde_json::json!({"update_id":4,"message":{"from":sender,"chat":chat,"sticker":{}}}),
            serde_json::json!({"update_id":5,"message":{"from":sender,"chat":chat,"text":"rating comment","reply_to_message":{"message_id":7}}}),
            serde_json::json!({"update_id":6,"callback_query":{"id":"callback","from":sender,"message":{"message_id":7,"chat":chat},"data":"tz_rating:5"}}),
        ];
        for update in updates {
            process_inbound_update(&state, &channel, update.clone())
                .await
                .unwrap();
            process_inbound_update(&state, &channel, update)
                .await
                .unwrap();
        }
        let counts = sqlx::query_as::<_, (i64, i64, i64)>(
            r#"
            SELECT count(*) FILTER (WHERE direction = 'inbound'),
                   count(*) FILTER (WHERE direction = 'outbound' AND author_kind = 'system'),
                   count(*) FILTER (WHERE direction = 'outbound' AND author_kind <> 'system')
            FROM messages WHERE tenant_id = $1
            "#,
        )
        .bind(tenant_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(counts, (6, 6, 0));
        let acknowledgement_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM outbox_events WHERE tenant_id = $1 AND event_type = $2",
        )
        .bind(tenant_id)
        .bind(super::REQUEST_ACKNOWLEDGEMENT_EVENT_TYPE)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(acknowledgement_count, 0);

        // An already persisted AI reply must not reach Telegram after blocking.
        let ai_message_id = Uuid::from_u128(106);
        sqlx::query(
            r#"
            INSERT INTO messages (
                id, tenant_id, project_id, inbox_id, conversation_id, sequence,
                direction, kind, author_kind, client_message_id, body, status
            )
            SELECT $1, tenant_id, project_id, inbox_id, id, last_message_sequence + 1,
                   'outbound', 'text', 'ai', $1, 'Stale AI reply', 'queued'
            FROM conversations WHERE tenant_id = $2 AND contact_id = $3
            "#,
        )
        .bind(ai_message_id)
        .bind(tenant_id)
        .bind(contact_id)
        .execute(&state.db)
        .await
        .unwrap();
        let mut transaction = state.db.begin().await.unwrap();
        super::enqueue_outbound_if_needed(
            &mut transaction,
            tenant_id,
            channel_connection_id,
            ai_message_id,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        super::deliver_message(&state, ai_message_id).await.unwrap();
        let status = sqlx::query_scalar::<_, String>("SELECT status FROM messages WHERE id = $1")
            .bind(ai_message_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(status, "failed");
    }

    #[test]
    fn enters_polling_only_for_a_current_backed_up_webhook_failure() {
        let now = Utc::now();
        let mut info = TelegramWebhookInfo {
            url: "https://support.example/telegram/v1/webhooks/channel".to_owned(),
            pending_update_count: 1,
            last_error_date: Some((now - TimeDelta::seconds(30)).timestamp()),
            allowed_updates: Some(vec!["message".to_owned(), "callback_query".to_owned()]),
        };
        assert!(webhook_delivery_is_failing(&info, now));

        info.pending_update_count = 0;
        assert!(!webhook_delivery_is_failing(&info, now));
        info.pending_update_count = 1;
        info.last_error_date = Some((now - TimeDelta::minutes(10)).timestamp());
        assert!(!webhook_delivery_is_failing(&info, now));
    }

    #[test]
    fn requires_rating_callbacks_in_webhook_updates() {
        let mut info = TelegramWebhookInfo {
            url: "https://support.example/telegram/v1/webhooks/channel".to_owned(),
            pending_update_count: 0,
            last_error_date: None,
            allowed_updates: Some(vec!["message".to_owned()]),
        };
        assert!(!webhook_receives_required_updates(&info));

        info.allowed_updates = Some(vec!["message".to_owned(), "callback_query".to_owned()]);
        assert!(webhook_receives_required_updates(&info));

        info.allowed_updates = Some(Vec::new());
        assert!(webhook_receives_required_updates(&info));

        info.allowed_updates = None;
        assert!(webhook_receives_required_updates(&info));
    }

    #[test]
    fn polling_offset_never_moves_backwards() {
        assert_eq!(advance_polling_offset(None, 41).unwrap(), 42);
        assert_eq!(advance_polling_offset(Some(50), 41).unwrap(), 50);
        assert!(advance_polling_offset(None, i64::MAX).is_err());
    }

    #[test]
    fn accepts_only_structured_rating_callbacks() {
        for rating in 1..=5 {
            assert_eq!(
                parse_rating_callback(&format!("tz_rating:{rating}")),
                Some(rating)
            );
        }
        assert_eq!(parse_rating_callback("tz_rating:0"), None);
        assert_eq!(parse_rating_callback("tz_rating:6"), None);
        assert_eq!(parse_rating_callback("tz_rating:skip"), None);
        assert_eq!(parse_rating_callback("tzomet_rating:5"), None);
        assert_eq!(parse_rating_callback("5"), None);
    }

    #[test]
    fn localizes_rating_conversation_with_a_safe_english_fallback() {
        assert!(rating_copy("ru-RU").prompt.contains("Оцените"));
        assert!(rating_copy("en").prompt.contains("Please rate"));
        assert!(rating_copy("de").prompt.contains("Please rate"));
    }

    #[test]
    fn localizes_request_acknowledgement_with_a_safe_english_fallback() {
        assert!(request_acknowledgement_text(Some("ru-RU")).contains("Ваш запрос получен"));
        assert!(request_acknowledgement_text(Some("zh-CN")).contains("您的请求已收到"));
        assert!(request_acknowledgement_text(Some("hi-IN")).contains("आपका अनुरोध"));
        assert!(request_acknowledgement_text(Some("ro-RO")).contains("Am primit solicitarea"));
        assert!(request_acknowledgement_text(Some("en")).contains("We've received"));
        assert!(request_acknowledgement_text(Some("de")).contains("We've received"));
        assert!(request_acknowledgement_text(None).contains("We've received"));
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn receiver_claim_and_mode_transitions_are_durable(db: PgPool) {
        sqlx::raw_sql(
            r#"
            INSERT INTO tenants (id, name)
            VALUES ('00000000-0000-4000-8000-000000000001', 'Telegram test');
            INSERT INTO projects (id, tenant_id, name, slug)
            VALUES (
                '00000000-0000-4000-8000-000000000002',
                '00000000-0000-4000-8000-000000000001',
                'Telegram test',
                'telegram-test'
            );
            INSERT INTO inboxes (id, tenant_id, project_id, name)
            VALUES (
                '00000000-0000-4000-8000-000000000003',
                '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002',
                'Telegram test'
            );
            INSERT INTO channel_connections (
                id, tenant_id, project_id, inbox_id, public_id, kind, name, status, config
            ) VALUES (
                '00000000-0000-4000-8000-000000000004',
                '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002',
                '00000000-0000-4000-8000-000000000003',
                '00000000-0000-4000-8000-000000000005',
                'telegram_bot',
                'Telegram test',
                'active',
                '{"webhook_url":"https://tzomet.io/telegram/v1/webhooks/test"}'
            );
            INSERT INTO channel_secrets (
                id, tenant_id, channel_connection_id, encrypted_payload, key_version
            ) VALUES (
                '00000000-0000-4000-8000-000000000006',
                '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000004',
                decode('00', 'hex'),
                'v1'
            );
            INSERT INTO telegram_bot_receivers (tenant_id, channel_connection_id)
            VALUES (
                '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000004'
            );
            "#,
        )
        .execute(&db)
        .await
        .unwrap();

        let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
        config.pg.url = std::env::var("DATABASE_URL").unwrap();
        config.pg.migrate_on_start = false;
        config.redis.url = None;
        config.attachments.enabled = false;
        let mut state = AppState::build(config).await.unwrap();
        state.db = db;

        let worker_id = Uuid::from_u128(7);
        let claim = claim_receiver(&state, worker_id).await.unwrap().unwrap();
        assert_eq!(claim.receive_mode, "webhook");
        enter_polling_mode(&state, worker_id, &claim).await.unwrap();
        let mode = sqlx::query_as::<_, (String, Option<i64>, bool)>(
            r#"
            SELECT receive_mode, polling_offset, next_webhook_retry_at IS NOT NULL
            FROM telegram_bot_receivers
            WHERE tenant_id = $1 AND channel_connection_id = $2
            "#,
        )
        .bind(claim.tenant_id)
        .bind(claim.channel_connection_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(mode, ("polling".to_owned(), None, true));

        let polling_claim = ReceiverClaim {
            receive_mode: "polling".to_owned(),
            ..claim
        };
        reset_receiver_to_webhook(&state, worker_id, &polling_claim)
            .await
            .unwrap();
        let mode = sqlx::query_scalar::<_, String>(
            r#"
            SELECT receive_mode
            FROM telegram_bot_receivers
            WHERE tenant_id = $1 AND channel_connection_id = $2
            "#,
        )
        .bind(polling_claim.tenant_id)
        .bind(polling_claim.channel_connection_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(mode, "webhook");
    }
}
