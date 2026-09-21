//! Operator workspace metadata, contacts, and channel settings.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    routing::{delete, get, patch},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, types::Json as SqlJson};
use uuid::Uuid;

use crate::{
    AppState, ai_settings,
    auth::ActorContext,
    contact_blacklist::BlacklistReply,
    error::AppError,
    realtime::RealtimeEvent,
    telegram_bot,
    visitor_environment::{GeoIpDetails, UserAgentDetails, VisitorEnvironment},
    visitor_intelligence::ONLINE_PRESENCE_WINDOW_SECONDS,
    widget_launcher::WidgetLauncher,
    widget_localization::{WidgetLanguage, WidgetTranslations},
    widget_theme::WidgetTheme,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/inboxes", get(list_inboxes))
        .route("/api/v1/contacts", get(list_contacts))
        .route(
            "/api/v1/contacts/{contact_id}/block",
            patch(update_contact_block),
        )
        .route(
            "/api/v1/inboxes/{inbox_id}/online-visitors",
            get(list_online_visitors),
        )
        .route("/api/v1/widgets", get(list_online_visitor_widgets))
        .route(
            "/api/v1/widgets/{widget_id}/online-visitors",
            get(list_widget_online_visitors),
        )
        .route("/api/v1/channels", get(list_channels).post(create_channel))
        .route("/api/v1/channels/{channel_id}", delete(delete_channel))
        .route(
            "/api/v1/channels/{channel_id}/blacklist",
            patch(update_channel_blacklist),
        )
        .route(
            "/api/v1/channels/{channel_id}/widget",
            patch(update_widget_channel),
        )
}

