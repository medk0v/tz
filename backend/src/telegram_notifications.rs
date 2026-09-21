//! Durable Telegram notifications for operator-facing widget events.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    ai_settings::{self, decrypt_secret, load_secret_encryption_key},
};

const BOT_TOKEN_KEY: &str = "TELEGRAM_NOTIFY_BOT_TOKEN";
const CHAT_IDS_KEY: &str = "TELEGRAM_NOTIFY_CHAT_IDS";
const MAX_RECIPIENTS: usize = 32;
const MAX_MESSAGE_CHARS: usize = 4_000;
const ROUTING_BOT_TOKEN_AAD_PREFIX: &str = "tzomet:inbox-telegram-bot-token:v1";
pub(crate) const ROUTING_TELEGRAM_EVENT_TYPE: &str = "telegram.routing_notification.requested";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TelegramNotificationKind {
    NewVisitor,
    NewMessage,
}

impl TelegramNotificationKind {
    fn flag_column(self) -> &'static str {
        match self {
            Self::NewVisitor => "telegram_notify_on_new_visitor",
            Self::NewMessage => "telegram_notify_on_new_message",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct TelegramNotificationPayload {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub inbox_id: Uuid,
    #[serde(default)]
    pub channel_connection_id: Uuid,
    pub kind: TelegramNotificationKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoutingTelegramNotificationKind {
    NewVisitor,
    NewMessage,
    OperatorRequest,
    UnassignedWarning,
    SlaBreach,
}

impl RoutingTelegramNotificationKind {
    fn flag_column(self) -> &'static str {
        match self {
            Self::NewVisitor => "telegram_new_visitor",
            Self::NewMessage => "telegram_new_message",
            Self::OperatorRequest => "telegram_operator_request",
            Self::UnassignedWarning => "telegram_unassigned_warning",
            Self::SlaBreach => "telegram_sla_breach",
        }
    }
}

/// A routed notification deliberately contains no bot credential. The worker
/// resolves the current Inbox credential immediately before delivery.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RoutingTelegramNotificationPayload {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub inbox_id: Uuid,
    pub chat_id: String,
    pub kind: RoutingTelegramNotificationKind,
    pub text: String,
}

#[derive(Debug, FromRow)]
struct EncryptedSecretRow {
    profile_id: Uuid,
    secret_key: String,
    encrypted_value: Vec<u8>,
    nonce: Vec<u8>,
    key_version: String,
}

#[derive(Debug, FromRow)]
struct InboxTelegramCredentialRow {
    encrypted_bot_token: Vec<u8>,
    bot_token_nonce: Vec<u8>,
    key_version: String,
}

#[derive(Default)]
struct ProfileSecrets {
    token: Option<String>,
    chat_ids: Option<String>,
}

#[derive(Default)]
pub(crate) struct RoutingOperatorRequestConfig {
    pub routing_configured: bool,
    pub secrets: Option<BTreeMap<String, String>>,
}

/// Enqueues one notification event when at least one active assigned profile opted in.
pub(crate) async fn enqueue(
    transaction: &mut Transaction<'_, Postgres>,
    payload: TelegramNotificationPayload,
) -> Result<()> {
    if payload.channel_connection_id.is_nil() {
        return Ok(());
    }
    let flag_column = payload.kind.flag_column();
    let configured = sqlx::query_scalar::<_, bool>(&format!(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM ai_profiles AS profile
            JOIN ai_profile_channel_connections AS assignment
              ON assignment.tenant_id = profile.tenant_id
             AND assignment.ai_profile_id = profile.id
            JOIN channel_connections AS connection
              ON connection.tenant_id = assignment.tenant_id
             AND connection.id = assignment.channel_connection_id
            JOIN inboxes AS inbox
              ON inbox.tenant_id = connection.tenant_id
             AND inbox.project_id = connection.project_id
             AND inbox.id = connection.inbox_id
            WHERE profile.tenant_id = $1
              AND profile.project_id = $2
              AND assignment.channel_connection_id = $3
              AND connection.project_id = $2
              AND connection.inbox_id = $4
              AND connection.status = 'active'
              AND connection.deleted_at IS NULL
              AND inbox.status = 'active'
              AND profile.status = 'active'
              AND profile.{flag_column}
              AND EXISTS (
                  SELECT 1 FROM ai_profile_secrets AS secret
                  WHERE secret.tenant_id = profile.tenant_id
                    AND secret.ai_profile_id = profile.id
                    AND lower(secret.secret_key) = lower('{BOT_TOKEN_KEY}')
              )
              AND EXISTS (
                  SELECT 1 FROM ai_profile_secrets AS secret
                  WHERE secret.tenant_id = profile.tenant_id
                    AND secret.ai_profile_id = profile.id
                    AND lower(secret.secret_key) = lower('{CHAT_IDS_KEY}')
              )
        )
        "#,
    ))
    .bind(payload.tenant_id)
    .bind(payload.project_id)
    .bind(payload.channel_connection_id)
    .bind(payload.inbox_id)
    .fetch_one(&mut **transaction)
    .await
    .context("failed to inspect Telegram notification settings")?;
    if !configured {
        return Ok(());
    }

