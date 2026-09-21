//! Project SMTP settings and secure post-conversation rating invitations.

use std::time::Duration;

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::get,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use lettre::{
    Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart, SinglePart, header as message_header},
    transport::smtp::authentication::Credentials,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Executor, FromRow, Postgres, Transaction, types::Json as SqlJson};
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    ai_settings::{decrypt_secret, encrypt_secret, load_secret_encryption_key},
    auth::{ActorContext, generate_token, hash_token},
    error::AppError,
    widget_localization::{WidgetLanguage, WidgetTranslations},
};

const SMTP_PASSWORD_AAD_PREFIX: &str = "tzomet:smtp-password:v1";
const RATING_TOKEN_AAD_PREFIX: &str = "tzomet:support-rating-email-token:v1";
const RATING_TOKEN_HEADER: &str = "x-support-rating-token";
const RATING_INVITATION_TTL_DAYS: i64 = 30;
const SMTP_TIMEOUT: Duration = Duration::from_secs(30);

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/email-settings",
            get(get_email_settings).put(update_email_settings),
        )
        .route(
            "/public/v1/support-rating",
            get(get_public_rating).post(submit_public_rating),
        )
}

#[derive(Debug, FromRow)]
struct EmailSettingsRow {
    enabled: bool,
    smtp_host: String,
    smtp_port: i32,
    smtp_security: String,
    smtp_username: Option<String>,
    password_configured: bool,
    from_name: String,
    from_email: String,
    reply_to_email: Option<String>,
    rating_page_url: String,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct EmailSettingsResponse {
    configured: bool,
    enabled: bool,
    smtp_host: String,
    smtp_port: u16,
    smtp_security: String,
    smtp_username: Option<String>,
    password_configured: bool,
    from_name: String,
    from_email: String,
    reply_to_email: Option<String>,
    rating_page_url: String,
    updated_at: Option<DateTime<Utc>>,
}

impl EmailSettingsResponse {
    fn unconfigured() -> Self {
        Self {
            configured: false,
            enabled: false,
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_security: "starttls".to_owned(),
            smtp_username: None,
            password_configured: false,
            from_name: "Support".to_owned(),
            from_email: String::new(),
            reply_to_email: None,
            rating_page_url: String::new(),
            updated_at: None,
        }
    }
}

impl TryFrom<EmailSettingsRow> for EmailSettingsResponse {
    type Error = AppError;

