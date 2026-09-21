//! Durable outbox claiming and delivery.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;
use sqlx::FromRow;
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    AppState, email,
    realtime::RealtimeEvent,
    telegram_bot,
    telegram_notifications::{
        self, ROUTING_TELEGRAM_EVENT_TYPE, RoutingTelegramNotificationPayload,
        TelegramNotificationPayload,
    },
};

#[derive(Debug, FromRow)]
struct OutboxJob {
    id: Uuid,
    aggregate_type: String,
    aggregate_id: Uuid,
    event_type: String,
    payload: Value,
    attempts: i32,
}

/// Claims one core event; mobile push has a separate worker lane so provider
/// latency cannot delay realtime events or channel replies.
///
/// # Errors
///
/// Returns an error when claiming or finalizing the job fails.
pub async fn process_once(state: &AppState, worker_id: Uuid) -> Result<bool> {
    process_queue_once(state, worker_id, false).await
}

/// Claims and processes at most one native mobile push delivery.
///
/// # Errors
///
/// Returns an error when claiming or finalizing the job fails.
pub async fn process_mobile_push_once(state: &AppState, worker_id: Uuid) -> Result<bool> {
    process_queue_once(state, worker_id, true).await
}

async fn process_queue_once(state: &AppState, worker_id: Uuid, mobile_push: bool) -> Result<bool> {
    let job = sqlx::query_as::<_, OutboxJob>(
        r#"
        WITH abandoned_mobile AS (
            UPDATE outbox_events
            SET status = 'failed', locked_at = NULL, locked_by = NULL,
                last_error = 'mobile push worker lease expired after final attempt'
            WHERE $3 AND aggregate_type = 'mobile_push' AND status = 'processing'
              AND locked_at < now() - interval '2 minutes' AND attempts >= $1
        ), candidate AS (
            SELECT id
            FROM outbox_events
            WHERE (($3 AND aggregate_type = 'mobile_push')
                  OR (NOT $3 AND aggregate_type IN ('realtime', 'email', 'telegram', 'telegram_bot')))
              AND (status = 'pending' OR ($3
                  AND status = 'processing' AND locked_at < now() - interval '2 minutes'))
              AND available_at <= now()
              AND attempts < $1
            ORDER BY available_at, id
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE outbox_events AS event
        SET status = 'processing', locked_at = now(), locked_by = $2,
            attempts = event.attempts + 1
        FROM candidate
        WHERE event.id = candidate.id
        RETURNING event.id, event.aggregate_type, event.aggregate_id,
                  event.event_type, event.payload, event.attempts
        "#,
    )
    .bind(state.config.outbox.max_attempts)
    .bind(worker_id)
    .bind(mobile_push)
    .fetch_optional(&state.db)
    .await
    .context("failed to claim outbox event")?;

    let Some(job) = job else {
        return Ok(false);
    };

    let result = deliver(state, &job).await;
    match result {
        Ok(()) => {
            sqlx::query(
                r#"
                UPDATE outbox_events
                SET status = 'completed', completed_at = now(),
                    locked_at = NULL, locked_by = NULL, last_error = NULL
                WHERE id = $1 AND locked_by = $2
                "#,
            )
            .bind(job.id)
            .bind(worker_id)
            .execute(&state.db)
            .await
            .context("failed to complete outbox event")?;
            info!(event_id = %job.id, "outbox event completed");
        }
        Err(delivery_error) => {
            let push_error = delivery_error.downcast_ref::<crate::mobile_push::PushDeliveryError>();
            let delay = if job.aggregate_type == "mobile_push" {
                let exponential = retry_delay(job.attempts)
                    .saturating_mul(60)
                    .min(Duration::from_secs(3600));
                let jitter = Duration::from_secs(rand::random_range(0..30));
                exponential.max(push_error.map_or(Duration::ZERO, |error| error.retry_after))
                    + jitter
            } else {
                retry_delay(job.attempts)
            };
            let terminal = job.attempts >= state.config.outbox.max_attempts
                || push_error.is_some_and(|error| error.permanent);
            let error_message = delivery_error.to_string();
            sqlx::query(
                r#"
                UPDATE outbox_events
                SET status = CASE WHEN $3 THEN 'failed' ELSE 'pending' END,
                    available_at = now() + ($4 * interval '1 second'),
                    locked_at = NULL, locked_by = NULL, last_error = $5
                WHERE id = $1 AND locked_by = $2
                "#,
            )
            .bind(job.id)
            .bind(worker_id)
            .bind(terminal)
            .bind(i64::try_from(delay.as_secs()).unwrap_or(i64::MAX))
            .bind(&error_message)
            .execute(&state.db)
            .await
            .context("failed to reschedule outbox event")?;
            error!(event_id = %job.id, attempts = job.attempts, terminal, error = %error_message, "outbox delivery failed");
            if terminal
                && job.aggregate_type == telegram_bot::OUTBOX_AGGREGATE_TYPE
                && let Err(mark_error) =
                    telegram_bot::mark_terminal_failure(state, &job.event_type, job.aggregate_id)
                        .await
            {
                error!(event_id = %job.id, error = ?mark_error, "failed to mark Telegram bot operation as terminal");
            }
        }
    }

    Ok(true)
}

async fn deliver(state: &AppState, job: &OutboxJob) -> Result<()> {
    if job.aggregate_type == "mobile_push" {
        let payload = serde_json::from_value(job.payload.clone())
            .context("outbox payload is not a mobile push notification")?;
        return crate::mobile_push::deliver(state, job.aggregate_id, payload).await;
    }
    if job.aggregate_type == telegram_bot::OUTBOX_AGGREGATE_TYPE {
        return telegram_bot::deliver_outbox(state, &job.event_type, job.aggregate_id).await;
    }
    if job.aggregate_type == "email" {
        return email::deliver_rating_invitation(state, job.aggregate_id).await;
    }
    if job.aggregate_type == "telegram" {
        if job.event_type == ROUTING_TELEGRAM_EVENT_TYPE {
            let payload: RoutingTelegramNotificationPayload =
                serde_json::from_value(job.payload.clone())
                    .context("outbox payload is not an Inbox Telegram notification")?;
            return telegram_notifications::deliver_routing(state, payload).await;
        }
        let payload: TelegramNotificationPayload = serde_json::from_value(job.payload.clone())
            .context("outbox payload is not a Telegram notification")?;
        return telegram_notifications::deliver(state, payload).await;
    }
    if job.aggregate_type != "realtime" {
        anyhow::bail!("unsupported outbox aggregate type");
    }
    let event: RealtimeEvent = serde_json::from_value(job.payload.clone())
        .context("outbox payload is not a realtime event")?;
    if event.event_type == "conversation.ai_joined"
        && !delayed_ai_join_is_still_current(state, &event).await?
    {
        info!(
            event_id = %event.event_id,
            conversation_id = %event.aggregate_id,
            "stale delayed AI join skipped"
        );
        return Ok(());
    }
    state.publish(&event).await?;

    if event.event_type == "message.created" {
        let direction = event
            .data
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let delivery_required = event
            .data
            .get("delivery_required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if direction == "outbound" && delivery_required {
            return Ok(());
        }
        let status = if direction == "inbound" {
            "delivered"
        } else {
            "sent"
        };
        let status = sqlx::query_scalar::<_, String>(
            "UPDATE messages SET status = CASE WHEN status IN ('read', 'delivered') \
             THEN status ELSE $3 END WHERE tenant_id = $1 AND id = $2 RETURNING status",
        )
        .bind(event.tenant_id)
        .bind(event.aggregate_id)
        .bind(status)
        .fetch_one(&state.db)
        .await
        .context("failed to update message delivery status")?;
        if direction == "outbound" {
            state
                .publish(&message_status_updated_event(&event, &status))
                .await
                .context("failed to publish message status update")?;
        }
    }

    Ok(())
}

fn message_status_updated_event(event: &RealtimeEvent, status: &str) -> RealtimeEvent {
    let mut data = event.data.as_object().cloned().unwrap_or_default();
    data.insert("status".to_owned(), Value::String(status.to_owned()));
    RealtimeEvent {
        event_id: Uuid::now_v7(),
        event_type: "message.status_updated".to_owned(),
        occurred_at: Utc::now(),
        data: Value::Object(data),
        ..event.clone()
    }
}

async fn delayed_ai_join_is_still_current(state: &AppState, event: &RealtimeEvent) -> Result<bool> {
    sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM conversations AS conversation
            JOIN conversation_participants AS participant
              ON participant.tenant_id = conversation.tenant_id
             AND participant.conversation_id = conversation.id
             AND participant.participant_kind = 'ai'
             AND participant.left_at IS NULL
             AND participant.joined_at <= now()
            JOIN ai_profile_channel_connections AS channel_assignment
              ON channel_assignment.tenant_id = conversation.tenant_id
             AND channel_assignment.ai_profile_id = participant.ai_profile_id
             AND channel_assignment.channel_connection_id = conversation.channel_connection_id
            JOIN ai_profiles AS profile
              ON profile.tenant_id = participant.tenant_id
             AND profile.id = participant.ai_profile_id
             AND profile.project_id = conversation.project_id
             AND profile.status = 'active'
            JOIN ai_provider_connections AS provider
              ON provider.tenant_id = profile.tenant_id
             AND provider.id = profile.provider_connection_id
             AND provider.status = 'active'
            JOIN channel_connections AS connection
              ON connection.tenant_id = channel_assignment.tenant_id
             AND connection.id = channel_assignment.channel_connection_id
             AND connection.project_id = conversation.project_id
             AND connection.inbox_id = conversation.inbox_id
             AND connection.status = 'active'
             AND connection.deleted_at IS NULL
            JOIN inboxes AS inbox
              ON inbox.tenant_id = connection.tenant_id
             AND inbox.project_id = connection.project_id
             AND inbox.id = connection.inbox_id
             AND inbox.status = 'active'
            WHERE conversation.tenant_id = $1
              AND conversation.id = $2
              AND conversation.status <> 'resolved'
              AND NOT EXISTS (
                  SELECT 1
                  FROM conversation_assignments AS assignment
                  WHERE assignment.tenant_id = conversation.tenant_id
                    AND assignment.conversation_id = conversation.id
                    AND assignment.user_id IS NOT NULL
                    AND assignment.unassigned_at IS NULL
              )
        )
        "#,
    )
    .bind(event.tenant_id)
    .bind(event.aggregate_id)
    .fetch_one(&state.db)
    .await
    .context("failed to validate delayed AI join")
}