    let event_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload,
            status, available_at, created_at
        ) VALUES (
            $1, $2, 'telegram', $1, 'telegram.notification.requested', $3,
            'pending', now(), now()
        )
        "#,
    )
    .bind(event_id)
    .bind(payload.tenant_id)
    .bind(serde_json::to_value(payload)?)
    .execute(&mut **transaction)
    .await
    .context("failed to enqueue Telegram notification")?;
    Ok(())
}

/// Enqueues one routed notification for one validated Inbox recipient.
pub(crate) async fn enqueue_routing(
    transaction: &mut Transaction<'_, Postgres>,
    payload: RoutingTelegramNotificationPayload,
) -> Result<()> {
    if !valid_chat_id(&payload.chat_id) {
        anyhow::bail!("Inbox Telegram recipient is invalid");
    }
    let text = payload.text.trim();
    if text.is_empty() || text.chars().count() > MAX_MESSAGE_CHARS {
        anyhow::bail!("Inbox Telegram notification text is invalid");
    }

    let event_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload,
            status, available_at, created_at
        ) VALUES (
            $1, $2, 'telegram', $1, $3, $4,
            'pending', now(), now()
        )
        "#,
    )
    .bind(event_id)
    .bind(payload.tenant_id)
    .bind(ROUTING_TELEGRAM_EVENT_TYPE)
    .bind(serde_json::to_value(payload)?)
    .execute(&mut **transaction)
    .await
    .context("failed to enqueue an Inbox Telegram notification")?;
    Ok(())
}

/// Delivers a queued notification to the deduplicated recipients of opted-in profiles.
pub(crate) async fn deliver(state: &AppState, payload: TelegramNotificationPayload) -> Result<()> {
    if payload.channel_connection_id.is_nil() {
        return Ok(());
    }
    let flag_column = payload.kind.flag_column();
    let rows = sqlx::query_as::<_, EncryptedSecretRow>(&format!(
        r#"
        SELECT profile.id AS profile_id, secret.secret_key,
               secret.encrypted_value, secret.nonce, secret.key_version
        FROM ai_profiles AS profile
        JOIN ai_profile_channel_connections AS assignment
          ON assignment.tenant_id = profile.tenant_id
         AND assignment.ai_profile_id = profile.id
        JOIN channel_connections AS connection
          ON connection.tenant_id = assignment.tenant_id
         AND connection.id = assignment.channel_connection_id
        JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
        JOIN ai_profile_secrets AS secret
          ON secret.tenant_id = profile.tenant_id
         AND secret.ai_profile_id = profile.id
         AND lower(secret.secret_key) IN (lower('{BOT_TOKEN_KEY}'), lower('{CHAT_IDS_KEY}'))
        WHERE profile.tenant_id = $1
          AND profile.project_id = $2
          AND assignment.channel_connection_id = $3
          AND connection.project_id = $2
          AND connection.inbox_id = $4
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
          AND inbox.status = 'active'
          AND profile.status = 'active'
          AND profile.{flag_column}
        ORDER BY profile.id, secret.secret_key
        "#,
    ))
    .bind(payload.tenant_id)
    .bind(payload.project_id)
    .bind(payload.channel_connection_id)
    .bind(payload.inbox_id)
    .fetch_all(&state.db)
    .await
    .context("failed to load Telegram notification recipients")?;

    let mut profiles = BTreeMap::<Uuid, ProfileSecrets>::new();
    for row in rows {
        let value = ai_settings::decrypt_profile_secret(
            state,
            payload.tenant_id,
            row.profile_id,
            Some(&row.secret_key),
            Some(&row.encrypted_value),
            Some(&row.nonce),
            Some(&row.key_version),
        )?
        .context("Telegram notification secret is missing")?;
        let secrets = profiles.entry(row.profile_id).or_default();
        if row.secret_key.eq_ignore_ascii_case(BOT_TOKEN_KEY) {
            secrets.token = Some(value);
        } else if row.secret_key.eq_ignore_ascii_case(CHAT_IDS_KEY) {
            secrets.chat_ids = Some(value);
        }
    }

    let mut recipients = HashSet::<(String, String)>::new();
    for secrets in profiles.into_values() {
        let (Some(token), Some(raw_chat_ids)) = (secrets.token, secrets.chat_ids) else {
            continue;
        };
        if !valid_bot_token(&token) {
            continue;
        }
        let Ok(chat_ids) = serde_json::from_str::<Vec<String>>(&raw_chat_ids) else {
            continue;
        };
        if chat_ids.is_empty()
            || chat_ids.len() > MAX_RECIPIENTS
            || chat_ids.iter().any(|chat_id| !valid_chat_id(chat_id))
        {
            continue;
        }
        recipients.extend(chat_ids.into_iter().map(|chat_id| (token.clone(), chat_id)));
    }
    if recipients.is_empty() {
        return Ok(());
    }

    let total = recipients.len();
    let deliveries = join_all(recipients.into_iter().map(|(token, chat_id)| {
        send_message(
            state.provider_http.clone(),
            token,
            chat_id,
            payload.text.clone(),
        )
    }))
    .await;
    let delivered = deliveries
        .into_iter()
        .filter(|delivered| *delivered)
        .count();
    if delivered == 0 {
        anyhow::bail!("Telegram notification delivery failed for all {total} recipients");
    }
    Ok(())
}

