//! Widget and operator conversation APIs.

mod ai_handoff;
mod blacklist;
use ai_handoff::return_conversation_to_ai;
pub use ai_handoff::return_unanswered_operator_conversations_to_ai;
pub mod model;
pub use blacklist::process_once as process_blacklist_reply_once;

use std::net::SocketAddr;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::{get, patch, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, Postgres, Transaction, types::Json as SqlJson};
use tracing::warn;
use uuid::Uuid;

use crate::{
    AppState,
    attachments::{AttachmentResponse, AttachmentStorage, PreparedAttachment},
    auth::{ActorContext, WidgetSessionContext, generate_token, hash_token},
    client_ip,
    contact_blacklist::BlacklistReply,
    email,
    error::AppError,
    inbox_routing::{self, InboundContext},
    operator_presence::inbox_has_online_support,
    provider_reply::{self, ContactMessageTrigger},
    realtime::RealtimeEvent,
    telegram_bot,
    telegram_notifications::{self, TelegramNotificationKind, TelegramNotificationPayload},
    visitor_environment::{GeoIpDetails, UserAgentDetails, VisitorEnvironment},
    visitor_intelligence::{ClientContext, ONLINE_PRESENCE_WINDOW_SECONDS, capture_headers},
    widget_launcher::WidgetLauncher,
    widget_localization::{WidgetLanguage, WidgetTranslations},
    widget_theme::WidgetTheme,
};

const MAX_MESSAGE_LENGTH: usize = 10_000;
const MAX_CONVERSATION_SEARCH_LENGTH: usize = 200;
const MAX_WIDGET_SESSION_BODY_LENGTH: usize = 32 * 1_024;
const AI_AUTO_JOIN_DELAY_MIN_SECONDS: i64 = 5;
const AI_AUTO_JOIN_DELAY_MAX_SECONDS: i64 = 10;

fn random_ai_auto_join_delay_seconds() -> i64 {
    rand::rng().random_range(AI_AUTO_JOIN_DELAY_MIN_SECONDS..=AI_AUTO_JOIN_DELAY_MAX_SECONDS)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/widget/v1/sessions",
            post(create_widget_session)
                .layer(DefaultBodyLimit::max(MAX_WIDGET_SESSION_BODY_LENGTH)),
        )
        .route("/widget/v1/presence", post(update_widget_presence))
        .route("/widget/v1/presentation", get(get_widget_presentation))
        .route("/widget/v1/contact", patch(update_widget_contact))
        .route("/widget/v1/conversations", post(create_widget_conversation))
        .route(
            "/api/v1/visitor-sessions/{session_id}/conversation",
            post(start_visitor_conversation),
        )
        .route(
            "/widget/v1/conversations/{conversation_id}/messages",
            get(list_widget_messages).post(create_widget_message),
        )
        .route(
            "/widget/v1/conversations/{conversation_id}/read",
            post(mark_widget_messages_read),
        )
        .route(
            "/widget/v1/conversations/{conversation_id}/draft",
            post(update_widget_draft),
        )
        .route(
            "/widget/v1/resolutions/{resolution_id}/rating",
            post(rate_resolution),
        )
        .route(
            "/api/v1/inboxes/{inbox_id}/conversations",
            get(list_operator_conversations),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/messages",
            get(list_operator_messages).post(create_operator_message),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/reply-suggestion-agents",
            get(list_reply_suggestion_agents),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/reply-suggestions",
            post(generate_reply_suggestion),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/read",
            post(mark_operator_conversation_read),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/join",
            post(join_conversation),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/take-over",
            post(take_over_conversation),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/return-to-ai",
            post(return_conversation_to_ai),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/visitor-intelligence",
            get(get_visitor_intelligence),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/resolve",
            post(resolve_conversation),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/reopen",
            post(reopen_conversation),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/widget-attachments",
            patch(update_conversation_attachment_policy),
        )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateWidgetSessionRequest {
    widget_id: Uuid,
    visitor_id: Uuid,
    language: Option<WidgetLanguage>,
    client_context: ClientContext,
}

fn validate_visitor_id(visitor_id: Uuid) -> Result<(), AppError> {
    if visitor_id.is_nil() || visitor_id.get_version_num() != 4 {
        return Err(AppError::BadRequest(
            "visitor_id must be a non-nil UUIDv4".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct WidgetSessionResponse {
    session_id: Uuid,
    token: String,
    inbox_id: Uuid,
    language: WidgetLanguage,
    support_name: String,
    greeting: Option<String>,
    offline_message: String,
    rating_prompt: String,
    rating_thanks: String,
    proactive_invitation_message: String,
    online_now: String,
    offline_now: String,
    message_placeholder: String,
    contact_title: String,
    contact_description: String,
    contact_name: String,
    contact_name_placeholder: String,
    contact_email: String,
    contact_email_placeholder: String,
    contact_save: String,
    contact_saving: String,
    contact_skip: String,
    contact_error: String,
    launcher: WidgetLauncher,
    theme: WidgetTheme,
    operators_online: bool,
    attachments_enabled: bool,
    contact: WidgetContactProfile,
    expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct WidgetContactProfile {
    display_name: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateWidgetContactRequest {
    display_name: String,
    email: String,
}

#[derive(Debug, Serialize)]
struct WidgetAvailabilityResponse {
    operators_online: bool,
}

#[derive(Debug, FromRow)]
struct WidgetConfigRow {
    channel_connection_id: Uuid,
    name: String,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    allowed_origins: Vec<String>,
    default_language: String,
    translations: SqlJson<WidgetTranslations>,
    launcher: SqlJson<WidgetLauncher>,
    theme: SqlJson<WidgetTheme>,
    notify_on_new_visitor: bool,
    attachments_enabled: bool,
}

async fn create_widget_session(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(HeaderMap, Json<WidgetSessionResponse>), AppError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .ok_or_else(|| AppError::BadRequest("Content-Type header is required".to_owned()))?
        .to_str()
        .map_err(|_| AppError::BadRequest("Content-Type header is invalid".to_owned()))?
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default();
    if !content_type.eq_ignore_ascii_case("application/json")
        && !content_type.eq_ignore_ascii_case("text/plain")
    {
        return Err(AppError::BadRequest(
            "Content-Type must be application/json or text/plain".to_owned(),
        ));
    }
    let request = serde_json::from_slice::<CreateWidgetSessionRequest>(&body)
        .map_err(|_| AppError::BadRequest("widget session body is invalid".to_owned()))?;
    validate_visitor_id(request.visitor_id)?;
    let mut origin_values = headers.get_all(header::ORIGIN).iter();
    let origin_header = origin_values
        .next()
        .ok_or_else(|| AppError::BadRequest("Origin header is required".to_owned()))?
        .clone();
    if origin_values.next().is_some() {
        return Err(AppError::BadRequest(
            "Origin header must be provided exactly once".to_owned(),
        ));
    }
    let origin = origin_header
        .to_str()
        .map_err(|_| AppError::BadRequest("Origin header is invalid".to_owned()))?;
    let origin = origin.to_owned();
    let client_context = request
        .client_context
        .validate_and_normalize(origin.as_str())?;
    let client_ip = client_ip::resolve(
        peer.ip(),
        &headers,
        state.config.server.trusted_proxies.as_slice(),
    )?;
    let captured_headers = capture_headers(&headers)?;
    let widget = sqlx::query_as::<_, WidgetConfigRow>(
        r#"
        SELECT connection.id AS channel_connection_id, connection.name,
               connection.tenant_id,
               connection.project_id, connection.inbox_id, widget.allowed_origins,
               widget.default_language, widget.translations, widget.launcher, widget.theme,
               widget.notify_on_new_visitor, widget.attachments_enabled
        FROM channel_connections AS connection
        JOIN widget_configs AS widget
          ON widget.tenant_id = connection.tenant_id
         AND widget.channel_connection_id = connection.id
        WHERE connection.public_id = $1
          AND connection.kind = 'widget'
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
        "#,
    )
    .bind(request.widget_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    state
        .config
        .product
        .resolve_project(Some(widget.project_id))?;

    if !widget
        .allowed_origins
        .iter()
        .any(|allowed| allowed == &origin)
    {
        return Err(AppError::Forbidden);
    }

    let default_language = WidgetLanguage::parse(&widget.default_language)?;
    let language = widget
        .translations
        .0
        .resolve_language(request.language.as_ref(), &default_language)?;
    let translation = widget.translations.0.get(&language)?;
    let support_name = translation.support_name.clone();
    let greeting = translation.greeting.clone();
    let offline_message = translation.offline_message.clone();
    let rating_prompt = translation.rating_prompt.clone();
    let rating_thanks = translation.rating_thanks.clone();
    let proactive_invitation_message = translation.proactive_invitation_message.clone();
    let online_now = translation.online_now.clone();
    let offline_now = translation.offline_now.clone();
    let message_placeholder = translation.message_placeholder.clone();
    let contact_title = translation.contact_title.clone();
    let contact_description = translation.contact_description.clone();
    let contact_name = translation.contact_name.clone();
    let contact_name_placeholder = translation.contact_name_placeholder.clone();
    let contact_email = translation.contact_email.clone();
    let contact_email_placeholder = translation.contact_email_placeholder.clone();
    let contact_save = translation.contact_save.clone();
    let contact_saving = translation.contact_saving.clone();
    let contact_skip = translation.contact_skip.clone();
    let contact_error = translation.contact_error.clone();
    let mut launcher = widget.launcher.0;
    launcher.label.clone_from(&translation.launcher_label);

    let session_id = Uuid::now_v7();
    let token = generate_token();
    let now = Utc::now();
    let expires_at = now
        + chrono::Duration::from_std(state.config.widget.session_ttl())
            .map_err(AppError::internal)?;
    let visitor_data_expires_at = now
        .checked_add_signed(chrono::Duration::days(i64::from(
            state.config.widget.visitor_data_retention_days,
        )))
        .ok_or_else(|| {
            AppError::internal(anyhow::anyhow!(
                "widget visitor data retention exceeds the supported timestamp range"
            ))
        })?;
    let mut transaction = state.db.begin().await?;

    let channel_is_active = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT true
        FROM channel_connections
        WHERE tenant_id = $1
          AND id = $2
          AND status = 'active'
          AND deleted_at IS NULL
        FOR SHARE
        "#,
    )
    .bind(widget.tenant_id)
    .bind(widget.channel_connection_id)
    .fetch_optional(&mut *transaction)
    .await?
    .unwrap_or(false);
    if !channel_is_active {
        return Err(AppError::NotFound);
    }

    let visitor_id_hash = if widget.channel_connection_id == crate::demo::DEMO_CHANNEL_ID {
        crate::demo_access::visitor_hash(request.visitor_id, client_ip.address)
    } else {
        hash_token(&request.visitor_id.to_string())
    };
    let visitor_id_digest = URL_SAFE_NO_PAD.encode(&visitor_id_hash);
    let external_id = format!("{}:{visitor_id_digest}", widget.channel_connection_id);
    let advisory_lock_key = format!("{}:{external_id}", widget.tenant_id);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(advisory_lock_key)
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
          AND identity.channel_kind = 'widget'
          AND identity.external_id = $2
        "#,
    )
    .bind(widget.tenant_id)
    .bind(&external_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let contact_id = if let Some((contact_id, project_id)) = existing_contact {
        if project_id != widget.project_id {
            return Err(AppError::internal(anyhow::anyhow!(
                "widget visitor identity resolved to a different project"
            )));
        }
        contact_id
    } else {
        let contact_id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO contacts (id, tenant_id, project_id, display_name)
            VALUES ($1, $2, $3, NULL)
            "#,
        )
        .bind(contact_id)
        .bind(widget.tenant_id)
        .bind(widget.project_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO contact_identities (
                id, tenant_id, contact_id, channel_kind, external_id
            ) VALUES ($1, $2, $3, 'widget', $4)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(widget.tenant_id)
        .bind(contact_id)
        .bind(&external_id)
        .execute(&mut *transaction)
        .await?;
        contact_id
    };
    sqlx::query("UPDATE contacts SET updated_at = now() WHERE tenant_id = $1 AND id = $2")
        .bind(widget.tenant_id)
        .bind(contact_id)
        .execute(&mut *transaction)
        .await?;
    let contact = sqlx::query_as::<_, WidgetContactProfile>(
        r#"
        SELECT display_name, email
        FROM contacts
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        "#,
    )
    .bind(widget.tenant_id)
    .bind(widget.project_id)
    .bind(contact_id)
    .fetch_one(&mut *transaction)
    .await?;

    let online_cutoff = now - chrono::Duration::seconds(ONLINE_PRESENCE_WINDOW_SECONDS);
    let already_online = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM widget_sessions
            WHERE tenant_id = $1
              AND contact_id = $2
              AND channel_connection_id = $3
              AND presence_last_seen_at >= $4
              AND expires_at > now()
              AND revoked_at IS NULL
        )
        "#,
    )
    .bind(widget.tenant_id)
    .bind(contact_id)
    .bind(widget.channel_connection_id)
    .bind(online_cutoff)
    .fetch_one(&mut *transaction)
    .await?;

    let client_ip_address = client_ip.address.to_string();
    let client_hints = captured_headers.client_hints.map(SqlJson);
    let page_url = client_context.page.url.clone();
    let page_title = client_context.page.title.clone();
    let client_context = SqlJson(serde_json::to_value(client_context).map_err(AppError::internal)?);
    sqlx::query(
        r#"
        INSERT INTO widget_sessions (
            id, tenant_id, project_id, inbox_id, channel_connection_id,
            contact_id, token_hash, origin, expires_at, visitor_id_hash,
            client_ip, client_ip_source, user_agent, accept_language,
            client_hints, client_context, visitor_data_expires_at, language,
            presence_last_seen_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
            $11::text::inet, $12, $13, $14, $15, $16, $17, $18, $19
        )
        "#,
    )
    .bind(session_id)
    .bind(widget.tenant_id)
    .bind(widget.project_id)
    .bind(widget.inbox_id)
    .bind(widget.channel_connection_id)
    .bind(contact_id)
    .bind(hash_token(&token))
    .bind(&origin)
    .bind(expires_at)
    .bind(visitor_id_hash)
    .bind(client_ip_address)
    .bind(client_ip.source.as_str())
    .bind(captured_headers.user_agent)
    .bind(captured_headers.accept_language)
    .bind(client_hints)
    .bind(client_context)
    .bind(visitor_data_expires_at)
    .bind(language.as_str())
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    let visitor_event = (!already_online).then(|| RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: widget.tenant_id,
        project_id: widget.project_id,
        inbox_id: widget.inbox_id,
        contact_id: Some(contact_id),
        event_type: "visitor.entered".to_owned(),
        aggregate_id: session_id,
        sequence: None,
        occurred_at: now,
        data: json!({
            "session_id": session_id,
            "channel_id": widget.channel_connection_id,
            "widget_name": &widget.name,
            "page_url": page_url,
            "page_title": page_title,
            "notify": widget.notify_on_new_visitor,
        }),
    });
    if visitor_event.is_some() {
        let page = if page_title.trim().is_empty() {
            page_url.as_str()
        } else {
            page_title.as_str()
        };
        let routing_configured = inbox_routing::on_new_visitor(
            &mut transaction,
            widget.tenant_id,
            widget.project_id,
            widget.inbox_id,
            widget.channel_connection_id,
            &widget.name,
            page,
        )
        .await?;
        if !routing_configured {
            let text = format!(
                "Новый посетитель сайта.\nВиджет: {}\nСтраница: {page}",
                widget.name
            )
            .chars()
            .take(4_000)
            .collect();
            telegram_notifications::enqueue(
                &mut transaction,
                TelegramNotificationPayload {
                    tenant_id: widget.tenant_id,
                    project_id: widget.project_id,
                    inbox_id: widget.inbox_id,
                    channel_connection_id: widget.channel_connection_id,
                    kind: TelegramNotificationKind::NewVisitor,
                    text,
                },
            )
            .await
            .map_err(AppError::internal)?;
        }
    }
    transaction.commit().await?;
    if let Some(event) = &visitor_event {
        publish_best_effort(&state, event).await;
    }
    let operators_online = inbox_has_online_support(
        &state.db,
        widget.tenant_id,
        widget.project_id,
        widget.inbox_id,
        widget.channel_connection_id,
        language.as_str(),
    )
    .await?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin_header);
    response_headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    Ok((
        response_headers,
        Json(WidgetSessionResponse {
            session_id,
            token,
            inbox_id: widget.inbox_id,
            language,
            support_name,
            greeting,
            offline_message,
            rating_prompt,
            rating_thanks,
            proactive_invitation_message,
            online_now,
            offline_now,
            message_placeholder,
            contact_title,
            contact_description,
            contact_name,
            contact_name_placeholder,
            contact_email,
            contact_email_placeholder,
            contact_save,
            contact_saving,
            contact_skip,
            contact_error,
            launcher,
            theme: widget.theme.0,
            operators_online,
            attachments_enabled: widget.attachments_enabled && state.config.attachments.enabled,
            contact,
            expires_at,
        }),
    ))
}

async fn update_widget_contact(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Json(request): Json<UpdateWidgetContactRequest>,
) -> Result<Json<WidgetContactProfile>, AppError> {
    let display_name = normalize_widget_contact_name(request.display_name)?;
    let email = normalize_widget_contact_email(request.email)?;
    let now = Utc::now();
    let mut transaction = state.db.begin().await?;
    let contact = sqlx::query_as::<_, WidgetContactProfile>(
        r#"
        UPDATE contacts
        SET display_name = $4, email = $5, updated_at = $6
        WHERE tenant_id = $1 AND project_id = $2 AND id = $3
        RETURNING display_name, email
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.contact_id)
    .bind(display_name)
    .bind(email)
    .bind(now)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: session.tenant_id,
        project_id: session.project_id,
        inbox_id: session.inbox_id,
        contact_id: Some(session.contact_id),
        event_type: "contact.updated".to_owned(),
        aggregate_id: session.contact_id,
        sequence: None,
        occurred_at: now,
        data: json!({"contact_id": session.contact_id}),
    };
    insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    publish_best_effort(&state, &event).await;

    Ok(Json(contact))
}

async fn update_widget_presence(
    State(state): State<AppState>,
    session: WidgetSessionContext,
) -> Result<Json<WidgetAvailabilityResponse>, AppError> {
    let updated = sqlx::query(
        r#"
        UPDATE widget_sessions
        SET presence_last_seen_at = now()
        WHERE tenant_id = $1 AND id = $2
          AND expires_at > now() AND revoked_at IS NULL
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.session_id)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Unauthorized);
    }
    let operators_online = inbox_has_online_support(
        &state.db,
        session.tenant_id,
        session.project_id,
        session.inbox_id,
        session.channel_connection_id,
        &session.language,
    )
    .await?;
    Ok(Json(WidgetAvailabilityResponse { operators_online }))
}

