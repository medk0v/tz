//! Credential-bound Android registrations and independent, durable FCM deliveries.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, Postgres, Transaction};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{ActorContext, authenticate_realtime_actor_for_project, hash_token},
    config::MobilePushConfig,
    error::AppError,
    realtime::RealtimeEvent,
};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const MESSAGING_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const MAX_EVENT_AGE_SECONDS: i64 = 3600;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/mobile-push/config", get(configuration))
        .route(
            "/api/v1/mobile-push/devices/{installation_id}",
            put(register).delete(unregister),
        )
}

async fn configuration(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<Value>, AppError> {
    require_scoped_access_token(&actor)?;
    Ok(Json(json!({"enabled": state.mobile_push.is_some()})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    project_id: Uuid,
    token: String,
}

async fn register(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(installation_id): Path<Uuid>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<Value>, AppError> {
    let (session_id, api_key_id) = credential_columns(&actor);
    actor.require("conversations:read")?;
    actor.require_project(request.project_id)?;
    // A tenant-level session has no selected Inbox access yet.
    if actor.project_id != Some(request.project_id) {
        return Err(AppError::Forbidden);
    }
    if state.mobile_push.is_none() {
        return Err(AppError::ServiceUnavailable(
            "Mobile push is not configured".to_owned(),
        ));
    }
    if installation_id.is_nil() || !valid_token(&request.token) {
        return Err(AppError::BadRequest(
            "A valid installation UUID and FCM token are required".to_owned(),
        ));
    }
    let mut transaction = state.db.begin().await?;
    // Serialize registrations for the same account before enforcing its bound.
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(actor.actor_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM mobile_push_devices AS device USING operator_sessions AS session WHERE device.user_id = $1 AND session.id = device.session_id AND (session.revoked_at IS NOT NULL OR session.idle_expires_at <= now() OR session.absolute_expires_at <= now())")
        .bind(actor.actor_id).execute(&mut *transaction).await?;
    sqlx::query("DELETE FROM mobile_push_devices AS device USING api_keys AS api_key WHERE device.user_id = $1 AND api_key.id = device.api_key_id AND (api_key.revoked_at IS NOT NULL OR api_key.expires_at <= now())")
        .bind(actor.actor_id).execute(&mut *transaction).await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mobile_push_devices WHERE user_id = $1 AND installation_id <> $2",
    )
    .bind(actor.actor_id)
    .bind(installation_id)
    .fetch_one(&mut *transaction)
    .await?;
    if count >= 32 {
        return Err(AppError::BadRequest(
            "At most 32 mobile devices may be registered per account".to_owned(),
        ));
    }
    // The token identifies one installation. Reinstall/login transfers it rather
    // than allowing old accounts or sessions to retain a duplicate registration.
    let token_hash = hash_token(&request.token);
    sqlx::query("DELETE FROM mobile_push_devices WHERE token_hash = $1 AND installation_id <> $2")
        .bind(&token_hash)
        .bind(installation_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(r#"
        INSERT INTO mobile_push_devices (id, installation_id, tenant_id, project_id, user_id, session_id, api_key_id, token, token_hash)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (installation_id) DO UPDATE
        SET id = CASE WHEN mobile_push_devices.session_id IS NOT DISTINCT FROM EXCLUDED.session_id
                          AND mobile_push_devices.api_key_id IS NOT DISTINCT FROM EXCLUDED.api_key_id
                          AND mobile_push_devices.project_id = EXCLUDED.project_id
                          AND mobile_push_devices.token = EXCLUDED.token
                      THEN mobile_push_devices.id ELSE EXCLUDED.id END,
            registered_at = CASE WHEN mobile_push_devices.session_id IS NOT DISTINCT FROM EXCLUDED.session_id
                          AND mobile_push_devices.api_key_id IS NOT DISTINCT FROM EXCLUDED.api_key_id
                          AND mobile_push_devices.project_id = EXCLUDED.project_id
                          AND mobile_push_devices.token = EXCLUDED.token
                      THEN mobile_push_devices.registered_at ELSE now() END,
            tenant_id = EXCLUDED.tenant_id, project_id = EXCLUDED.project_id,
            user_id = EXCLUDED.user_id, session_id = EXCLUDED.session_id,
            api_key_id = EXCLUDED.api_key_id,
            token = EXCLUDED.token, token_hash = EXCLUDED.token_hash, updated_at = now()
    "#).bind(Uuid::now_v7()).bind(installation_id).bind(actor.tenant_id)
        .bind(request.project_id).bind(actor.actor_id).bind(session_id).bind(api_key_id).bind(request.token).bind(token_hash)
        .execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(Json(
        json!({"installation_id": installation_id, "project_id": request.project_id, "enabled": true}),
    ))
}

async fn unregister(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(installation_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_scoped_access_token(&actor)?;
    let (session_id, api_key_id) = credential_columns(&actor);
    sqlx::query("DELETE FROM mobile_push_devices WHERE installation_id = $1 AND tenant_id = $2 AND user_id = $3 AND session_id IS NOT DISTINCT FROM $4 AND api_key_id IS NOT DISTINCT FROM $5")
        .bind(installation_id).bind(actor.tenant_id).bind(actor.actor_id).bind(session_id).bind(api_key_id)
        .execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn require_scoped_access_token(actor: &ActorContext) -> Result<(), AppError> {
    if !actor.is_password_session() {
        actor.require("conversations:read")?;
        if actor.project_id.is_none() {
            return Err(AppError::Forbidden);
        }
    }
    Ok(())
}

fn credential_columns(actor: &ActorContext) -> (Option<Uuid>, Option<Uuid>) {
    let (_, credential_id) = actor.realtime_credential();
    if actor.is_password_session() {
        (Some(credential_id), None)
    } else {
        (None, Some(credential_id))
    }
}

fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.len() <= 4096 && token.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PushPayload {
    event_id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    conversation_id: Uuid,
    sequence: Option<i64>,
    occurred_at: DateTime<Utc>,
    kind: PushKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum PushKind {
    NewMessage,
}

/// Enqueues before the conversation transaction commits; provider delivery never
/// retries the original realtime event or any channel's outbound message.
pub(crate) async fn enqueue_realtime(
    transaction: &mut Transaction<'_, Postgres>,
    event: &RealtimeEvent,
) -> Result<(), AppError> {
    if event.event_type != "message.created"
        || event.data.get("direction").and_then(Value::as_str) != Some("inbound")
    {
        return Ok(());
    }
    let Some(conversation_id) = event
        .data
        .get("conversation_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
    else {
        return Ok(());
    };
    enqueue(
        transaction,
        &PushPayload {
            event_id: event.event_id,
            tenant_id: event.tenant_id,
            project_id: event.project_id,
            inbox_id: event.inbox_id,
            conversation_id,
            sequence: event.sequence,
            occurred_at: event.occurred_at,
            kind: PushKind::NewMessage,
        },
    )
    .await
}

async fn enqueue(
    transaction: &mut Transaction<'_, Postgres>,
    payload: &PushPayload,
) -> Result<(), AppError> {
    // Delivery revalidates access to the registration's project. The session's
    // legacy default project can change independently in another window.
    sqlx::query(
        r#"
        INSERT INTO outbox_events (id, tenant_id, aggregate_type, aggregate_id, event_type, payload)
        SELECT gen_random_uuid(), device.tenant_id, 'mobile_push', device.id,
               'mobile_push.requested', $4
        FROM mobile_push_devices AS device
        LEFT JOIN operator_sessions AS session ON session.id = device.session_id
        LEFT JOIN api_keys AS api_key ON api_key.id = device.api_key_id
        WHERE device.tenant_id = $1 AND device.project_id = $2
          AND (
              (session.revoked_at IS NULL AND session.idle_expires_at > now()
               AND session.absolute_expires_at > now())
              OR (api_key.revoked_at IS NULL AND api_key.expires_at > now()
                  AND (api_key.project_id = device.project_id
                       OR (api_key.project_id IS NULL AND api_key.role_project_id IS NOT NULL)))
          )
          AND device.registered_at <= $3
        ON CONFLICT (aggregate_id, (payload ->> 'event_id'))
            WHERE aggregate_type = 'mobile_push' DO NOTHING
    "#,
    )
    .bind(payload.tenant_id)
    .bind(payload.project_id)
    .bind(payload.occurred_at)
    .bind(serde_json::to_value(payload).map_err(AppError::internal)?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[derive(FromRow)]
struct Device {
    installation_id: Uuid,
    user_id: Uuid,
    session_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
    token: String,
}

pub(crate) async fn deliver(state: &AppState, device_id: Uuid, payload: PushPayload) -> Result<()> {
    let Some(client) = &state.mobile_push else {
        return Ok(());
    };
    if Utc::now()
        .signed_duration_since(payload.occurred_at)
        .num_seconds()
        > MAX_EVENT_AGE_SECONDS
    {
        return Ok(());
    }
    let device = sqlx::query_as::<_, Device>("SELECT installation_id, user_id, session_id, api_key_id, token FROM mobile_push_devices WHERE id = $1 AND tenant_id = $2 AND project_id = $3")
        .bind(device_id).bind(payload.tenant_id).bind(payload.project_id).fetch_optional(&state.db).await?;
    let Some(device) = device else {
        return Ok(());
    };
    let (credential_kind, credential_id) = match (device.session_id, device.api_key_id) {
        (Some(id), None) => ("operator_session", id),
        (None, Some(id)) => ("access_token", id),
        _ => anyhow::bail!("mobile push registration has an invalid credential binding"),
    };
    let actor = match authenticate_realtime_actor_for_project(
        state,
        credential_kind,
        credential_id,
        Some(payload.project_id),
    )
    .await
    {
        Ok(actor) => actor,
        Err(AppError::Unauthorized | AppError::Forbidden | AppError::NotFound) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if actor.tenant_id != payload.tenant_id
        || actor.actor_id != device.user_id
        || actor.project_id != Some(payload.project_id)
        || actor.require("conversations:read").is_err()
        || actor
            .require_inbox(payload.project_id, payload.inbox_id)
            .is_err()
    {
        return Ok(());
    }
    // Recheck current conversation scope, channel status, and the per-operator
    // read cursor after any wait/retry. Resolved/read messages no longer alert.
    let relevant: bool = sqlx::query_scalar(r#"
        SELECT EXISTS (
            SELECT 1 FROM conversations AS conversation
            JOIN inboxes AS inbox ON inbox.tenant_id = conversation.tenant_id AND inbox.id = conversation.inbox_id
            JOIN channel_connections AS channel ON channel.tenant_id = conversation.tenant_id AND channel.id = conversation.channel_connection_id
            LEFT JOIN conversation_read_cursors AS cursor ON cursor.tenant_id = conversation.tenant_id
              AND cursor.conversation_id = conversation.id AND cursor.actor_id = $5
            WHERE conversation.tenant_id = $1 AND conversation.project_id = $2
              AND conversation.inbox_id = $3 AND conversation.id = $4
              AND conversation.status <> 'resolved' AND inbox.status = 'active'
              AND channel.status = 'active' AND channel.deleted_at IS NULL
              AND ($6::bigint IS NULL OR $6 > GREATEST(COALESCE(cursor.last_read_sequence, 0), conversation.operator_unread_baseline_sequence))
        )
    "#).bind(payload.tenant_id).bind(payload.project_id).bind(payload.inbox_id)
        .bind(payload.conversation_id).bind(device.user_id).bind(payload.sequence)
        .fetch_one(&state.db).await?;
    if !relevant {
        return Ok(());
    }
    if client
        .send(&device.token, notification_data(&payload, &device))
        .await?
        == SendOutcome::InvalidToken
    {
        // Registration generations prevent a late failure from removing a fresh token.
        sqlx::query("DELETE FROM mobile_push_devices WHERE id = $1 AND token = $2")
            .bind(device_id)
            .bind(device.token)
            .execute(&state.db)
            .await?;
    }
    Ok(())
}

fn notification_data(payload: &PushPayload, device: &Device) -> Value {
    let (kind, body) = match payload.kind {
        PushKind::NewMessage => ("message.created", "New message in a conversation"),
    };
    json!({
        "event_id": payload.event_id.to_string(), "project_id": payload.project_id.to_string(),
        "inbox_id": payload.inbox_id.to_string(), "conversation_id": payload.conversation_id.to_string(),
        "actor_id": device.user_id.to_string(), "installation_id": device.installation_id.to_string(),
        "type": kind, "title": "Support", "body": body,
    })
}

#[derive(Deserialize)]
struct ServiceAccount {
    #[serde(rename = "type")]
    account_type: String,
    project_id: String,
    client_email: String,
    private_key: String,
    private_key_id: Option<String>,
}

pub(crate) struct FcmClient {
    http: reqwest::Client,
    endpoint: String,
    client_email: String,
    key: EncodingKey,
    key_id: Option<String>,
    access_token: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    token_type: String,
}

impl FcmClient {
    pub(crate) async fn from_config(config: &MobilePushConfig) -> Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }
        let path = config
            .service_account_path
            .as_ref()
            .context("mobile push service account path is missing")?;
        let bytes = tokio::fs::read(path)
            .await
            .context("failed to read mobile push service account")?;
        let account: ServiceAccount =
            serde_json::from_slice(&bytes).context("invalid mobile push service account JSON")?;
        if account.account_type != "service_account"
            || account.client_email.is_empty()
            || account.project_id.is_empty()
            || !account
                .project_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            anyhow::bail!("invalid mobile push service account identity");
        }
        Ok(Some(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint: format!(
                "https://fcm.googleapis.com/v1/projects/{}/messages:send",
                account.project_id
            ),
            client_email: account.client_email,
            key: EncodingKey::from_rsa_pem(account.private_key.as_bytes())
                .context("invalid mobile push signing key")?,
            key_id: account.private_key_id,
            access_token: Mutex::new(None),
        }))
    }

    async fn token(&self) -> Result<String> {
        let mut cache = self.access_token.lock().await;
        if let Some(token) = &*cache
            && token.expires_at > Instant::now()
        {
            return Ok(token.value.clone());
        }
        let now = Utc::now().timestamp();
        let mut header = Header::new(Algorithm::RS256);
        header.kid.clone_from(&self.key_id);
        let assertion = jsonwebtoken::encode(
            &header,
            &Claims {
                iss: &self.client_email,
                scope: MESSAGING_SCOPE,
                aud: TOKEN_URL,
                iat: now,
                exp: now + 3600,
            },
            &self.key,
        )
        .context("failed to sign mobile push OAuth assertion")?;
        let response = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .context("mobile push OAuth request failed")?;
        if !response.status().is_success() {
            anyhow::bail!(
                "mobile push OAuth returned HTTP {}",
                response.status().as_u16()
            );
        }
        let response: TokenResponse = response
            .json()
            .await
            .context("invalid mobile push OAuth response")?;
        if response.token_type != "Bearer"
            || response.access_token.is_empty()
            || response.expires_in <= 60
        {
            anyhow::bail!("mobile push OAuth response has no usable bearer token");
        }
        let value = response.access_token;
        *cache = Some(CachedToken {
            value: value.clone(),
            expires_at: Instant::now() + Duration::from_secs(response.expires_in.min(3600) - 60),
        });
        Ok(value)
    }

    async fn send(&self, token: &str, data: Value) -> Result<SendOutcome> {
        let payload = json!({"message": {"token": token, "data": data,
            "android": {"priority": "high", "ttl": "3600s"}}});
        for attempt in 0..2 {
            let access_token = self.token().await?;
            let response = self
                .http
                .post(&self.endpoint)
                .bearer_auth(access_token)
                .json(&payload)
                .send()
                .await
                .context("mobile push delivery request failed")?;
            let status = response.status();
            if status.is_success() {
                return Ok(SendOutcome::Sent);
            }
            if status == StatusCode::UNAUTHORIZED && attempt == 0 {
                *self.access_token.lock().await = None;
                continue;
            }
            let retry_after = retry_after_seconds(
                response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok()),
                Utc::now(),
            );
            let body: Value = response.json().await.unwrap_or(Value::Null);
            if invalid_registration(status, &body) {
                return Ok(SendOutcome::InvalidToken);
            }
            return Err(PushDeliveryError {
                status: status.as_u16(),
                retry_after,
                permanent: status.is_client_error() && status != StatusCode::TOO_MANY_REQUESTS,
            }
            .into());
        }
        unreachable!("the final FCM request always returns a delivery outcome")
    }
}

#[derive(Debug, Eq, PartialEq)]
enum SendOutcome {
    Sent,
    InvalidToken,
}

#[derive(Debug, thiserror::Error)]
#[error("mobile push delivery returned HTTP {status}")]
pub(crate) struct PushDeliveryError {
    status: u16,
    pub(crate) retry_after: Duration,
    pub(crate) permanent: bool,
}

fn invalid_registration(status: StatusCode, body: &Value) -> bool {
    let detail = body.pointer("/error/details").and_then(Value::as_array);
    detail.is_some_and(|details| {
        details.iter().any(|entry| {
            entry.get("@type").and_then(Value::as_str)
                == Some("type.googleapis.com/google.firebase.fcm.v1.FcmError")
                && match entry.get("errorCode").and_then(Value::as_str) {
                    Some("UNREGISTERED") => status == StatusCode::NOT_FOUND,
                    Some("INVALID_ARGUMENT") => status == StatusCode::BAD_REQUEST,
                    _ => false,
                }
        })
    })
}

fn retry_after_seconds(value: Option<&str>, now: DateTime<Utc>) -> Duration {
    let seconds = value
        .and_then(|value| {
            value.parse::<u64>().ok().or_else(|| {
                DateTime::parse_from_rfc2822(value).ok().and_then(|date| {
                    u64::try_from(date.signed_duration_since(now).num_seconds()).ok()
                })
            })
        })
        .unwrap_or(60);
    Duration::from_secs(seconds.max(60))
}

#[cfg(test)]
#[path = "mobile_push_db_tests.rs"]
mod db_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_push_requires_no_credential_and_enabled_push_requires_one() {
        assert!(
            FcmClient::from_config(&MobilePushConfig::default())
                .await
                .unwrap()
                .is_none()
        );
        let enabled = MobilePushConfig {
            enabled: true,
            service_account_path: None,
        };
        assert!(FcmClient::from_config(&enabled).await.is_err());
    }

    #[test]
    fn payload_contains_only_scope_and_generic_text() {
        let payload = PushPayload {
            event_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            inbox_id: Uuid::now_v7(),
            conversation_id: Uuid::now_v7(),
            sequence: Some(1),
            occurred_at: Utc::now(),
            kind: PushKind::NewMessage,
        };
        let device = Device {
            installation_id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            session_id: Some(Uuid::now_v7()),
            api_key_id: None,
            token: "private-token".to_owned(),
        };
        let data = notification_data(&payload, &device);
        assert_eq!(data["title"], "Support");
        assert!(data.as_object().unwrap().values().all(Value::is_string));
        assert_eq!(data["actor_id"], device.user_id.to_string());
        assert_eq!(data["conversation_id"], payload.conversation_id.to_string());
        assert_eq!(data.as_object().unwrap().len(), 9);
        assert!(!data.to_string().contains("private-token"));
        assert!(
            !data
                .to_string()
                .contains(&device.session_id.unwrap().to_string())
        );
    }

    #[test]
    fn only_specific_fcm_token_errors_remove_registration() {
        let bad_payload = json!({"error": {"status": "INVALID_ARGUMENT", "details": [{"@type": "type.googleapis.com/google.rpc.BadRequest"}]}});
        assert!(!invalid_registration(StatusCode::BAD_REQUEST, &bad_payload));
        for (code, status) in [
            ("UNREGISTERED", StatusCode::NOT_FOUND),
            ("INVALID_ARGUMENT", StatusCode::BAD_REQUEST),
        ] {
            let body = json!({"error": {"details": [{"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": code}]}});
            assert!(invalid_registration(status, &body));
            assert!(!invalid_registration(StatusCode::FORBIDDEN, &body));
        }
        assert!(!invalid_registration(StatusCode::NOT_FOUND, &Value::Null));
    }

    #[test]
    fn retries_honor_seconds_http_dates_and_fcm_minimum() {
        let now = DateTime::parse_from_rfc3339("2026-09-09T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(retry_after_seconds(None, now).as_secs(), 60);
        assert_eq!(retry_after_seconds(Some("5"), now).as_secs(), 60);
        assert_eq!(retry_after_seconds(Some("120"), now).as_secs(), 120);
        assert_eq!(
            retry_after_seconds(Some("Wed, 09 Sep 2026 00:03:00 GMT"), now).as_secs(),
            180
        );
    }

    #[test]
    fn rejects_whitespace_control_and_oversized_tokens() {
        assert!(valid_token("dGVzdA:APA91-b_abc"));
        for value in [
            "",
            " bad",
            "bad\ntoken",
            "bad\u{7f}token",
            &"a".repeat(4097),
        ] {
            assert!(!valid_token(value));
        }
    }
}
