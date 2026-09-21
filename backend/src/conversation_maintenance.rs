//! Background maintenance for inactive conversations.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    inbox_routing,
    provider_reply::{self, OPERATOR_AUTO_RESOLUTION_EVENT_TYPE, OperatorAutoResolutionTrigger},
    realtime::RealtimeEvent,
};

/// Conversations are automatically resolved after this period without messages.
pub const CONVERSATION_INACTIVITY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Conversations receive an AI-generated operator closing message and are
/// resolved after this period when the latest message is a non-failed operator reply.
pub(crate) const OPERATOR_REPLY_AUTO_RESOLVE_TTL: Duration = Duration::from_secs(60 * 60);

/// Bounds row locks and transaction size while clearing an accumulated backlog.
pub const INACTIVE_CONVERSATION_BATCH_SIZE: usize = 100;

#[derive(Debug, FromRow)]
struct InactiveConversation {
    id: Uuid,
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    contact_id: Uuid,
    last_message_sequence: i64,
    last_activity_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct DueOperatorReply {
    tenant_id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    contact_id: Uuid,
    channel_connection_id: Uuid,
    conversation_id: Uuid,
    operator_id: Uuid,
    triggering_message_id: Uuid,
    triggering_sequence: i64,
    widget_language: Option<String>,
}

impl InactiveConversation {
    const fn resolution_reason(&self) -> &'static str {
        if self.last_message_sequence == 0 {
            "empty_conversation_timeout"
        } else {
            "conversation_inactivity_timeout"
        }
    }

    const fn audit_action(&self) -> &'static str {
        if self.last_message_sequence == 0 {
            "conversation.auto_resolved_empty"
        } else {
            "conversation.auto_resolved_inactive"
        }
    }
}

/// Schedules AI-generated operator closing messages for conversations whose
/// latest operator reply has been unanswered for one hour.
///
/// The provider worker rechecks the triggering message before generation and
/// again before saving the message and resolution. A customer response or
/// manual reopening therefore cancels the pending automatic close.
///
/// # Errors
///
/// Returns an error when due conversations cannot be selected or their durable
/// provider jobs cannot be stored.
pub async fn schedule_operator_auto_resolutions(db: &PgPool) -> Result<usize> {
    let mut transaction = db
        .begin()
        .await
        .context("failed to start operator auto-resolution scheduling transaction")?;
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
        .fetch_one(&mut *transaction)
        .await
        .context("failed to read the database clock for operator auto-resolution")?;
    let cutoff = operator_reply_cutoff(now)?;
    let batch_limit = i64::try_from(INACTIVE_CONVERSATION_BATCH_SIZE)
        .context("operator auto-resolution batch size exceeds PostgreSQL bigint")?;
    let conversations = sqlx::query_as::<_, DueOperatorReply>(
        r#"
        SELECT conversation.tenant_id, conversation.project_id,
               conversation.inbox_id, conversation.contact_id,
               conversation.channel_connection_id,
               conversation.id AS conversation_id,
               latest_message.author_id AS operator_id,
               latest_message.id AS triggering_message_id,
               latest_message.sequence AS triggering_sequence,
               conversation.widget_language
        FROM conversations AS conversation
        JOIN messages AS latest_message
          ON latest_message.tenant_id = conversation.tenant_id
         AND latest_message.conversation_id = conversation.id
         AND latest_message.sequence = conversation.last_message_sequence
        WHERE conversation.status <> 'resolved'
          AND latest_message.direction = 'outbound'
          AND latest_message.author_kind = 'operator'
          AND latest_message.author_id IS NOT NULL
          AND latest_message.kind IN ('text', 'attachment')
          AND latest_message.status <> 'failed'
          AND GREATEST(
              COALESCE(conversation.last_message_at, conversation.created_at),
              conversation.last_reopened_at
          ) <= $3
          AND NOT EXISTS (
              SELECT 1
              FROM outbox_events AS pending_close
              WHERE pending_close.tenant_id = conversation.tenant_id
                AND pending_close.aggregate_type = $1
                AND pending_close.aggregate_id = conversation.id
                AND pending_close.event_type = $2
                AND pending_close.payload->>'triggering_sequence'
                    = conversation.last_message_sequence::text
          )
        ORDER BY GREATEST(
                     COALESCE(conversation.last_message_at, conversation.created_at),
                     conversation.last_reopened_at
                 ),
                 conversation.id
        FOR UPDATE OF conversation SKIP LOCKED
        LIMIT $4
        "#,
    )
    .bind(provider_reply::AGGREGATE_TYPE)
    .bind(OPERATOR_AUTO_RESOLUTION_EVENT_TYPE)
    .bind(cutoff)
    .bind(batch_limit)
    .fetch_all(&mut *transaction)
    .await
    .context("failed to select due operator auto-resolutions")?;

    for conversation in &conversations {
        provider_reply::enqueue_operator_auto_resolution(
            &mut transaction,
            OperatorAutoResolutionTrigger {
                tenant_id: conversation.tenant_id,
                project_id: conversation.project_id,
                inbox_id: conversation.inbox_id,
                contact_id: conversation.contact_id,
                channel_connection_id: conversation.channel_connection_id,
                conversation_id: conversation.conversation_id,
                operator_id: conversation.operator_id,
                triggering_message_id: conversation.triggering_message_id,
                triggering_sequence: conversation.triggering_sequence,
                widget_language: conversation.widget_language.clone(),
            },
        )
        .await
        .map_err(anyhow::Error::new)
        .context("failed to enqueue an operator auto-resolution")?;
    }
    transaction
        .commit()
        .await
        .context("failed to commit operator auto-resolution scheduling")?;
    Ok(conversations.len())
}