/// Delivers one routed notification using the current Inbox credential.
pub(crate) async fn deliver_routing(
    state: &AppState,
    payload: RoutingTelegramNotificationPayload,
) -> Result<()> {
    if !valid_chat_id(&payload.chat_id) {
        anyhow::bail!("Inbox Telegram recipient is invalid");
    }
    let flag_column = payload.kind.flag_column();
    let credential = sqlx::query_as::<_, InboxTelegramCredentialRow>(&format!(
        r#"
        SELECT credential.encrypted_bot_token, credential.bot_token_nonce,
               credential.key_version
        FROM inbox_telegram_credentials AS credential
        JOIN inbox_routing_policies AS policy
          ON policy.tenant_id = credential.tenant_id
         AND policy.project_id = credential.project_id
         AND policy.inbox_id = credential.inbox_id
        WHERE credential.tenant_id = $1
          AND credential.project_id = $2
          AND credential.inbox_id = $3
          AND policy.enabled
          AND policy.{flag_column}
          AND $4 = ANY(policy.telegram_chat_ids)
        "#,
    ))
    .bind(payload.tenant_id)
    .bind(payload.project_id)
    .bind(payload.inbox_id)
    .bind(&payload.chat_id)
    .fetch_optional(&state.db)
    .await
    .context("failed to load the Inbox Telegram credential")?;
    let Some(credential) = credential else {
        return Ok(());
    };
    if credential.key_version != state.config.secrets.key_version {
        anyhow::bail!("Inbox Telegram credential uses an unsupported key version");
    }
    let key = load_secret_encryption_key(state).map_err(anyhow::Error::new)?;
    let token = decrypt_secret(
        &key,
        &credential.encrypted_bot_token,
        &credential.bot_token_nonce,
        &routing_bot_token_aad(payload.tenant_id, payload.project_id, payload.inbox_id),
    )?;
    let token = String::from_utf8(token).context("Inbox Telegram credential is not valid UTF-8")?;
    if !valid_bot_token(&token) {
        anyhow::bail!("Inbox Telegram credential is invalid");
    }
    if !send_message(
        state.provider_http.clone(),
        token,
        payload.chat_id,
        payload.text,
    )
    .await
    {
        anyhow::bail!("Inbox Telegram notification delivery failed");
    }
    Ok(())
}

async fn send_message(
    client: reqwest::Client,
    token: String,
    chat_id: String,
    text: String,
) -> bool {
    let endpoint = format!("https://api.telegram.org/bot{token}/sendMessage");
    let request = client
        .post(endpoint)
        .json(&json!({ "chat_id": chat_id, "text": text }));
    let Ok(Ok(response)) =
        tokio::time::timeout(std::time::Duration::from_secs(15), request.send()).await
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|value| value.get("ok").and_then(serde_json::Value::as_bool))
        == Some(true)
}

pub(crate) fn routing_bot_token_aad(tenant_id: Uuid, project_id: Uuid, inbox_id: Uuid) -> Vec<u8> {
    format!("{ROUTING_BOT_TOKEN_AAD_PREFIX}:{tenant_id}:{project_id}:{inbox_id}").into_bytes()
}

