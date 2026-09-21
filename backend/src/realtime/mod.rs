//! Authenticated realtime tickets and WebSocket fan-out.

use std::{net::SocketAddr, time::Duration};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{
        ConnectInfo, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::sync::broadcast;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        ActorContext, WidgetSessionContext, authenticate_realtime_actor_for_project,
        generate_token, hash_token, load_widget_session,
    },
    error::AppError,
};

const WEBSOCKET_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);

/// Event delivered to authenticated realtime clients.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RealtimeEvent {
    pub event_id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub inbox_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub aggregate_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i64>,
    pub occurred_at: DateTime<Utc>,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct TicketQuery {
    ticket: String,
}

#[derive(Debug, Deserialize)]
struct OperatorTicketRequest {
    inbox_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct TicketResponse {
    ticket: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct TicketScope {
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    contact_id: Option<Uuid>,
    inbox_scope: Option<Vec<Uuid>>,
    can_read_visitor_network: bool,
    credential_kind: Option<String>,
    credential_id: Option<Uuid>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/widget/v1/realtime-tickets", post(widget_ticket))
        .route("/api/v1/realtime-tickets", post(operator_ticket))
        .route("/ws", get(websocket))
}

async fn widget_ticket(
    State(state): State<AppState>,
    session: WidgetSessionContext,
) -> Result<Json<TicketResponse>, AppError> {
    let token = generate_token();
    let expires_at = Utc::now()
        + chrono::Duration::from_std(state.config.realtime.ticket_ttl())
            .map_err(AppError::internal)?;
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO realtime_tickets (
            id, tenant_id, project_id, contact_id, inbox_scope, token_hash, expires_at,
            can_read_visitor_network, credential_kind, credential_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, false, 'widget_session', $8)
        "#,
    )
    .bind(id)
    .bind(session.tenant_id)
    .bind(session.project_id)
    .bind(session.contact_id)
    .bind(vec![session.inbox_id])
    .bind(hash_token(&token))
    .bind(expires_at)
    .bind(session.session_id)
    .execute(&state.db)
    .await?;

    Ok(Json(TicketResponse {
        ticket: token,
        expires_at,
    }))
}

async fn operator_ticket(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<OperatorTicketRequest>,
) -> Result<Json<TicketResponse>, AppError> {
    actor.require("conversations:read")?;
    if let Some(inbox_id) = request.inbox_id {
        let project_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT project_id FROM inboxes WHERE tenant_id = $1 AND id = $2",
        )
        .bind(actor.tenant_id)
        .bind(inbox_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
        actor.require_inbox(project_id, inbox_id)?;
    }
    let inbox_scope = operator_ticket_inbox_scope(request.inbox_id, actor.inbox_scope());
    let (credential_kind, credential_id) = actor.realtime_credential();

    let token = generate_token();
    let can_read_visitor_network = actor.require("visitor_network:read").is_ok();
    let expires_at = Utc::now()
        + chrono::Duration::from_std(state.config.realtime.ticket_ttl())
            .map_err(AppError::internal)?;
    sqlx::query(
        r#"
        INSERT INTO realtime_tickets (
            id, tenant_id, project_id, actor_id, inbox_scope, token_hash, expires_at,
            can_read_visitor_network, credential_kind, credential_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor.tenant_id)
    .bind(actor.project_id)
    .bind(actor.actor_id)
    .bind(inbox_scope)
    .bind(hash_token(&token))
    .bind(expires_at)
    .bind(can_read_visitor_network)
    .bind(credential_kind)
    .bind(credential_id)
    .execute(&state.db)
    .await?;

    Ok(Json(TicketResponse {
        ticket: token,
        expires_at,
    }))
}

fn operator_ticket_inbox_scope(
    requested_inbox_id: Option<Uuid>,
    actor_inbox_scope: Option<&[Uuid]>,
) -> Option<Vec<Uuid>> {
    requested_inbox_id
        .map(|inbox_id| vec![inbox_id])
        .or_else(|| actor_inbox_scope.map(<[Uuid]>::to_vec))
}

async fn websocket(
    ws: WebSocketUpgrade,
    Query(query): Query<TicketQuery>,
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let scope = sqlx::query_as::<_, TicketScope>(
        r#"
        UPDATE realtime_tickets
        SET consumed_at = now()
        WHERE token_hash = $1
          AND expires_at > now()
          AND consumed_at IS NULL
        RETURNING tenant_id, project_id, actor_id, contact_id, inbox_scope,
                  can_read_visitor_network, credential_kind, credential_id
        "#,
    )
    .bind(hash_token(&query.ticket))
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    scope.revalidate(&state).await?;
    let demo_login_ip = scope.demo_ip(&state).await?;
    if let Some(expected_ip) = demo_login_ip.as_deref() {
        let peer = peer.ok_or(AppError::Forbidden)?.0.0;
        let current =
            crate::client_ip::resolve(peer.ip(), &headers, &state.config.server.trusted_proxies)?;
        if expected_ip.parse::<std::net::IpAddr>().ok() != Some(current.address) {
            return Err(AppError::Forbidden);
        }
    }
    Ok(ws.on_upgrade(move |socket| serve_socket(socket, scope, state, demo_login_ip)))
}

async fn serve_socket(
    mut socket: WebSocket,
    scope: TicketScope,
    state: AppState,
    demo_login_ip: Option<String>,
) {
    let mut events = state.realtime.subscribe();
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + WEBSOCKET_HEARTBEAT_INTERVAL,
        WEBSOCKET_HEARTBEAT_INTERVAL,
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if scope.revalidate(&state).await.is_err() {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
            received = events.recv() => {
                match received {
                    Ok(event) if scope.allows(&event) => {
                        if scope.revalidate(&state).await.is_err() {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                        if let (true, Some(ip)) = (scope.actor_id.is_some(), demo_login_ip.as_deref()) {
                                match crate::demo_access::contact_is_visible(
                                    &state.db, scope.tenant_id, ip, event.contact_id,
                                ).await {
                                    Ok(true) => {}
                                    Ok(false) => continue,
                                    Err(_) => {
                                        let _ = socket.send(Message::Close(None)).await;
                                        break;
                                    }
                                }
                        }
                        let Ok(payload) = serde_json::to_string(&event) else {
                            continue;
                        };
                        if socket.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

impl TicketScope {
    async fn demo_ip(&self, state: &AppState) -> Result<Option<String>, AppError> {
        let kind = self
            .credential_kind
            .as_deref()
            .ok_or(AppError::Unauthorized)?;
        let id = self.credential_id.ok_or(AppError::Unauthorized)?;
        if kind == "widget_session" {
            let session = load_widget_session(state, None, Some(id)).await?;
            if session.channel_connection_id == crate::demo::DEMO_CHANNEL_ID {
                return session.client_ip.map(Some).ok_or(AppError::Forbidden);
            }
        } else {
            let actor =
                authenticate_realtime_actor_for_project(state, kind, id, self.project_id).await?;
            if actor.is_demo() {
                return actor
                    .demo_login_ip()
                    .map(|ip| Some(ip.to_owned()))
                    .ok_or(AppError::Forbidden);
            }
        }
        Ok(None)
    }

    async fn revalidate(&self, state: &AppState) -> Result<(), AppError> {
        let kind = self
            .credential_kind
            .as_deref()
            .ok_or(AppError::Unauthorized)?;
        let id = self.credential_id.ok_or(AppError::Unauthorized)?;
        if kind == "widget_session" {
            let session = load_widget_session(state, None, Some(id)).await?;
            if self.actor_id.is_some()
                || self.tenant_id != session.tenant_id
                || self.project_id != Some(session.project_id)
                || self.contact_id != Some(session.contact_id)
                || self.inbox_scope.as_deref() != Some(&[session.inbox_id])
                || self.can_read_visitor_network
            {
                return Err(AppError::Unauthorized);
            }
        } else {
            let actor =
                authenticate_realtime_actor_for_project(state, kind, id, self.project_id).await?;
            actor.require("conversations:read")?;
            if self.actor_id != Some(actor.actor_id)
                || self.tenant_id != actor.tenant_id
                || self.project_id != actor.project_id
                || self.contact_id.is_some()
                || !inbox_scope_still_allowed(self.inbox_scope.as_deref(), actor.inbox_scope())
                || (self.can_read_visitor_network && actor.require("visitor_network:read").is_err())
            {
                return Err(AppError::Unauthorized);
            }
        }
        Ok(())
    }

    fn allows(&self, event: &RealtimeEvent) -> bool {
        (event.event_type != "draft.updated" || self.actor_id.is_some())
            && (!event.event_type.starts_with("visitor.") || self.can_read_visitor_network)
            && self.tenant_id == event.tenant_id
            && self.project_id.is_none_or(|id| id == event.project_id)
            && self
                .contact_id
                .is_none_or(|contact_id| event.contact_id == Some(contact_id))
            && self
                .inbox_scope
                .as_ref()
                .is_none_or(|scope| scope.contains(&event.inbox_id))
    }
}

fn inbox_scope_still_allowed(ticket: Option<&[Uuid]>, current: Option<&[Uuid]>) -> bool {
    match (ticket, current) {
        (_, None) => true,
        (Some(ticket), Some(current)) => ticket.iter().all(|id| current.contains(id)),
        (None, Some(_)) => false,
    }
}

/// Starts Redis Pub/Sub fan-out for this API instance.
pub fn spawn_redis_subscriber(state: AppState) {
    let Some(client) = state.redis.clone() else {
        return;
    };

    tokio::spawn(async move {
        loop {
            if let Err(error) =
                subscribe_once(&client, &state.realtime, crate::config::REALTIME_CHANNEL).await
            {
                error!(?error, "Redis realtime subscriber stopped");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

async fn subscribe_once(
    client: &redis::Client,
    sender: &broadcast::Sender<RealtimeEvent>,
    channel: &str,
) -> anyhow::Result<()> {
    let mut pubsub = client.get_async_pubsub().await?;
    pubsub.subscribe(channel).await?;
    let mut stream = pubsub.on_message();

    while let Some(message) = stream.next().await {
        let payload: String = message.get_payload()?;
        match serde_json::from_str::<RealtimeEvent>(&payload) {
            Ok(event) => {
                let _ = sender.send(event);
            }
            Err(error) => warn!(?error, "ignored malformed realtime event"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::{Json, extract::State};
    use chrono::Utc;
    use serde_json::json;
    use sqlx::PgPool;
    use tokio::io::AsyncReadExt;
    use uuid::Uuid;

    use super::{
        OperatorTicketRequest, RealtimeEvent, TicketScope, inbox_scope_still_allowed,
        operator_ticket, operator_ticket_inbox_scope,
    };
    use crate::{
        AppState, Config,
        auth::{authenticate_realtime_actor, hash_token},
    };

    fn id(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    async fn credential_test_state(db: PgPool) -> AppState {
        sqlx::raw_sql(&format!(
            r#"
            INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Realtime security');
            INSERT INTO users (id, email, display_name)
                VALUES ('{user}', 'realtime@example.test', 'Operator');
            INSERT INTO projects (id, tenant_id, name, slug)
                VALUES ('{project}', '{tenant}', 'Realtime', 'realtime');
            INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
                VALUES ('{tenant}', '{project}', 'operator', 'Operator', 'operator',
                    ARRAY['conversations:read', 'visitor_network:read']);
            INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
                VALUES ('{membership}', '{tenant}', '{project}', '{user}', 'operator');
            INSERT INTO inboxes (id, tenant_id, project_id, name) VALUES
                ('{inbox}', '{tenant}', '{project}', 'Support'),
                ('{other_inbox}', '{tenant}', '{project}', 'Other');
            INSERT INTO channel_connections (id, tenant_id, project_id, inbox_id, public_id,
                                             kind, name, status)
                VALUES ('{channel}', '{tenant}', '{project}', '{inbox}', '{channel}',
                        'widget', 'Widget', 'active');
            INSERT INTO contacts (id, tenant_id, project_id)
                VALUES ('{contact}', '{tenant}', '{project}');
            INSERT INTO widget_sessions (id, tenant_id, project_id, inbox_id,
                channel_connection_id, contact_id, token_hash, origin, expires_at)
                VALUES ('{widget_session}', '{tenant}', '{project}', '{inbox}',
                        '{channel}', '{contact}', decode('01', 'hex'), 'https://example.test',
                        now() + interval '1 hour');
            INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id,
                token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at)
                VALUES ('{session}', '{tenant}', '{project}', '{user}', '{membership}',
                        decode('02', 'hex'), decode('03', 'hex'), now() + interval '1 hour',
                        now() + interval '2 hours');
            "#,
            tenant = id(1),
            project = id(2),
            inbox = id(3),
            other_inbox = id(4),
            user = id(5),
            channel = id(6),
            contact = id(7),
            membership = id(8),
            session = id(10),
            widget_session = id(11),
        ))
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, role, expires_at) VALUES ($1, $2, $3, $4, 'Test', $5, ARRAY['conversations:read', 'visitor_network:read'], 'operator', now() + interval '1 hour')",
        )
        .bind(id(9)).bind(id(1)).bind(id(2)).bind(id(5)).bind(hash_token("realtime-test-key"))
        .execute(&db).await.unwrap();
        let mut config: Config = toml::from_str(include_str!("../../Config.example.toml")).unwrap();
        config.pg.migrate_on_start = false;
        config.redis.url = None;
        config.attachments.enabled = false;
        let mut state = AppState::build(config).await.unwrap();
        state.db = db;
        state
    }

    fn operator_scope(kind: &str, credential_id: Uuid) -> TicketScope {
        TicketScope {
            tenant_id: id(1),
            project_id: Some(id(2)),
            actor_id: Some(id(5)),
            contact_id: None,
            inbox_scope: Some(vec![id(3)]),
            can_read_visitor_network: true,
            credential_kind: Some(kind.to_owned()),
            credential_id: Some(credential_id),
        }
    }

    #[test]
    fn refreshed_inbox_scope_cannot_widen_or_preserve_revoked_access() {
        assert!(inbox_scope_still_allowed(Some(&[id(3)]), None));
        assert!(inbox_scope_still_allowed(
            Some(&[id(3)]),
            Some(&[id(3), id(4)])
        ));
        assert!(!inbox_scope_still_allowed(Some(&[id(3)]), Some(&[id(4)])));
        assert!(!inbox_scope_still_allowed(None, Some(&[id(3)])));
        assert!(!inbox_scope_still_allowed(Some(&[id(3)]), Some(&[])));
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn tickets_recheck_expiry_revocation_membership_and_current_permissions(db: PgPool) {
        let state = credential_test_state(db.clone()).await;
        let session = operator_scope("operator_session", id(10));
        let key = operator_scope("access_token", id(9));
        let idle_before: chrono::DateTime<Utc> =
            sqlx::query_scalar("SELECT idle_expires_at FROM operator_sessions WHERE id = $1")
                .bind(id(10))
                .fetch_one(&db)
                .await
                .unwrap();
        session.revalidate(&state).await.unwrap();
        key.revalidate(&state).await.unwrap();
        let idle_after: chrono::DateTime<Utc> =
            sqlx::query_scalar("SELECT idle_expires_at FROM operator_sessions WHERE id = $1")
                .bind(id(10))
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(
            idle_before, idle_after,
            "socket validation must not extend idle expiration"
        );
        let unchanged_usage: Option<chrono::DateTime<Utc>> =
            sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = $1")
                .bind(id(9))
                .fetch_one(&db)
                .await
                .unwrap();
        assert!(
            unchanged_usage.is_none(),
            "socket validation must not write last_used_at"
        );
        for (change, restore) in [
            (
                "UPDATE operator_sessions SET idle_expires_at = now() - interval '1 second'",
                "UPDATE operator_sessions SET idle_expires_at = now() + interval '1 hour'",
            ),
            (
                "UPDATE operator_sessions SET idle_expires_at = now() - interval '1 second', absolute_expires_at = now() - interval '1 second'",
                "UPDATE operator_sessions SET idle_expires_at = now() + interval '1 hour', absolute_expires_at = now() + interval '2 hours'",
            ),
            (
                "UPDATE operator_sessions SET revoked_at = now()",
                "UPDATE operator_sessions SET revoked_at = NULL",
            ),
            (
                "UPDATE project_roles SET permissions = ARRAY['visitor_network:read']",
                "UPDATE project_roles SET permissions = ARRAY['conversations:read', 'visitor_network:read']",
            ),
        ] {
            sqlx::query(change).execute(&db).await.unwrap();
            assert!(session.revalidate(&state).await.is_err(), "{change}");
            sqlx::query(restore).execute(&db).await.unwrap();
            session.revalidate(&state).await.unwrap();
        }
        for (change, restore) in [
            (
                "UPDATE api_keys SET expires_at = now() - interval '1 second'",
                "UPDATE api_keys SET expires_at = now() + interval '1 hour'",
            ),
            (
                "UPDATE api_keys SET revoked_at = now()",
                "UPDATE api_keys SET revoked_at = NULL",
            ),
            (
                "UPDATE api_keys SET permissions = ARRAY['conversations:read']",
                "UPDATE api_keys SET permissions = ARRAY['conversations:read', 'visitor_network:read']",
            ),
            (
                "UPDATE api_keys SET permissions = ARRAY['visitor_network:read']",
                "UPDATE api_keys SET permissions = ARRAY['conversations:read', 'visitor_network:read']",
            ),
            (
                "UPDATE api_keys SET inbox_scope = ARRAY[]::uuid[]",
                "UPDATE api_keys SET inbox_scope = NULL",
            ),
        ] {
            sqlx::query(change).execute(&db).await.unwrap();
            assert!(key.revalidate(&state).await.is_err(), "{change}");
            sqlx::query(restore).execute(&db).await.unwrap();
            key.revalidate(&state).await.unwrap();
        }
        for (change, restore) in [
            (
                "UPDATE memberships SET revoked_at = now()",
                "UPDATE memberships SET revoked_at = NULL",
            ),
            (
                "UPDATE users SET status = 'disabled'",
                "UPDATE users SET status = 'active'",
            ),
            (
                "UPDATE projects SET status = 'disabled'",
                "UPDATE projects SET status = 'active'",
            ),
        ] {
            sqlx::query(change).execute(&db).await.unwrap();
            assert!(key.revalidate(&state).await.is_err(), "{change}");
            assert!(session.revalidate(&state).await.is_err(), "{change}");
            sqlx::query(restore).execute(&db).await.unwrap();
        }
        let mut legacy = key;
        legacy.credential_kind = None;
        legacy.credential_id = None;
        assert!(legacy.revalidate(&state).await.is_err());
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn session_tickets_remain_bound_to_their_project_windows(db: PgPool) {
        let state = credential_test_state(db.clone()).await;
        sqlx::raw_sql(&format!(
            r#"
            INSERT INTO projects (id, tenant_id, name, slug)
            VALUES ('{project}', '{tenant}', 'Second project', 'second-project');
            INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
            VALUES ('{tenant}', '{project}', 'operator', 'Operator', 'operator',
                    ARRAY['conversations:read', 'visitor_network:read']);
            INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
            VALUES ('{membership}', '{tenant}', '{project}', '{user}', 'operator');
            INSERT INTO departments (id, tenant_id, project_id, name) VALUES
                ('{department}', '{tenant}', '{project}', 'Support'),
                ('{other_department}', '{tenant}', '{project}', 'Sales');
            UPDATE projects SET default_department_id = '{department}' WHERE id = '{project}';
            INSERT INTO inboxes (id, tenant_id, project_id, department_id, name) VALUES
                ('{inbox}', '{tenant}', '{project}', '{department}', 'Support'),
                ('{other_inbox}', '{tenant}', '{project}', '{other_department}', 'Sales');
            INSERT INTO operator_session_projects (session_id, tenant_id, project_id,
                membership_id, department_id)
            VALUES ('{session}', '{tenant}', '{project}', '{membership}', '{department}');
        "#,
            tenant = id(1),
            project = id(12),
            membership = id(13),
            user = id(5),
            department = id(14),
            other_department = id(15),
            inbox = id(16),
            other_inbox = id(17),
            session = id(10)
        ))
        .execute(&db)
        .await
        .unwrap();
        let first = operator_scope("operator_session", id(10));
        let mut second = operator_scope("operator_session", id(10));
        second.project_id = Some(id(12));
        second.inbox_scope = Some(vec![id(16)]);
        first.revalidate(&state).await.unwrap();
        second.revalidate(&state).await.unwrap();

        sqlx::query(
            "UPDATE operator_sessions SET project_id = $1, membership_id = $2 WHERE id = $3",
        )
        .bind(id(12))
        .bind(id(13))
        .bind(id(10))
        .execute(&db)
        .await
        .unwrap();
        first.revalidate(&state).await.unwrap();
        second.revalidate(&state).await.unwrap();
        sqlx::query(
            "UPDATE operator_session_projects SET department_id = $1 WHERE project_id = $2",
        )
        .bind(id(15))
        .bind(id(12))
        .execute(&db)
        .await
        .unwrap();
        first.revalidate(&state).await.unwrap();
        assert!(second.revalidate(&state).await.is_err());
        second.inbox_scope = Some(vec![id(17)]);
        second.revalidate(&state).await.unwrap();

        sqlx::query("UPDATE memberships SET revoked_at = now() WHERE id = $1")
            .bind(id(13))
            .execute(&db)
            .await
            .unwrap();
        assert!(second.revalidate(&state).await.is_err());
        first.revalidate(&state).await.unwrap();
        sqlx::query("UPDATE memberships SET revoked_at = NULL WHERE id = $1")
            .bind(id(13))
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("UPDATE projects SET status = 'disabled' WHERE id = $1")
            .bind(id(12))
            .execute(&db)
            .await
            .unwrap();
        assert!(second.revalidate(&state).await.is_err());
        first.revalidate(&state).await.unwrap();
        sqlx::query("UPDATE projects SET status = 'active' WHERE id = $1")
            .bind(id(12))
            .execute(&db)
            .await
            .unwrap();
        second.revalidate(&state).await.unwrap();
        sqlx::query("UPDATE operator_sessions SET revoked_at = now() WHERE id = $1")
            .bind(id(10))
            .execute(&db)
            .await
            .unwrap();
        assert!(first.revalidate(&state).await.is_err());
        assert!(second.revalidate(&state).await.is_err());
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn widget_ticket_rechecks_its_original_session_and_contact(db: PgPool) {
        let state = credential_test_state(db.clone()).await;
        let mut scope = TicketScope {
            tenant_id: id(1),
            project_id: Some(id(2)),
            actor_id: None,
            contact_id: Some(id(7)),
            inbox_scope: Some(vec![id(3)]),
            can_read_visitor_network: false,
            credential_kind: Some("widget_session".to_owned()),
            credential_id: Some(id(11)),
        };
        scope.revalidate(&state).await.unwrap();
        scope.contact_id = Some(id(5));
        assert!(scope.revalidate(&state).await.is_err());
        scope.contact_id = Some(id(7));
        sqlx::query("UPDATE widget_sessions SET expires_at = now() - interval '1 second'")
            .execute(&db)
            .await
            .unwrap();
        assert!(scope.revalidate(&state).await.is_err());
        sqlx::query(
            "UPDATE widget_sessions SET expires_at = now() + interval '1 hour', revoked_at = now()",
        )
        .execute(&db)
        .await
        .unwrap();
        assert!(scope.revalidate(&state).await.is_err());
    }

    async fn read_server_frame(stream: &mut reqwest::Upgraded) -> (u8, Vec<u8>) {
        tokio::time::timeout(Duration::from_secs(5), async {
            let first = stream.read_u8().await.unwrap();
            let second = stream.read_u8().await.unwrap();
            assert_eq!(second & 0x80, 0, "server frames must be unmasked");
            let length = match second & 0x7f {
                126 => usize::from(stream.read_u16().await.unwrap()),
                127 => usize::try_from(stream.read_u64().await.unwrap()).unwrap(),
                value => usize::from(value),
            };
            assert!(length <= 16_384);
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await.unwrap();
            (first & 0x0f, bytes)
        })
        .await
        .expect("websocket response timed out")
    }

    async fn upgrade_response(
        client: &reqwest::Client,
        address: std::net::SocketAddr,
        ticket: &str,
    ) -> reqwest::Response {
        client
            .get(format!("http://{address}/ws"))
            .query(&[("ticket", ticket)])
            .header("connection", "upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap()
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn revoked_key_closes_live_socket_before_another_draft_and_denies_upgrade(db: PgPool) {
        let state = credential_test_state(db.clone()).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router().with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let actor = authenticate_realtime_actor(&state, "access_token", id(9))
            .await
            .unwrap();
        let mut tickets = Vec::new();
        for _ in 0..2 {
            tickets.push(
                operator_ticket(
                    State(state.clone()),
                    actor.clone(),
                    Json(OperatorTicketRequest {
                        inbox_id: Some(id(3)),
                    }),
                )
                .await
                .unwrap()
                .0
                .ticket,
            );
        }
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = upgrade_response(&client, address, &tickets[0]).await;
        assert_eq!(response.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);
        let mut stream = response.upgrade().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.realtime.receiver_count() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let event = RealtimeEvent {
            event_id: Uuid::now_v7(),
            tenant_id: id(1),
            project_id: id(2),
            inbox_id: id(3),
            contact_id: Some(id(7)),
            event_type: "draft.updated".to_owned(),
            aggregate_id: id(12),
            sequence: None,
            occurred_at: Utc::now(),
            data: json!({"body": "Allowed draft"}),
        };
        state.publish(&event).await.unwrap();
        let (opcode, body) = read_server_frame(&mut stream).await;
        assert_eq!(opcode, 1);
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["data"]["body"], "Allowed draft");
        sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id = $1")
            .bind(id(9))
            .execute(&db)
            .await
            .unwrap();
        state
            .publish(&RealtimeEvent {
                data: json!({"body": "Must stay private"}),
                ..event
            })
            .await
            .unwrap();
        let (opcode, _) = read_server_frame(&mut stream).await;
        assert_eq!(
            opcode, 8,
            "revoked clients must receive Close, not the next draft"
        );
        let rejected = upgrade_response(&client, address, &tickets[1]).await;
        assert_eq!(rejected.status(), reqwest::StatusCode::UNAUTHORIZED);
        server.abort();
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn demo_socket_checks_handshake_ip_and_never_sends_other_visitors_events(db: PgPool) {
        use crate::demo::{DEMO_CHANNEL_ID, DEMO_INBOX_ID};
        let mut config: Config = toml::from_str(include_str!("../../Config.example.toml")).unwrap();
        config.pg.migrate_on_start = false;
        config.redis.url = None;
        config.attachments.enabled = false;
        config.server.trusted_proxies = vec!["127.0.0.1/32".parse().unwrap()];
        let mut state = AppState::build(config).await.unwrap();
        state.db = db.clone();
        let (tenant_id, project_id): (Uuid, Uuid) =
            sqlx::query_as("SELECT tenant_id, project_id FROM channel_connections WHERE id = $1")
                .bind(DEMO_CHANNEL_ID)
                .fetch_one(&db)
                .await
                .unwrap();
        sqlx::query(
            r#"INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id,
                token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at, login_ip)
            SELECT $1, tenant_id, project_id, user_id, id, $2, $3,
                now() + interval '1 hour', now() + interval '2 hours', '203.0.113.1'::inet
            FROM memberships WHERE user_id = (SELECT id FROM users WHERE is_demo)"#,
        )
        .bind(id(150))
        .bind(hash_token("demo-socket"))
        .bind(hash_token("csrf"))
        .execute(&db)
        .await
        .unwrap();
        for (contact, session, address) in [
            (id(151), id(153), "203.0.113.1"),
            (id(152), id(154), "203.0.113.2"),
        ] {
            sqlx::query("INSERT INTO contacts (id, tenant_id, project_id) VALUES ($1, $2, $3)")
                .bind(contact)
                .bind(tenant_id)
                .bind(project_id)
                .execute(&db)
                .await
                .unwrap();
            sqlx::query(
                r#"INSERT INTO widget_sessions (id, tenant_id, project_id, inbox_id,
                    channel_connection_id, contact_id, token_hash, origin, expires_at,
                    client_ip, client_ip_source, visitor_data_expires_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, 'https://demo.tzomet.io',
                    now() + interval '1 hour', $8::text::inet, 'trusted_proxy', now() + interval '30 days')"#,
            )
            .bind(session)
            .bind(tenant_id)
            .bind(project_id)
            .bind(DEMO_INBOX_ID)
            .bind(DEMO_CHANNEL_ID)
            .bind(contact)
            .bind(hash_token(&session.to_string()))
            .bind(address)
            .execute(&db)
            .await
            .unwrap();
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router().with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let actor = authenticate_realtime_actor(&state, "operator_session", id(150))
            .await
            .unwrap();
        let mut stream = None;
        for (ip, expected) in [
            ("203.0.113.2", reqwest::StatusCode::FORBIDDEN),
            ("203.0.113.1", reqwest::StatusCode::SWITCHING_PROTOCOLS),
        ] {
            let ticket = operator_ticket(
                State(state.clone()),
                actor.clone(),
                Json(OperatorTicketRequest {
                    inbox_id: Some(DEMO_INBOX_ID),
                }),
            )
            .await
            .unwrap()
            .0
            .ticket;
            let response = client
                .get(format!("http://{address}/ws"))
                .query(&[("ticket", ticket)])
                .header("connection", "upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                .header("x-tzomet-client-ip", ip)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            if expected == reqwest::StatusCode::SWITCHING_PROTOCOLS {
                stream = Some(response.upgrade().await.unwrap());
            }
        }
        let mut stream = stream.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.realtime.receiver_count() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        for (contact, body) in [
            (Some(id(152)), "Private other IP"),
            (None, "Unscoped event"),
            (Some(id(151)), "Own visitor"),
        ] {
            state
                .publish(&RealtimeEvent {
                    event_id: Uuid::now_v7(),
                    tenant_id,
                    project_id,
                    inbox_id: DEMO_INBOX_ID,
                    contact_id: contact,
                    event_type: "message.created".to_owned(),
                    aggregate_id: id(160),
                    sequence: Some(1),
                    occurred_at: Utc::now(),
                    data: json!({"body": body}),
                })
                .await
                .unwrap();
        }
        let (opcode, body) = read_server_frame(&mut stream).await;
        assert_eq!(opcode, 1);
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["body"], "Own visitor");
        sqlx::query("UPDATE operator_sessions SET revoked_at = now() WHERE id = $1")
            .bind(id(150))
            .execute(&db)
            .await
            .unwrap();
        state
            .publish(&RealtimeEvent {
                event_id: Uuid::now_v7(),
                tenant_id,
                project_id,
                inbox_id: DEMO_INBOX_ID,
                contact_id: Some(id(151)),
                event_type: "message.created".to_owned(),
                aggregate_id: id(160),
                sequence: Some(2),
                occurred_at: Utc::now(),
                data: json!({"body": "After logout"}),
            })
            .await
            .unwrap();
        assert_eq!(read_server_frame(&mut stream).await.0, 8);
        server.abort();
    }

    #[test]
    fn project_ticket_preserves_an_explicit_actor_inbox_scope() {
        let allowed_inbox_id = Uuid::now_v7();
        let requested_inbox_id = Uuid::now_v7();
        let actor_scope = [allowed_inbox_id];

        assert_eq!(
            operator_ticket_inbox_scope(None, Some(&actor_scope)),
            Some(vec![allowed_inbox_id])
        );
        assert_eq!(operator_ticket_inbox_scope(None, None), None);
        assert_eq!(
            operator_ticket_inbox_scope(Some(requested_inbox_id), Some(&actor_scope)),
            Some(vec![requested_inbox_id])
        );
    }

    #[test]
    fn widget_ticket_filters_other_contacts() {
        let tenant_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let inbox_id = Uuid::now_v7();
        let contact_id = Uuid::now_v7();
        let scope = TicketScope {
            tenant_id,
            project_id: Some(project_id),
            actor_id: None,
            contact_id: Some(contact_id),
            inbox_scope: Some(vec![inbox_id]),
            can_read_visitor_network: false,
            credential_kind: Some("widget_session".to_owned()),
            credential_id: Some(Uuid::now_v7()),
        };
        let event = RealtimeEvent {
            event_id: Uuid::now_v7(),
            tenant_id,
            project_id,
            inbox_id,
            contact_id: Some(Uuid::now_v7()),
            event_type: "message.created".to_owned(),
            aggregate_id: Uuid::now_v7(),
            sequence: Some(1),
            occurred_at: Utc::now(),
            data: json!({}),
        };

        assert!(!scope.allows(&event));
    }

    #[test]
    fn visitor_events_require_network_permission_on_the_ticket() {
        let tenant_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let inbox_id = Uuid::now_v7();
        let contact_id = Uuid::now_v7();
        let event = RealtimeEvent {
            event_id: Uuid::now_v7(),
            tenant_id,
            project_id,
            inbox_id,
            contact_id: Some(contact_id),
            event_type: "visitor.entered".to_owned(),
            aggregate_id: Uuid::now_v7(),
            sequence: None,
            occurred_at: Utc::now(),
            data: json!({}),
        };
        let mut scope = TicketScope {
            tenant_id,
            project_id: Some(project_id),
            actor_id: None,
            contact_id: None,
            inbox_scope: Some(vec![inbox_id]),
            can_read_visitor_network: false,
            credential_kind: Some("widget_session".to_owned()),
            credential_id: Some(Uuid::now_v7()),
        };

        assert!(!scope.allows(&event));
        scope.can_read_visitor_network = true;
        assert!(scope.allows(&event));
    }

    #[test]
    fn draft_events_are_delivered_only_to_operators() {
        let tenant_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let inbox_id = Uuid::now_v7();
        let contact_id = Uuid::now_v7();
        let event = RealtimeEvent {
            event_id: Uuid::now_v7(),
            tenant_id,
            project_id,
            inbox_id,
            contact_id: Some(contact_id),
            event_type: "draft.updated".to_owned(),
            aggregate_id: Uuid::now_v7(),
            sequence: None,
            occurred_at: Utc::now(),
            data: json!({}),
        };
        let mut scope = TicketScope {
            tenant_id,
            project_id: Some(project_id),
            actor_id: None,
            contact_id: Some(contact_id),
            inbox_scope: Some(vec![inbox_id]),
            can_read_visitor_network: false,
            credential_kind: Some("widget_session".to_owned()),
            credential_id: Some(Uuid::now_v7()),
        };

        assert!(!scope.allows(&event));
        scope.actor_id = Some(Uuid::now_v7());
        scope.contact_id = None;
        assert!(scope.allows(&event));
    }
}