pub(crate) fn retry_delay(attempt: i32) -> Duration {
    let exponent = u32::try_from(attempt.saturating_sub(1).clamp(0, 8)).unwrap_or(0);
    Duration::from_secs(2_u64.pow(exponent).min(300))
}

#[cfg(test)]
mod tests {
    use super::{message_status_updated_event, retry_delay};
    use crate::realtime::RealtimeEvent;
    use chrono::Utc;
    use serde_json::json;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn caps_exponential_retry_delay() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(4), Duration::from_secs(8));
        assert_eq!(retry_delay(100), Duration::from_secs(256));
    }

    #[test]
    fn status_update_event_preserves_message_scope_and_announces_status() {
        let created = RealtimeEvent {
            event_id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            inbox_id: Uuid::now_v7(),
            contact_id: Some(Uuid::now_v7()),
            event_type: "message.created".to_owned(),
            aggregate_id: Uuid::now_v7(),
            sequence: Some(7),
            occurred_at: Utc::now(),
            data: json!({
                "conversation_id": Uuid::now_v7(),
                "direction": "outbound",
            }),
        };

        let status_updated = message_status_updated_event(&created, "sent");

        assert_ne!(status_updated.event_id, created.event_id);
        assert_eq!(status_updated.event_type, "message.status_updated");
        assert_eq!(status_updated.tenant_id, created.tenant_id);
        assert_eq!(status_updated.project_id, created.project_id);
        assert_eq!(status_updated.inbox_id, created.inbox_id);
        assert_eq!(status_updated.contact_id, created.contact_id);
        assert_eq!(status_updated.aggregate_id, created.aggregate_id);
        assert_eq!(status_updated.sequence, created.sequence);
        assert_eq!(status_updated.data["status"], "sent");
        assert_eq!(
            status_updated.data["conversation_id"],
            created.data["conversation_id"]
        );
        assert_eq!(status_updated.data["direction"], "outbound");
    }
}