/// Loads Inbox-owned credentials for the existing `OpenClaw` operator-request command.
/// A configured routing policy owns the toggle even when the event itself is disabled.
pub(crate) async fn load_routing_operator_request_config(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
) -> Result<RoutingOperatorRequestConfig> {
    let row = sqlx::query_as::<
        _,
        (
            bool,
            Vec<String>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<String>,
        ),
    >(
        r#"
        SELECT policy.telegram_operator_request, policy.telegram_chat_ids,
               credential.encrypted_bot_token, credential.bot_token_nonce,
               credential.key_version
        FROM inbox_routing_policies AS policy
        LEFT JOIN inbox_telegram_credentials AS credential
          ON credential.tenant_id = policy.tenant_id
         AND credential.project_id = policy.project_id
         AND credential.inbox_id = policy.inbox_id
        WHERE policy.tenant_id = $1 AND policy.project_id = $2 AND policy.inbox_id = $3
          AND policy.enabled
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .fetch_optional(&state.db)
    .await
    .context("failed to load Inbox operator-request Telegram settings")?;
    let Some((enabled, chat_ids, encrypted_token, nonce, key_version)) = row else {
        return Ok(RoutingOperatorRequestConfig::default());
    };
    if !enabled {
        return Ok(RoutingOperatorRequestConfig {
            routing_configured: true,
            secrets: None,
        });
    }
    if chat_ids.is_empty()
        || chat_ids.len() > MAX_RECIPIENTS
        || chat_ids.iter().any(|chat_id| !valid_chat_id(chat_id))
    {
        anyhow::bail!("Inbox operator-request Telegram recipients are invalid");
    }
    let encrypted_token = encrypted_token.context("Inbox Telegram credential is missing")?;
    let nonce = nonce.context("Inbox Telegram credential nonce is missing")?;
    let key_version = key_version.context("Inbox Telegram credential key version is missing")?;
    if key_version != state.config.secrets.key_version {
        anyhow::bail!("Inbox Telegram credential uses an unsupported key version");
    }
    let key = load_secret_encryption_key(state).map_err(anyhow::Error::new)?;
    let token = decrypt_secret(
        &key,
        &encrypted_token,
        &nonce,
        &routing_bot_token_aad(tenant_id, project_id, inbox_id),
    )?;
    let token = String::from_utf8(token).context("Inbox Telegram credential is not valid UTF-8")?;
    if !valid_bot_token(&token) {
        anyhow::bail!("Inbox Telegram credential is invalid");
    }
    let mut secrets = BTreeMap::new();
    secrets.insert(BOT_TOKEN_KEY.to_owned(), token);
    secrets.insert(
        CHAT_IDS_KEY.to_owned(),
        serde_json::to_string(&chat_ids).context("failed to encode Inbox Telegram recipients")?,
    );
    Ok(RoutingOperatorRequestConfig {
        routing_configured: true,
        secrets: Some(secrets),
    })
}

pub(crate) fn valid_bot_token(value: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{
        RoutingTelegramNotificationKind, RoutingTelegramNotificationPayload,
        TelegramNotificationPayload, routing_bot_token_aad, valid_bot_token, valid_chat_id,
    };

    #[test]
    fn validates_telegram_credentials_without_exposing_them() {
        assert!(valid_bot_token("123456:abcdefghijklmnopqrst"));
        assert!(!valid_bot_token("invalid"));
        assert!(valid_chat_id("123456"));
        assert!(valid_chat_id("-1001234567890"));
        assert!(!valid_chat_id("chat-name"));
    }

    #[test]
    fn legacy_notification_payloads_have_no_channel_grant() {
        let payload = serde_json::from_value::<TelegramNotificationPayload>(serde_json::json!({
            "tenant_id": "00000000-0000-4000-8000-000000000001",
            "project_id": "00000000-0000-4000-8000-000000000002",
            "inbox_id": "00000000-0000-4000-8000-000000000003",
            "kind": "new_message",
            "text": "New message"
        }))
        .expect("legacy notification payload should remain readable");

        assert!(payload.channel_connection_id.is_nil());
    }

    #[test]
    fn routed_payload_contains_no_bot_credential() {
        let payload = RoutingTelegramNotificationPayload {
            tenant_id: uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
            project_id: uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap(),
            inbox_id: uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000003").unwrap(),
            chat_id: "-1001234567890".to_owned(),
            kind: RoutingTelegramNotificationKind::SlaBreach,
            text: "SLA breach. Conversation: 42".to_owned(),
        };

        let encoded = serde_json::to_value(&payload).unwrap();
        assert_eq!(encoded["kind"], "sla_breach");
        assert_eq!(encoded["chat_id"], "-1001234567890");
        assert!(encoded.get("token").is_none());
        assert!(encoded.get("bot_token").is_none());
        assert_eq!(
            serde_json::from_value::<RoutingTelegramNotificationPayload>(encoded).unwrap(),
            payload
        );
    }

    #[test]
    fn binds_routed_bot_credentials_to_their_inbox_scope() {
        let tenant_id = uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let project_id = uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap();
        let inbox_id = uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000003").unwrap();

        assert_eq!(
            String::from_utf8(routing_bot_token_aad(tenant_id, project_id, inbox_id)).unwrap(),
            "tzomet:inbox-telegram-bot-token:v1:00000000-0000-4000-8000-000000000001:00000000-0000-4000-8000-000000000002:00000000-0000-4000-8000-000000000003"
        );
    }
}