/// Resolves one batch of conversations that reached the inactivity TTL.
///
/// Candidates are locked with `SKIP LOCKED`, so multiple worker processes can
/// run this job safely. Message creation locks the same conversation row before
/// incrementing its sequence and updating `last_message_at`, preventing a
/// conversation from being resolved concurrently with a new message.
/// Manual reopening restarts the timeout through `last_reopened_at`.
/// A recently joined AI also gets time to reply after an overdue operator handoff.
///
/// An automatic inactivity timeout is audited but does not create a rateable
/// support-resolution cycle or show a rating prompt to the visitor.
///
/// # Errors
///
/// Returns an error when `PostgreSQL` cannot select or resolve the batch, or an
/// outbox event cannot be serialized and persisted.
pub async fn auto_resolve_inactive(db: &PgPool) -> Result<usize> {
    let mut transaction = db
        .begin()
        .await
        .context("failed to start inactive-conversation maintenance transaction")?;
    let resolved_at = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
        .fetch_one(&mut *transaction)
        .await
        .context("failed to read the database clock for conversation maintenance")?;
    let cutoff = inactivity_cutoff(resolved_at)?;
    let batch_limit = i64::try_from(INACTIVE_CONVERSATION_BATCH_SIZE)
        .context("inactive-conversation batch size exceeds PostgreSQL bigint")?;

    let conversations = sqlx::query_as::<_, InactiveConversation>(
        r#"
        SELECT conversation.id, conversation.tenant_id, conversation.project_id,
               conversation.inbox_id, conversation.contact_id,
               conversation.last_message_sequence,
               GREATEST(COALESCE(conversation.last_message_at, conversation.created_at),
                        conversation.last_reopened_at) AS last_activity_at
        FROM conversations AS conversation
        WHERE conversation.status <> 'resolved'
          AND GREATEST(COALESCE(conversation.last_message_at, conversation.created_at),
                       conversation.last_reopened_at) <= $1
          AND NOT EXISTS (
              SELECT 1
              FROM conversation_participants AS ai
              WHERE ai.tenant_id = conversation.tenant_id
                AND ai.conversation_id = conversation.id
                AND ai.participant_kind = 'ai'
                AND ai.left_at IS NULL
                AND ai.joined_at > $1
          )
          AND NOT EXISTS (
              SELECT 1
              FROM messages AS latest_message
              WHERE latest_message.tenant_id = conversation.tenant_id
                AND latest_message.conversation_id = conversation.id
                AND latest_message.sequence = conversation.last_message_sequence
                AND latest_message.direction = 'outbound'
                AND latest_message.author_kind = 'operator'
                AND latest_message.kind IN ('text', 'attachment')
                AND latest_message.status <> 'failed'
          )
        ORDER BY GREATEST(COALESCE(conversation.last_message_at, conversation.created_at),
                          conversation.last_reopened_at),
                 conversation.id
        FOR UPDATE OF conversation SKIP LOCKED
        LIMIT $2
        "#,
    )
    .bind(cutoff)
    .bind(batch_limit)
    .fetch_all(&mut *transaction)
    .await
    .context("failed to select inactive conversations for automatic resolution")?;

    for conversation in &conversations {
        resolve_inactive_conversation(&mut transaction, conversation, resolved_at).await?;
    }

    transaction
        .commit()
        .await
        .context("failed to commit automatic inactive-conversation resolutions")?;
    Ok(conversations.len())
}