#[derive(Debug, FromRow, Serialize)]
struct InboxResponse {
    id: Uuid,
    project_id: Uuid,
    name: String,
    status: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct InboxListResponse {
    items: Vec<InboxResponse>,
}

async fn list_inboxes(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<InboxListResponse>, AppError> {
    actor
        .require("conversations:read")
        .or_else(|_| actor.require("routing:manage"))
        .or_else(|_| actor.require("reply_templates:manage"))?;
    let mut items = sqlx::query_as::<_, InboxResponse>(
        r#"
        SELECT id, project_id, name, status, created_at
        FROM inboxes
        WHERE tenant_id = $1
          AND ($2::uuid IS NULL OR project_id = $2)
        ORDER BY name ASC, id ASC
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .fetch_all(&state.db)
    .await?;
    items.retain(|inbox| actor.require_inbox(inbox.project_id, inbox.id).is_ok());
    Ok(Json(InboxListResponse { items }))
}

#[derive(Debug, FromRow)]
struct ContactRow {
    id: Uuid,
    project_id: Uuid,
    display_name: Option<String>,
    email: Option<String>,
    is_blocked: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    client_ip: Option<String>,
    user_agent: Option<String>,
    browser_timezone: Option<String>,
    last_activity_at: DateTime<Utc>,
    channel_kinds: Vec<String>,
}

const DEFAULT_CONTACTS_PER_PAGE: i64 = 20;
const MAX_CONTACTS_PER_PAGE: i64 = 100;
const MAX_CONTACT_PAGE: i64 = 1_000_000;
const MAX_CONTACT_SEARCH_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
struct ContactListQuery {
    period: Option<ContactPeriod>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    page: Option<i64>,
    per_page: Option<i64>,
    search: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContactPeriod {
    #[default]
    Day,
    Week,
    Month,
    Custom,
    All,
}

impl ContactListQuery {
    fn window(
        &self,
        now: DateTime<Utc>,
    ) -> Result<(Option<DateTime<Utc>>, DateTime<Utc>), AppError> {
        let from = match self.period.as_ref().unwrap_or(&ContactPeriod::Day) {
            ContactPeriod::Day => Some(now - chrono::Duration::hours(24)),
            ContactPeriod::Week => Some(now - chrono::Duration::days(7)),
            ContactPeriod::Month => now.checked_sub_months(chrono::Months::new(1)),
            ContactPeriod::All => None,
            ContactPeriod::Custom => {
                let (Some(from), Some(to)) = (self.from, self.to) else {
                    return Err(AppError::BadRequest(
                        "custom period requires from and to".into(),
                    ));
                };
                if from >= to {
                    return Err(AppError::BadRequest("from must be earlier than to".into()));
                }
                return Ok((Some(from), to));
            }
        };
        Ok((from, now))
    }
}

#[derive(Debug, FromRow)]
struct ContactStatisticsRow {
    client_ip: Option<String>,
    contacts: i64,
    new_contacts: i64,
    conversations: i64,
    widget_sessions: i64,
    widget_contacts: i64,
    telegram_contacts: i64,
}

#[derive(Debug, Default, Serialize)]
struct ContactStatistics {
    contacts: i64,
    new_contacts: i64,
    returning_contacts: i64,
    conversations: i64,
    widget_sessions: i64,
    widget_contacts: i64,
    telegram_contacts: i64,
    countries: Vec<ContactCountryStatistics>,
}

#[derive(Debug, Serialize)]
struct ContactCountryStatistics {
    country_code: Option<String>,
    country: Option<String>,
    contacts: i64,
}

#[derive(Debug, Serialize)]
struct ContactResponse {
    id: Uuid,
    project_id: Uuid,
    display_name: Option<String>,
    email: Option<String>,
    is_blocked: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    client_ip: Option<String>,
    geo_ip: Option<GeoIpDetails>,
    user_agent_details: Option<UserAgentDetails>,
    browser_timezone: Option<String>,
    last_activity_at: DateTime<Utc>,
    channel_kinds: Vec<String>,
}

impl ContactResponse {
    fn from_row(row: ContactRow, environment: &VisitorEnvironment) -> Self {
        let geo_ip = environment.geo_ip(row.client_ip.as_deref());
        let user_agent_details = environment.user_agent(row.user_agent.as_deref());
        Self {
            id: row.id,
            project_id: row.project_id,
            display_name: row.display_name,
            email: row.email,
            is_blocked: row.is_blocked,
            created_at: row.created_at,
            updated_at: row.updated_at,
            client_ip: row.client_ip,
            geo_ip,
            user_agent_details,
            browser_timezone: row.browser_timezone,
            last_activity_at: row.last_activity_at,
            channel_kinds: row.channel_kinds,
        }
    }
}

#[derive(Debug, Serialize)]
struct ContactListResponse {
    statistics: ContactStatistics,
    items: Vec<ContactResponse>,
    total: i64,
    page: i64,
    per_page: i64,
    total_pages: i64,
}

async fn list_contacts(
    State(state): State<AppState>,
    actor: ActorContext,
    Query(query): Query<ContactListQuery>,
) -> Result<(HeaderMap, Json<ContactListResponse>), AppError> {
    actor.require("contacts:read")?;
    let include_visitor_environment = actor.require("visitor_network:read").is_ok();
    let page = query.page.unwrap_or(1).clamp(1, MAX_CONTACT_PAGE);
    let per_page = query
        .per_page
        .unwrap_or(DEFAULT_CONTACTS_PER_PAGE)
        .clamp(1, MAX_CONTACTS_PER_PAGE);
    let (active_since, active_until) = query.window(Utc::now())?;
    let search = normalize_contact_search(query.search)?;
    let source = r#"
        FROM contacts AS contact
        JOIN LATERAL (
            SELECT MAX(activity.last_activity_at) AS last_activity_at,
                   MIN(conversation.created_at) AS first_conversation_at,
                   COUNT(*) FILTER (WHERE activity.last_activity_at IS NOT NULL) AS conversations,
                   array_agg(DISTINCT channel.kind ORDER BY channel.kind) AS channel_kinds
            FROM conversations AS conversation
            JOIN channel_connections AS channel
              ON channel.tenant_id = conversation.tenant_id
             AND channel.id = conversation.channel_connection_id
            LEFT JOIN LATERAL (
                SELECT MAX(event.at) AS last_activity_at
                FROM (
                    SELECT conversation.created_at AS at
                    UNION ALL
                    SELECT message.created_at FROM messages AS message
                    WHERE message.tenant_id = conversation.tenant_id
                      AND message.conversation_id = conversation.id
                      AND message.direction IN ('inbound', 'outbound')
                ) AS event
                WHERE ($6::timestamptz IS NULL OR event.at >= $6) AND event.at < $7
            ) AS activity ON true
            WHERE conversation.tenant_id = contact.tenant_id
              AND conversation.project_id = contact.project_id
              AND conversation.contact_id = contact.id
              AND ($3::uuid[] IS NULL OR conversation.inbox_id = ANY($3))
        ) AS conversation_activity ON conversation_activity.first_conversation_at IS NOT NULL
        LEFT JOIN LATERAL (
            SELECT MAX(activity.last_seen_at) AS last_seen_at,
                   COUNT(*) FILTER (WHERE activity.last_seen_at IS NOT NULL) AS widget_sessions
            FROM widget_sessions AS session
            LEFT JOIN LATERAL (
                SELECT MAX(event.at) AS last_seen_at
                FROM (VALUES (session.created_at), (session.presence_last_seen_at)) AS event(at)
                WHERE ($6::timestamptz IS NULL OR event.at >= $6) AND event.at < $7
            ) AS activity ON true
            WHERE session.tenant_id = contact.tenant_id
              AND session.project_id = contact.project_id
              AND session.contact_id = contact.id
              AND ($3::uuid[] IS NULL OR session.inbox_id = ANY($3))
        ) AS presence ON true
        LEFT JOIN LATERAL (
            SELECT host(session.client_ip) AS client_ip, session.user_agent,
                   session.client_context #>> '{locale,timezone}' AS browser_timezone
            FROM widget_sessions AS session
            WHERE $4::boolean
              AND session.tenant_id = contact.tenant_id
              AND session.project_id = contact.project_id
              AND session.contact_id = contact.id
              AND ($3::uuid[] IS NULL OR session.inbox_id = ANY($3))
              AND session.visitor_data_expires_at > now()
              AND (session.client_ip IS NOT NULL OR session.user_agent IS NOT NULL
                   OR session.client_context IS NOT NULL)
            ORDER BY COALESCE(session.presence_last_seen_at, session.created_at) DESC, session.id DESC
            LIMIT 1
        ) AS latest_observation ON true
        WHERE contact.tenant_id = $1
          AND ($2::uuid IS NULL OR contact.project_id = $2)
          AND (NOT $8 OR EXISTS (
              SELECT 1 FROM widget_sessions AS demo_session
              WHERE demo_session.tenant_id = contact.tenant_id
                AND demo_session.project_id = contact.project_id
                AND demo_session.contact_id = contact.id
                AND demo_session.channel_connection_id = $10
                AND demo_session.client_ip = $9::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND GREATEST(conversation_activity.last_activity_at, presence.last_seen_at) IS NOT NULL
          AND (
              $5::text IS NULL
              OR strpos(lower(contact.id::text), lower($5)) > 0
              OR strpos(lower(contact.project_id::text), lower($5)) > 0
              OR strpos(lower(COALESCE(contact.display_name, '')), lower($5)) > 0
              OR strpos(lower(COALESCE(contact.email, '')), lower($5)) > 0
              OR strpos(lower(COALESCE(latest_observation.client_ip, '')), lower($5)) > 0
              OR strpos(lower(COALESCE(latest_observation.user_agent, '')), lower($5)) > 0
          )
    "#;
    let statistics_rows = sqlx::query_as::<_, ContactStatisticsRow>(&format!(
        r#"SELECT latest_observation.client_ip, COUNT(*) AS contacts,
                  COUNT(*) FILTER (WHERE $6::timestamptz IS NULL OR conversation_activity.first_conversation_at >= $6) AS new_contacts,
                  COALESCE(SUM(conversation_activity.conversations), 0)::bigint AS conversations,
                  COALESCE(SUM(presence.widget_sessions), 0)::bigint AS widget_sessions,
                  COUNT(*) FILTER (WHERE 'widget' = ANY(conversation_activity.channel_kinds)) AS widget_contacts,
                  COUNT(*) FILTER (WHERE 'telegram_bot' = ANY(conversation_activity.channel_kinds)) AS telegram_contacts
           {source}
           GROUP BY latest_observation.client_ip"#,
    ))
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .bind(actor.inbox_scope())
    .bind(include_visitor_environment)
    .bind(search.as_deref())
    .bind(active_since)
    .bind(active_until)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_all(&state.db)
    .await?;
    let mut statistics = ContactStatistics::default();
    for row in statistics_rows {
        statistics.contacts += row.contacts;
        statistics.new_contacts += row.new_contacts;
        statistics.conversations += row.conversations;
        statistics.widget_sessions += row.widget_sessions;
        statistics.widget_contacts += row.widget_contacts;
        statistics.telegram_contacts += row.telegram_contacts;
        let geo = state.visitor_environment.geo_ip(row.client_ip.as_deref());
        let country_code = geo.as_ref().and_then(|geo| geo.country_code.clone());
        if let Some(country) = statistics
            .countries
            .iter_mut()
            .find(|country| country.country_code == country_code)
        {
            country.contacts += row.contacts;
        } else {
            statistics.countries.push(ContactCountryStatistics {
                country_code,
                country: geo.and_then(|geo| geo.country),
                contacts: row.contacts,
            });
        }
    }
    statistics.returning_contacts = statistics.contacts - statistics.new_contacts;
    statistics.countries.sort_by(|left, right| {
        right
            .contacts
            .cmp(&left.contacts)
            .then_with(|| left.country_code.cmp(&right.country_code))
    });
    let total = statistics.contacts;

    let rows = sqlx::query_as::<_, ContactRow>(&format!(
        r#"
        SELECT contact.id, contact.project_id, contact.display_name, contact.email, contact.is_blocked,
               contact.created_at, contact.updated_at,
               latest_observation.client_ip, latest_observation.user_agent,
               latest_observation.browser_timezone, conversation_activity.channel_kinds,
               GREATEST(conversation_activity.last_activity_at, presence.last_seen_at) AS last_activity_at
        {source}
        ORDER BY last_activity_at DESC, contact.id DESC
        LIMIT $11 OFFSET $12
        "#,
    ))
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .bind(actor.inbox_scope())
    .bind(include_visitor_environment)
    .bind(search.as_deref())
    .bind(active_since)
    .bind(active_until)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(&state.db)
    .await?;
    let items = rows
        .into_iter()
        .map(|row| ContactResponse::from_row(row, &state.visitor_environment))
        .collect();

    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let total_pages = if total == 0 {
        0
    } else {
        (total + per_page - 1) / per_page
    };
    Ok((
        headers,
        Json(ContactListResponse {
            statistics,
            items,
            total,
            page,
            per_page,
            total_pages,
        }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateContactBlockRequest {
    blocked: bool,
}

#[derive(Debug, Serialize)]
struct ContactBlockResponse {
    id: Uuid,
    is_blocked: bool,
}

async fn update_contact_block(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(contact_id): Path<Uuid>,
    Json(request): Json<UpdateContactBlockRequest>,
) -> Result<Json<ContactBlockResponse>, AppError> {
    actor.require("contacts:manage")?;
    let mut transaction = state.db.begin().await?;
    // Incoming messages take this same lock before locking their conversation.
    let (project_id, was_blocked): (Uuid, bool) = sqlx::query_as(
        "SELECT project_id, is_blocked FROM contacts WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(contact_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_project(project_id)?;
    let inbox_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT inbox_id FROM conversations
        WHERE tenant_id = $1 AND project_id = $2 AND contact_id = $3
        UNION
        SELECT inbox_id FROM widget_sessions
        WHERE tenant_id = $1 AND project_id = $2 AND contact_id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(contact_id)
    .fetch_all(&mut *transaction)
    .await?;
    // A contact-wide block must not alter conversations outside the actor's Inbox scope.
    if inbox_ids.is_empty() && actor.inbox_scope().is_some() {
        return Err(AppError::Forbidden);
    }
    for inbox_id in &inbox_ids {
        actor.require_inbox(project_id, *inbox_id)?;
    }
    let mut events = Vec::new();
    if request.blocked != was_blocked {
        sqlx::query(
            "UPDATE contacts SET is_blocked = $3, updated_at = now() WHERE tenant_id = $1 AND id = $2",
        )
        .bind(actor.tenant_id)
        .bind(contact_id)
        .bind(request.blocked)
        .execute(&mut *transaction)
        .await?;
        if request.blocked {
            let conversation_ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT id FROM conversations WHERE tenant_id = $1 AND contact_id = $2 ORDER BY id FOR UPDATE",
            )
            .bind(actor.tenant_id)
            .bind(contact_id)
            .fetch_all(&mut *transaction)
            .await?;
            for conversation_id in &conversation_ids {
                crate::provider_reply::cancel_pending(
                    &mut transaction,
                    actor.tenant_id,
                    *conversation_id,
                )
                .await?;
            }
            sqlx::query(
                r#"
                UPDATE outbox_events
                SET status = 'completed', completed_at = now(),
                    locked_at = NULL, locked_by = NULL,
                    last_error = 'cancelled because the contact was blocked'
                WHERE tenant_id = $1 AND aggregate_id = ANY($2::uuid[])
                  AND aggregate_type = 'provider_reply'
                  AND status IN ('pending', 'processing')
                "#,
            )
            .bind(actor.tenant_id)
            .bind(&conversation_ids)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            r#"
            INSERT INTO audit_log (
                id, tenant_id, project_id, actor_id, action, resource_kind, resource_id, metadata
            ) VALUES ($1, $2, $3, $4, $5, 'contact', $6,
                      jsonb_build_object('is_blocked', $7::boolean))
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(actor.actor_id)
        .bind(if request.blocked {
            "contact.blocked"
        } else {
            "contact.unblocked"
        })
        .bind(contact_id)
        .bind(request.blocked)
        .execute(&mut *transaction)
        .await?;
        for inbox_id in inbox_ids {
            let event = RealtimeEvent {
                event_id: Uuid::now_v7(),
                tenant_id: actor.tenant_id,
                project_id,
                inbox_id,
                contact_id: Some(contact_id),
                event_type: "contact.updated".to_owned(),
                aggregate_id: contact_id,
                sequence: None,
                occurred_at: Utc::now(),
                data: serde_json::json!({"contact_id": contact_id}),
            };
            sqlx::query(
                r#"
                INSERT INTO outbox_events (
                    id, tenant_id, aggregate_type, aggregate_id, event_type, payload
                ) VALUES ($1, $2, 'realtime', $3, $4, $5)
                "#,
            )
            .bind(event.event_id)
            .bind(event.tenant_id)
            .bind(event.aggregate_id)
            .bind(&event.event_type)
            .bind(serde_json::to_value(&event).map_err(AppError::internal)?)
            .execute(&mut *transaction)
            .await?;
            events.push(event);
        }
    }
    transaction.commit().await?;
    ai_settings::publish_realtime_events_best_effort(&state, &events).await;
    Ok(Json(ContactBlockResponse {
        id: contact_id,
        is_blocked: request.blocked,
    }))
}

fn normalize_contact_search(search: Option<String>) -> Result<Option<String>, AppError> {
    let Some(search) = search else {
        return Ok(None);
    };
    let search = search.trim();
    if search.is_empty() {
        return Ok(None);
    }
    if search.chars().count() > MAX_CONTACT_SEARCH_CHARS {
        return Err(AppError::BadRequest(format!(
            "contact search must contain at most {MAX_CONTACT_SEARCH_CHARS} characters"
        )));
    }
    Ok(Some(search.to_owned()))
}

#[derive(Debug, FromRow, Serialize)]
struct OnlineVisitorWidgetResponse {
    id: Uuid,
    inbox_id: Uuid,
    name: String,
}

#[derive(Debug, Serialize)]
struct OnlineVisitorWidgetListResponse {
    items: Vec<OnlineVisitorWidgetResponse>,
}

#[derive(Debug, FromRow)]
struct OnlineVisitorRow {
    session_id: Uuid,
    contact_id: Uuid,
    channel_id: Uuid,
    widget_name: String,
    conversation_id: Option<Uuid>,
    client_ip: Option<String>,
    user_agent: Option<String>,
    origin: String,
    page_url: Option<String>,
    page_title: Option<String>,
    referrer: Option<String>,
    language: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct OnlineVisitorResponse {
    session_id: Uuid,
    contact_id: Uuid,
    channel_id: Uuid,
    widget_name: String,
    conversation_id: Option<Uuid>,
    client_ip: Option<String>,
    user_agent: Option<String>,
    geo_ip: Option<GeoIpDetails>,
    user_agent_details: Option<UserAgentDetails>,
    origin: String,
    page_url: Option<String>,
    page_title: Option<String>,
    referrer: Option<String>,
    language: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

impl OnlineVisitorResponse {
    fn from_row(row: OnlineVisitorRow, environment: &VisitorEnvironment) -> Self {
        let geo_ip = environment.geo_ip(row.client_ip.as_deref());
        let user_agent_details = environment.user_agent(row.user_agent.as_deref());
        Self {
            session_id: row.session_id,
            contact_id: row.contact_id,
            channel_id: row.channel_id,
            widget_name: row.widget_name,
            conversation_id: row.conversation_id,
            client_ip: row.client_ip,
            user_agent: row.user_agent,
            geo_ip,
            user_agent_details,
            origin: row.origin,
            page_url: row.page_url,
            page_title: row.page_title,
            referrer: row.referrer,
            language: row.language,
            first_seen_at: row.first_seen_at,
            last_seen_at: row.last_seen_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct OnlineVisitorListResponse {
    items: Vec<OnlineVisitorResponse>,
    online_window_seconds: i64,
}

async fn list_online_visitor_widgets(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<OnlineVisitorWidgetListResponse>, AppError> {
    actor.require("visitor_network:read")?;
    let inbox_scope = actor.inbox_scope().map(<[Uuid]>::to_vec);
    let items = sqlx::query_as::<_, OnlineVisitorWidgetResponse>(
        r#"
        SELECT id, inbox_id, name
        FROM channel_connections
        WHERE tenant_id = $1
          AND kind = 'widget'
          AND status = 'active'
          AND deleted_at IS NULL
          AND ($2::uuid IS NULL OR project_id = $2)
          AND ($3::uuid[] IS NULL OR inbox_id = ANY($3))
        ORDER BY name ASC, id ASC
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .bind(inbox_scope.as_deref())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(OnlineVisitorWidgetListResponse { items }))
}

async fn list_online_visitors(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(inbox_id): Path<Uuid>,
) -> Result<(HeaderMap, Json<OnlineVisitorListResponse>), AppError> {
    actor.require("visitor_network:read")?;
    let project_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT project_id FROM inboxes WHERE tenant_id = $1 AND id = $2",
    )
    .bind(actor.tenant_id)
    .bind(inbox_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(project_id, inbox_id)?;

    load_online_visitors(&state, &actor, project_id, inbox_id, None).await
}

async fn list_widget_online_visitors(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(widget_id): Path<Uuid>,
) -> Result<(HeaderMap, Json<OnlineVisitorListResponse>), AppError> {
    actor.require("visitor_network:read")?;
    let (project_id, inbox_id) = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT project_id, inbox_id
        FROM channel_connections
        WHERE tenant_id = $1
          AND id = $2
          AND kind = 'widget'
          AND status = 'active'
          AND deleted_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(widget_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(project_id, inbox_id)?;

    load_online_visitors(&state, &actor, project_id, inbox_id, Some(widget_id)).await
}

async fn load_online_visitors(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    inbox_id: Uuid,
    widget_id: Option<Uuid>,
) -> Result<(HeaderMap, Json<OnlineVisitorListResponse>), AppError> {
    let cutoff = Utc::now() - chrono::Duration::seconds(ONLINE_PRESENCE_WINDOW_SECONDS);
    let rows = sqlx::query_as::<_, OnlineVisitorRow>(
        r#"
        WITH latest AS (
            SELECT DISTINCT ON (session.contact_id, session.channel_connection_id)
                   session.id AS session_id, session.contact_id,
                   session.channel_connection_id AS channel_id,
                   connection.name AS widget_name,
                   host(session.client_ip) AS client_ip, session.user_agent,
                   session.origin,
                   session.client_context #>> '{page,url}' AS page_url,
                   session.client_context #>> '{page,title}' AS page_title,
                   session.client_context #>> '{page,referrer}' AS referrer,
                   session.language,
                   session.presence_last_seen_at AS last_seen_at
            FROM widget_sessions AS session
            JOIN channel_connections AS connection
              ON connection.tenant_id = session.tenant_id
             AND connection.id = session.channel_connection_id
            WHERE session.tenant_id = $1
              AND session.project_id = $2
              AND session.inbox_id = $3
              AND session.presence_last_seen_at >= $4
              AND (NOT $6 OR (
                  session.channel_connection_id = $8
                  AND session.client_ip = $7::text::inet
                  AND session.visitor_data_expires_at > now()
              ))
              AND (
                $5::uuid IS NULL
                OR (
                  session.channel_connection_id = $5
                  AND connection.kind = 'widget'
                  AND connection.status = 'active'
                  AND connection.deleted_at IS NULL
                )
              )
              AND session.expires_at > now()
              AND session.revoked_at IS NULL
            ORDER BY session.contact_id, session.channel_connection_id,
                     session.presence_last_seen_at DESC, session.id DESC
        )
        SELECT latest.session_id, latest.contact_id, latest.channel_id,
               latest.widget_name, conversation.id AS conversation_id,
               latest.client_ip, latest.user_agent, latest.origin, latest.page_url,
               latest.page_title, latest.referrer, latest.language,
               first_seen.at AS first_seen_at, latest.last_seen_at
        FROM latest
        CROSS JOIN LATERAL (
            SELECT MIN(session.created_at) AS at
            FROM widget_sessions AS session
            WHERE session.tenant_id = $1
              AND session.contact_id = latest.contact_id
              AND session.channel_connection_id = latest.channel_id
        ) AS first_seen
        LEFT JOIN LATERAL (
            SELECT id
            FROM conversations
            WHERE tenant_id = $1
              AND project_id = $2
              AND inbox_id = $3
              AND contact_id = latest.contact_id
              AND channel_connection_id = latest.channel_id
            ORDER BY created_at DESC, id DESC
            LIMIT 1
        ) AS conversation ON true
        ORDER BY latest.last_seen_at DESC, latest.session_id DESC
        LIMIT 500
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(inbox_id)
    .bind(cutoff)
    .bind(widget_id)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_all(&state.db)
    .await?;
    let items = rows
        .into_iter()
        .map(|row| OnlineVisitorResponse::from_row(row, &state.visitor_environment))
        .collect();

    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((
        headers,
        Json(OnlineVisitorListResponse {
            items,
            online_window_seconds: ONLINE_PRESENCE_WINDOW_SECONDS,
        }),
    ))
}

#[derive(Debug, FromRow, Serialize)]
struct ChannelResponse {
    id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    public_id: Uuid,
    kind: String,
    name: String,
    status: String,
    blacklist_reply: SqlJson<BlacklistReply>,
    allowed_origins: Option<Vec<String>>,
    greeting: Option<String>,
    default_language: Option<String>,
    translations: Option<SqlJson<WidgetTranslations>>,
    launcher: Option<SqlJson<WidgetLauncher>>,
    theme: Option<SqlJson<WidgetTheme>>,
    notify_on_new_visitor: Option<bool>,
    attachments_enabled: Option<bool>,
    telegram_bot_username: Option<String>,
    telegram_bot_token_configured: Option<bool>,
    telegram_webhook_url: Option<String>,
    #[sqlx(default)]
    phone_number: Option<String>,
    #[sqlx(default)]
    phone_gateway: Option<String>,
    #[sqlx(default)]
    phone_greeting: Option<String>,
    #[sqlx(default)]
    phone_transfer_number: Option<String>,
    #[sqlx(default)]
    phone_max_call_seconds: Option<i32>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ChannelListResponse {
    items: Vec<ChannelResponse>,
}

#[derive(Debug, FromRow)]
struct DeleteChannelScope {
    project_id: Uuid,
    inbox_id: Uuid,
    public_id: Uuid,
    kind: String,
    name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreateChannelKind {
    Widget,
}

impl CreateChannelKind {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "widget" => Ok(Self::Widget),
            _ => Err(AppError::BadRequest(
                "kind must be one of: widget".to_owned(),
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Widget => "widget",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateChannelRequest {
    kind: String,
    inbox_id: Uuid,
    name: String,
    allowed_origins: Vec<String>,
    #[serde(default)]
    default_language: WidgetLanguage,
    #[serde(default)]
    translations: WidgetTranslations,
    #[serde(default)]
    launcher: WidgetLauncher,
    #[serde(default)]
    theme: WidgetTheme,
    #[serde(default)]
    notify_on_new_visitor: bool,
    #[serde(default)]
    attachments_enabled: bool,
}

async fn list_channels(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<ChannelListResponse>, AppError> {
    actor.require("channels:read")?;
    let mut items = sqlx::query_as::<_, ChannelResponse>(
        r#"
        SELECT connection.id, connection.project_id, connection.inbox_id,
               connection.public_id, connection.kind, connection.name,
               connection.status, connection.blacklist_reply, widget.allowed_origins, widget.greeting,
               widget.default_language, widget.translations, widget.launcher, widget.theme,
               widget.notify_on_new_visitor, widget.attachments_enabled,
               CASE WHEN connection.kind = 'telegram_bot'
                    THEN connection.config ->> 'bot_username' END
                   AS telegram_bot_username,
               CASE WHEN connection.kind = 'telegram_bot' THEN EXISTS (
                   SELECT 1 FROM channel_secrets AS secret
                   WHERE secret.tenant_id = connection.tenant_id
                     AND secret.channel_connection_id = connection.id
               ) END AS telegram_bot_token_configured,
               CASE WHEN connection.kind = 'telegram_bot'
                    THEN connection.config ->> 'webhook_url' END
                   AS telegram_webhook_url,
               phone.phone_number, phone.gateway AS phone_gateway,
               phone.greeting AS phone_greeting,
               phone.transfer_number AS phone_transfer_number,
               phone.max_call_seconds AS phone_max_call_seconds,
               connection.updated_at
        FROM channel_connections AS connection
        LEFT JOIN widget_configs AS widget
          ON widget.tenant_id = connection.tenant_id
         AND widget.channel_connection_id = connection.id
        LEFT JOIN phone_channel_configs AS phone
          ON phone.tenant_id = connection.tenant_id
         AND phone.channel_connection_id = connection.id
        WHERE connection.tenant_id = $1
          AND connection.deleted_at IS NULL
          AND ($2::uuid IS NULL OR connection.project_id = $2)
        ORDER BY connection.name ASC, connection.id ASC
        "#,
    )
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .fetch_all(&state.db)
    .await?;
    items.retain(|channel| {
        actor
            .require_inbox(channel.project_id, channel.inbox_id)
            .is_ok()
    });
    Ok(Json(ChannelListResponse { items }))
}

async fn delete_channel(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(channel_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    actor.require("channels:manage")?;
    let mut transaction = state.db.begin().await?;
    let scope = sqlx::query_as::<_, DeleteChannelScope>(
        r#"
        SELECT project_id, inbox_id, public_id, kind, name
        FROM channel_connections
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(scope.project_id, scope.inbox_id)?;
    if !matches!(scope.kind.as_str(), "widget" | "telegram_bot" | "phone") {
        return Err(AppError::BadRequest(
            "this channel type cannot be deleted here".to_owned(),
        ));
    }
    if scope.kind == "telegram_bot" {
        actor.require_password_session()?;
        telegram_bot::enqueue_unregister(&mut transaction, actor.tenant_id, channel_id).await?;
    }

    let revoked_sessions = sqlx::query(
        r#"
        UPDATE widget_sessions
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE tenant_id = $1
          AND channel_connection_id = $2
          AND revoked_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    sqlx::query(
        r#"
        UPDATE channel_connections
        SET status = 'disabled', deleted_at = now(), updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .execute(&mut *transaction)
    .await?;
    let affected_profile_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT ai_profile_id
        FROM ai_profile_channel_connections
        WHERE tenant_id = $1 AND channel_connection_id = $2
        ORDER BY ai_profile_id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .fetch_all(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        DELETE FROM ai_profile_channel_connections
        WHERE tenant_id = $1 AND channel_connection_id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .execute(&mut *transaction)
    .await?;
    let mut ai_left_events = Vec::new();
    for profile_id in affected_profile_ids {
        ai_left_events.extend(
            ai_settings::leave_profile_participants_without_runtime_access(
                &mut transaction,
                actor.tenant_id,
                profile_id,
                false,
            )
            .await?,
        );
    }
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, $5,
                  'channel', $6, jsonb_build_object(
                      'inbox_id', $7::uuid,
                      'public_id', $8::uuid,
                      'name', $9::text,
                      'revoked_sessions', $10::bigint
                  ))
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(scope.project_id)
    .bind(actor.actor_id)
    .bind(format!("channel.{}.deleted", scope.kind))
    .bind(channel_id)
    .bind(scope.inbox_id)
    .bind(scope.public_id)
    .bind(&scope.name)
    .bind(i64::try_from(revoked_sessions).map_err(AppError::internal)?)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    ai_settings::publish_realtime_events_best_effort(&state, &ai_left_events).await;

    Ok(StatusCode::NO_CONTENT)
}

async fn create_channel(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<ChannelResponse>), AppError> {
    actor.require("channels:manage")?;
    let kind = CreateChannelKind::parse(&request.kind)?;
    let name = normalize_required_text(request.name, 200)?;
    let allowed_origins = validate_origins(request.allowed_origins)?;
    let translations = request.translations.validate()?;
    if !translations.contains(&request.default_language) {
        return Err(AppError::BadRequest(
            "default_language must exist in translations".to_owned(),
        ));
    }
    let default_translation = translations.get(&request.default_language)?;
    let greeting = default_translation.greeting.clone();
    let mut launcher = request.launcher.validate()?;
    launcher
        .label
        .clone_from(&default_translation.launcher_label);
    let theme = request.theme.validate()?;
    let mut transaction = state.db.begin().await?;
    let inbox = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT project_id, status
        FROM inboxes
        WHERE tenant_id = $1 AND id = $2
        FOR SHARE
        "#,
    )
    .bind(actor.tenant_id)
    .bind(request.inbox_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(inbox.0, request.inbox_id)?;
    if inbox.1 != "active" {
        return Err(AppError::Conflict(
            "channels cannot be created in a disabled Inbox".to_owned(),
        ));
    }

    let channel_id = Uuid::now_v7();
    let public_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO channel_connections (
            id, tenant_id, project_id, inbox_id, public_id, kind, name, status
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'active')
        "#,
    )
    .bind(channel_id)
    .bind(actor.tenant_id)
    .bind(inbox.0)
    .bind(request.inbox_id)
    .bind(public_id)
    .bind(kind.as_str())
    .bind(&name)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO widget_configs (
            channel_connection_id, tenant_id, allowed_origins, theme, greeting, launcher,
            default_language, translations, notify_on_new_visitor,
            attachments_enabled
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(channel_id)
    .bind(actor.tenant_id)
    .bind(&allowed_origins)
    .bind(SqlJson(&theme))
    .bind(&greeting)
    .bind(SqlJson(&launcher))
    .bind(request.default_language.as_str())
    .bind(SqlJson(&translations))
    .bind(request.notify_on_new_visitor)
    .bind(request.attachments_enabled)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, 'channel.widget.created',
                  'channel', $5, jsonb_build_object(
                      'inbox_id', $6::uuid,
                      'public_id', $7::uuid,
                      'name', $8::text
                  ))
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(inbox.0)
    .bind(actor.actor_id)
    .bind(channel_id)
    .bind(request.inbox_id)
    .bind(public_id)
    .bind(&name)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let channel = load_channel(&state, actor.tenant_id, channel_id).await?;
    Ok((StatusCode::CREATED, Json(channel)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateWidgetChannelRequest {
    name: Option<String>,
    allowed_origins: Vec<String>,
    default_language: WidgetLanguage,
    translations: WidgetTranslations,
    launcher: WidgetLauncher,
    theme: WidgetTheme,
    notify_on_new_visitor: bool,
    #[serde(default)]
    attachments_enabled: Option<bool>,
}

async fn update_widget_channel(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(channel_id): Path<Uuid>,
    Json(request): Json<UpdateWidgetChannelRequest>,
) -> Result<Json<ChannelResponse>, AppError> {
    actor.require("channels:manage")?;
    let scope = sqlx::query_as::<_, (Uuid, Uuid, String)>(
        r#"
        SELECT project_id, inbox_id, kind
        FROM channel_connections
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(scope.0, scope.1)?;
    if scope.2 != "widget" {
        return Err(AppError::BadRequest(
            "only widget channels have widget settings".to_owned(),
        ));
    }
    let name = request
        .name
        .map(|name| normalize_required_text(name, 200))
        .transpose()?;
    let allowed_origins = validate_origins(request.allowed_origins)?;
    let translations = request.translations.validate()?;
    if !translations.contains(&request.default_language) {
        return Err(AppError::BadRequest(
            "default_language must exist in translations".to_owned(),
        ));
    }
    let default_translation = translations.get(&request.default_language)?;
    let greeting = default_translation.greeting.clone();
    let mut launcher = request.launcher.validate()?;
    launcher
        .label
        .clone_from(&default_translation.launcher_label);
    let theme = request.theme.validate()?;
    let mut transaction = state.db.begin().await?;
    let attachments_enabled = sqlx::query_scalar::<_, bool>(
        r#"
        UPDATE widget_configs
        SET allowed_origins = $3, greeting = $4, launcher = $5, theme = $6,
            default_language = $7, translations = $8,
            notify_on_new_visitor = $9,
            attachments_enabled = COALESCE($10, attachments_enabled),
            updated_at = now()
        WHERE tenant_id = $1 AND channel_connection_id = $2
        RETURNING attachments_enabled
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .bind(&allowed_origins)
    .bind(&greeting)
    .bind(SqlJson(&launcher))
    .bind(SqlJson(&theme))
    .bind(request.default_language.as_str())
    .bind(SqlJson(&translations))
    .bind(request.notify_on_new_visitor)
    .bind(request.attachments_enabled)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    sqlx::query(
        r#"
        UPDATE channel_connections
        SET name = COALESCE($3, name), updated_at = now()
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .bind(&name)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, 'channel.widget.updated',
                  'channel', $5, jsonb_build_object(
                      'allowed_origins', $6::text[],
                      'theme', $7::jsonb,
                      'launcher', $8::jsonb,
                      'notify_on_new_visitor', $9::boolean,
                      'attachments_enabled', $10::boolean,
                      'name', $11::text
                  ))
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(scope.0)
    .bind(actor.actor_id)
    .bind(channel_id)
    .bind(&allowed_origins)
    .bind(SqlJson(&theme))
    .bind(SqlJson(&launcher))
    .bind(request.notify_on_new_visitor)
    .bind(attachments_enabled)
    .bind(&name)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let channel = load_channel(&state, actor.tenant_id, channel_id).await?;
    Ok(Json(channel))
}

async fn update_channel_blacklist(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(channel_id): Path<Uuid>,
    Json(request): Json<BlacklistReply>,
) -> Result<Json<ChannelResponse>, AppError> {
    actor.require("channels:manage")?;
    let mut transaction = state.db.begin().await?;
    let (project_id, inbox_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT project_id, inbox_id FROM channel_connections WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(project_id, inbox_id)?;
    let reply = request.validate()?;
    sqlx::query(
        "UPDATE channel_connections SET blacklist_reply = $3, updated_at = now() WHERE tenant_id = $1 AND id = $2",
    )
    .bind(actor.tenant_id)
    .bind(channel_id)
    .bind(SqlJson(&reply))
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action, resource_kind, resource_id, metadata
        ) VALUES ($1, $2, $3, $4, 'channel.blacklist.updated', 'channel', $5,
                  jsonb_build_object('inbox_id', $6::uuid, 'default_language', $7::text,
                                     'languages', $8::text[]))
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(actor.actor_id)
    .bind(channel_id)
    .bind(inbox_id)
    .bind(&reply.default_language)
    .bind(reply.translations.keys().cloned().collect::<Vec<_>>())
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(
        load_channel(&state, actor.tenant_id, channel_id).await?,
    ))
}

async fn load_channel(
    state: &AppState,
    tenant_id: Uuid,
    channel_id: Uuid,
) -> Result<ChannelResponse, AppError> {
    sqlx::query_as::<_, ChannelResponse>(
        r#"
        SELECT connection.id, connection.project_id, connection.inbox_id,
               connection.public_id, connection.kind, connection.name,
               connection.status, connection.blacklist_reply, widget.allowed_origins, widget.greeting,
               widget.default_language, widget.translations, widget.launcher, widget.theme,
               widget.notify_on_new_visitor, widget.attachments_enabled,
               CASE WHEN connection.kind = 'telegram_bot'
                    THEN connection.config ->> 'bot_username' END
                   AS telegram_bot_username,
               CASE WHEN connection.kind = 'telegram_bot' THEN EXISTS (
                   SELECT 1 FROM channel_secrets AS secret
                   WHERE secret.tenant_id = connection.tenant_id
                     AND secret.channel_connection_id = connection.id
               ) END AS telegram_bot_token_configured,
               CASE WHEN connection.kind = 'telegram_bot'
                    THEN connection.config ->> 'webhook_url' END
                   AS telegram_webhook_url,
               connection.updated_at
        FROM channel_connections AS connection
        LEFT JOIN widget_configs AS widget
          ON widget.tenant_id = connection.tenant_id
         AND widget.channel_connection_id = connection.id
        WHERE connection.tenant_id = $1 AND connection.id = $2
          AND connection.deleted_at IS NULL
        "#,
    )
    .bind(tenant_id)
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

fn validate_origins(origins: Vec<String>) -> Result<Vec<String>, AppError> {
    if origins.is_empty() || origins.len() > 50 {
        return Err(AppError::BadRequest(
            "allowed_origins must contain between 1 and 50 origins".to_owned(),
        ));
    }
    origins
        .into_iter()
        .map(|origin| {
            let origin = origin.trim();
            let uri = origin
                .parse::<Uri>()
                .map_err(|_| AppError::BadRequest(format!("invalid origin: {origin}")))?;
            if !matches!(uri.scheme_str(), Some("http" | "https"))
                || uri.authority().is_none()
                || uri.path() != "/"
                || uri.query().is_some()
            {
                return Err(AppError::BadRequest(format!("invalid origin: {origin}")));
            }
            Ok(origin.trim_end_matches('/').to_owned())
        })
        .collect()
}

fn normalize_required_text(value: String, max_chars: usize) -> Result<String, AppError> {
    let value = value.trim();
    let length = value.chars().count();
    if length == 0 || length > max_chars {
        return Err(AppError::BadRequest(format!(
            "text must contain between 1 and {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        CreateChannelKind, normalize_contact_search, normalize_required_text, validate_origins,
    };

    #[test]
    fn accepts_only_implemented_channel_kinds() {
        assert_eq!(
            CreateChannelKind::parse("widget").unwrap(),
            CreateChannelKind::Widget
        );
        assert!(CreateChannelKind::parse("telegram_bot").is_err());
    }

    #[test]
    fn normalizes_required_channel_names() {
        assert_eq!(
            normalize_required_text("  Support widget  ".to_owned(), 200).unwrap(),
            "Support widget"
        );
        assert!(normalize_required_text("  ".to_owned(), 200).is_err());
        assert!(normalize_required_text("too long".to_owned(), 3).is_err());
    }

    #[test]
    fn normalizes_contact_search() {
        assert_eq!(normalize_contact_search(None).unwrap(), None);
        assert_eq!(
            normalize_contact_search(Some("  ".to_owned())).unwrap(),
            None
        );
        assert_eq!(
            normalize_contact_search(Some("  Anna@example.com  ".to_owned())).unwrap(),
            Some("Anna@example.com".to_owned())
        );
        assert!(normalize_contact_search(Some("x".repeat(201))).is_err());
    }

    #[test]
    fn validates_browser_origins() {
        assert_eq!(
            validate_origins(vec!["https://example.com/".to_owned()]).unwrap(),
            ["https://example.com"]
        );
        assert!(validate_origins(vec!["javascript:alert(1)".to_owned()]).is_err());
        assert!(validate_origins(vec!["https://example.com/?token=secret".to_owned()]).is_err());
        assert!(validate_origins(Vec::new()).is_err());
    }
}