    fn try_from(row: EmailSettingsRow) -> Result<Self, Self::Error> {
        Ok(Self {
            configured: true,
            enabled: row.enabled,
            smtp_host: row.smtp_host,
            smtp_port: u16::try_from(row.smtp_port).map_err(AppError::internal)?,
            smtp_security: row.smtp_security,
            smtp_username: row.smtp_username,
            password_configured: row.password_configured,
            from_name: row.from_name,
            from_email: row.from_email,
            reply_to_email: row.reply_to_email,
            rating_page_url: row.rating_page_url,
            updated_at: Some(row.updated_at),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailSettingsRequest {
    enabled: bool,
    smtp_host: String,
    smtp_port: u16,
    smtp_security: String,
    smtp_username: Option<String>,
    smtp_password: Option<String>,
    #[serde(default)]
    clear_smtp_password: bool,
    from_name: String,
    from_email: String,
    reply_to_email: Option<String>,
    rating_page_url: String,
}

struct NormalizedEmailSettings {
    enabled: bool,
    smtp_host: String,
    smtp_port: u16,
    smtp_security: String,
    smtp_username: Option<String>,
    smtp_password: Option<String>,
    clear_smtp_password: bool,
    from_name: String,
    from_email: String,
    reply_to_email: Option<String>,
    rating_page_url: String,
}

async fn get_email_settings(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<EmailSettingsResponse>, AppError> {
    let project_id = require_email_management(&actor)?;
    let row = load_email_settings(&state.db, actor.tenant_id, project_id).await?;
    let response = row.map_or_else(
        || Ok(EmailSettingsResponse::unconfigured()),
        EmailSettingsResponse::try_from,
    )?;
    Ok(Json(response))
}

async fn update_email_settings(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<EmailSettingsRequest>,
) -> Result<Json<EmailSettingsResponse>, AppError> {
    let project_id = require_email_management(&actor)?;
    let input = normalize_email_settings(request)?;
    let mut transaction = state.db.begin().await?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM projects WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    let current = load_email_settings(&mut *transaction, actor.tenant_id, project_id).await?;
    validate_smtp_password_destination(current.as_ref(), &input)?;
    if input.enabled {
        let _ = load_secret_encryption_key(&state)?;
    }
    let current_password_configured = current.as_ref().is_some_and(|row| row.password_configured);
    let effective_password_configured =
        if input.smtp_username.is_none() || input.clear_smtp_password {
            false
        } else {
            input.smtp_password.is_some() || current_password_configured
        };
    if input.smtp_username.is_some() != effective_password_configured {
        return Err(AppError::BadRequest(
            "smtp_username and smtp_password must be configured together".to_owned(),
        ));
    }

    let encrypted = input
        .smtp_password
        .as_deref()
        .map(|password| {
            let key = load_secret_encryption_key(&state)?;
            encrypt_secret(
                &key,
                password.as_bytes(),
                state.config.secrets.key_version.clone(),
                &smtp_password_aad(actor.tenant_id, project_id),
            )
        })
        .transpose()?;
    let clear_password = input.clear_smtp_password || input.smtp_username.is_none();
    if current.is_some() {
        sqlx::query(
            r#"
            UPDATE project_email_settings
            SET enabled = $3, smtp_host = $4, smtp_port = $5,
                smtp_security = $6, smtp_username = $7,
                encrypted_smtp_password = CASE
                    WHEN $8 THEN NULL
                    WHEN $9::bytea IS NOT NULL THEN $9
                    ELSE encrypted_smtp_password
                END,
                smtp_password_nonce = CASE
                    WHEN $8 THEN NULL
                    WHEN $10::bytea IS NOT NULL THEN $10
                    ELSE smtp_password_nonce
                END,
                key_version = CASE
                    WHEN $8 THEN NULL
                    WHEN $11::text IS NOT NULL THEN $11
                    ELSE key_version
                END,
                from_name = $12, from_email = $13, reply_to_email = $14,
                rating_page_url = $15, updated_at = now()
            WHERE tenant_id = $1 AND project_id = $2
            "#,
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(input.enabled)
        .bind(&input.smtp_host)
        .bind(i32::from(input.smtp_port))
        .bind(&input.smtp_security)
        .bind(input.smtp_username.as_deref())
        .bind(clear_password)
        .bind(
            encrypted
                .as_ref()
                .map(|secret| secret.ciphertext.as_slice()),
        )
        .bind(encrypted.as_ref().map(|secret| secret.nonce.as_slice()))
        .bind(encrypted.as_ref().map(|secret| secret.key_version.as_str()))
        .bind(&input.from_name)
        .bind(&input.from_email)
        .bind(input.reply_to_email.as_deref())
        .bind(&input.rating_page_url)
        .execute(&mut *transaction)
        .await?;
    } else {
        sqlx::query(
            r#"
            INSERT INTO project_email_settings (
                project_id, tenant_id, enabled, smtp_host, smtp_port, smtp_security,
                smtp_username, encrypted_smtp_password, smtp_password_nonce,
                key_version, from_name, from_email, reply_to_email, rating_page_url
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14
            )
            "#,
        )
        .bind(project_id)
        .bind(actor.tenant_id)
        .bind(input.enabled)
        .bind(&input.smtp_host)
        .bind(i32::from(input.smtp_port))
        .bind(&input.smtp_security)
        .bind(input.smtp_username.as_deref())
        .bind(
            encrypted
                .as_ref()
                .map(|secret| secret.ciphertext.as_slice()),
        )
        .bind(encrypted.as_ref().map(|secret| secret.nonce.as_slice()))
        .bind(encrypted.as_ref().map(|secret| secret.key_version.as_str()))
        .bind(&input.from_name)
        .bind(&input.from_email)
        .bind(input.reply_to_email.as_deref())
        .bind(&input.rating_page_url)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES (
            $1, $2, $3, $4, 'email_settings.updated',
            'project', $3, jsonb_build_object('enabled', $5::boolean)
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(input.enabled)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let row = load_email_settings(&state.db, actor.tenant_id, project_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(EmailSettingsResponse::try_from(row)?))
}

async fn load_email_settings<'e>(
    executor: impl Executor<'e, Database = Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
) -> Result<Option<EmailSettingsRow>, AppError> {
    sqlx::query_as::<_, EmailSettingsRow>(
        r#"
        SELECT enabled, smtp_host, smtp_port, smtp_security, smtp_username,
               encrypted_smtp_password IS NOT NULL AS password_configured,
               from_name, from_email, reply_to_email, rating_page_url, updated_at
        FROM project_email_settings
        WHERE tenant_id = $1 AND project_id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .fetch_optional(executor)
    .await
    .map_err(AppError::from)
}

pub(crate) fn require_email_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    actor.require("channels:manage")?;
    if actor.has_restricted_inbox_scope() {
        return Err(AppError::Forbidden);
    }
    actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))
}

fn validate_smtp_password_destination(
    current: Option<&EmailSettingsRow>,
    input: &NormalizedEmailSettings,
) -> Result<(), AppError> {
    let Some(current) = current.filter(|current| current.password_configured) else {
        return Ok(());
    };
    if input.smtp_password.is_some() || input.clear_smtp_password || input.smtp_username.is_none() {
        return Ok(());
    }
    if !current.smtp_host.eq_ignore_ascii_case(&input.smtp_host)
        || current.smtp_port != i32::from(input.smtp_port)
        || current.smtp_security != input.smtp_security
        || current.smtp_username != input.smtp_username
    {
        return Err(AppError::BadRequest(
            "re-enter or clear the SMTP password when changing its connection settings".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_email_settings(
    request: EmailSettingsRequest,
) -> Result<NormalizedEmailSettings, AppError> {
    let smtp_host = normalize_text(request.smtp_host, 253, "smtp_host")?;
    if smtp_host.contains(char::is_whitespace) || smtp_host.contains('/') || smtp_host.contains(':')
    {
        return Err(AppError::BadRequest("smtp_host is invalid".to_owned()));
    }
    if request.smtp_port == 0 {
        return Err(AppError::BadRequest(
            "smtp_port must be between 1 and 65535".to_owned(),
        ));
    }
    let smtp_security = request.smtp_security.trim().to_lowercase();
    if !matches!(smtp_security.as_str(), "tls" | "starttls") {
        return Err(AppError::BadRequest(
            "smtp_security must be tls or starttls".to_owned(),
        ));
    }
    let smtp_username = normalize_optional_text(request.smtp_username, 320, "smtp_username")?;
    let smtp_password = request
        .smtp_password
        .filter(|password| !password.is_empty());
    if request.clear_smtp_password && smtp_password.is_some() {
        return Err(AppError::BadRequest(
            "smtp_password and clear_smtp_password cannot be used together".to_owned(),
        ));
    }
    if smtp_password
        .as_ref()
        .is_some_and(|value| value.chars().count() > 1_000)
    {
        return Err(AppError::BadRequest(
            "smtp_password must not exceed 1000 characters".to_owned(),
        ));
    }
    let from_name = normalize_text(request.from_name, 120, "from_name")?;
    let from_email = normalize_email(request.from_email, "from_email")?;
    let reply_to_email = request
        .reply_to_email
        .map(|value| normalize_email(value, "reply_to_email"))
        .transpose()?;
    let rating_page_url = normalize_rating_page_url(request.rating_page_url)?;

    Ok(NormalizedEmailSettings {
        enabled: request.enabled,
        smtp_host,
        smtp_port: request.smtp_port,
        smtp_security,
        smtp_username,
        smtp_password,
        clear_smtp_password: request.clear_smtp_password,
        from_name,
        from_email,
        reply_to_email,
        rating_page_url,
    })
}

fn normalize_text(value: String, max_chars: usize, field: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars || value.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(format!(
            "{field} must contain between 1 and {max_chars} printable characters"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_optional_text(
    value: Option<String>,
    max_chars: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(|value| normalize_text(value, max_chars, field))
        .transpose()
}

fn normalize_email(value: String, field: &str) -> Result<String, AppError> {
    let value = value.trim().to_lowercase();
    if !(3..=320).contains(&value.chars().count()) || value.parse::<Address>().is_err() {
        return Err(AppError::BadRequest(format!("{field} is invalid")));
    }
    Ok(value)
}

fn normalize_rating_page_url(value: String) -> Result<String, AppError> {
    let value = value.trim().trim_end_matches('/');
    let parsed = Url::parse(value)
        .map_err(|_| AppError::BadRequest("rating_page_url is invalid".to_owned()))?;
    let loopback_http = parsed.scheme() == "http"
        && parsed
            .host_str()
            .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
    if !(parsed.scheme() == "https" || loopback_http)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || value.chars().count() > 2_000
    {
        return Err(AppError::BadRequest(
            "rating_page_url must be HTTPS (or loopback HTTP) without credentials, query, or fragment"
                .to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn smtp_password_aad(tenant_id: Uuid, project_id: Uuid) -> Vec<u8> {
    format!("{SMTP_PASSWORD_AAD_PREFIX}:{tenant_id}:{project_id}").into_bytes()
}

fn rating_token_aad(tenant_id: Uuid, invitation_id: Uuid, resolution_id: Uuid) -> Vec<u8> {
    format!("{RATING_TOKEN_AAD_PREFIX}:{tenant_id}:{invitation_id}:{resolution_id}").into_bytes()
}

#[derive(Debug, FromRow)]
struct InvitationCandidate {
    recipient_email: String,
    language: String,
}

/// Enqueues one email invitation for a newly created, rateable resolution.
///
/// The transaction owns both the resolution and invitation, so a rollback can
/// never leave mail queued for a conversation that was not actually closed.
pub(crate) async fn enqueue_rating_invitation(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    conversation_id: Uuid,
    resolution_id: Uuid,
) -> Result<(), AppError> {
    let candidate = sqlx::query_as::<_, InvitationCandidate>(
        r#"
        SELECT contact.email AS recipient_email,
               COALESCE(conversation.widget_language, widget.default_language) AS language
        FROM conversations AS conversation
        JOIN contacts AS contact
          ON contact.tenant_id = conversation.tenant_id
         AND contact.id = conversation.contact_id
        JOIN channel_connections AS channel
          ON channel.tenant_id = conversation.tenant_id
         AND channel.id = conversation.channel_connection_id
         AND channel.kind = 'widget'
        JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        JOIN project_email_settings AS settings
          ON settings.tenant_id = conversation.tenant_id
         AND settings.project_id = conversation.project_id
         AND settings.enabled
        WHERE conversation.tenant_id = $1
          AND conversation.project_id = $2
          AND conversation.inbox_id = $3
          AND conversation.id = $4
          AND contact.email IS NOT NULL
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(conversation_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(candidate) = candidate else {
        return Ok(());
    };

    let invitation_id = Uuid::now_v7();
    let access_token = generate_token();
    let access_token_hash = hash_token(&access_token);
    let key = load_secret_encryption_key(state)?;
    let encrypted = encrypt_secret(
        &key,
        access_token.as_bytes(),
        state.config.secrets.key_version.clone(),
        &rating_token_aad(tenant_id, invitation_id, resolution_id),
    )?;
    let expires_at = Utc::now() + ChronoDuration::days(RATING_INVITATION_TTL_DAYS);
    let inserted = sqlx::query(
        r#"
        INSERT INTO support_rating_email_invitations (
            id, tenant_id, project_id, inbox_id, resolution_id,
            recipient_email, language, access_token_hash,
            encrypted_access_token, access_token_nonce, key_version, expires_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT (tenant_id, resolution_id) DO NOTHING
        "#,
    )
    .bind(invitation_id)
    .bind(tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(resolution_id)
    .bind(candidate.recipient_email)
    .bind(candidate.language)
    .bind(access_token_hash)
    .bind(encrypted.ciphertext)
    .bind(encrypted.nonce.as_slice())
    .bind(encrypted.key_version)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await?;
    if inserted.rows_affected() == 0 {
        return Ok(());
    }
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload,
            status, available_at, created_at
        ) VALUES (
            $1, $2, 'email', $3, 'support_rating.requested', $4,
            'pending', now(), now()
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(tenant_id)
    .bind(invitation_id)
    .bind(json!({ "invitation_id": invitation_id }))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
struct DeliveryRow {
    invitation_id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    resolution_id: Uuid,
    recipient_email: String,
    language: String,
    encrypted_access_token: Option<Vec<u8>>,
    access_token_nonce: Option<Vec<u8>>,
    token_key_version: Option<String>,
    expires_at: DateTime<Utc>,
    sent_at: Option<DateTime<Utc>>,
    consumed_at: Option<DateTime<Utc>>,
    enabled: bool,
    smtp_host: String,
    smtp_port: i32,
    smtp_security: String,
    smtp_username: Option<String>,
    encrypted_smtp_password: Option<Vec<u8>>,
    smtp_password_nonce: Option<Vec<u8>>,
    smtp_key_version: Option<String>,
    from_name: String,
    from_email: String,
    reply_to_email: Option<String>,
    rating_page_url: String,
    translations: SqlJson<WidgetTranslations>,
    already_rated: bool,
}

pub(crate) async fn deliver_rating_invitation(state: &AppState, invitation_id: Uuid) -> Result<()> {
    let row = sqlx::query_as::<_, DeliveryRow>(
        r#"
        SELECT invitation.id AS invitation_id, invitation.tenant_id,
               invitation.project_id, invitation.resolution_id,
               invitation.recipient_email, invitation.language,
               invitation.encrypted_access_token, invitation.access_token_nonce,
               invitation.key_version AS token_key_version,
               invitation.expires_at, invitation.sent_at, invitation.consumed_at,
               settings.enabled, settings.smtp_host, settings.smtp_port,
               settings.smtp_security, settings.smtp_username,
               settings.encrypted_smtp_password, settings.smtp_password_nonce,
               settings.key_version AS smtp_key_version,
               settings.from_name, settings.from_email, settings.reply_to_email,
               settings.rating_page_url, widget.translations,
               EXISTS(
                   SELECT 1 FROM support_ratings AS rating
                   WHERE rating.tenant_id = invitation.tenant_id
                     AND rating.resolution_id = invitation.resolution_id
               ) AS already_rated
        FROM support_rating_email_invitations AS invitation
        JOIN conversation_resolutions AS resolution
          ON resolution.tenant_id = invitation.tenant_id
         AND resolution.id = invitation.resolution_id
        JOIN conversations AS conversation
          ON conversation.tenant_id = resolution.tenant_id
         AND conversation.id = resolution.conversation_id
        JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        JOIN project_email_settings AS settings
          ON settings.tenant_id = invitation.tenant_id
         AND settings.project_id = invitation.project_id
        WHERE invitation.id = $1
        "#,
    )
    .bind(invitation_id)
    .fetch_optional(&state.db)
    .await
    .context("failed to load rating email invitation")?;
    let Some(row) = row else {
        return Ok(());
    };
    if row.sent_at.is_some() {
        return Ok(());
    }
    if !row.enabled
        || row.already_rated
        || row.consumed_at.is_some()
        || row.expires_at <= Utc::now()
    {
        clear_undelivered_token(&state.db, invitation_id).await?;
        return Ok(());
    }

    let (Some(encrypted_token), Some(token_nonce), Some(token_key_version)) = (
        row.encrypted_access_token.as_deref(),
        row.access_token_nonce.as_deref(),
        row.token_key_version.as_deref(),
    ) else {
        anyhow::bail!("rating email access token encryption metadata is incomplete");
    };
    if token_key_version != state.config.secrets.key_version {
        anyhow::bail!("rating email access token uses an unsupported key version");
    }
    let key = load_secret_encryption_key(state).map_err(anyhow::Error::new)?;
    let token = decrypt_secret(
        &key,
        encrypted_token,
        token_nonce,
        &rating_token_aad(row.tenant_id, row.invitation_id, row.resolution_id),
    )?;
    let token = String::from_utf8(token).context("rating email access token is not valid UTF-8")?;
    let smtp_password = decrypt_smtp_password(state, &row, &key)?;
    let language = WidgetLanguage::parse(&row.language).map_err(anyhow::Error::new)?;
    let translation = row
        .translations
        .get(&language)
        .map_err(anyhow::Error::new)?;
    let link = format!("{}#access={token}", row.rating_page_url);
    let message = build_rating_message(&row, translation.rating_prompt.as_str(), &link)?;
    let mut transport = match row.smtp_security.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&row.smtp_host)
            .context("failed to configure implicit TLS SMTP")?,
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&row.smtp_host)
            .context("failed to configure STARTTLS SMTP")?,
        _ => anyhow::bail!("stored SMTP security mode is invalid"),
    }
    .port(u16::try_from(row.smtp_port).context("stored SMTP port is invalid")?)
    .timeout(Some(SMTP_TIMEOUT));
    match (row.smtp_username, smtp_password) {
        (Some(username), Some(password)) => {
            transport = transport.credentials(Credentials::new(username, password));
        }
        (None, None) => {}
        _ => anyhow::bail!("SMTP credential encryption metadata is incomplete"),
    }
    transport
        .build()
        .send(message)
        .await
        .context("SMTP rejected the rating invitation")?;
    sqlx::query(
        r#"
        UPDATE support_rating_email_invitations
        SET sent_at = now(), encrypted_access_token = NULL,
            access_token_nonce = NULL, key_version = NULL
        WHERE id = $1 AND sent_at IS NULL
        "#,
    )
    .bind(invitation_id)
    .execute(&state.db)
    .await
    .context("failed to mark rating invitation sent")?;
    Ok(())
}

fn decrypt_smtp_password(
    state: &AppState,
    row: &DeliveryRow,
    key: &[u8; 32],
) -> Result<Option<String>> {
    let (Some(ciphertext), Some(nonce), Some(key_version)) = (
        row.encrypted_smtp_password.as_deref(),
        row.smtp_password_nonce.as_deref(),
        row.smtp_key_version.as_deref(),
    ) else {
        if row.encrypted_smtp_password.is_none()
            && row.smtp_password_nonce.is_none()
            && row.smtp_key_version.is_none()
        {
            return Ok(None);
        }
        anyhow::bail!("SMTP password encryption metadata is incomplete");
    };
    if key_version != state.config.secrets.key_version {
        anyhow::bail!("SMTP password uses an unsupported key version");
    }
    let plaintext = decrypt_secret(
        key,
        ciphertext,
        nonce,
        &smtp_password_aad(row.tenant_id, row.project_id),
    )?;
    String::from_utf8(plaintext)
        .map(Some)
        .context("SMTP password is not valid UTF-8")
}

fn build_rating_message(row: &DeliveryRow, rating_prompt: &str, link: &str) -> Result<Message> {
    let russian = row.language.split('-').next() == Some("ru");
    let subject = if russian {
        "Оцените качество поддержки"
    } else {
        "Rate your support experience"
    };
    let button = if russian {
        "Оценить чат"
    } else {
        "Rate the chat"
    };
    let fallback = if russian {
        "Если кнопка не открывается, скопируйте эту ссылку:"
    } else {
        "If the button does not open, copy this link:"
    };
    let from_address = row.from_email.parse::<Address>()?;
    let recipient = row.recipient_email.parse::<Address>()?;
    let mut builder = Message::builder()
        .from(Mailbox::new(Some(row.from_name.clone()), from_address))
        .to(Mailbox::new(None, recipient))
        .subject(subject)
        .message_id(Some(format!(
            "<{}@{}-rating>",
            row.invitation_id,
            crate::config::PRODUCT_NAMESPACE
        )));
    if let Some(reply_to) = row.reply_to_email.as_deref() {
        builder = builder.reply_to(Mailbox::new(None, reply_to.parse::<Address>()?));
    }
    let plain = format!("{rating_prompt}\n\n{button}: {link}\n\n{fallback}\n{link}");
    let html_prompt = escape_html(rating_prompt);
    let html_link = escape_html(link);
    let html = format!(
        r#"<!doctype html><html><body style="margin:0;background:#f4f6f8;padding:32px 16px"><div style="max-width:560px;margin:0 auto;background:#fff;border:1px solid #dfe4ea;border-radius:12px;padding:32px;font-family:Arial,sans-serif;color:#17212b"><p style="font-size:18px;line-height:1.55;margin:0 0 24px">{html_prompt}</p><a href="{html_link}" style="display:inline-block;background:#17212b;color:#fff;text-decoration:none;border-radius:8px;padding:12px 20px;font-weight:700">{button}</a><p style="font-size:12px;line-height:1.5;color:#65717e;margin:28px 0 6px">{fallback}</p><p style="font-size:12px;line-height:1.5;word-break:break-all;margin:0"><a href="{html_link}" style="color:#3867d6">{html_link}</a></p></div></body></html>"#,
    );
    builder
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(message_header::ContentType::TEXT_PLAIN)
                        .body(plain),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(message_header::ContentType::TEXT_HTML)
                        .body(html),
                ),
        )
        .context("failed to build rating invitation email")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

async fn clear_undelivered_token(db: &sqlx::PgPool, invitation_id: Uuid) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE support_rating_email_invitations
        SET encrypted_access_token = NULL, access_token_nonce = NULL,
            key_version = NULL, expires_at = LEAST(expires_at, now())
        WHERE id = $1 AND sent_at IS NULL
        "#,
    )
    .bind(invitation_id)
    .execute(db)
    .await
    .context("failed to suppress rating email invitation")?;
    Ok(())
}

#[derive(Debug, FromRow)]
struct PublicRatingRow {
    invitation_id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    resolution_id: Uuid,
    language: String,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    translations: SqlJson<WidgetTranslations>,
    already_rated: bool,
}

#[derive(Debug, Serialize)]
struct PublicRatingResponse {
    language: String,
    rating_prompt: String,
    rating_thanks: String,
    already_rated: bool,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicRatingRequest {
    rating: i16,
    reasons: Vec<String>,
    comment: Option<String>,
}

#[derive(Debug, Serialize)]
struct PublicRatingSubmissionResponse {
    rating: i16,
    created_at: DateTime<Utc>,
}

async fn get_public_rating(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<PublicRatingResponse>), AppError> {
    let token = rating_access_token(&headers)?;
    let row = load_public_rating(&state, &token).await?;
    let language = WidgetLanguage::parse(&row.language).map_err(|_| AppError::NotFound)?;
    let translation = row
        .translations
        .get(&language)
        .map_err(|_| AppError::NotFound)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        response_headers,
        Json(PublicRatingResponse {
            language: row.language,
            rating_prompt: translation.rating_prompt.clone(),
            rating_thanks: translation.rating_thanks.clone(),
            already_rated: row.already_rated || row.consumed_at.is_some(),
            expires_at: row.expires_at,
        }),
    ))
}

async fn submit_public_rating(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PublicRatingRequest>,
) -> Result<(StatusCode, HeaderMap, Json<PublicRatingSubmissionResponse>), AppError> {
    validate_rating_request(&request)?;
    let token = rating_access_token(&headers)?;
    let token_hash = hash_token(&token);
    let reasons = request
        .reasons
        .into_iter()
        .map(|reason| normalize_text(reason, 100, "rating reason"))
        .collect::<Result<Vec<_>, _>>()?;
    let comment = normalize_optional_text(request.comment, 2_000, "comment")?;
    let mut transaction = state.db.begin().await?;
    let row = sqlx::query_as::<_, PublicRatingRow>(
        r#"
        SELECT invitation.id AS invitation_id, invitation.tenant_id,
               invitation.project_id, invitation.inbox_id, invitation.resolution_id,
               invitation.language, invitation.expires_at, invitation.sent_at,
               invitation.consumed_at, widget.translations,
               EXISTS(
                   SELECT 1 FROM support_ratings AS rating
                   WHERE rating.tenant_id = invitation.tenant_id
                     AND rating.resolution_id = invitation.resolution_id
               ) AS already_rated
        FROM support_rating_email_invitations AS invitation
        JOIN conversation_resolutions AS resolution
          ON resolution.tenant_id = invitation.tenant_id
         AND resolution.id = invitation.resolution_id
        JOIN conversations AS conversation
          ON conversation.tenant_id = resolution.tenant_id
         AND conversation.id = resolution.conversation_id
        JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        WHERE invitation.access_token_hash = $1
          AND invitation.expires_at > now()
          AND invitation.sent_at IS NOT NULL
        FOR UPDATE OF invitation
        "#,
    )
    .bind(token_hash)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.already_rated || row.consumed_at.is_some() {
        return Err(AppError::Conflict(
            "this resolution has already been rated".to_owned(),
        ));
    }
    let now = Utc::now();
    let rating_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO support_ratings (
            id, tenant_id, project_id, inbox_id, resolution_id,
            rating, comment, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(rating_id)
    .bind(row.tenant_id)
    .bind(row.project_id)
    .bind(row.inbox_id)
    .bind(row.resolution_id)
    .bind(request.rating)
    .bind(comment)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    for reason in reasons {
        sqlx::query(
            r#"
            INSERT INTO support_rating_reasons (
                id, tenant_id, support_rating_id, reason
            ) VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(row.tenant_id)
        .bind(rating_id)
        .bind(reason)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query("UPDATE support_rating_email_invitations SET consumed_at = $2 WHERE id = $1")
        .bind(row.invitation_id)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        StatusCode::CREATED,
        response_headers,
        Json(PublicRatingSubmissionResponse {
            rating: request.rating,
            created_at: now,
        }),
    ))
}

async fn load_public_rating(state: &AppState, token: &str) -> Result<PublicRatingRow, AppError> {
    sqlx::query_as::<_, PublicRatingRow>(
        r#"
        SELECT invitation.id AS invitation_id, invitation.tenant_id,
               invitation.project_id, invitation.inbox_id, invitation.resolution_id,
               invitation.language, invitation.expires_at, invitation.sent_at,
               invitation.consumed_at, widget.translations,
               EXISTS(
                   SELECT 1 FROM support_ratings AS rating
                   WHERE rating.tenant_id = invitation.tenant_id
                     AND rating.resolution_id = invitation.resolution_id
               ) AS already_rated
        FROM support_rating_email_invitations AS invitation
        JOIN conversation_resolutions AS resolution
          ON resolution.tenant_id = invitation.tenant_id
         AND resolution.id = invitation.resolution_id
        JOIN conversations AS conversation
          ON conversation.tenant_id = resolution.tenant_id
         AND conversation.id = resolution.conversation_id
        JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        WHERE invitation.access_token_hash = $1
          AND invitation.expires_at > now()
          AND invitation.sent_at IS NOT NULL
        "#,
    )
    .bind(hash_token(token))
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

fn rating_access_token(headers: &HeaderMap) -> Result<String, AppError> {
    let mut values = headers.get_all(RATING_TOKEN_HEADER).iter();
    let token = values
        .next()
        .ok_or(AppError::Unauthorized)?
        .to_str()
        .map_err(|_| AppError::Unauthorized)?;
    if values.next().is_some() || token.len() > 128 || token.is_empty() {
        return Err(AppError::Unauthorized);
    }
    Ok(token.to_owned())
}

fn validate_rating_request(request: &PublicRatingRequest) -> Result<(), AppError> {
    if !(1..=5).contains(&request.rating) {
        return Err(AppError::BadRequest(
            "rating must be between 1 and 5".to_owned(),
        ));
    }
    if request.reasons.len() > 10 {
        return Err(AppError::BadRequest(
            "at most 10 rating reasons are allowed".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        EmailSettingsRequest, EmailSettingsRow, escape_html, normalize_email_settings,
        normalize_rating_page_url, validate_smtp_password_destination,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use chrono::Utc;
    use sha2::{Digest, Sha256};
    use sqlx::PgPool;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::{AppState, Config};

    fn valid_request() -> EmailSettingsRequest {
        EmailSettingsRequest {
            enabled: true,
            smtp_host: "smtp.example.com".to_owned(),
            smtp_port: 587,
            smtp_security: "starttls".to_owned(),
            smtp_username: Some("mailer@example.com".to_owned()),
            smtp_password: Some("secret".to_owned()),
            clear_smtp_password: false,
            from_name: "Customer Support".to_owned(),
            from_email: "support@example.com".to_owned(),
            reply_to_email: None,
            rating_page_url: "https://support.example.com/rate-chat".to_owned(),
        }
    }

    #[test]
    fn validates_secure_smtp_settings() {
        let normalized = normalize_email_settings(valid_request()).unwrap();
        assert_eq!(normalized.smtp_security, "starttls");
        assert_eq!(normalized.from_email, "support@example.com");

        let mut invalid = valid_request();
        invalid.smtp_security = "none".to_owned();
        assert!(normalize_email_settings(invalid).is_err());

        assert!(normalize_rating_page_url("http://example.com/rate-chat".to_owned()).is_err());
        assert!(normalize_rating_page_url("http://localhost:5173/rate-chat".to_owned()).is_ok());
    }

    #[test]
    fn stored_smtp_password_cannot_follow_connection_changes() {
        let current = EmailSettingsRow {
            enabled: true,
            smtp_host: "smtp.example.com".to_owned(),
            smtp_port: 587,
            smtp_security: "starttls".to_owned(),
            smtp_username: Some("mailer@example.com".to_owned()),
            password_configured: true,
            from_name: "Customer Support".to_owned(),
            from_email: "support@example.com".to_owned(),
            reply_to_email: None,
            rating_page_url: "https://support.example.com/rate-chat".to_owned(),
            updated_at: Utc::now(),
        };
        let mut unchanged = normalize_email_settings(valid_request()).unwrap();
        unchanged.smtp_password = None;
        assert!(validate_smtp_password_destination(Some(&current), &unchanged).is_ok());
        unchanged.smtp_host = "SMTP.EXAMPLE.COM".to_owned();
        assert!(validate_smtp_password_destination(Some(&current), &unchanged).is_ok());

        for field in ["host", "port", "security", "username"] {
            let mut changed = normalize_email_settings(valid_request()).unwrap();
            changed.smtp_password = None;
            match field {
                "host" => changed.smtp_host = "attacker.example".to_owned(),
                "port" => changed.smtp_port = 465,
                "security" => changed.smtp_security = "tls".to_owned(),
                "username" => changed.smtp_username = Some("other@example.com".to_owned()),
                _ => unreachable!(),
            }
            assert!(
                validate_smtp_password_destination(Some(&current), &changed).is_err(),
                "stored password must not follow a changed {field}",
            );
            changed.smtp_password = Some("replacement".to_owned());
            assert!(validate_smtp_password_destination(Some(&current), &changed).is_ok());
            changed.smtp_password = None;
            changed.clear_smtp_password = true;
            assert!(validate_smtp_password_destination(Some(&current), &changed).is_ok());
            changed.clear_smtp_password = false;
            changed.smtp_username = None;
            assert!(validate_smtp_password_destination(Some(&current), &changed).is_ok());
        }
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn smtp_updates_enforce_scope_and_preserve_credential_destination_in_database(
        db: PgPool,
    ) {
        let tenant_id = Uuid::from_u128(1);
        let project_id = Uuid::from_u128(2);
        let user_id = Uuid::from_u128(3);
        sqlx::raw_sql(&format!(
            r#"
            INSERT INTO tenants (id, name) VALUES ('{tenant_id}', 'SMTP test');
            INSERT INTO users (id, email, display_name)
                VALUES ('{user_id}', 'smtp@example.test', 'SMTP manager');
            INSERT INTO projects (id, tenant_id, name, slug)
                VALUES ('{project_id}', '{tenant_id}', 'SMTP test', 'smtp-test');
            INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
                VALUES ('{tenant_id}', '{project_id}', 'manager', 'Manager', 'manager',
                    ARRAY['channels:manage']);
            INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
                VALUES ('{membership_id}', '{tenant_id}', '{project_id}', '{user_id}', 'manager');
            INSERT INTO project_email_settings (
                tenant_id, project_id, enabled, smtp_host, smtp_port, smtp_security,
                smtp_username, encrypted_smtp_password, smtp_password_nonce, key_version,
                from_name, from_email, rating_page_url
            ) VALUES (
                '{tenant_id}', '{project_id}', false, 'smtp.example.com', 587, 'starttls',
                'mailer@example.com', decode(repeat('07', 32), 'hex'),
                decode(repeat('08', 12), 'hex'), 'v1', 'Support', 'support@example.com',
                'https://support.example.com/rate-chat'
            );
            "#,
            membership_id = Uuid::from_u128(4),
        ))
        .execute(&db)
        .await
        .unwrap();
        for (token, scope) in [
            ("smtp-project", None),
            ("smtp-inbox", Some(vec![Uuid::from_u128(5)])),
        ] {
            sqlx::query(
                r#"
                INSERT INTO api_keys (
                    id, tenant_id, project_id, actor_user_id, name, token_hash,
                    permissions, inbox_scope, role, expires_at
                ) VALUES ($1, $2, $3, $4, $5, $6, ARRAY['channels:manage'], $7,
                          'manager', now() + interval '1 hour')
                "#,
            )
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(project_id)
            .bind(user_id)
            .bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec())
            .bind(scope)
            .execute(&db)
            .await
            .unwrap();
        }
        let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
        config.pg.migrate_on_start = false;
        config.redis.url = None;
        config.attachments.enabled = false;
        let mut state = AppState::build(config).await.unwrap();
        state.db = db.clone();
        let app = super::router().with_state(state);
        let update = |token: &str, host: &str, clear_password: bool| {
            let app = app.clone();
            let request = Request::put("/api/v1/email-settings")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "enabled": false,
                        "smtp_host": host,
                        "smtp_port": 587,
                        "smtp_security": "starttls",
                        "smtp_username": (!clear_password).then_some("mailer@example.com"),
                        "clear_smtp_password": clear_password,
                        "from_name": "Support",
                        "from_email": "support@example.com",
                        "rating_page_url": "https://support.example.com/rate-chat"
                    })
                    .to_string(),
                ))
                .unwrap();
            async move { app.oneshot(request).await.unwrap().status() }
        };
        for (token, host, expected) in [
            ("smtp-inbox", "attacker.example", StatusCode::FORBIDDEN),
            ("smtp-project", "attacker.example", StatusCode::BAD_REQUEST),
            ("smtp-project", "smtp.example.com", StatusCode::OK),
        ] {
            assert_eq!(update(token, host, false).await, expected);
            let stored = sqlx::query_as::<_, (String, Vec<u8>, Vec<u8>, String)>(
                "SELECT smtp_host, encrypted_smtp_password, smtp_password_nonce, key_version \
                 FROM project_email_settings WHERE tenant_id = $1 AND project_id = $2",
            )
            .bind(tenant_id)
            .bind(project_id)
            .fetch_one(&db)
            .await
            .unwrap();
            assert_eq!(
                stored,
                (
                    "smtp.example.com".to_owned(),
                    vec![7; 32],
                    vec![8; 12],
                    "v1".to_owned()
                )
            );
        }
        let (preserve, clear) = tokio::join!(
            update("smtp-project", "smtp.example.com", false),
            update("smtp-project", "new-smtp.example", true),
        );
        assert!(matches!(preserve, StatusCode::OK | StatusCode::BAD_REQUEST));
        assert_eq!(clear, StatusCode::OK);
        let stored = sqlx::query_as::<_, (String, bool)>(
            "SELECT smtp_host, encrypted_smtp_password IS NULL AND smtp_password_nonce IS NULL \
             AND key_version IS NULL AND smtp_username IS NULL \
             FROM project_email_settings WHERE tenant_id = $1 AND project_id = $2",
        )
        .bind(tenant_id)
        .bind(project_id)
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(stored, ("new-smtp.example".to_owned(), true));
    }

    #[test]
    fn escapes_customer_visible_html() {
        assert_eq!(
            escape_html("<b>Tom & 'Ana'</b>"),
            "&lt;b&gt;Tom &amp; &#39;Ana&#39;&lt;/b&gt;"
        );
    }
}