fn inactivity_cutoff(now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let ttl = chrono::Duration::from_std(CONVERSATION_INACTIVITY_TTL)
        .context("conversation inactivity TTL exceeds chrono duration")?;
    now.checked_sub_signed(ttl)
        .context("conversation inactivity cutoff exceeds the supported timestamp range")
}

fn operator_reply_cutoff(now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let ttl = chrono::Duration::from_std(OPERATOR_REPLY_AUTO_RESOLVE_TTL)
        .context("operator reply auto-resolution TTL exceeds chrono duration")?;
    now.checked_sub_signed(ttl)
        .context("operator reply auto-resolution cutoff exceeds the supported timestamp range")
}

async fn resolve_inactive_conversation(
    transaction: &mut Transaction<'_, Postgres>,
    conversation: &InactiveConversation,
    resolved_at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE conversations
        SET status = 'resolved', resolved_at = $3,
            updated_at = $3, version = version + 1
        WHERE tenant_id = $1 AND id = $2
        "#,
    )
    .bind(conversation.tenant_id)
    .bind(conversation.id)
    .bind(resolved_at)
    .execute(&mut **transaction)
    .await
    .context("failed to mark an inactive conversation as resolved")?;
    inbox_routing::on_conversation_resolved(transaction, conversation.tenant_id, conversation.id)
        .await
        .map_err(anyhow::Error::new)
        .context("failed to close Inbox SLA after automatic conversation resolution")?;

    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata, occurred_at
        ) VALUES (
            $1, $2, $3, NULL, $4,
            'conversation', $5, $6, $7
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(conversation.tenant_id)
    .bind(conversation.project_id)
    .bind(conversation.audit_action())
    .bind(conversation.id)
    .bind(json!({
        "reason": conversation.resolution_reason(),
        "message_count": conversation.last_message_sequence,
        "last_activity_at": conversation.last_activity_at,
        "ttl_seconds": CONVERSATION_INACTIVITY_TTL.as_secs(),
    }))
    .bind(resolved_at)
    .execute(&mut **transaction)
    .await
    .context("failed to audit automatic inactive-conversation resolution")?;

    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: conversation.tenant_id,
        project_id: conversation.project_id,
        inbox_id: conversation.inbox_id,
        contact_id: Some(conversation.contact_id),
        event_type: "conversation.resolved".to_owned(),
        aggregate_id: conversation.id,
        sequence: None,
        occurred_at: resolved_at,
        data: json!({
            "conversation_id": conversation.id,
            "automatic": true,
            "reason": conversation.resolution_reason(),
        }),
    };
    insert_outbox(transaction, &event).await
}