#[derive(Debug, FromRow)]
struct WidgetPresentationRow {
    translations: SqlJson<WidgetTranslations>,
    launcher: SqlJson<WidgetLauncher>,
    theme: SqlJson<WidgetTheme>,
    attachments_enabled: bool,
}

#[derive(Debug, Serialize)]
struct WidgetPresentationResponse {
    language: WidgetLanguage,
    support_name: String,
    greeting: Option<String>,
    offline_message: String,
    rating_prompt: String,
    rating_thanks: String,
    proactive_invitation_message: String,
    online_now: String,
    offline_now: String,
    message_placeholder: String,
    contact_title: String,
    contact_description: String,
    contact_name: String,
    contact_name_placeholder: String,
    contact_email: String,
    contact_email_placeholder: String,
    contact_save: String,
    contact_saving: String,
    contact_skip: String,
    contact_error: String,
    launcher: WidgetLauncher,
    theme: WidgetTheme,
    operators_online: bool,
    attachments_enabled: bool,
}

async fn get_widget_presentation(
    State(state): State<AppState>,
    session: WidgetSessionContext,
) -> Result<Json<WidgetPresentationResponse>, AppError> {
    let presentation = sqlx::query_as::<_, WidgetPresentationRow>(
        r#"
        SELECT translations, launcher, theme, attachments_enabled
        FROM widget_configs
        WHERE tenant_id = $1 AND channel_connection_id = $2
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.channel_connection_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let language = WidgetLanguage::parse(&session.language)?;
    let translation = presentation.translations.0.get(&language)?;
    let mut launcher = presentation.launcher.0;
    launcher.label.clone_from(&translation.launcher_label);
    let operators_online = inbox_has_online_support(
        &state.db,
        session.tenant_id,
        session.project_id,
        session.inbox_id,
        session.channel_connection_id,
        language.as_str(),
    )
    .await?;

    Ok(Json(WidgetPresentationResponse {
        language,
        support_name: translation.support_name.clone(),
        greeting: translation.greeting.clone(),
        offline_message: translation.offline_message.clone(),
        rating_prompt: translation.rating_prompt.clone(),
        rating_thanks: translation.rating_thanks.clone(),
        proactive_invitation_message: translation.proactive_invitation_message.clone(),
        online_now: translation.online_now.clone(),
        offline_now: translation.offline_now.clone(),
        message_placeholder: translation.message_placeholder.clone(),
        contact_title: translation.contact_title.clone(),
        contact_description: translation.contact_description.clone(),
        contact_name: translation.contact_name.clone(),
        contact_name_placeholder: translation.contact_name_placeholder.clone(),
        contact_email: translation.contact_email.clone(),
        contact_email_placeholder: translation.contact_email_placeholder.clone(),
        contact_save: translation.contact_save.clone(),
        contact_saving: translation.contact_saving.clone(),
        contact_skip: translation.contact_skip.clone(),
        contact_error: translation.contact_error.clone(),
        launcher,
        theme: presentation.theme.0,
        operators_online,
        attachments_enabled: presentation.attachments_enabled && state.config.attachments.enabled,
    }))
}

#[derive(Debug, Deserialize, Default)]
struct CreateConversationRequest {
    subject: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ConversationOperatorResponse {
    display_name: String,
    avatar_url: Option<String>,
    joined_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
struct ConversationAiAgentResponse {
    display_name: String,
    avatar_url: Option<String>,
    joined_at: DateTime<Utc>,
    active: bool,
}

#[derive(Debug, FromRow)]
struct ConversationRow {
    id: Uuid,
    inbox_id: Uuid,
    contact_id: Uuid,
    contact_display_name: Option<String>,
    contact_email: Option<String>,
    contact_is_blocked: bool,
    status: String,
    subject: Option<String>,
    last_message_sequence: i64,
    unread_customer_messages: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    widget_attachments_enabled: bool,
    operator_user_id: Option<Uuid>,
    operator_display_name: Option<String>,
    operator_avatar_url: Option<String>,
    operator_joined_at: Option<DateTime<Utc>>,
    ai_profile_id: Option<Uuid>,
    ai_display_name: Option<String>,
    ai_avatar_url: Option<String>,
    ai_joined_at: Option<DateTime<Utc>>,
    ai_left_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct ConversationResponse {
    id: Uuid,
    inbox_id: Uuid,
    contact_id: Uuid,
    status: String,
    subject: Option<String>,
    last_message_sequence: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    widget_attachments_enabled: bool,
    operator: Option<ConversationOperatorResponse>,
    ai_agent: Option<ConversationAiAgentResponse>,
}

impl ConversationRow {
    fn operator(&self) -> Option<ConversationOperatorResponse> {
        self.operator_user_id
            .zip(self.operator_joined_at)
            .map(|(_, joined_at)| ConversationOperatorResponse {
                display_name: self
                    .operator_display_name
                    .clone()
                    .unwrap_or_else(|| "Operator".to_owned()),
                avatar_url: self.operator_avatar_url.clone(),
                joined_at,
            })
    }

    fn ai_agent(&self) -> Option<ConversationAiAgentResponse> {
        self.ai_profile_id
            .zip(self.ai_joined_at)
            .map(|(_, joined_at)| ConversationAiAgentResponse {
                display_name: self
                    .ai_display_name
                    .clone()
                    .unwrap_or_else(|| "Operator".to_owned()),
                avatar_url: self.ai_avatar_url.clone(),
                joined_at,
                active: self.ai_left_at.is_none(),
            })
    }
}

impl From<ConversationRow> for ConversationResponse {
    fn from(row: ConversationRow) -> Self {
        let operator = row.operator();
        let ai_agent = row.ai_agent();
        Self {
            id: row.id,
            inbox_id: row.inbox_id,
            contact_id: row.contact_id,
            status: row.status,
            subject: row.subject,
            last_message_sequence: row.last_message_sequence,
            created_at: row.created_at,
            updated_at: row.updated_at,
            widget_attachments_enabled: row.widget_attachments_enabled,
            operator,
            ai_agent,
        }
    }
}

impl ConversationResponse {
    fn apply_attachment_runtime_gate(mut self, attachments_available: bool) -> Self {
        self.widget_attachments_enabled &= attachments_available;
        self
    }
}

async fn create_widget_conversation(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    payload: Option<Json<CreateConversationRequest>>,
) -> Result<Json<ConversationResponse>, AppError> {
    let request = payload.map_or_else(CreateConversationRequest::default, |Json(value)| value);
    ensure_widget_conversation(&state, &session, request)
        .await
        .map(Json)
}

#[derive(Debug, Serialize)]
struct StartVisitorConversationResponse {
    id: Uuid,
    inbox_id: Uuid,
}

async fn start_visitor_conversation(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(session_id): Path<Uuid>,
) -> Result<Json<StartVisitorConversationResponse>, AppError> {
    actor.require("visitor_network:read")?;
    actor.require("conversations:read")?;
    actor.require("conversations:reply")?;
    let session = sqlx::query_as::<_, WidgetSessionContext>(
        r#"
        SELECT session.id AS session_id, session.tenant_id, session.project_id,
               session.inbox_id, session.channel_connection_id, session.contact_id,
               session.language, host(session.client_ip) AS client_ip
        FROM widget_sessions AS session
        JOIN channel_connections AS channel
          ON channel.tenant_id = session.tenant_id
         AND channel.id = session.channel_connection_id
        WHERE session.tenant_id = $1 AND session.id = $2
          AND session.expires_at > now() AND session.revoked_at IS NULL
          AND session.presence_last_seen_at >= $3
          AND channel.kind = 'widget' AND channel.status = 'active'
          AND channel.deleted_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(session_id)
    .bind(Utc::now() - chrono::Duration::seconds(ONLINE_PRESENCE_WINDOW_SECONDS))
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(session.project_id, session.inbox_id)?;

    let conversation =
        ensure_widget_conversation(&state, &session, CreateConversationRequest::default()).await?;
    let assigned = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM conversation_assignments
            WHERE tenant_id = $1 AND conversation_id = $2
              AND user_id IS NOT NULL AND unassigned_at IS NULL
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(conversation.id)
    .fetch_one(&state.db)
    .await?;
    if !assigned {
        let _ = join_conversation(State(state.clone()), actor, Path(conversation.id)).await?;
    }
    Ok(Json(StartVisitorConversationResponse {
        id: conversation.id,
        inbox_id: conversation.inbox_id,
    }))
}

async fn ensure_widget_conversation(
    state: &AppState,
    session: &WidgetSessionContext,
    request: CreateConversationRequest,
) -> Result<ConversationResponse, AppError> {
    let mut transaction = state.db.begin().await?;
    // Widget initialization and an operator's first contact must share one conversation.
    sqlx::query("SELECT id FROM contacts WHERE tenant_id = $1 AND id = $2 FOR UPDATE")
        .bind(session.tenant_id)
        .bind(session.contact_id)
        .fetch_one(&mut *transaction)
        .await?;
    if let Some(existing) = sqlx::query_as::<_, ConversationRow>(
        r#"
        SELECT conversation.id, conversation.inbox_id, conversation.contact_id,
               NULL::text AS contact_display_name,
               NULL::text AS contact_email,
               false AS contact_is_blocked,
               conversation.status, conversation.subject,
               conversation.last_message_sequence,
               NULL::bigint AS unread_customer_messages,
               conversation.created_at,
               conversation.updated_at,
               COALESCE(conversation.widget_attachments_enabled, widget.attachments_enabled)
                   AS widget_attachments_enabled,
               assignment.user_id AS operator_user_id,
               COALESCE(profile.display_name, app_user.display_name) AS operator_display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS operator_avatar_url,
               assignment.assigned_at AS operator_joined_at,
               ai_participant.ai_profile_id,
               COALESCE(
                   ai_identity.display_name,
                   NULLIF(
                       btrim(widget.translations -> conversation.widget_language ->> 'support_name'),
                       ''
                   ),
                   'Support'
               ) AS ai_display_name,
               '/public/v1/avatars/' || ai_identity.avatar_public_id::text AS ai_avatar_url,
               ai_participant.joined_at AS ai_joined_at,
               ai_participant.left_at AS ai_left_at
        FROM conversations AS conversation
        JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        LEFT JOIN LATERAL (
            SELECT active_assignment.user_id, active_assignment.assigned_at
            FROM conversation_assignments AS active_assignment
            WHERE active_assignment.tenant_id = conversation.tenant_id
              AND active_assignment.conversation_id = conversation.id
              AND active_assignment.user_id IS NOT NULL
              AND active_assignment.unassigned_at IS NULL
            ORDER BY active_assignment.assigned_at DESC, active_assignment.id DESC
            LIMIT 1
        ) AS assignment ON CASE
            WHEN jsonb_typeof(widget.launcher->'show_operator_profile') = 'boolean'
                THEN (widget.launcher->>'show_operator_profile')::boolean
            ELSE false
        END
        LEFT JOIN users AS app_user ON app_user.id = assignment.user_id
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = conversation.tenant_id
         AND profile.project_id = conversation.project_id
         AND profile.user_id = assignment.user_id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = conversation.tenant_id
         AND stored_avatar.project_id = conversation.project_id
         AND stored_avatar.user_id = assignment.user_id
        LEFT JOIN LATERAL (
            SELECT participant.ai_profile_id,
                   GREATEST(participant.joined_at, first_contact_message.created_at) AS joined_at,
                   participant.left_at
            FROM conversation_participants AS participant
            JOIN LATERAL (
                SELECT contact_message.created_at
                FROM messages AS contact_message
                WHERE contact_message.tenant_id = participant.tenant_id
                  AND contact_message.conversation_id = participant.conversation_id
                  AND contact_message.author_kind = 'contact'
                  AND contact_message.kind = 'text'
                  AND contact_message.created_at <= COALESCE(
                      participant.left_at,
                      'infinity'::timestamptz
                  )
                ORDER BY contact_message.sequence
                LIMIT 1
            ) AS first_contact_message ON true
            WHERE participant.tenant_id = conversation.tenant_id
              AND participant.conversation_id = conversation.id
              AND participant.participant_kind = 'ai'
              AND participant.ai_profile_id IS NOT NULL
              AND participant.joined_at <= COALESCE(participant.left_at, now())
            ORDER BY participant.joined_at DESC, participant.id DESC
            LIMIT 1
        ) AS ai_participant ON true
        LEFT JOIN ai_profiles AS ai_profile
          ON ai_profile.tenant_id = conversation.tenant_id
         AND ai_profile.project_id = conversation.project_id
         AND ai_profile.id = ai_participant.ai_profile_id
        LEFT JOIN LATERAL (
            SELECT identity.display_name, identity_avatar.public_id AS avatar_public_id
            FROM ai_profile_public_identities AS identity
            LEFT JOIN ai_profile_public_identity_avatars AS identity_avatar
              ON identity_avatar.tenant_id = identity.tenant_id
             AND identity_avatar.ai_profile_id = identity.ai_profile_id
             AND identity_avatar.language = identity.language
            WHERE identity.tenant_id = ai_profile.tenant_id
              AND identity.ai_profile_id = ai_profile.id
              AND (
                  identity.language = lower(replace($6, '_', '-'))
                  OR (
                      split_part(identity.language, '-', 1)
                          = split_part(lower(replace($6, '_', '-')), '-', 1)
                      AND (
                          position('-' IN identity.language) = 0
                          OR position('-' IN lower(replace($6, '_', '-'))) = 0
                      )
                  )
              )
            ORDER BY
                (identity.language = lower(replace($6, '_', '-'))) DESC,
                identity.language
            LIMIT 1
        ) AS ai_identity ON true
        WHERE conversation.tenant_id = $1 AND conversation.project_id = $2
          AND conversation.inbox_id = $3 AND conversation.contact_id = $4
          AND conversation.channel_connection_id = $5
          AND conversation.status <> 'resolved'
        ORDER BY conversation.created_at DESC
        LIMIT 1
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(session.contact_id)
    .bind(session.channel_connection_id)
    .bind(&session.language)
    .fetch_optional(&mut *transaction)
    .await?
    {
        transaction.commit().await?;
        return Ok(
            ConversationResponse::from(existing)
                .apply_attachment_runtime_gate(state.config.attachments.enabled),
        );
    }

    let subject = normalize_optional_text(request.subject, 200)?;
    let id = Uuid::now_v7();
    let now = Utc::now();
    let conversation = sqlx::query_as::<_, ConversationRow>(
        r#"
        INSERT INTO conversations (
            id, tenant_id, project_id, inbox_id, channel_connection_id,
            contact_id, status, subject, widget_language, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, 'new', $7, $8, $9, $9)
        RETURNING id, inbox_id, contact_id,
                  NULL::text AS contact_display_name,
                  NULL::text AS contact_email,
                  false AS contact_is_blocked,
                  status, subject,
                  last_message_sequence,
                  NULL::bigint AS unread_customer_messages,
                  created_at, updated_at,
                  (SELECT attachments_enabled
                   FROM widget_configs
                   WHERE tenant_id = $2 AND channel_connection_id = $5)
                      AS widget_attachments_enabled,
                  NULL::uuid AS operator_user_id,
                  NULL::text AS operator_display_name,
                  NULL::text AS operator_avatar_url,
                  NULL::timestamptz AS operator_joined_at,
                  NULL::uuid AS ai_profile_id,
                  NULL::text AS ai_display_name,
                  NULL::text AS ai_avatar_url,
                  NULL::timestamptz AS ai_joined_at,
                  NULL::timestamptz AS ai_left_at
        "#,
    )
    .bind(id)
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(session.channel_connection_id)
    .bind(session.contact_id)
    .bind(subject)
    .bind(&session.language)
    .bind(now)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO conversation_participants (
            id, tenant_id, conversation_id, participant_kind, contact_id
        ) VALUES ($1, $2, $3, 'contact', $4)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(session.tenant_id)
    .bind(id)
    .bind(session.contact_id)
    .execute(&mut *transaction)
    .await?;

    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: session.tenant_id,
        project_id: session.project_id,
        inbox_id: session.inbox_id,
        contact_id: Some(session.contact_id),
        event_type: "conversation.created".to_owned(),
        aggregate_id: id,
        sequence: None,
        occurred_at: now,
        data: json!({"conversation_id": id}),
    };
    insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    publish_best_effort(state, &event).await;

    Ok(ConversationResponse::from(conversation)
        .apply_attachment_runtime_gate(state.config.attachments.enabled))
}

#[derive(Debug, Deserialize)]
struct CreateMessageRequest {
    client_message_id: Uuid,
    body: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum MessageBodyFormat {
    #[default]
    Plain,
    Markdown,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum OperatorMessageSender {
    #[default]
    Operator,
    Ai,
    /// An administrator writes as the operator who handles the conversation.
    AssignedOperator,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateOperatorMessageRequest {
    client_message_id: Uuid,
    body: String,
    #[serde(default)]
    body_format: MessageBodyFormat,
    #[serde(default)]
    send_as: OperatorMessageSender,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateWidgetDraftRequest {
    body: String,
}

async fn update_widget_draft(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Path(conversation_id): Path<Uuid>,
    Json(request): Json<UpdateWidgetDraftRequest>,
) -> Result<StatusCode, AppError> {
    if request.body.chars().count() > MAX_MESSAGE_LENGTH {
        return Err(AppError::BadRequest(format!(
            "body must be at most {MAX_MESSAGE_LENGTH} characters"
        )));
    }

    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM conversations
            WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
              AND contact_id = $4 AND id = $5
        )
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(session.contact_id)
    .bind(conversation_id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }

    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: session.tenant_id,
        project_id: session.project_id,
        inbox_id: session.inbox_id,
        contact_id: Some(session.contact_id),
        event_type: "draft.updated".to_owned(),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: Utc::now(),
        data: json!({
            "conversation_id": conversation_id,
            "body": request.body,
        }),
    };
    publish_best_effort(&state, &event).await;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, FromRow, Serialize)]
pub(crate) struct MessageResponse {
    id: Uuid,
    conversation_id: Uuid,
    sequence: i64,
    direction: String,
    kind: String,
    author_kind: String,
    body: String,
    body_format: MessageBodyFormat,
    status: String,
    created_at: DateTime<Utc>,
    #[sqlx(default)]
    attachments: SqlJson<Vec<AttachmentResponse>>,
    #[sqlx(default)]
    is_voice_message: bool,
}

#[derive(Clone, Copy)]
enum MessageAuthor {
    Contact(Uuid),
    Operator(Uuid),
    Ai(Uuid),
}

impl MessageAuthor {
    const fn direction(self) -> &'static str {
        match self {
            Self::Contact(_) => "inbound",
            Self::Operator(_) | Self::Ai(_) => "outbound",
        }
    }

    const fn kind(self) -> &'static str {
        "text"
    }

    const fn author_kind(self) -> &'static str {
        match self {
            Self::Contact(_) => "contact",
            Self::Operator(_) => "operator",
            Self::Ai(_) => "ai",
        }
    }

    const fn author_id(self) -> Uuid {
        match self {
            Self::Contact(id) | Self::Operator(id) | Self::Ai(id) => id,
        }
    }

    const fn is_contact(self) -> bool {
        matches!(self, Self::Contact(_))
    }
}

#[derive(Clone, Copy)]
enum MessageAuthorSelection {
    Contact(Uuid),
    Operator(Uuid),
    ActiveAi { administrator_id: Uuid },
    AssignedOperator { administrator_id: Uuid },
}

async fn auto_join_ai_on_contact_message(
    transaction: &mut Transaction<'_, Postgres>,
    scope: &ConversationScope,
    conversation_id: Uuid,
    widget_language: Option<&str>,
    joined_at: DateTime<Utc>,
) -> Result<bool, AppError> {
    let ai_profiles = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT profile.id, profile.language
        FROM ai_profile_channel_connections AS assignment
        JOIN ai_profiles AS profile
          ON profile.tenant_id = assignment.tenant_id
         AND profile.id = assignment.ai_profile_id
        JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
        JOIN channel_connections AS connection
          ON connection.tenant_id = assignment.tenant_id
         AND connection.id = assignment.channel_connection_id
        JOIN inboxes AS inbox
          ON inbox.tenant_id = connection.tenant_id
         AND inbox.project_id = connection.project_id
         AND inbox.id = connection.inbox_id
        WHERE assignment.tenant_id = $1
          AND assignment.channel_connection_id = $5
          AND profile.project_id = $3
          AND connection.project_id = $3
          AND connection.inbox_id = $2
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
          AND inbox.status = 'active'
          AND profile.status = 'active'
          AND profile.auto_join_new_conversations
          AND provider.status = 'active'
          AND provider.provider_kind IN ('openai', 'openai_compatible')
          AND NOT EXISTS (
              SELECT 1
              FROM conversation_participants AS active_ai
              WHERE active_ai.tenant_id = $1
                AND active_ai.conversation_id = $4
                AND active_ai.participant_kind = 'ai'
                AND active_ai.left_at IS NULL
          )
          AND NOT EXISTS (
              SELECT 1
              FROM conversation_assignments AS active_assignment
              WHERE active_assignment.tenant_id = $1
                AND active_assignment.conversation_id = $4
                AND active_assignment.user_id IS NOT NULL
                AND active_assignment.unassigned_at IS NULL
        )
        ORDER BY assignment.created_at, profile.id
        FOR SHARE OF assignment, connection, inbox, profile, provider
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.inbox_id)
    .bind(scope.project_id)
    .bind(conversation_id)
    .bind(scope.channel_connection_id)
    .fetch_all(&mut **transaction)
    .await?;
    let Some(ai_profile_id) = first_compatible_ai_profile(ai_profiles, widget_language) else {
        return Ok(false);
    };

    let inserted = sqlx::query(
        r#"
        INSERT INTO conversation_participants (
            id, tenant_id, conversation_id, participant_kind, ai_profile_id, joined_at
        ) VALUES ($1, $2, $3, 'ai', $4, $5)
        ON CONFLICT (tenant_id, conversation_id)
            WHERE participant_kind = 'ai' AND left_at IS NULL
        DO UPDATE SET ai_profile_id = EXCLUDED.ai_profile_id,
                      joined_at = EXCLUDED.joined_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(ai_profile_id)
    .bind(joined_at)
    .execute(&mut **transaction)
    .await?;

    Ok(inserted.rows_affected() == 1)
}

fn first_compatible_ai_profile(
    ai_profiles: Vec<(Uuid, String)>,
    widget_language: Option<&str>,
) -> Option<Uuid> {
    ai_profiles
        .into_iter()
        .find(|(_, profile_languages)| {
            provider_reply::profile_supports_widget_language(profile_languages, widget_language)
        })
        .map(|(profile_id, _)| profile_id)
}

async fn create_widget_message(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Path(conversation_id): Path<Uuid>,
    Json(request): Json<CreateMessageRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let scope = ConversationScope {
        tenant_id: session.tenant_id,
        project_id: session.project_id,
        inbox_id: session.inbox_id,
        contact_id: session.contact_id,
        channel_connection_id: session.channel_connection_id,
    };
    create_message(
        &state,
        scope,
        conversation_id,
        MessageAuthorSelection::Contact(session.contact_id),
        Some(session.language),
        MessageBodyFormat::Plain,
        request,
    )
    .await
    .map(Json)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_channel_contact_message(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    channel_connection_id: Uuid,
    contact_id: Uuid,
    conversation_id: Uuid,
    client_message_id: Uuid,
    body: String,
    language: Option<String>,
) -> Result<(), AppError> {
    create_message(
        state,
        ConversationScope {
            tenant_id,
            project_id,
            inbox_id,
            contact_id,
            channel_connection_id,
        },
        conversation_id,
        MessageAuthorSelection::Contact(contact_id),
        language,
        MessageBodyFormat::Plain,
        CreateMessageRequest {
            client_message_id,
            body,
        },
    )
    .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
struct ConversationScope {
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    contact_id: Uuid,
    channel_connection_id: Uuid,
}

#[derive(Debug, FromRow)]
struct ReplySuggestionConversationState {
    status: String,
    widget_language: Option<String>,
    last_message_sequence: i64,
    assigned_to_actor: bool,
    can_reply_as_ai: bool,
}

#[derive(Debug, Serialize)]
struct ReplySuggestionAgentResponse {
    id: Uuid,
    name: String,
    avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReplySuggestionAgentListResponse {
    items: Vec<ReplySuggestionAgentResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerateReplySuggestionRequest {
    ai_profile_id: Uuid,
    draft_body: Option<String>,
}

#[derive(Debug, Serialize)]
struct GenerateReplySuggestionResponse {
    body: String,
    body_format: MessageBodyFormat,
}

async fn load_reply_suggestion_conversation_state(
    state: &AppState,
    scope: &ConversationScope,
    conversation_id: Uuid,
    actor: &ActorContext,
) -> Result<ReplySuggestionConversationState, AppError> {
    let conversation = sqlx::query_as::<_, ReplySuggestionConversationState>(
        r#"
        SELECT conversation.status, conversation.widget_language,
               conversation.last_message_sequence,
               EXISTS (
                   SELECT 1
                   FROM conversation_assignments AS assignment
                   WHERE assignment.tenant_id = conversation.tenant_id
                     AND assignment.conversation_id = conversation.id
                     AND assignment.user_id = $7
                     AND assignment.unassigned_at IS NULL
               ) AS assigned_to_actor,
               EXISTS (
                   SELECT 1
                   FROM conversation_participants AS participant
                   JOIN ai_profiles AS profile
                     ON profile.tenant_id = participant.tenant_id
                    AND profile.project_id = conversation.project_id
                    AND profile.id = participant.ai_profile_id
                   JOIN ai_profile_channel_connections AS channel_assignment
                     ON channel_assignment.tenant_id = profile.tenant_id
                    AND channel_assignment.ai_profile_id = profile.id
                    AND channel_assignment.channel_connection_id = conversation.channel_connection_id
                   WHERE participant.tenant_id = conversation.tenant_id
                     AND participant.conversation_id = conversation.id
                     AND participant.participant_kind = 'ai'
                     AND participant.left_at IS NULL
                     AND participant.joined_at <= now()
                     AND profile.status = 'active'
               ) AND NOT EXISTS (
                   SELECT 1
                   FROM conversation_assignments AS assignment
                   WHERE assignment.tenant_id = conversation.tenant_id
                     AND assignment.conversation_id = conversation.id
                     AND assignment.user_id IS NOT NULL
                     AND assignment.unassigned_at IS NULL
               ) AS can_reply_as_ai
        FROM conversations AS conversation
        WHERE conversation.tenant_id = $1
          AND conversation.project_id = $2
          AND conversation.inbox_id = $3
          AND conversation.contact_id = $4
          AND conversation.channel_connection_id = $5
          AND conversation.id = $6
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(scope.contact_id)
    .bind(scope.channel_connection_id)
    .bind(conversation_id)
    .bind(actor.actor_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if conversation.status == "resolved" {
        return Err(AppError::Conflict(
            "resolved conversations cannot generate AI reply suggestions".to_owned(),
        ));
    }
    if !(conversation.assigned_to_actor
        || conversation.can_reply_as_ai && actor.require_session_admin().is_ok())
    {
        return Err(AppError::Conflict(
            "join the conversation before generating an AI reply suggestion".to_owned(),
        ));
    }
    Ok(conversation)
}

async fn list_reply_suggestion_agents(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
) -> Result<Json<ReplySuggestionAgentListResponse>, AppError> {
    actor.require("conversations:reply")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    crate::demo_access::require_conversation(&state.db, &actor, conversation_id).await?;
    let conversation =
        load_reply_suggestion_conversation_state(&state, &scope, conversation_id, &actor).await?;
    let items = provider_reply::list_reply_suggestion_agents(
        &state,
        scope.tenant_id,
        scope.project_id,
        scope.inbox_id,
        scope.channel_connection_id,
        conversation.widget_language.as_deref(),
    )
    .await?
    .into_iter()
    .map(|agent| ReplySuggestionAgentResponse {
        id: agent.id,
        name: agent.name,
        avatar_url: agent.avatar_url,
    })
    .collect();
    Ok(Json(ReplySuggestionAgentListResponse { items }))
}

async fn generate_reply_suggestion(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
    Json(request): Json<GenerateReplySuggestionRequest>,
) -> Result<Json<GenerateReplySuggestionResponse>, AppError> {
    if request
        .draft_body
        .as_ref()
        .is_some_and(|body| body.chars().count() > MAX_MESSAGE_LENGTH)
    {
        return Err(AppError::BadRequest(format!(
            "draft_body must be at most {MAX_MESSAGE_LENGTH} characters"
        )));
    }
    actor.require("conversations:reply")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    let conversation =
        load_reply_suggestion_conversation_state(&state, &scope, conversation_id, &actor).await?;
    let body = provider_reply::generate_reply_suggestion(
        &state,
        scope.tenant_id,
        scope.project_id,
        scope.inbox_id,
        scope.channel_connection_id,
        conversation_id,
        request.ai_profile_id,
        request.draft_body.as_deref(),
        conversation.widget_language.as_deref(),
        conversation.last_message_sequence,
        Uuid::now_v7(),
    )
    .await?;
    let current =
        load_reply_suggestion_conversation_state(&state, &scope, conversation_id, &actor).await?;
    if current.last_message_sequence != conversation.last_message_sequence {
        return Err(AppError::Conflict(
            "the conversation changed while the AI reply suggestion was generated".to_owned(),
        ));
    }
    Ok(Json(GenerateReplySuggestionResponse {
        body,
        body_format: MessageBodyFormat::Markdown,
    }))
}

#[derive(Debug, Serialize)]
struct JoinConversationResponse {
    operator: ConversationOperatorResponse,
    joined_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ReturnConversationToAiResponse {
    ai_agent: ConversationAiAgentResponse,
    returned_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct PublicOperatorRow {
    display_name: String,
    avatar_url: Option<String>,
}

async fn join_conversation(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
) -> Result<Json<JoinConversationResponse>, AppError> {
    join_as_actor(state, actor, conversation_id, false).await
}

/// Lets a session administrator join in place of the operator who handles the
/// conversation, the way joining replaces an active AI agent.
async fn take_over_conversation(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
) -> Result<Json<JoinConversationResponse>, AppError> {
    actor.require_session_admin()?;
    join_as_actor(state, actor, conversation_id, true).await
}

async fn join_as_actor(
    state: AppState,
    actor: ActorContext,
    conversation_id: Uuid,
    take_over: bool,
) -> Result<Json<JoinConversationResponse>, AppError> {
    actor.require("conversations:reply")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    crate::demo_access::require_conversation(&state.db, &actor, conversation_id).await?;

    let mut transaction = state.db.begin().await?;
    let status = sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM conversations
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3 AND id = $4
        FOR UPDATE
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(conversation_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if status == "resolved" {
        return Err(AppError::Conflict(
            "resolved conversations cannot be joined".to_owned(),
        ));
    }

    let existing_operator = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT user_id
        FROM conversation_assignments
        WHERE tenant_id = $1 AND conversation_id = $2
          AND user_id IS NOT NULL AND unassigned_at IS NULL
        ORDER BY assigned_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let replaced_operator = existing_operator.filter(|operator_id| *operator_id != actor.actor_id);
    if replaced_operator.is_some() && !take_over {
        return Err(AppError::Conflict(
            "another operator has already joined this conversation".to_owned(),
        ));
    }

    let profile = sqlx::query_as::<_, PublicOperatorRow>(
        r#"
        SELECT COALESCE(profile.display_name, app_user.display_name) AS display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS avatar_url
        FROM users AS app_user
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = $1
         AND profile.project_id = $2
         AND profile.user_id = app_user.id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = $1
         AND stored_avatar.project_id = $2
         AND stored_avatar.user_id = app_user.id
        WHERE app_user.id = $3 AND app_user.status = 'active'
          AND EXISTS (
              SELECT 1
              FROM memberships AS membership
              WHERE membership.tenant_id = $1
                AND (membership.project_id = $2 OR membership.project_id IS NULL)
                AND membership.user_id = app_user.id
                AND membership.revoked_at IS NULL
                AND (membership.department_id IS NULL OR membership.department_id = (
                    SELECT department_id FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND id = $4
                ))
          )
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(actor.actor_id)
    .bind(scope.inbox_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::Forbidden)?;
    let joined_at = if existing_operator == Some(actor.actor_id) {
        sqlx::query_scalar::<_, DateTime<Utc>>(
            r#"
            SELECT assigned_at
            FROM conversation_assignments
            WHERE tenant_id = $1 AND conversation_id = $2
              AND user_id = $3 AND unassigned_at IS NULL
            ORDER BY assigned_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .bind(actor.actor_id)
        .fetch_one(&mut *transaction)
        .await?
    } else {
        let now = Utc::now();
        if let Some(previous_operator_id) = replaced_operator {
            sqlx::query(
                r#"
                UPDATE conversation_assignments
                SET unassigned_at = $4
                WHERE tenant_id = $1 AND conversation_id = $2
                  AND user_id = $3 AND unassigned_at IS NULL
                "#,
            )
            .bind(scope.tenant_id)
            .bind(conversation_id)
            .bind(previous_operator_id)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                r#"
                UPDATE conversation_participants
                SET left_at = $4
                WHERE tenant_id = $1 AND conversation_id = $2
                  AND participant_kind = 'operator' AND user_id = $3 AND left_at IS NULL
                "#,
            )
            .bind(scope.tenant_id)
            .bind(conversation_id)
            .bind(previous_operator_id)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO audit_log (
                    id, tenant_id, project_id, actor_id, action,
                    resource_kind, resource_id, metadata, occurred_at
                ) VALUES (
                    $1, $2, $3, $4, 'conversation.taken_over_by_admin',
                    'conversation', $5, $6, $7
                )
                "#,
            )
            .bind(Uuid::now_v7())
            .bind(scope.tenant_id)
            .bind(scope.project_id)
            .bind(actor.actor_id)
            .bind(conversation_id)
            .bind(json!({ "previous_operator_id": previous_operator_id }))
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            r#"
            INSERT INTO conversation_assignments (
                id, tenant_id, conversation_id, user_id, assigned_by, assigned_at
            ) VALUES ($1, $2, $3, $4, $4, $5)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .bind(actor.actor_id)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        provider_reply::cancel_pending(&mut transaction, scope.tenant_id, conversation_id).await?;
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
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .bind(actor.actor_id)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE conversation_participants
            SET left_at = $3
            WHERE tenant_id = $1 AND conversation_id = $2
              AND participant_kind = 'ai' AND left_at IS NULL
            "#,
        )
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE conversations
            SET status = CASE WHEN status = 'new' THEN 'open' ELSE status END,
                updated_at = $3, version = version + 1
            WHERE tenant_id = $1 AND id = $2
            "#,
        )
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .bind(now)
        .execute(&mut *transaction)
        .await?;

        let event = RealtimeEvent {
            event_id: Uuid::now_v7(),
            tenant_id: scope.tenant_id,
            project_id: scope.project_id,
            inbox_id: scope.inbox_id,
            contact_id: Some(scope.contact_id),
            event_type: "conversation.operator_joined".to_owned(),
            aggregate_id: conversation_id,
            sequence: None,
            occurred_at: now,
            data: json!({
                "conversation_id": conversation_id,
            }),
        };
        insert_outbox(&mut transaction, &event).await?;
        transaction.commit().await?;
        publish_best_effort(&state, &event).await;
        return Ok(Json(JoinConversationResponse {
            operator: ConversationOperatorResponse {
                display_name: profile.display_name,
                avatar_url: profile.avatar_url,
                joined_at: now,
            },
            joined_at: now,
        }));
    };

    transaction.commit().await?;
    Ok(Json(JoinConversationResponse {
        operator: ConversationOperatorResponse {
            display_name: profile.display_name,
            avatar_url: profile.avatar_url,
            joined_at,
        },
        joined_at,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateConversationAttachmentPolicyRequest {
    enabled: bool,
}

#[derive(Debug, Serialize)]
struct ConversationAttachmentPolicyResponse {
    attachments_enabled: bool,
}

async fn update_conversation_attachment_policy(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
    Json(request): Json<UpdateConversationAttachmentPolicyRequest>,
) -> Result<Json<ConversationAttachmentPolicyResponse>, AppError> {
    actor.require("conversations:reply")?;
    actor.require_password_session()?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    if request.enabled && !state.config.attachments.enabled {
        return Err(AppError::ServiceUnavailable(
            "secure file uploads are not enabled".to_owned(),
        ));
    }

    let mut transaction = state.db.begin().await?;
    let status = sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM conversations
        WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3 AND id = $4
        FOR UPDATE
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(conversation_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if status == "resolved" {
        return Err(AppError::Conflict(
            "file uploads cannot be changed for a resolved conversation".to_owned(),
        ));
    }
    let assigned = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM conversation_assignments
            WHERE tenant_id = $1 AND conversation_id = $2
              AND user_id = $3 AND unassigned_at IS NULL
        )
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(actor.actor_id)
    .fetch_one(&mut *transaction)
    .await?;
    if !assigned {
        return Err(AppError::Conflict(
            "join the conversation before changing file uploads".to_owned(),
        ));
    }

    let now = Utc::now();
    sqlx::query(
        r#"
        UPDATE conversations
        SET widget_attachments_enabled = $3, updated_at = $4, version = version + 1
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(request.enabled)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata, occurred_at
        ) VALUES (
            $1, $2, $3, $4, 'conversation.widget_attachments.updated',
            'conversation', $5, jsonb_build_object('enabled', $6::boolean), $7
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(actor.actor_id)
    .bind(conversation_id)
    .bind(request.enabled)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: scope.tenant_id,
        project_id: scope.project_id,
        inbox_id: scope.inbox_id,
        contact_id: Some(scope.contact_id),
        event_type: "conversation.attachment_policy_changed".to_owned(),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: now,
        data: json!({
            "conversation_id": conversation_id,
            "attachments_enabled": request.enabled,
        }),
    };
    insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    publish_best_effort(&state, &event).await;

    Ok(Json(ConversationAttachmentPolicyResponse {
        attachments_enabled: request.enabled,
    }))
}

async fn create_operator_message(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
    Json(request): Json<CreateOperatorMessageRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    actor.require("conversations:reply")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    crate::demo_access::require_conversation(&state.db, &actor, conversation_id).await?;
    let author = match request.send_as {
        OperatorMessageSender::Operator => {
            let has_user_identity = actor.is_password_session()
                || sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND status = 'active')",
                )
                .bind(actor.actor_id)
                .fetch_one(&state.db)
                .await?;
            if has_user_identity {
                let joined = sqlx::query_scalar::<_, bool>(
                    r#"
                    SELECT EXISTS(
                        SELECT 1
                        FROM conversation_assignments
                        WHERE tenant_id = $1 AND conversation_id = $2
                          AND user_id = $3 AND unassigned_at IS NULL
                    )
                    "#,
                )
                .bind(actor.tenant_id)
                .bind(conversation_id)
                .bind(actor.actor_id)
                .fetch_one(&state.db)
                .await?;
                if !joined {
                    return Err(AppError::Conflict(
                        "join the conversation before replying".to_owned(),
                    ));
                }
            }
            MessageAuthorSelection::Operator(actor.actor_id)
        }
        OperatorMessageSender::Ai => {
            actor.require_session_admin()?;
            MessageAuthorSelection::ActiveAi {
                administrator_id: actor.actor_id,
            }
        }
        OperatorMessageSender::AssignedOperator => {
            actor.require_session_admin()?;
            MessageAuthorSelection::AssignedOperator {
                administrator_id: actor.actor_id,
            }
        }
    };
    create_message(
        &state,
        scope,
        conversation_id,
        author,
        None,
        request.body_format,
        CreateMessageRequest {
            client_message_id: request.client_message_id,
            body: request.body,
        },
    )
    .await
    .map(Json)
}

#[allow(clippy::too_many_arguments)]
async fn create_message(
    state: &AppState,
    scope: ConversationScope,
    conversation_id: Uuid,
    author: MessageAuthorSelection,
    widget_language: Option<String>,
    body_format: MessageBodyFormat,
    request: CreateMessageRequest,
) -> Result<MessageResponse, AppError> {
    let body = normalize_required_text(request.body, MAX_MESSAGE_LENGTH)?;
    let mut transaction = state.db.begin().await?;
    let contact_is_blocked = lock_contact_block_status(&mut transaction, &scope).await?;
    let (status, channel_kind, channel_status, channel_deleted_at, last_message_sequence) =
        sqlx::query_as::<_, (String, String, String, Option<DateTime<Utc>>, i64)>(
            r#"
        SELECT conversation.status, channel.kind, channel.status, channel.deleted_at,
               conversation.last_message_sequence
        FROM conversations AS conversation
        JOIN channel_connections AS channel
          ON channel.tenant_id = conversation.tenant_id
         AND channel.id = conversation.channel_connection_id
        WHERE conversation.tenant_id = $1 AND conversation.project_id = $2
          AND conversation.inbox_id = $3 AND conversation.contact_id = $4
          AND conversation.id = $5 AND conversation.channel_connection_id = $6
        FOR UPDATE OF conversation, channel
        "#,
        )
        .bind(scope.tenant_id)
        .bind(scope.project_id)
        .bind(scope.inbox_id)
        .bind(scope.contact_id)
        .bind(conversation_id)
        .bind(scope.channel_connection_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound)?;
    if channel_kind == "custom_ai" {
        return Err(AppError::Conflict(
            "custom AI channels are configuration drafts and cannot send or receive messages"
                .to_owned(),
        ));
    }
    let acknowledge_telegram_request = !contact_is_blocked
        && should_acknowledge_telegram_request(&channel_kind, author, last_message_sequence);
    if channel_kind == "telegram_bot" && body.chars().count() > 4_096 {
        return Err(AppError::BadRequest(
            "Telegram messages cannot exceed 4096 characters".to_owned(),
        ));
    }
    if channel_kind == "telegram_bot"
        && !matches!(author, MessageAuthorSelection::Contact(_))
        && (channel_status != "active" || channel_deleted_at.is_some())
    {
        return Err(AppError::Conflict(
            "messages cannot be sent through an inactive Telegram bot".to_owned(),
        ));
    }

    if let Some(existing) = sqlx::query_as::<_, MessageResponse>(
        r#"
        SELECT id, conversation_id, sequence, direction, kind, author_kind,
               body, body_format, status, created_at, is_voice_message,
               COALESCE((
                   SELECT jsonb_agg(jsonb_build_object(
                       'id', attachment.id,
                       'file_name', attachment.file_name,
                       'content_type', attachment.content_type,
                       'byte_size', attachment.byte_size,
                       'available', attachment.scan_status = 'clean'
                           AND attachment.expires_at > now()
                           AND attachment.purged_at IS NULL,
                       'expires_at', attachment.expires_at
                   ) ORDER BY attachment.created_at, attachment.id)
                   FROM message_attachments AS attachment
                   WHERE attachment.tenant_id = messages.tenant_id
                     AND attachment.message_id = messages.id
               ), '[]'::jsonb) AS attachments
        FROM messages
        WHERE tenant_id = $1 AND conversation_id = $2 AND client_message_id = $3
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(request.client_message_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        transaction.commit().await?;
        return Ok(existing);
    }

    let on_behalf_administrator_id = match author {
        MessageAuthorSelection::AssignedOperator { administrator_id } => Some(administrator_id),
        _ => None,
    };
    let (author, manual_ai_administrator_id) = match author {
        MessageAuthorSelection::Contact(contact_id) => (MessageAuthor::Contact(contact_id), None),
        MessageAuthorSelection::Operator(operator_id) => {
            (MessageAuthor::Operator(operator_id), None)
        }
        MessageAuthorSelection::AssignedOperator { .. } => {
            if status == "resolved" {
                return Err(AppError::Conflict(
                    "resolved conversations cannot receive a reply on behalf of an operator"
                        .to_owned(),
                ));
            }
            // The conversation row is locked, so the assignment cannot change before commit.
            let operator_id = sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT user_id
                FROM conversation_assignments
                WHERE tenant_id = $1 AND conversation_id = $2
                  AND user_id IS NOT NULL AND unassigned_at IS NULL
                ORDER BY assigned_at DESC, id DESC
                LIMIT 1
                "#,
            )
            .bind(scope.tenant_id)
            .bind(conversation_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| {
                AppError::Conflict(
                    "an operator must handle the conversation before sending on their behalf"
                        .to_owned(),
                )
            })?;
            (MessageAuthor::Operator(operator_id), None)
        }
        MessageAuthorSelection::ActiveAi { administrator_id } => {
            if contact_is_blocked {
                return Err(AppError::Conflict(
                    "AI replies are disabled for blocked contacts".to_owned(),
                ));
            }
            if status == "resolved" {
                return Err(AppError::Conflict(
                    "resolved conversations cannot receive a manual AI reply".to_owned(),
                ));
            }
            let ai_profile_id = sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT participant.ai_profile_id
                FROM conversation_participants AS participant
                JOIN ai_profiles AS profile
                  ON profile.tenant_id = participant.tenant_id
                 AND profile.project_id = $3
                 AND profile.id = participant.ai_profile_id
                JOIN ai_profile_channel_connections AS channel_assignment
                  ON channel_assignment.tenant_id = profile.tenant_id
                 AND channel_assignment.ai_profile_id = profile.id
                 AND channel_assignment.channel_connection_id = $4
                JOIN channel_connections AS connection
                  ON connection.tenant_id = channel_assignment.tenant_id
                 AND connection.id = channel_assignment.channel_connection_id
                 AND connection.project_id = profile.project_id
                 AND connection.inbox_id = $5
                JOIN inboxes AS inbox
                  ON inbox.tenant_id = connection.tenant_id
                 AND inbox.project_id = connection.project_id
                 AND inbox.id = connection.inbox_id
                WHERE participant.tenant_id = $1
                  AND participant.conversation_id = $2
                  AND participant.participant_kind = 'ai'
                  AND participant.ai_profile_id IS NOT NULL
                  AND participant.left_at IS NULL
                  AND participant.joined_at <= now()
                  AND profile.status = 'active'
                  AND connection.status = 'active'
                  AND connection.deleted_at IS NULL
                  AND inbox.status = 'active'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM conversation_assignments AS assignment
                      WHERE assignment.tenant_id = participant.tenant_id
                        AND assignment.conversation_id = participant.conversation_id
                        AND assignment.user_id IS NOT NULL
                        AND assignment.unassigned_at IS NULL
                  )
                ORDER BY participant.joined_at DESC, participant.id DESC
                LIMIT 1
                FOR SHARE OF channel_assignment, connection, inbox, profile
                "#,
            )
            .bind(scope.tenant_id)
            .bind(conversation_id)
            .bind(scope.project_id)
            .bind(scope.channel_connection_id)
            .bind(scope.inbox_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| {
                AppError::Conflict(
                    "an active AI agent must be connected before sending as AI".to_owned(),
                )
            })?;
            (MessageAuthor::Ai(ai_profile_id), Some(administrator_id))
        }
    };

    let blocked_inbound = contact_is_blocked && author.is_contact();
    if status == "resolved" && !blocked_inbound {
        sqlx::query(
            r#"
            UPDATE conversation_resolutions
            SET reopened_at = now()
            WHERE tenant_id = $1 AND conversation_id = $2
              AND cycle_number = (
                  SELECT MAX(latest.cycle_number)
                  FROM conversation_resolutions AS latest
                  WHERE latest.tenant_id = $1 AND latest.conversation_id = $2
              )
              AND reopened_at IS NULL
            "#,
        )
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            UPDATE conversations
            SET status = 'open', reopened_count = reopened_count + 1,
                version = version + 1, updated_at = now()
            WHERE tenant_id = $1 AND id = $2
            "#,
        )
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .execute(&mut *transaction)
        .await?;
    } else if status == "new" && !blocked_inbound {
        sqlx::query(
            r#"
            UPDATE conversations
            SET status = 'open', version = version + 1, updated_at = now()
            WHERE tenant_id = $1 AND id = $2
            "#,
        )
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .execute(&mut *transaction)
        .await?;
    }

    let (sequence, now) = sqlx::query_as::<_, (i64, DateTime<Utc>)>(
        r#"
        UPDATE conversations
        SET last_message_sequence = last_message_sequence + 1,
            last_message_at = now(), updated_at = now(), version = version + 1
        WHERE tenant_id = $1 AND id = $2
        RETURNING last_message_sequence, last_message_at
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .fetch_one(&mut *transaction)
    .await?;
    let message_id = Uuid::now_v7();
    let is_voice_message = false;
    let message = sqlx::query_as::<_, MessageResponse>(
        r#"
        INSERT INTO messages (
            id, tenant_id, project_id, inbox_id, conversation_id, sequence,
            direction, kind, author_kind, author_id, client_message_id,
            body, body_format, status, created_at, is_voice_message
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'queued', $14, $15)
        RETURNING id, conversation_id, sequence, direction, kind, author_kind,
                  body, body_format, status, created_at, is_voice_message,
                  '[]'::jsonb AS attachments
        "#,
    )
    .bind(message_id)
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(conversation_id)
    .bind(sequence)
    .bind(author.direction())
    .bind(author.kind())
    .bind(author.author_kind())
    .bind(author.author_id())
    .bind(request.client_message_id)
    .bind(&body)
    .bind(body_format)
    .bind(now)
    .bind(is_voice_message)
    .fetch_one(&mut *transaction)
    .await?;

    if let Some(administrator_id) = manual_ai_administrator_id {
        provider_reply::supersede_pending_with_manual_reply(
            &mut transaction,
            scope.tenant_id,
            conversation_id,
        )
        .await?;
        sqlx::query(
            r#"
            INSERT INTO audit_log (
                id, tenant_id, project_id, actor_id, action,
                resource_kind, resource_id, metadata, occurred_at
            ) VALUES (
                $1, $2, $3, $4, 'conversation.ai_message.sent_by_admin',
                'conversation', $5, $6, $7
            )
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(scope.tenant_id)
        .bind(scope.project_id)
        .bind(administrator_id)
        .bind(conversation_id)
        .bind(json!({
            "message_id": message_id,
            "ai_profile_id": author.author_id(),
        }))
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    }
    if let Some(administrator_id) = on_behalf_administrator_id
        && administrator_id != author.author_id()
    {
        sqlx::query(
            r#"
            INSERT INTO audit_log (
                id, tenant_id, project_id, actor_id, action,
                resource_kind, resource_id, metadata, occurred_at
            ) VALUES (
                $1, $2, $3, $4, 'conversation.operator_message.sent_by_admin',
                'conversation', $5, $6, $7
            )
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(scope.tenant_id)
        .bind(scope.project_id)
        .bind(administrator_id)
        .bind(conversation_id)
        .bind(json!({
            "message_id": message_id,
            "operator_id": author.author_id(),
        }))
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    }

    let delivery_required = if author.is_contact() {
        false
    } else {
        telegram_bot::enqueue_outbound_if_needed(
            &mut transaction,
            scope.tenant_id,
            scope.channel_connection_id,
            message_id,
        )
        .await?
    };
    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: scope.tenant_id,
        project_id: scope.project_id,
        inbox_id: scope.inbox_id,
        contact_id: Some(scope.contact_id),
        event_type: "message.created".to_owned(),
        aggregate_id: message_id,
        sequence: Some(sequence),
        occurred_at: now,
        data: json!({
            "message_id": message_id,
            "conversation_id": conversation_id,
            "direction": author.direction(),
            "author_kind": author.author_kind(),
            "channel_connection_id": scope.channel_connection_id,
            "delivery_required": delivery_required,
            "contact_is_blocked": contact_is_blocked,
        }),
    };
    insert_outbox(&mut transaction, &event).await?;
    if acknowledge_telegram_request {
        telegram_bot::enqueue_request_acknowledgement(
            &mut transaction,
            scope.tenant_id,
            message_id,
        )
        .await?;
    }
    let blacklist_event = if blocked_inbound {
        blacklist::prepare_reply(
            &mut transaction,
            &scope,
            conversation_id,
            message_id,
            widget_language.as_deref(),
        )
        .await?
    } else {
        None
    };
    if author.is_contact() && !blocked_inbound {
        let routing = inbox_routing::on_inbound_message(
            state,
            &mut transaction,
            &InboundContext {
                tenant_id: scope.tenant_id,
                project_id: scope.project_id,
                inbox_id: scope.inbox_id,
                conversation_id,
                channel_connection_id: scope.channel_connection_id,
                widget_language: widget_language.clone(),
            },
        )
        .await?;
        if !routing.routing_configured {
            let message_text = format!(
                "Новое сообщение клиента.\nСообщение: {}\nЧат: {conversation_id}",
                body.trim()
            )
            .chars()
            .take(4_000)
            .collect();
            telegram_notifications::enqueue(
                &mut transaction,
                TelegramNotificationPayload {
                    tenant_id: scope.tenant_id,
                    project_id: scope.project_id,
                    inbox_id: scope.inbox_id,
                    channel_connection_id: scope.channel_connection_id,
                    kind: TelegramNotificationKind::NewMessage,
                    text: message_text,
                },
            )
            .await
            .map_err(AppError::internal)?;
        }
        if routing.allow_ai && !routing.operator_assigned {
            let joined_at =
                Utc::now() + chrono::Duration::seconds(random_ai_auto_join_delay_seconds());
            let joined = auto_join_ai_on_contact_message(
                &mut transaction,
                &scope,
                conversation_id,
                widget_language.as_deref(),
                joined_at,
            )
            .await?;
            if joined {
                let event = RealtimeEvent {
                    event_id: Uuid::now_v7(),
                    tenant_id: scope.tenant_id,
                    project_id: scope.project_id,
                    inbox_id: scope.inbox_id,
                    contact_id: Some(scope.contact_id),
                    event_type: "conversation.ai_joined".to_owned(),
                    aggregate_id: conversation_id,
                    sequence: None,
                    occurred_at: joined_at,
                    data: json!({ "conversation_id": conversation_id }),
                };
                insert_outbox_at(&mut transaction, &event, joined_at).await?;
            }
            provider_reply::enqueue_for_contact_message(
                &mut transaction,
                ContactMessageTrigger {
                    tenant_id: scope.tenant_id,
                    project_id: scope.project_id,
                    inbox_id: scope.inbox_id,
                    contact_id: scope.contact_id,
                    channel_connection_id: scope.channel_connection_id,
                    conversation_id,
                    message_id,
                    sequence,
                    widget_language,
                },
            )
            .await?;
        }
    } else if !author.is_contact() {
        inbox_routing::on_outbound_message(
            &mut transaction,
            scope.tenant_id,
            conversation_id,
            author.author_kind(),
        )
        .await?;
    }
    transaction.commit().await?;
    publish_best_effort(state, &event).await;
    if let Some(blacklist_event) = blacklist_event {
        publish_best_effort(state, &blacklist_event).await;
    }

    Ok(message)
}

async fn lock_contact_block_status(
    transaction: &mut Transaction<'_, Postgres>,
    scope: &ConversationScope,
) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT is_blocked FROM contacts WHERE tenant_id = $1 AND project_id = $2 AND id = $3 FOR UPDATE",
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.contact_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)
}

fn should_acknowledge_telegram_request(
    channel_kind: &str,
    author: MessageAuthorSelection,
    last_message_sequence: i64,
) -> bool {
    channel_kind == "telegram_bot"
        && matches!(author, MessageAuthorSelection::Contact(_))
        && last_message_sequence == 0
}

#[derive(Debug, FromRow)]
struct AttachmentConversationLock {
    status: String,
    attachments_enabled: bool,
    widget_language: Option<String>,
}

#[derive(Debug, FromRow)]
struct AttachmentQuota {
    attachment_count: i64,
    conversation_bytes: i64,
    tenant_bytes: i64,
}

pub(crate) async fn authorize_widget_attachment_upload(
    state: &AppState,
    session: &WidgetSessionContext,
    conversation_id: Uuid,
) -> Result<(), AppError> {
    let policy = sqlx::query_as::<_, (String, bool)>(
        r#"
        SELECT conversation.status,
               COALESCE(conversation.widget_attachments_enabled, widget.attachments_enabled)
        FROM conversations AS conversation
        JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        WHERE conversation.tenant_id = $1 AND conversation.project_id = $2
          AND conversation.inbox_id = $3 AND conversation.contact_id = $4
          AND conversation.channel_connection_id = $5 AND conversation.id = $6
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(session.contact_id)
    .bind(session.channel_connection_id)
    .bind(conversation_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if policy.0 == "resolved" {
        return Err(AppError::Conflict(
            "resolved conversations do not accept file uploads".to_owned(),
        ));
    }
    if !policy.1 {
        return Err(AppError::Conflict(
            "file uploads are disabled for this conversation".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) async fn authorize_operator_attachment_upload(
    state: &AppState,
    actor: &ActorContext,
    conversation_id: Uuid,
) -> Result<(), AppError> {
    actor.require("conversations:reply")?;
    let scope = load_conversation_scope(state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    let allowed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM conversations AS conversation
            JOIN conversation_assignments AS assignment
              ON assignment.tenant_id = conversation.tenant_id
             AND assignment.conversation_id = conversation.id
             AND assignment.user_id = $3
             AND assignment.unassigned_at IS NULL
            WHERE conversation.tenant_id = $1 AND conversation.id = $2
              AND conversation.status <> 'resolved'
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(conversation_id)
    .bind(actor.actor_id)
    .fetch_one(&state.db)
    .await?;
    if !allowed {
        return Err(AppError::Conflict(
            "join an active conversation before uploading a file".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) async fn create_widget_attachment_message(
    state: &AppState,
    session: &WidgetSessionContext,
    conversation_id: Uuid,
    client_message_id: Uuid,
    prepared: &mut PreparedAttachment,
) -> Result<MessageResponse, AppError> {
    let scope = ConversationScope {
        tenant_id: session.tenant_id,
        project_id: session.project_id,
        inbox_id: session.inbox_id,
        contact_id: session.contact_id,
        channel_connection_id: session.channel_connection_id,
    };
    create_attachment_message(
        state,
        scope,
        conversation_id,
        MessageAuthor::Contact(session.contact_id),
        client_message_id,
        Some(&session.language),
        prepared,
    )
    .await
}

pub(crate) async fn create_operator_attachment_message(
    state: &AppState,
    actor: &ActorContext,
    conversation_id: Uuid,
    client_message_id: Uuid,
    prepared: &mut PreparedAttachment,
) -> Result<MessageResponse, AppError> {
    actor.require("conversations:reply")?;
    let scope = load_conversation_scope(state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    create_attachment_message(
        state,
        scope,
        conversation_id,
        MessageAuthor::Operator(actor.actor_id),
        client_message_id,
        None,
        prepared,
    )
    .await
}

async fn create_attachment_message(
    state: &AppState,
    scope: ConversationScope,
    conversation_id: Uuid,
    author: MessageAuthor,
    client_message_id: Uuid,
    widget_language: Option<&str>,
    prepared: &mut PreparedAttachment,
) -> Result<MessageResponse, AppError> {
    let mut transaction = state.db.begin().await?;
    let contact_is_blocked = lock_contact_block_status(&mut transaction, &scope).await?;
    let blocked_inbound = contact_is_blocked && author.is_contact();
    let conversation = sqlx::query_as::<_, AttachmentConversationLock>(
        r#"
        SELECT conversation.status,
               COALESCE(conversation.widget_attachments_enabled, widget.attachments_enabled)
                   AS attachments_enabled,
               conversation.widget_language
        FROM conversations AS conversation
        JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        WHERE conversation.tenant_id = $1 AND conversation.project_id = $2
          AND conversation.inbox_id = $3 AND conversation.contact_id = $4
          AND conversation.channel_connection_id = $5 AND conversation.id = $6
        FOR UPDATE OF conversation, widget
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(scope.contact_id)
    .bind(scope.channel_connection_id)
    .bind(conversation_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if let Some(existing) = sqlx::query_as::<_, MessageResponse>(
        r#"
        SELECT message.id, message.conversation_id, message.sequence, message.direction,
               message.kind, message.author_kind, message.body, message.body_format, message.status,
               message.created_at, message.is_voice_message,
               COALESCE((
                   SELECT jsonb_agg(jsonb_build_object(
                       'id', attachment.id,
                       'file_name', attachment.file_name,
                       'content_type', attachment.content_type,
                       'byte_size', attachment.byte_size,
                       'available', attachment.scan_status = 'clean'
                           AND attachment.expires_at > now()
                           AND attachment.purged_at IS NULL,
                       'expires_at', attachment.expires_at
                   ) ORDER BY attachment.created_at, attachment.id)
                   FROM message_attachments AS attachment
                   WHERE attachment.tenant_id = message.tenant_id
                     AND attachment.message_id = message.id
               ), '[]'::jsonb) AS attachments
        FROM messages AS message
        WHERE message.tenant_id = $1 AND message.conversation_id = $2
          AND message.client_message_id = $3
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(client_message_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        transaction.commit().await?;
        return Ok(existing);
    }

    if conversation.status == "resolved" {
        return Err(AppError::Conflict(
            "resolved conversations do not accept file uploads".to_owned(),
        ));
    }
    if author.is_contact() && !conversation.attachments_enabled {
        return Err(AppError::Conflict(
            "file uploads are disabled for this conversation".to_owned(),
        ));
    }
    if let MessageAuthor::Operator(operator_id) = author {
        let assigned = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM conversation_assignments
                WHERE tenant_id = $1 AND conversation_id = $2
                  AND user_id = $3 AND unassigned_at IS NULL
            )
            "#,
        )
        .bind(scope.tenant_id)
        .bind(conversation_id)
        .bind(operator_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !assigned {
            return Err(AppError::Conflict(
                "join the conversation before uploading a file".to_owned(),
            ));
        }
    }

    let quota_lock = format!("attachments:{}", scope.tenant_id);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(quota_lock)
        .execute(&mut *transaction)
        .await?;
    let quota = sqlx::query_as::<_, AttachmentQuota>(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE message.conversation_id = $2) AS attachment_count,
            COALESCE(SUM(attachment.byte_size) FILTER (
                WHERE message.conversation_id = $2
            ), 0)::bigint AS conversation_bytes,
            (COALESCE(SUM(attachment.byte_size), 0) +
             COALESCE((SELECT SUM(byte_size) FROM ai_task_attachments WHERE tenant_id=$1), 0))::bigint AS tenant_bytes
        FROM message_attachments AS attachment
        JOIN messages AS message
          ON message.tenant_id = attachment.tenant_id
         AND message.id = attachment.message_id
        WHERE attachment.tenant_id = $1
          AND attachment.scan_status IN ('pending', 'clean', 'expired')
          AND attachment.purged_at IS NULL
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .fetch_one(&mut *transaction)
    .await?;
    if quota.attachment_count >= 100 {
        return Err(AppError::Conflict(
            "this conversation has reached its attachment limit".to_owned(),
        ));
    }
    let next_conversation_bytes = quota
        .conversation_bytes
        .checked_add(prepared.byte_size)
        .ok_or_else(|| AppError::PayloadTooLarge("attachment quota overflow".to_owned()))?;
    let next_tenant_bytes = quota
        .tenant_bytes
        .checked_add(prepared.byte_size)
        .ok_or_else(|| AppError::PayloadTooLarge("attachment quota overflow".to_owned()))?;
    let max_conversation_bytes = i64::try_from(state.config.attachments.max_conversation_bytes)
        .map_err(AppError::internal)?;
    let max_tenant_bytes =
        i64::try_from(state.config.attachments.max_tenant_bytes).map_err(AppError::internal)?;
    if next_conversation_bytes > max_conversation_bytes {
        return Err(AppError::PayloadTooLarge(
            "this conversation has reached its attachment storage quota".to_owned(),
        ));
    }
    if next_tenant_bytes > max_tenant_bytes {
        return Err(AppError::PayloadTooLarge(
            "the workspace has reached its attachment storage quota".to_owned(),
        ));
    }

    let (sequence, now) = sqlx::query_as::<_, (i64, DateTime<Utc>)>(
        r#"
        UPDATE conversations
        SET status = CASE WHEN status = 'new' THEN 'open' ELSE status END,
            last_message_sequence = last_message_sequence + 1,
            last_message_at = now(), updated_at = now(), version = version + 1
        WHERE tenant_id = $1 AND id = $2 AND status <> 'resolved'
        RETURNING last_message_sequence, last_message_at
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| {
        AppError::Conflict("resolved conversations do not accept file uploads".to_owned())
    })?;
    let message_id = Uuid::now_v7();
    let expires_at = now
        .checked_add_signed(chrono::Duration::days(i64::from(
            state.config.attachments.retention_days,
        )))
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("attachment expiry overflow")))?;
    sqlx::query(
        r#"
        INSERT INTO messages (
            id, tenant_id, project_id, inbox_id, conversation_id, sequence,
            direction, kind, author_kind, author_id, client_message_id,
            body, status, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'attachment', $8, $9, $10, '', 'queued', $11)
        "#,
    )
    .bind(message_id)
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(conversation_id)
    .bind(sequence)
    .bind(author.direction())
    .bind(author.author_kind())
    .bind(author.author_id())
    .bind(client_message_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO message_attachments (
            id, tenant_id, message_id, object_key, file_name, content_type,
            byte_size, checksum_sha256, scan_status, created_at, expires_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending', $9, $10)
        "#,
    )
    .bind(prepared.id)
    .bind(scope.tenant_id)
    .bind(message_id)
    .bind(prepared.id.to_string())
    .bind(&prepared.file_name)
    .bind(prepared.content_type)
    .bind(prepared.byte_size)
    .bind(&prepared.checksum_sha256)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;

    state.attachment_storage.publish(prepared).await?;
    sqlx::query(
        "UPDATE message_attachments SET scan_status = 'clean' WHERE tenant_id = $1 AND id = $2",
    )
    .bind(scope.tenant_id)
    .bind(prepared.id)
    .execute(&mut *transaction)
    .await?;

    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: scope.tenant_id,
        project_id: scope.project_id,
        inbox_id: scope.inbox_id,
        contact_id: Some(scope.contact_id),
        event_type: "message.created".to_owned(),
        aggregate_id: message_id,
        sequence: Some(sequence),
        occurred_at: now,
        data: json!({
            "message_id": message_id,
            "conversation_id": conversation_id,
            "direction": author.direction(),
            "kind": "attachment",
            "channel_connection_id": scope.channel_connection_id,
            "contact_is_blocked": contact_is_blocked,
        }),
    };
    insert_outbox(&mut transaction, &event).await?;
    let blacklist_event = if blocked_inbound {
        blacklist::prepare_reply(
            &mut transaction,
            &scope,
            conversation_id,
            message_id,
            widget_language.or(conversation.widget_language.as_deref()),
        )
        .await?
    } else {
        None
    };
    if author.is_contact() && !blocked_inbound {
        let routing = inbox_routing::on_inbound_message(
            state,
            &mut transaction,
            &InboundContext {
                tenant_id: scope.tenant_id,
                project_id: scope.project_id,
                inbox_id: scope.inbox_id,
                conversation_id,
                channel_connection_id: scope.channel_connection_id,
                widget_language: conversation.widget_language.clone(),
            },
        )
        .await?;
        if !routing.routing_configured {
            telegram_notifications::enqueue(
                &mut transaction,
                TelegramNotificationPayload {
                    tenant_id: scope.tenant_id,
                    project_id: scope.project_id,
                    inbox_id: scope.inbox_id,
                    channel_connection_id: scope.channel_connection_id,
                    kind: TelegramNotificationKind::NewMessage,
                    text: format!(
                        "Новое вложение от клиента.\nФайл: {}\nЧат: {conversation_id}",
                        prepared.file_name
                    )
                    .chars()
                    .take(4_000)
                    .collect(),
                },
            )
            .await
            .map_err(AppError::internal)?;
        }
    } else if !author.is_contact() {
        inbox_routing::on_outbound_message(
            &mut transaction,
            scope.tenant_id,
            conversation_id,
            author.author_kind(),
        )
        .await?;
    }
    transaction.commit().await?;
    AttachmentStorage::release_upload_permit(prepared);
    publish_best_effort(state, &event).await;
    if let Some(blacklist_event) = blacklist_event {
        publish_best_effort(state, &blacklist_event).await;
    }

    Ok(MessageResponse {
        id: message_id,
        conversation_id,
        sequence,
        direction: author.direction().to_owned(),
        kind: "attachment".to_owned(),
        author_kind: author.author_kind().to_owned(),
        body: String::new(),
        body_format: MessageBodyFormat::Plain,
        status: "queued".to_owned(),
        created_at: now,
        attachments: SqlJson(vec![AttachmentResponse {
            id: prepared.id,
            file_name: prepared.file_name.clone(),
            content_type: prepared.content_type.to_owned(),
            byte_size: prepared.byte_size,
            available: true,
            expires_at,
        }]),
        is_voice_message: false,
    })
}

#[derive(Debug, Deserialize)]
struct MessageListQuery {
    #[serde(default)]
    after_sequence: i64,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct MessageListResponse {
    items: Vec<MessageResponse>,
    next_sequence: i64,
}

async fn list_widget_messages(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Path(conversation_id): Path<Uuid>,
    Query(query): Query<MessageListQuery>,
) -> Result<Json<MessageListResponse>, AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM conversations
            WHERE tenant_id = $1 AND project_id = $2 AND inbox_id = $3
              AND contact_id = $4 AND id = $5
              AND channel_connection_id = $6
        )
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(session.contact_id)
    .bind(conversation_id)
    .bind(session.channel_connection_id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 100);
    let mut items = sqlx::query_as::<_, MessageResponse>(
        r#"
        SELECT message.id, message.conversation_id, message.sequence, message.direction,
               message.kind, message.author_kind, message.body, message.body_format, message.status,
               message.created_at, message.is_voice_message,
               COALESCE((
                   SELECT jsonb_agg(jsonb_build_object(
                       'id', attachment.id,
                       'file_name', attachment.file_name,
                       'content_type', attachment.content_type,
                       'byte_size', attachment.byte_size,
                       'available', attachment.scan_status = 'clean'
                           AND attachment.expires_at > now()
                           AND attachment.purged_at IS NULL,
                       'expires_at', attachment.expires_at
                   ) ORDER BY attachment.created_at, attachment.id)
                   FROM message_attachments AS attachment
                   WHERE attachment.tenant_id = message.tenant_id
                     AND attachment.message_id = message.id
               ), '[]'::jsonb) AS attachments
        FROM messages AS message
        WHERE message.tenant_id = $1 AND message.conversation_id = $2
          AND message.sequence > $3
        ORDER BY message.sequence ASC
        LIMIT $4
        "#,
    )
    .bind(session.tenant_id)
    .bind(conversation_id)
    .bind(query.after_sequence)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let next_sequence = items
        .last()
        .map_or(query.after_sequence, |message| message.sequence);
    // Keep diagnostic evidence in the operator view while never exposing a legacy
    // provider failure placeholder through the customer-facing widget API.
    items.retain(|message| {
        message.author_kind != "ai"
            || !provider_reply::is_provider_failure_placeholder(&message.body)
    });

    Ok(Json(MessageListResponse {
        items,
        next_sequence,
    }))
}

#[derive(Debug, Deserialize)]
struct WidgetMessagesReadRequest {
    message_ids: Vec<Uuid>,
}

async fn mark_widget_messages_read(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Path(conversation_id): Path<Uuid>,
    Json(request): Json<WidgetMessagesReadRequest>,
) -> Result<StatusCode, AppError> {
    if request.message_ids.is_empty() || request.message_ids.len() > 100 {
        return Err(AppError::BadRequest(
            "message_ids must contain between 1 and 100 message IDs".to_owned(),
        ));
    }
    let scope = load_conversation_scope(&state, session.tenant_id, conversation_id).await?;
    if scope.project_id != session.project_id
        || scope.inbox_id != session.inbox_id
        || scope.contact_id != session.contact_id
        || scope.channel_connection_id != session.channel_connection_id
    {
        return Err(AppError::NotFound);
    }

    let mut transaction = state.db.begin().await?;
    let message_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        UPDATE messages
        SET status = 'read'
        WHERE tenant_id = $1 AND conversation_id = $2
          AND id = ANY($3) AND direction = 'outbound'
          AND status IN ('queued', 'sending', 'sent', 'delivered')
        RETURNING id
        "#,
    )
    .bind(session.tenant_id)
    .bind(conversation_id)
    .bind(&request.message_ids)
    .fetch_all(&mut *transaction)
    .await?;

    if message_ids.is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }
    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        event_type: "message.status_updated".to_owned(),
        tenant_id: scope.tenant_id,
        project_id: scope.project_id,
        inbox_id: scope.inbox_id,
        contact_id: Some(scope.contact_id),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: Utc::now(),
        data: json!({
            "conversation_id": conversation_id,
            "message_ids": message_ids,
            "direction": "outbound",
            "status": "read",
        }),
    };
    insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    publish_best_effort(&state, &event).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_operator_messages(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
    Query(query): Query<MessageListQuery>,
) -> Result<Json<MessageListResponse>, AppError> {
    actor.require("conversations:read")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    crate::demo_access::require_conversation(&state.db, &actor, conversation_id).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, 100);
    let items = sqlx::query_as::<_, MessageResponse>(
        r#"
        SELECT message.id, message.conversation_id, message.sequence, message.direction,
               message.kind, message.author_kind, message.body, message.body_format, message.status,
               message.created_at, message.is_voice_message,
               COALESCE((
                   SELECT jsonb_agg(jsonb_build_object(
                       'id', attachment.id,
                       'file_name', attachment.file_name,
                       'content_type', attachment.content_type,
                       'byte_size', attachment.byte_size,
                       'available', attachment.scan_status = 'clean'
                           AND attachment.expires_at > now()
                           AND attachment.purged_at IS NULL,
                       'expires_at', attachment.expires_at
                   ) ORDER BY attachment.created_at, attachment.id)
                   FROM message_attachments AS attachment
                   WHERE attachment.tenant_id = message.tenant_id
                     AND attachment.message_id = message.id
               ), '[]'::jsonb) AS attachments
        FROM messages AS message
        WHERE message.tenant_id = $1 AND message.conversation_id = $2
          AND message.sequence > $3
        ORDER BY message.sequence ASC
        LIMIT $4
        "#,
    )
    .bind(actor.tenant_id)
    .bind(conversation_id)
    .bind(query.after_sequence)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let next_sequence = items
        .last()
        .map_or(query.after_sequence, |message| message.sequence);
    Ok(Json(MessageListResponse {
        items,
        next_sequence,
    }))
}

async fn mark_operator_conversation_read(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    actor.require("conversations:read")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    crate::demo_access::require_conversation(&state.db, &actor, conversation_id).await?;

    let result = sqlx::query(
        r#"
        INSERT INTO conversation_read_cursors (
            tenant_id, conversation_id, actor_id, last_read_sequence, read_at
        )
        SELECT conversation.tenant_id, conversation.id, $3,
               GREATEST(
                   conversation.operator_unread_baseline_sequence,
                   COALESCE(MAX(message.sequence), 0)
               ),
               now()
        FROM conversations AS conversation
        LEFT JOIN messages AS message
          ON message.tenant_id = conversation.tenant_id
         AND message.conversation_id = conversation.id
         AND message.direction = 'inbound'
         AND message.author_kind = 'contact'
        WHERE conversation.tenant_id = $1 AND conversation.id = $2
        GROUP BY conversation.tenant_id, conversation.id,
                 conversation.operator_unread_baseline_sequence
        ON CONFLICT (tenant_id, conversation_id, actor_id) DO UPDATE
        SET last_read_sequence = GREATEST(
                conversation_read_cursors.last_read_sequence,
                EXCLUDED.last_read_sequence
            ),
            read_at = now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(conversation_id)
    .bind(actor.actor_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ConversationListQuery {
    limit: Option<i64>,
    search: Option<String>,
    include_conversation_id: Option<Uuid>,
    #[serde(default)]
    needs_reply: bool,
    #[serde(default)]
    mine: bool,
    contact_id: Option<Uuid>,
    #[serde(default)]
    status: ConversationListStatus,
    channel_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum ConversationListStatus {
    Active,
    Resolved,
    #[default]
    All,
}

#[derive(Debug, Serialize)]
struct ConversationListResponse {
    items: Vec<OperatorConversationResponse>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
struct ConversationChannelResponse {
    id: Uuid,
    name: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct ConversationTelegramContactResponse {
    username: Option<String>,
    user_id: Option<String>,
    chat_id: Option<String>,
    language_code: Option<String>,
}

#[derive(Debug, FromRow)]
struct OperatorConversationRow {
    #[sqlx(flatten)]
    conversation: ConversationRow,
    channel_id: Uuid,
    channel_name: String,
    channel_kind: String,
    telegram_username: Option<String>,
    telegram_user_id: Option<String>,
    telegram_chat_id: Option<String>,
    telegram_language_code: Option<String>,
    last_message: Option<SqlJson<ConversationLastMessage>>,
    awaiting_reply_since: Option<DateTime<Utc>>,
    contact_conversation_count: i64,
}

#[derive(Debug, Deserialize, Serialize)]
struct ConversationLastMessage {
    body: String,
    body_format: MessageBodyFormat,
    direction: String,
    kind: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct OperatorConversationResponse {
    #[serde(flatten)]
    conversation: ConversationResponse,
    channel: ConversationChannelResponse,
    contact: OperatorContactProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    telegram: Option<ConversationTelegramContactResponse>,
    assigned_to_me: bool,
    unread_customer_messages: i64,
    last_message: Option<ConversationLastMessage>,
    awaiting_reply_since: Option<DateTime<Utc>>,
    contact_conversation_count: i64,
}

#[derive(Debug, Serialize)]
struct OperatorContactProfile {
    display_name: Option<String>,
    email: Option<String>,
    is_blocked: bool,
}

async fn list_operator_conversations(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
    Query(query): Query<ConversationListQuery>,
) -> Result<Json<ConversationListResponse>, AppError> {
    actor.require("conversations:read")?;
    let project_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT project_id FROM inboxes WHERE tenant_id = $1 AND id = $2",
    )
    .bind(actor.tenant_id)
    .bind(inbox_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(project_id, inbox_id)?;
    let search = normalize_conversation_search(query.search)?;
    let search_id = search
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok());
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    // Exclude the same legacy AI placeholders as the widget before filtering or pagination.
    let provider_failure_patterns = provider_reply::PROVIDER_FAILURE_PREFIXES
        .iter()
        .map(|prefix| format!("{prefix}%"))
        .collect::<Vec<_>>();
    let provider_failure_trim_characters =
        format!("{}⚠️", provider_reply::PROVIDER_FAILURE_WHITESPACE);
    let provider_failure_whitespace_pattern =
        format!("[{}]+", provider_reply::PROVIDER_FAILURE_WHITESPACE);

    let mut rows = sqlx::query_as::<_, OperatorConversationRow>(
        r#"
        SELECT conversation.id, conversation.inbox_id, conversation.contact_id,
               contact.display_name AS contact_display_name,
               contact.email AS contact_email,
               contact.is_blocked AS contact_is_blocked,
               conversation.status, conversation.subject,
               conversation.last_message_sequence,
               (
                   SELECT COUNT(*)
                   FROM messages AS unread_message
                   WHERE unread_message.tenant_id = conversation.tenant_id
                     AND unread_message.conversation_id = conversation.id
                     AND unread_message.direction = 'inbound'
                     AND unread_message.author_kind = 'contact'
                     AND unread_message.sequence > GREATEST(
                         conversation.operator_unread_baseline_sequence,
                         COALESCE(read_cursor.last_read_sequence, 0)
                     )
               ) AS unread_customer_messages,
               conversation.created_at,
               conversation.updated_at,
               CASE
                   WHEN channel.kind = 'widget' THEN COALESCE(
                       conversation.widget_attachments_enabled,
                       widget.attachments_enabled,
                       false
                   )
                   ELSE false
               END
                   AS widget_attachments_enabled,
               channel.id AS channel_id,
               channel.name AS channel_name,
               channel.kind AS channel_kind,
               telegram_identity.metadata ->> 'username' AS telegram_username,
               telegram_identity.metadata ->> 'telegram_user_id' AS telegram_user_id,
               telegram_identity.metadata ->> 'chat_id' AS telegram_chat_id,
               telegram_identity.metadata ->> 'language_code' AS telegram_language_code,
               assignment.user_id AS operator_user_id,
               COALESCE(profile.display_name, app_user.display_name) AS operator_display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS operator_avatar_url,
               assignment.assigned_at AS operator_joined_at,
               ai_participant.ai_profile_id,
               ai_identity.display_name AS ai_display_name,
               '/public/v1/avatars/' || ai_identity.avatar_public_id::text AS ai_avatar_url,
               ai_participant.joined_at AS ai_joined_at,
               ai_participant.left_at AS ai_left_at,
               latest_message.preview AS last_message,
               awaiting_reply.created_at AS awaiting_reply_since,
               (
                   SELECT COUNT(*)
                   FROM conversations AS contact_conversation
                   WHERE contact_conversation.tenant_id = conversation.tenant_id
                     AND contact_conversation.project_id = conversation.project_id
                     AND contact_conversation.inbox_id = conversation.inbox_id
                     AND contact_conversation.contact_id = conversation.contact_id
                     AND contact_conversation.last_message_sequence > 0
               ) AS contact_conversation_count
        FROM conversations AS conversation
        JOIN channel_connections AS channel
          ON channel.tenant_id = conversation.tenant_id
         AND channel.id = conversation.channel_connection_id
        LEFT JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        JOIN contacts AS contact
          ON contact.tenant_id = conversation.tenant_id
         AND contact.id = conversation.contact_id
        LEFT JOIN contact_identities AS telegram_identity
          ON telegram_identity.tenant_id = conversation.tenant_id
         AND telegram_identity.contact_id = conversation.contact_id
         AND telegram_identity.channel_kind = 'telegram_bot'
         AND channel.kind = 'telegram_bot'
         AND telegram_identity.external_id = concat(
             channel.id::text, ':',
             telegram_identity.metadata ->> 'telegram_user_id'
         )
        LEFT JOIN conversation_read_cursors AS read_cursor
          ON read_cursor.tenant_id = conversation.tenant_id
         AND read_cursor.conversation_id = conversation.id
         AND read_cursor.actor_id = $4
        LEFT JOIN LATERAL (
            SELECT jsonb_build_object(
                'body', LEFT(message.body, 280),
                'body_format', message.body_format,
                'direction', message.direction,
                'kind', message.kind,
                'created_at', message.created_at
            ) AS preview
            FROM messages AS message
            WHERE message.tenant_id = conversation.tenant_id
              AND message.conversation_id = conversation.id
              AND message.direction IN ('inbound', 'outbound')
              AND message.kind IN ('text', 'attachment')
              AND (
                  message.author_kind <> 'ai'
                  OR NOT (
                      TRANSLATE(
                          REPLACE(REGEXP_REPLACE(LTRIM(message.body, $14), $15, ' ', 'g'), '’', CHR(39)),
                          'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'
                      ) LIKE ANY($13::text[])
                  )
              )
            ORDER BY message.sequence DESC
            LIMIT 1
        ) AS latest_message ON true
        LEFT JOIN LATERAL (
            SELECT customer_message.created_at
            FROM messages AS customer_message
            WHERE conversation.status <> 'resolved'
              AND customer_message.tenant_id = conversation.tenant_id
              AND customer_message.conversation_id = conversation.id
              AND customer_message.direction = 'inbound'
              AND customer_message.author_kind = 'contact'
              AND customer_message.kind IN ('text', 'attachment')
              AND customer_message.sequence > COALESCE((
                  SELECT reply.sequence
                  FROM messages AS reply
                  WHERE reply.tenant_id = conversation.tenant_id
                    AND reply.conversation_id = conversation.id
                    AND reply.direction = 'outbound'
                    AND reply.author_kind IN ('operator', 'ai')
                    AND reply.kind IN ('text', 'attachment')
                    AND reply.status <> 'failed'
                    AND (
                        reply.author_kind <> 'ai'
                        OR NOT (
                            TRANSLATE(
                                REPLACE(REGEXP_REPLACE(LTRIM(reply.body, $14), $15, ' ', 'g'), '’', CHR(39)),
                                'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'
                            ) LIKE ANY($13::text[])
                        )
                    )
                  ORDER BY reply.sequence DESC
                  LIMIT 1
              ), 0)
            ORDER BY customer_message.sequence
            LIMIT 1
        ) AS awaiting_reply ON true
        LEFT JOIN LATERAL (
            SELECT active_assignment.user_id, active_assignment.assigned_at
            FROM conversation_assignments AS active_assignment
            WHERE active_assignment.tenant_id = conversation.tenant_id
              AND active_assignment.conversation_id = conversation.id
              AND active_assignment.user_id IS NOT NULL
              AND active_assignment.unassigned_at IS NULL
            ORDER BY active_assignment.assigned_at DESC, active_assignment.id DESC
            LIMIT 1
        ) AS assignment ON true
        LEFT JOIN users AS app_user ON app_user.id = assignment.user_id
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = conversation.tenant_id
         AND profile.project_id = conversation.project_id
         AND profile.user_id = assignment.user_id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = conversation.tenant_id
         AND stored_avatar.project_id = conversation.project_id
         AND stored_avatar.user_id = assignment.user_id
        LEFT JOIN LATERAL (
            SELECT participant.ai_profile_id,
                   GREATEST(participant.joined_at, first_contact_message.created_at) AS joined_at,
                   participant.left_at
            FROM conversation_participants AS participant
            JOIN LATERAL (
                SELECT contact_message.created_at
                FROM messages AS contact_message
                WHERE contact_message.tenant_id = participant.tenant_id
                  AND contact_message.conversation_id = participant.conversation_id
                  AND contact_message.author_kind = 'contact'
                  AND contact_message.kind = 'text'
                  AND contact_message.created_at <= COALESCE(
                      participant.left_at,
                      'infinity'::timestamptz
                  )
                ORDER BY contact_message.sequence
                LIMIT 1
            ) AS first_contact_message ON true
            WHERE participant.tenant_id = conversation.tenant_id
              AND participant.conversation_id = conversation.id
              AND participant.participant_kind = 'ai'
              AND participant.ai_profile_id IS NOT NULL
              AND participant.joined_at <= COALESCE(participant.left_at, now())
            ORDER BY participant.joined_at DESC, participant.id DESC
            LIMIT 1
        ) AS ai_participant ON true
        LEFT JOIN ai_profiles AS ai_profile
          ON ai_profile.tenant_id = conversation.tenant_id
         AND ai_profile.project_id = conversation.project_id
         AND ai_profile.id = ai_participant.ai_profile_id
        LEFT JOIN LATERAL (
            SELECT identity.display_name,
                   identity_avatar.public_id AS avatar_public_id
            FROM ai_profile_public_identities AS identity
            LEFT JOIN ai_profile_public_identity_avatars AS identity_avatar
              ON identity_avatar.tenant_id = identity.tenant_id
             AND identity_avatar.ai_profile_id = identity.ai_profile_id
             AND identity_avatar.language = identity.language
            WHERE identity.tenant_id = ai_profile.tenant_id
              AND identity.ai_profile_id = ai_profile.id
              AND conversation.widget_language IS NOT NULL
              AND (
                  identity.language = lower(replace(conversation.widget_language, '_', '-'))
                  OR (
                      split_part(identity.language, '-', 1)
                          = split_part(
                              lower(replace(conversation.widget_language, '_', '-')),
                              '-',
                              1
                          )
                      AND (
                          position('-' IN identity.language) = 0
                          OR position(
                              '-' IN lower(replace(conversation.widget_language, '_', '-'))
                          ) = 0
                      )
                  )
              )
            ORDER BY
                (
                    identity.language
                        = lower(replace(conversation.widget_language, '_', '-'))
                ) DESC,
                identity.language
            LIMIT 1
        ) AS ai_identity ON true
        WHERE conversation.tenant_id = $1 AND conversation.project_id = $2
          AND conversation.inbox_id = $3
          AND (NOT $16 OR EXISTS (
              SELECT 1 FROM widget_sessions AS demo_session
              WHERE demo_session.tenant_id = conversation.tenant_id
                AND demo_session.project_id = conversation.project_id
                AND demo_session.channel_connection_id = conversation.channel_connection_id
                AND demo_session.contact_id = conversation.contact_id
                AND conversation.channel_connection_id = $18
                AND demo_session.client_ip = $17::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND (conversation.last_message_sequence > 0 OR conversation.id = $7)
          AND (NOT $8 OR awaiting_reply.created_at IS NOT NULL)
          AND (NOT $9 OR assignment.user_id = $4)
          AND ($10::uuid IS NULL OR conversation.contact_id = $10)
          AND (
              $11 = 'all'
              OR ($11 = 'active' AND conversation.status <> 'resolved')
              OR ($11 = 'resolved' AND conversation.status = 'resolved')
          )
          AND ($12::uuid IS NULL OR conversation.channel_connection_id = $12)
          AND (
              $5::text IS NULL
              OR conversation.id = $19::uuid
              OR EXISTS (
                  SELECT 1
                  FROM messages AS matching_message
                  WHERE matching_message.tenant_id = conversation.tenant_id
                    AND matching_message.project_id = conversation.project_id
                    AND matching_message.inbox_id = conversation.inbox_id
                    AND matching_message.conversation_id = conversation.id
                    AND matching_message.kind = 'text'
                    AND matching_message.body <> ''
                    AND to_tsvector(
                        'simple'::regconfig,
                        matching_message.body
                    ) @@ plainto_tsquery('simple'::regconfig, $5::text)
              )
          )
        ORDER BY (conversation.id = $7) DESC NULLS LAST,
                 conversation.updated_at DESC, conversation.id DESC
        LIMIT $6
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(actor.actor_id)
    .bind(search)
    .bind(limit + 1)
    .bind(query.include_conversation_id)
    .bind(query.needs_reply)
    .bind(query.mine)
    .bind(query.contact_id)
    .bind(query.status)
    .bind(query.channel_id)
    .bind(provider_failure_patterns)
    .bind(provider_failure_trim_characters)
    .bind(provider_failure_whitespace_pattern)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .bind(search_id)
    .fetch_all(&state.db)
    .await?;
    let has_more = rows.len() > usize::try_from(limit).unwrap_or(100);
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(|row| {
            let assigned_to_me = row.conversation.operator_user_id == Some(actor.actor_id);
            let unread_customer_messages = row
                .conversation
                .unread_customer_messages
                .unwrap_or_default();
            let contact = OperatorContactProfile {
                display_name: row.conversation.contact_display_name.clone(),
                email: row.conversation.contact_email.clone(),
                is_blocked: row.conversation.contact_is_blocked,
            };
            let telegram = (row.channel_kind == "telegram_bot").then_some(
                ConversationTelegramContactResponse {
                    username: row.telegram_username,
                    user_id: row.telegram_user_id,
                    chat_id: row.telegram_chat_id,
                    language_code: row.telegram_language_code,
                },
            );
            let channel = ConversationChannelResponse {
                id: row.channel_id,
                name: row.channel_name,
                kind: row.channel_kind,
            };
            OperatorConversationResponse {
                conversation: ConversationResponse::from(row.conversation)
                    .apply_attachment_runtime_gate(state.config.attachments.enabled),
                channel,
                contact,
                telegram,
                assigned_to_me,
                unread_customer_messages,
                last_message: row.last_message.map(|message| message.0),
                awaiting_reply_since: row.awaiting_reply_since,
                contact_conversation_count: row.contact_conversation_count,
            }
        })
        .collect();

    Ok(Json(ConversationListResponse { items, has_more }))
}

#[derive(Debug, FromRow)]
struct VisitorObservationRow {
    session_id: Uuid,
    widget_language: String,
    client_ip: Option<String>,
    client_ip_source: Option<String>,
    user_agent: Option<String>,
    accept_language: Option<String>,
    client_hints: Option<SqlJson<Value>>,
    client_context: Option<SqlJson<Value>>,
    origin: String,
    captured_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct VisitorObservationResponse {
    session_id: Uuid,
    widget_language: String,
    client_ip: Option<String>,
    client_ip_source: Option<String>,
    user_agent: Option<String>,
    geo_ip: Option<GeoIpDetails>,
    user_agent_details: Option<UserAgentDetails>,
    accept_language: Option<String>,
    client_hints: Option<Value>,
    client_context: Option<Value>,
    origin: String,
    captured_at: DateTime<Utc>,
}

impl VisitorObservationResponse {
    fn from_row(row: VisitorObservationRow, environment: &VisitorEnvironment) -> Self {
        let geo_ip = environment.geo_ip(row.client_ip.as_deref());
        let user_agent_details = environment.user_agent(row.user_agent.as_deref());
        Self {
            session_id: row.session_id,
            widget_language: row.widget_language,
            client_ip: row.client_ip,
            client_ip_source: row.client_ip_source,
            user_agent: row.user_agent,
            geo_ip,
            user_agent_details,
            accept_language: row.accept_language,
            client_hints: row.client_hints.map(|value| value.0),
            client_context: row.client_context.map(|value| value.0),
            origin: row.origin,
            captured_at: row.captured_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct VisitorIntelligenceResponse {
    session_count: i64,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    observations: Vec<VisitorObservationResponse>,
}

async fn get_visitor_intelligence(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
) -> Result<(HeaderMap, Json<VisitorIntelligenceResponse>), AppError> {
    actor.require("visitor_network:read")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    crate::demo_access::require_conversation(&state.db, &actor, conversation_id).await?;

    let mut transaction = state.db.begin().await?;
    let aggregate = sqlx::query_as::<_, (i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>)>(
        r#"
        SELECT COUNT(*)::bigint, MIN(created_at), MAX(created_at)
        FROM widget_sessions
        WHERE tenant_id = $1 AND project_id = $2
          AND contact_id = $3 AND channel_connection_id = $4
          AND inbox_id = $5
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.contact_id)
    .bind(scope.channel_connection_id)
    .bind(scope.inbox_id)
    .fetch_one(&mut *transaction)
    .await?;
    if aggregate.0 == 0 {
        return Err(AppError::NotFound);
    }
    let first_seen_at = aggregate
        .1
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("visitor first seen time is missing")))?;
    let last_seen_at = aggregate
        .2
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("visitor last seen time is missing")))?;
    let rows = sqlx::query_as::<_, VisitorObservationRow>(
        r#"
        SELECT id AS session_id, language AS widget_language,
               host(client_ip) AS client_ip,
               client_ip_source, user_agent, accept_language,
               client_hints, client_context, origin, created_at AS captured_at
        FROM widget_sessions
        WHERE tenant_id = $1 AND project_id = $2
          AND contact_id = $3 AND channel_connection_id = $4
          AND inbox_id = $5
        ORDER BY created_at DESC, id DESC
        LIMIT 20
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.contact_id)
    .bind(scope.channel_connection_id)
    .bind(scope.inbox_id)
    .fetch_all(&mut *transaction)
    .await?;
    let observations = rows
        .into_iter()
        .map(|row| VisitorObservationResponse::from_row(row, &state.visitor_environment))
        .collect::<Vec<_>>();

    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES (
            $1, $2, $3, $4, 'visitor_intelligence.viewed',
            'conversation', $5, $6
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(actor.actor_id)
    .bind(conversation_id)
    .bind(json!({
        "session_count": aggregate.0,
        "observation_count": observations.len(),
    }))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        response_headers,
        Json(VisitorIntelligenceResponse {
            session_count: aggregate.0,
            first_seen_at,
            last_seen_at,
            observations,
        }),
    ))
}

#[derive(Debug, Deserialize)]
struct ResolveRequest {
    note: Option<String>,
    request_rating: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ResolutionResponse {
    id: Uuid,
    conversation_id: Uuid,
    resolved_at: DateTime<Utc>,
}

fn resolution_responsible_actor_id(
    assigned_actor_id: Option<Uuid>,
    closing_actor_id: Uuid,
) -> Uuid {
    assigned_actor_id.unwrap_or(closing_actor_id)
}

async fn resolve_conversation(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
    payload: Option<Json<ResolveRequest>>,
) -> Result<Json<ResolutionResponse>, AppError> {
    actor.require("conversations:close")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    let request_rating = payload
        .as_ref()
        .and_then(|Json(value)| value.request_rating)
        .unwrap_or(true);
    let note = normalize_optional_text(payload.and_then(|Json(value)| value.note), 1_000)?;
    let mut transaction = state.db.begin().await?;
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM conversations WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(conversation_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    if status == "resolved" {
        let existing = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
            r#"
            SELECT id, resolved_at
            FROM conversation_resolutions
            WHERE tenant_id = $1 AND conversation_id = $2
            ORDER BY cycle_number DESC
            LIMIT 1
            "#,
        )
        .bind(actor.tenant_id)
        .bind(conversation_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| {
            AppError::Conflict(
                "an automatically resolved conversation has no rateable resolution".to_owned(),
            )
        })?;
        transaction.commit().await?;
        return Ok(Json(ResolutionResponse {
            id: existing.0,
            conversation_id,
            resolved_at: existing.1,
        }));
    }

    let cycle_number = sqlx::query_scalar::<_, i32>(
        r#"
        SELECT COALESCE(MAX(cycle_number), 0) + 1
        FROM conversation_resolutions
        WHERE tenant_id = $1 AND conversation_id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(conversation_id)
    .fetch_one(&mut *transaction)
    .await?;
    let (assigned_actor_id, assigned_team_id) = sqlx::query_as::<_, (Option<Uuid>, Option<Uuid>)>(
        r#"
            SELECT
                (
                    SELECT assignment.user_id
                    FROM conversation_assignments AS assignment
                    WHERE assignment.tenant_id = $1
                      AND assignment.conversation_id = $2
                      AND assignment.user_id IS NOT NULL
                      AND assignment.unassigned_at IS NULL
                    ORDER BY assignment.assigned_at DESC, assignment.id DESC
                    LIMIT 1
                ),
                (
                    SELECT assignment.team_id
                    FROM conversation_assignments AS assignment
                    WHERE assignment.tenant_id = $1
                      AND assignment.conversation_id = $2
                      AND assignment.team_id IS NOT NULL
                      AND assignment.unassigned_at IS NULL
                    ORDER BY assignment.assigned_at DESC, assignment.id DESC
                    LIMIT 1
                )
            "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .fetch_one(&mut *transaction)
    .await?;
    let responsible_actor_id = resolution_responsible_actor_id(assigned_actor_id, actor.actor_id);
    let id = Uuid::now_v7();
    let now = Utc::now();
    provider_reply::cancel_pending(&mut transaction, scope.tenant_id, conversation_id).await?;
    sqlx::query(
        r#"
        UPDATE conversation_participants
        SET left_at = $3
        WHERE tenant_id = $1 AND conversation_id = $2
          AND participant_kind = 'ai' AND left_at IS NULL
          AND joined_at > $3
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO conversation_resolutions (
            id, tenant_id, project_id, inbox_id, conversation_id,
            cycle_number, responsible_actor_id, responsible_team_id, note, resolved_at, rating_requested
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(id)
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(conversation_id)
    .bind(cycle_number)
    .bind(responsible_actor_id)
    .bind(assigned_team_id)
    .bind(note)
    .bind(now)
    .bind(request_rating)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE conversations
        SET status = 'resolved', resolved_at = $3,
            updated_at = $3, version = version + 1
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    inbox_routing::on_conversation_resolved(&mut transaction, scope.tenant_id, conversation_id)
        .await?;
    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: scope.tenant_id,
        project_id: scope.project_id,
        inbox_id: scope.inbox_id,
        contact_id: Some(scope.contact_id),
        event_type: "conversation.resolved".to_owned(),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: now,
        data: json!({"conversation_id": conversation_id, "resolution_id": if request_rating { Some(id) } else { None }, "rating_requested": request_rating}),
    };
    insert_outbox(&mut transaction, &event).await?;
    if request_rating {
        email::enqueue_rating_invitation(
            &state,
            &mut transaction,
            scope.tenant_id,
            scope.project_id,
            scope.inbox_id,
            conversation_id,
            id,
        )
        .await?;
        telegram_bot::enqueue_rating_invitation(
            &mut transaction,
            scope.tenant_id,
            scope.project_id,
            scope.inbox_id,
            conversation_id,
            id,
        )
        .await?;
    }
    transaction.commit().await?;
    publish_best_effort(&state, &event).await;

    Ok(Json(ResolutionResponse {
        id,
        conversation_id,
        resolved_at: now,
    }))
}

async fn reopen_conversation(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    actor.require("conversations:close")?;
    let scope = load_conversation_scope(&state, actor.tenant_id, conversation_id).await?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;

    let mut transaction = state.db.begin().await?;
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM conversations WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    if status != "resolved" {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let now = Utc::now();
    sqlx::query(
        r#"
        UPDATE conversation_resolutions
        SET reopened_at = $3
        WHERE tenant_id = $1 AND conversation_id = $2
          AND cycle_number = (
              SELECT MAX(latest.cycle_number)
              FROM conversation_resolutions AS latest
              WHERE latest.tenant_id = $1 AND latest.conversation_id = $2
          )
          AND reopened_at IS NULL
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE conversations
        SET status = 'open', resolved_at = NULL, last_reopened_at = $3,
            reopened_count = reopened_count + 1, updated_at = $3, version = version + 1
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, occurred_at
        ) VALUES ($1, $2, $3, $4, 'conversation.reopened', 'conversation', $5, $6)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(actor.actor_id)
    .bind(conversation_id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: scope.tenant_id,
        project_id: scope.project_id,
        inbox_id: scope.inbox_id,
        contact_id: Some(scope.contact_id),
        event_type: "conversation.reopened".to_owned(),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: now,
        data: json!({"conversation_id": conversation_id}),
    };
    insert_outbox(&mut transaction, &event).await?;
    transaction.commit().await?;
    publish_best_effort(&state, &event).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct RatingRequest {
    rating: i16,
    reasons: Vec<String>,
    comment: Option<String>,
}

#[derive(Debug, Serialize)]
struct RatingResponse {
    id: Uuid,
    resolution_id: Uuid,
    rating: i16,
    created_at: DateTime<Utc>,
}

async fn rate_resolution(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Path(resolution_id): Path<Uuid>,
    Json(request): Json<RatingRequest>,
) -> Result<Json<RatingResponse>, AppError> {
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
    let reasons = request
        .reasons
        .into_iter()
        .map(|reason| normalize_required_text(reason, 100))
        .collect::<Result<Vec<_>, _>>()?;
    let comment = normalize_optional_text(request.comment, 2_000)?;
    let resolution_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM conversation_resolutions AS resolution
            JOIN conversations AS conversation
              ON conversation.tenant_id = resolution.tenant_id
             AND conversation.id = resolution.conversation_id
            WHERE resolution.tenant_id = $1 AND resolution.project_id = $2
              AND resolution.inbox_id = $3 AND resolution.id = $4
              AND conversation.contact_id = $5
              AND resolution.rating_requested
        )
        "#,
    )
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(resolution_id)
    .bind(session.contact_id)
    .fetch_one(&state.db)
    .await?;
    if !resolution_exists {
        return Err(AppError::NotFound);
    }

    let id = Uuid::now_v7();
    let now = Utc::now();
    let mut transaction = state.db.begin().await?;
    let inserted = sqlx::query(
        r#"
        INSERT INTO support_ratings (
            id, tenant_id, project_id, inbox_id, resolution_id,
            rating, comment, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (tenant_id, resolution_id) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(resolution_id)
    .bind(request.rating)
    .bind(comment)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "this resolution has already been rated".to_owned(),
        ));
    }
    for reason in reasons {
        sqlx::query(
            r#"
            INSERT INTO support_rating_reasons (
                id, tenant_id, support_rating_id, reason
            ) VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(session.tenant_id)
        .bind(id)
        .bind(reason)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;

    Ok(Json(RatingResponse {
        id,
        resolution_id,
        rating: request.rating,
        created_at: now,
    }))
}

async fn load_conversation_scope(
    state: &AppState,
    tenant_id: Uuid,
    conversation_id: Uuid,
) -> Result<ConversationScope, AppError> {
    sqlx::query_as::<_, ConversationScope>(
        r#"
        SELECT tenant_id, project_id, inbox_id, contact_id, channel_connection_id
        FROM conversations
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(conversation_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

pub(crate) async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    event: &RealtimeEvent,
) -> Result<(), AppError> {
    insert_outbox_record(transaction, event, None).await
}

async fn insert_outbox_at(
    transaction: &mut Transaction<'_, Postgres>,
    event: &RealtimeEvent,
    available_at: DateTime<Utc>,
) -> Result<(), AppError> {
    insert_outbox_record(transaction, event, Some(available_at)).await
}

async fn insert_outbox_record(
    transaction: &mut Transaction<'_, Postgres>,
    event: &RealtimeEvent,
    available_at: Option<DateTime<Utc>>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload,
            status, available_at, created_at
        ) VALUES (
            $1, $2, 'realtime', $3, $4, $5,
            'pending', COALESCE($6, now()), now()
        )
        "#,
    )
    .bind(event.event_id)
    .bind(event.tenant_id)
    .bind(event.aggregate_id)
    .bind(&event.event_type)
    .bind(serde_json::to_value(event).map_err(AppError::internal)?)
    .bind(available_at)
    .execute(&mut **transaction)
    .await?;
    if event
        .data
        .get("contact_is_blocked")
        .and_then(Value::as_bool)
        != Some(true)
    {
        crate::mobile_push::enqueue_realtime(transaction, event).await?;
    }
    Ok(())
}

pub(crate) async fn publish_best_effort(state: &AppState, event: &RealtimeEvent) {
    if let Err(error) = state.publish(event).await {
        warn!(?error, event_id = %event.event_id, "realtime publish deferred to outbox");
    }
}

fn normalize_required_text(value: String, max_chars: usize) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::BadRequest("text cannot be empty".to_owned()));
    }
    if value.chars().count() > max_chars {
        return Err(AppError::BadRequest(format!(
            "text cannot exceed {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

fn normalize_optional_text(
    value: Option<String>,
    max_chars: usize,
) -> Result<Option<String>, AppError> {
    value
        .map(|text| normalize_required_text(text, max_chars))
        .transpose()
}

fn normalize_widget_contact_name(value: String) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 200 || value.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "display_name must contain between 1 and 200 printable characters".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn normalize_widget_contact_email(value: String) -> Result<String, AppError> {
    let email = value.trim().to_lowercase();
    let valid_parts = email.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty() && !domain.is_empty() && !domain.contains('@')
    });
    if !(3..=320).contains(&email.chars().count())
        || email.chars().any(char::is_whitespace)
        || email.chars().any(char::is_control)
        || !valid_parts
    {
        return Err(AppError::BadRequest("email is invalid".to_owned()));
    }
    Ok(email)
}

fn normalize_conversation_search(search: Option<String>) -> Result<Option<String>, AppError> {
    let Some(search) = search else {
        return Ok(None);
    };
    let search = search.trim();
    if search.is_empty() {
        return Ok(None);
    }
    if search.chars().count() > MAX_CONVERSATION_SEARCH_LENGTH
        || search.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(format!(
            "conversation search must contain at most {MAX_CONVERSATION_SEARCH_LENGTH} printable characters"
        )));
    }
    Ok(Some(search.to_owned()))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use sqlx::PgPool;

    use super::{
        AI_AUTO_JOIN_DELAY_MAX_SECONDS, AI_AUTO_JOIN_DELAY_MIN_SECONDS,
        ConversationChannelResponse, ConversationLastMessage, ConversationListQuery,
        ConversationListStatus, ConversationResponse, ConversationTelegramContactResponse,
        CreateOperatorMessageRequest, MAX_CONVERSATION_SEARCH_LENGTH, MessageAuthor,
        MessageAuthorSelection, MessageBodyFormat, OperatorConversationResponse,
        OperatorMessageSender, first_compatible_ai_profile, normalize_conversation_search,
        normalize_optional_text, normalize_required_text, normalize_widget_contact_email,
        normalize_widget_contact_name, random_ai_auto_join_delay_seconds,
        resolution_responsible_actor_id, should_acknowledge_telegram_request, validate_visitor_id,
    };
    use uuid::Uuid;

    async fn search_conversation_ids(
        db: &PgPool,
        tenant_id: Uuid,
        project_id: Uuid,
        inbox_id: Uuid,
        search: &str,
    ) -> Vec<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT conversation.id
            FROM conversations AS conversation
            WHERE conversation.tenant_id = $1
              AND conversation.project_id = $2
              AND conversation.inbox_id = $3
              AND conversation.last_message_sequence > 0
              AND (conversation.id = $5::uuid OR EXISTS (
                  SELECT 1
                  FROM messages AS matching_message
                  WHERE matching_message.tenant_id = conversation.tenant_id
                    AND matching_message.project_id = conversation.project_id
                    AND matching_message.inbox_id = conversation.inbox_id
                    AND matching_message.conversation_id = conversation.id
                    AND matching_message.kind = 'text'
                    AND matching_message.body <> ''
                    AND to_tsvector(
                        'simple'::regconfig,
                        matching_message.body
                    ) @@ plainto_tsquery('simple'::regconfig, $4)
              ))
            ORDER BY conversation.id
            "#,
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(inbox_id)
        .bind(search)
        .bind(Uuid::parse_str(search).ok())
        .fetch_all(db)
        .await
        .unwrap()
    }

    #[test]
    fn trims_and_validates_message_text() {
        assert_eq!(
            normalize_required_text(" hello ".to_owned(), 10).unwrap(),
            "hello"
        );
        assert!(normalize_required_text("  ".to_owned(), 10).is_err());
        assert!(normalize_required_text("long".to_owned(), 3).is_err());
    }

    #[test]
    fn acknowledges_only_the_first_contact_message_in_a_telegram_conversation() {
        let actor_id = Uuid::now_v7();
        assert!(should_acknowledge_telegram_request(
            "telegram_bot",
            MessageAuthorSelection::Contact(actor_id),
            0,
        ));
        assert!(!should_acknowledge_telegram_request(
            "telegram_bot",
            MessageAuthorSelection::Contact(actor_id),
            1,
        ));
        assert!(!should_acknowledge_telegram_request(
            "widget",
            MessageAuthorSelection::Contact(actor_id),
            0,
        ));
        assert!(!should_acknowledge_telegram_request(
            "telegram_bot",
            MessageAuthorSelection::Operator(actor_id),
            0,
        ));
    }

    #[test]
    fn operator_messages_default_to_the_human_sender_and_accept_explicit_ai() {
        let operator_request: CreateOperatorMessageRequest = serde_json::from_value(json!({
            "client_message_id": Uuid::now_v7(),
            "body": "Hello"
        }))
        .unwrap();
        let ai_request: CreateOperatorMessageRequest = serde_json::from_value(json!({
            "client_message_id": Uuid::now_v7(),
            "body": "Hello",
            "send_as": "ai"
        }))
        .unwrap();
        let on_behalf_request: CreateOperatorMessageRequest = serde_json::from_value(json!({
            "client_message_id": Uuid::now_v7(),
            "body": "Hello",
            "send_as": "assigned_operator"
        }))
        .unwrap();

        assert_eq!(operator_request.send_as, OperatorMessageSender::Operator);
        assert_eq!(operator_request.body_format, MessageBodyFormat::Plain);
        assert_eq!(ai_request.send_as, OperatorMessageSender::Ai);
        assert_eq!(ai_request.body_format, MessageBodyFormat::Plain);
        assert_eq!(
            on_behalf_request.send_as,
            OperatorMessageSender::AssignedOperator
        );
        let ai_profile_id = Uuid::now_v7();
        let author = MessageAuthor::Ai(ai_profile_id);
        assert_eq!(author.author_kind(), "ai");
        assert_eq!(author.author_id(), ai_profile_id);
    }

    #[test]
    fn operator_messages_accept_explicit_markdown_and_reject_unknown_formats() {
        let request: CreateOperatorMessageRequest = serde_json::from_value(json!({
            "client_message_id": Uuid::now_v7(),
            "body": "**Hello**",
            "body_format": "markdown"
        }))
        .unwrap();

        assert_eq!(request.body_format, MessageBodyFormat::Markdown);
        assert_eq!(request.body, "**Hello**");
        assert_eq!(
            serde_json::to_value(request.body_format).unwrap(),
            "markdown"
        );
        assert!(
            serde_json::from_value::<CreateOperatorMessageRequest>(json!({
                "client_message_id": Uuid::now_v7(),
                "body": "<b>Hello</b>",
                "body_format": "html"
            }))
            .is_err()
        );
    }

    #[test]
    fn operator_conversation_serializes_channel_context_for_non_widget_sources() {
        let conversation_id = Uuid::now_v7();
        let channel_id = Uuid::now_v7();
        let response = OperatorConversationResponse {
            conversation: ConversationResponse {
                id: conversation_id,
                inbox_id: Uuid::now_v7(),
                contact_id: Uuid::now_v7(),
                status: "open".to_owned(),
                subject: Some("Email question".to_owned()),
                last_message_sequence: 1,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                widget_attachments_enabled: false,
                operator: None,
                ai_agent: None,
            },
            channel: ConversationChannelResponse {
                id: channel_id,
                name: "support@example.com".to_owned(),
                kind: "imap_smtp".to_owned(),
            },
            contact: super::OperatorContactProfile {
                display_name: None,
                email: Some("customer@example.com".to_owned()),
                is_blocked: false,
            },
            telegram: None,
            assigned_to_me: false,
            unread_customer_messages: 0,
            last_message: Some(ConversationLastMessage {
                body: "**Payment received**".to_owned(),
                body_format: MessageBodyFormat::Markdown,
                direction: "outbound".to_owned(),
                kind: "text".to_owned(),
                created_at: Utc::now(),
            }),
            awaiting_reply_since: None,
            contact_conversation_count: 3,
        };

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["id"], json!(conversation_id));
        assert_eq!(
            value["channel"],
            json!({
                "id": channel_id,
                "name": "support@example.com",
                "kind": "imap_smtp",
            })
        );
        assert_eq!(value["widget_attachments_enabled"], json!(false));
        assert_eq!(value["last_message"]["body_format"], "markdown");
        assert_eq!(value["last_message"]["body"], "**Payment received**");
        assert!(value["awaiting_reply_since"].is_null());
        assert_eq!(value["contact_conversation_count"], 3);
        assert!(value.get("telegram").is_none());
    }

    #[test]
    fn telegram_contact_serializes_available_provider_identity() {
        let value = serde_json::to_value(ConversationTelegramContactResponse {
            username: Some("ginotix".to_owned()),
            user_id: Some("7795261514".to_owned()),
            chat_id: Some("7795261514".to_owned()),
            language_code: Some("en".to_owned()),
        })
        .unwrap();

        assert_eq!(
            value,
            json!({
                "username": "ginotix",
                "user_id": "7795261514",
                "chat_id": "7795261514",
                "language_code": "en",
            })
        );
    }

    #[test]
    fn conversation_list_filters_default_to_all_and_validate_status() {
        let defaults: ConversationListQuery = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults.status, ConversationListStatus::All);
        assert!(!defaults.needs_reply);
        assert!(!defaults.mine);
        assert!(defaults.contact_id.is_none());
        assert!(defaults.channel_id.is_none());

        let filters: ConversationListQuery = serde_json::from_value(json!({
            "status": "active",
            "needs_reply": true,
            "mine": true
        }))
        .unwrap();
        assert_eq!(filters.status, ConversationListStatus::Active);
        assert!(filters.needs_reply);
        assert!(filters.mine);
        assert!(
            serde_json::from_value::<ConversationListQuery>(json!({"status": "unknown"})).is_err()
        );
    }

    #[test]
    fn preserves_absent_optional_text() {
        assert_eq!(normalize_optional_text(None, 10).unwrap(), None);
    }

    #[test]
    fn normalizes_and_bounds_conversation_search() {
        assert_eq!(
            normalize_conversation_search(Some("  payment status  ".to_owned())).unwrap(),
            Some("payment status".to_owned())
        );
        assert_eq!(
            normalize_conversation_search(Some("   ".to_owned())).unwrap(),
            None
        );
        assert!(
            normalize_conversation_search(Some("a".repeat(MAX_CONVERSATION_SEARCH_LENGTH + 1)))
                .is_err()
        );
        assert!(normalize_conversation_search(Some("payment\nstatus".to_owned())).is_err());
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn conversation_search_matches_scoped_text_messages(db: PgPool) {
        sqlx::raw_sql(
            r#"
            INSERT INTO tenants (id, name) VALUES
                ('00000000-0000-4000-8000-000000000001', 'Search tenant'),
                ('00000000-0000-4000-8000-000000000101', 'Other tenant');

            INSERT INTO projects (id, tenant_id, name, slug) VALUES
                (
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000001',
                    'Search project', 'search-project'
                ),
                (
                    '00000000-0000-4000-8000-000000000102',
                    '00000000-0000-4000-8000-000000000101',
                    'Other project', 'other-project'
                );

            INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
                (
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    'Search inbox'
                ),
                (
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    'Other inbox'
                ),
                (
                    '00000000-0000-4000-8000-000000000103',
                    '00000000-0000-4000-8000-000000000101',
                    '00000000-0000-4000-8000-000000000102',
                    'Other tenant inbox'
                );

            INSERT INTO channel_connections (
                id, tenant_id, project_id, inbox_id, public_id, kind, name
            ) VALUES
                (
                    '00000000-0000-4000-8000-000000000005',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000006',
                    'widget', 'Search widget'
                ),
                (
                    '00000000-0000-4000-8000-000000000007',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000008',
                    'widget', 'Other widget'
                ),
                (
                    '00000000-0000-4000-8000-000000000105',
                    '00000000-0000-4000-8000-000000000101',
                    '00000000-0000-4000-8000-000000000102',
                    '00000000-0000-4000-8000-000000000103',
                    '00000000-0000-4000-8000-000000000106',
                    'widget', 'Other tenant widget'
                );

            INSERT INTO contacts (id, tenant_id, project_id, display_name) VALUES
                (
                    '00000000-0000-4000-8000-000000000009',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    'Search contact'
                ),
                (
                    '00000000-0000-4000-8000-000000000109',
                    '00000000-0000-4000-8000-000000000101',
                    '00000000-0000-4000-8000-000000000102',
                    'Other tenant contact'
                );

            INSERT INTO conversations (
                id, tenant_id, project_id, inbox_id, channel_connection_id,
                contact_id, status, last_message_sequence
            ) VALUES
                (
                    '00000000-0000-4000-8000-000000000010',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000005',
                    '00000000-0000-4000-8000-000000000009', 'open', 1
                ),
                (
                    '00000000-0000-4000-8000-000000000011',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000005',
                    '00000000-0000-4000-8000-000000000009', 'open', 1
                ),
                (
                    '00000000-0000-4000-8000-000000000012',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000005',
                    '00000000-0000-4000-8000-000000000009', 'open', 1
                ),
                (
                    '00000000-0000-4000-8000-000000000013',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000005',
                    '00000000-0000-4000-8000-000000000009', 'open', 2
                ),
                (
                    '00000000-0000-4000-8000-000000000014',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000005',
                    '00000000-0000-4000-8000-000000000009', 'open', 1
                ),
                (
                    '00000000-0000-4000-8000-000000000015',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000007',
                    '00000000-0000-4000-8000-000000000009', 'open', 1
                ),
                (
                    '00000000-0000-4000-8000-000000000110',
                    '00000000-0000-4000-8000-000000000101',
                    '00000000-0000-4000-8000-000000000102',
                    '00000000-0000-4000-8000-000000000103',
                    '00000000-0000-4000-8000-000000000105',
                    '00000000-0000-4000-8000-000000000109', 'open', 1
                );

            INSERT INTO messages (
                id, tenant_id, project_id, inbox_id, conversation_id, sequence,
                direction, kind, author_kind, body, client_message_id
            ) VALUES
                (
                    '00000000-0000-4000-8000-000000000020',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000010', 1,
                    'inbound', 'text', 'contact', 'Payment was DELAYED', gen_random_uuid()
                ),
                (
                    '00000000-0000-4000-8000-000000000021',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000011', 1,
                    'inbound', 'text', 'contact', 'Оплата задержана', gen_random_uuid()
                ),
                (
                    '00000000-0000-4000-8000-000000000022',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000012', 1,
                    'inbound', 'text', 'contact', 'בקשת תשלום חדשה', gen_random_uuid()
                ),
                (
                    '00000000-0000-4000-8000-000000000023',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000013', 1,
                    'inbound', 'text', 'contact', 'split', gen_random_uuid()
                ),
                (
                    '00000000-0000-4000-8000-000000000024',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000013', 2,
                    'outbound', 'text', 'operator', 'terms', gen_random_uuid()
                ),
                (
                    '00000000-0000-4000-8000-000000000025',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000014', 1,
                    'inbound', 'attachment', 'contact', 'attachment only', gen_random_uuid()
                ),
                (
                    '00000000-0000-4000-8000-000000000026',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000015', 1,
                    'inbound', 'text', 'contact', 'outside inbox', gen_random_uuid()
                ),
                (
                    '00000000-0000-4000-8000-000000000120',
                    '00000000-0000-4000-8000-000000000101',
                    '00000000-0000-4000-8000-000000000102',
                    '00000000-0000-4000-8000-000000000103',
                    '00000000-0000-4000-8000-000000000110', 1,
                    'inbound', 'text', 'contact', 'other tenant secret', gen_random_uuid()
                );
            "#,
        )
        .execute(&db)
        .await
        .unwrap();

        let tenant_id = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let project_id = Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap();
        let inbox_id = Uuid::parse_str("00000000-0000-4000-8000-000000000003").unwrap();
        let expected = |value| Uuid::parse_str(value).unwrap();

        assert_eq!(
            search_conversation_ids(
                &db,
                tenant_id,
                project_id,
                inbox_id,
                "00000000-0000-4000-8000-000000000014",
            )
            .await,
            vec![expected("00000000-0000-4000-8000-000000000014")]
        );
        for inaccessible_id in [
            "00000000-0000-4000-8000-000000000015",
            "00000000-0000-4000-8000-000000000110",
            "00000000-0000-4000-8000-000000000099",
        ] {
            assert!(
                search_conversation_ids(&db, tenant_id, project_id, inbox_id, inaccessible_id,)
                    .await
                    .is_empty()
            );
        }

        assert_eq!(
            search_conversation_ids(&db, tenant_id, project_id, inbox_id, "PAYMENT delayed").await,
            vec![expected("00000000-0000-4000-8000-000000000010")]
        );
        assert_eq!(
            search_conversation_ids(&db, tenant_id, project_id, inbox_id, "ОПЛАТА ЗАДЕРЖАНА").await,
            vec![expected("00000000-0000-4000-8000-000000000011")]
        );
        assert_eq!(
            search_conversation_ids(&db, tenant_id, project_id, inbox_id, "בקשת תשלום").await,
            vec![expected("00000000-0000-4000-8000-000000000012")]
        );
        assert!(
            search_conversation_ids(&db, tenant_id, project_id, inbox_id, "split terms")
                .await
                .is_empty()
        );
        assert!(
            search_conversation_ids(&db, tenant_id, project_id, inbox_id, "attachment only")
                .await
                .is_empty()
        );
        assert!(
            search_conversation_ids(&db, tenant_id, project_id, inbox_id, "outside inbox")
                .await
                .is_empty()
        );
        assert!(
            search_conversation_ids(&db, tenant_id, project_id, inbox_id, "other tenant secret")
                .await
                .is_empty()
        );

        let search_index_is_valid = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT index_state.indisvalid
            FROM pg_index AS index_state
            WHERE index_state.indexrelid = to_regclass('messages_text_body_search_idx')
            "#,
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert!(search_index_is_valid);
    }

    #[test]
    fn schedules_ai_auto_join_between_five_and_ten_seconds() {
        for _ in 0..256 {
            let delay = random_ai_auto_join_delay_seconds();
            assert!(
                (AI_AUTO_JOIN_DELAY_MIN_SECONDS..=AI_AUTO_JOIN_DELAY_MAX_SECONDS).contains(&delay)
            );
        }
    }

    #[test]
    fn selects_the_ai_profile_that_supports_the_widget_language() {
        let english_russian_profile = Uuid::now_v7();
        let romanian_profile = Uuid::now_v7();
        let hindi_profile = Uuid::now_v7();
        let profiles = vec![
            (english_russian_profile, "en, ru".to_owned()),
            (romanian_profile, "ro".to_owned()),
            (hindi_profile, "hi".to_owned()),
        ];

        assert_eq!(
            first_compatible_ai_profile(profiles.clone(), Some("ro")),
            Some(romanian_profile)
        );
        assert_eq!(
            first_compatible_ai_profile(profiles, Some("hi")),
            Some(hindi_profile)
        );
    }

    #[test]
    fn attributes_resolution_to_the_assignee_or_closing_actor() {
        let assigned_actor_id = Uuid::now_v7();
        let closing_actor_id = Uuid::now_v7();

        assert_eq!(
            resolution_responsible_actor_id(Some(assigned_actor_id), closing_actor_id),
            assigned_actor_id
        );
        assert_eq!(
            resolution_responsible_actor_id(None, closing_actor_id),
            closing_actor_id
        );
    }

    #[test]
    fn visitor_identity_requires_non_nil_uuid_v4() {
        let visitor_id = Uuid::parse_str("60a8b88a-7145-4f56-95a1-2cbfcdb037d1").unwrap();
        assert!(validate_visitor_id(visitor_id).is_ok());
        assert!(validate_visitor_id(Uuid::nil()).is_err());
        assert!(validate_visitor_id(Uuid::now_v7()).is_err());
    }

    #[test]
    fn validates_widget_contact_profile_fields() {
        assert_eq!(
            normalize_widget_contact_name(" Anna Petrova ".to_owned()).unwrap(),
            "Anna Petrova"
        );
        assert_eq!(
            normalize_widget_contact_email(" Visitor@Example.COM ".to_owned()).unwrap(),
            "visitor@example.com"
        );
        assert!(normalize_widget_contact_name("  ".to_owned()).is_err());
        assert!(normalize_widget_contact_email("visitor example.com".to_owned()).is_err());
        assert!(normalize_widget_contact_email("visitor@@example.com".to_owned()).is_err());
    }
}