async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    event: &RealtimeEvent,
) -> Result<()> {
    let payload = serde_json::to_value(event)
        .context("failed to serialize automatic conversation-resolution event")?;
    sqlx::query(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, aggregate_type, aggregate_id, event_type, payload,
            status, available_at, created_at
        ) VALUES ($1, $2, 'realtime', $3, $4, $5, 'pending', now(), now())
        "#,
    )
    .bind(event.event_id)
    .bind(event.tenant_id)
    .bind(event.aggregate_id)
    .bind(&event.event_type)
    .bind(payload)
    .execute(&mut **transaction)
    .await
    .context("failed to enqueue automatic conversation-resolution event")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CONVERSATION_INACTIVITY_TTL, InactiveConversation, OPERATOR_REPLY_AUTO_RESOLVE_TTL,
        auto_resolve_inactive, inactivity_cutoff, operator_reply_cutoff,
        schedule_operator_auto_resolutions,
    };
    use chrono::{TimeZone, Utc};
    use sqlx::PgPool;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn conversation_inactivity_cutoff_is_exactly_twenty_four_hours() {
        let now = Utc.with_ymd_and_hms(2026, 8, 14, 3, 15, 24).unwrap();

        assert_eq!(CONVERSATION_INACTIVITY_TTL, Duration::from_secs(86_400));
        assert_eq!(
            inactivity_cutoff(now).unwrap(),
            Utc.with_ymd_and_hms(2026, 8, 13, 3, 15, 24).unwrap()
        );
    }

    #[test]
    fn operator_reply_cutoff_is_exactly_one_hour() {
        let now = Utc.with_ymd_and_hms(2026, 8, 14, 3, 15, 24).unwrap();

        assert_eq!(OPERATOR_REPLY_AUTO_RESOLVE_TTL, Duration::from_secs(3_600));
        assert_eq!(
            operator_reply_cutoff(now).unwrap(),
            Utc.with_ymd_and_hms(2026, 8, 14, 2, 15, 24).unwrap()
        );
    }

    #[test]
    fn automatic_resolution_reason_distinguishes_empty_and_inactive_conversations() {
        let empty = conversation_with_message_sequence(0);
        let inactive = conversation_with_message_sequence(4);

        assert_eq!(empty.resolution_reason(), "empty_conversation_timeout");
        assert_eq!(empty.audit_action(), "conversation.auto_resolved_empty");
        assert_eq!(
            inactive.resolution_reason(),
            "conversation_inactivity_timeout"
        );
        assert_eq!(
            inactive.audit_action(),
            "conversation.auto_resolved_inactive"
        );
    }

    #[sqlx::test]
    #[ignore = "requires a PostgreSQL role that can create disposable test databases"]
    async fn operator_replies_are_scheduled_separately_from_other_inactivity(db: PgPool) {
        sqlx::raw_sql(
            r#"
            INSERT INTO tenants (id, name)
            VALUES ('00000000-0000-4000-8000-000000000001', 'Test tenant');

            INSERT INTO projects (id, tenant_id, name, slug)
            VALUES (
                '00000000-0000-4000-8000-000000000002',
                '00000000-0000-4000-8000-000000000001',
                'Test project',
                'test-project'
            );

            INSERT INTO inboxes (id, tenant_id, project_id, name)
            VALUES (
                '00000000-0000-4000-8000-000000000003',
                '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002',
                'Test inbox'
            );

            INSERT INTO channel_connections (
                id, tenant_id, project_id, inbox_id, public_id, kind, name
            ) VALUES (
                '00000000-0000-4000-8000-000000000004',
                '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002',
                '00000000-0000-4000-8000-000000000003',
                '00000000-0000-4000-8000-000000000005',
                'widget',
                'Test widget'
            );

            INSERT INTO contacts (id, tenant_id, project_id, display_name)
            VALUES (
                '00000000-0000-4000-8000-000000000006',
                '00000000-0000-4000-8000-000000000001',
                '00000000-0000-4000-8000-000000000002',
                'Test contact'
            );

            INSERT INTO conversations (
                id, tenant_id, project_id, inbox_id, channel_connection_id,
                contact_id, status, last_message_sequence, last_message_at,
                created_at, updated_at
            ) VALUES
                (
                    '00000000-0000-4000-8000-000000000010',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000006',
                    'new', 0, NULL, now() - interval '25 hours', now()
                ),
                (
                    '00000000-0000-4000-8000-000000000011',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000006',
                    'open', 1, now() - interval '25 hours',
                    now() - interval '48 hours', now()
                ),
                (
                    '00000000-0000-4000-8000-000000000012',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000006',
                    'open', 1, now() - interval '23 hours',
                    now() - interval '48 hours', now() - interval '48 hours'
                ),
                (
                    '00000000-0000-4000-8000-000000000013',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000006',
                    'open', 1, now() - interval '61 minutes',
                    now() - interval '48 hours', now()
                ),
                (
                    '00000000-0000-4000-8000-000000000014',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000006',
                    'open', 1, now() - interval '59 minutes',
                    now() - interval '48 hours', now()
                ),
                (
                    '00000000-0000-4000-8000-000000000015',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000004',
                    '00000000-0000-4000-8000-000000000006',
                    'open', 1, now() - interval '61 minutes',
                    now() - interval '48 hours', now()
                );

            INSERT INTO messages (
                id, tenant_id, project_id, inbox_id, conversation_id, sequence,
                direction, kind, author_kind, author_id, body, created_at
            ) VALUES
                (
                    '00000000-0000-4000-8000-000000000021',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000011',
                    1, 'inbound', 'text', 'contact',
                    '00000000-0000-4000-8000-000000000006',
                    'Inactive message', now() - interval '25 hours'
                ),
                (
                    '00000000-0000-4000-8000-000000000022',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000012',
                    1, 'inbound', 'text', 'contact',
                    '00000000-0000-4000-8000-000000000006',
                    'Active message', now() - interval '23 hours'
                ),
                (
                    '00000000-0000-4000-8000-000000000023',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000013',
                    1, 'outbound', 'text', 'operator',
                    '00000000-0000-4000-8000-000000000007',
                    'Old operator reply', now() - interval '61 minutes'
                ),
                (
                    '00000000-0000-4000-8000-000000000024',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000014',
                    1, 'outbound', 'text', 'operator',
                    '00000000-0000-4000-8000-000000000007',
                    'Recent operator reply', now() - interval '59 minutes'
                ),
                (
                    '00000000-0000-4000-8000-000000000025',
                    '00000000-0000-4000-8000-000000000001',
                    '00000000-0000-4000-8000-000000000002',
                    '00000000-0000-4000-8000-000000000003',
                    '00000000-0000-4000-8000-000000000015',
                    1, 'inbound', 'text', 'contact',
                    '00000000-0000-4000-8000-000000000006',
                    'Recent customer message', now() - interval '61 minutes'
                );
            "#,
        )
        .execute(&db)
        .await
        .unwrap();

        assert_eq!(schedule_operator_auto_resolutions(&db).await.unwrap(), 1);
        assert_eq!(schedule_operator_auto_resolutions(&db).await.unwrap(), 0);
        let scheduled = sqlx::query_as::<_, (Uuid, String)>(
            r#"
            SELECT aggregate_id, payload->>'triggering_sequence'
            FROM outbox_events
            WHERE event_type = 'provider.operator_auto_resolution.requested'
            "#,
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(
            scheduled,
            (
                Uuid::parse_str("00000000-0000-4000-8000-000000000013").unwrap(),
                "1".to_owned(),
            )
        );

        assert_eq!(auto_resolve_inactive(&db).await.unwrap(), 2);

        let statuses = sqlx::query_as::<_, (Uuid, String)>(
            r#"
            SELECT id, status
            FROM conversations
            ORDER BY id
            "#,
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(
            statuses,
            vec![
                (
                    Uuid::parse_str("00000000-0000-4000-8000-000000000010").unwrap(),
                    "resolved".to_owned(),
                ),
                (
                    Uuid::parse_str("00000000-0000-4000-8000-000000000011").unwrap(),
                    "resolved".to_owned(),
                ),
                (
                    Uuid::parse_str("00000000-0000-4000-8000-000000000012").unwrap(),
                    "open".to_owned(),
                ),
                (
                    Uuid::parse_str("00000000-0000-4000-8000-000000000013").unwrap(),
                    "open".to_owned(),
                ),
                (
                    Uuid::parse_str("00000000-0000-4000-8000-000000000014").unwrap(),
                    "open".to_owned(),
                ),
                (
                    Uuid::parse_str("00000000-0000-4000-8000-000000000015").unwrap(),
                    "open".to_owned(),
                ),
            ]
        );

        let audit_reasons = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT action, metadata->>'reason'
            FROM audit_log
            ORDER BY resource_id
            "#,
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(
            audit_reasons,
            vec![
                (
                    "conversation.auto_resolved_empty".to_owned(),
                    "empty_conversation_timeout".to_owned(),
                ),
                (
                    "conversation.auto_resolved_inactive".to_owned(),
                    "conversation_inactivity_timeout".to_owned(),
                ),
            ]
        );
    }

    fn conversation_with_message_sequence(last_message_sequence: i64) -> InactiveConversation {
        InactiveConversation {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            project_id: Uuid::nil(),
            inbox_id: Uuid::nil(),
            contact_id: Uuid::nil(),
            last_message_sequence,
            last_activity_at: Utc.with_ymd_and_hms(2026, 8, 13, 3, 15, 24).unwrap(),
        }
    }
}
