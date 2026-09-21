//! Manual and automatic handoff of operator conversations to AI.

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::Utc;
use serde_json::json;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{
    ConversationAiAgentResponse, ConversationScope, ReturnConversationToAiResponse, insert_outbox,
    load_conversation_scope, publish_best_effort,
};
use crate::{
    AppState,
    auth::ActorContext,
    error::AppError,
    provider_reply::{self, ContactMessageTrigger},
    realtime::RealtimeEvent,
};

#[derive(Debug, FromRow)]
struct ResumableAiProfileRow {
    ai_profile_id: Uuid,
    display_name: String,
    avatar_url: Option<String>,
    profile_languages: String,
}

#[derive(Debug, FromRow)]
struct UnansweredOperatorConversation {
    id: Uuid,
    operator_id: Uuid,
    #[sqlx(flatten)]
    scope: ConversationScope,
}

/// Returns conversations to AI after 30 minutes of waiting for an operator reply.
/// Assignment and reopening give the operator a fresh response window. Customer
/// follow-ups and read receipts do not restart it. Conversations without an
/// eligible AI keep their operator and are retried on the next maintenance pass.
///
/// # Errors
/// Returns an error if a handoff or its durable reply/realtime events cannot be saved.
pub async fn return_unanswered_operator_conversations_to_ai(
    db: &PgPool,
) -> Result<usize, AppError> {
    let mut after_id: Option<Uuid> = None;
    let mut returned = 0;
    let failure_patterns: Vec<String> = provider_reply::PROVIDER_FAILURE_PREFIXES
        .iter()
        .map(|prefix| format!("{prefix}%"))
        .collect();
    // Page past ineligible AI profiles so they cannot starve later conversations.
    loop {
        let mut transaction = db.begin().await?;
        let conversations = sqlx::query_as::<_, UnansweredOperatorConversation>(
            r#"
            SELECT conversation.id, conversation.tenant_id, conversation.project_id,
                   conversation.inbox_id, conversation.contact_id,
                   conversation.channel_connection_id, assignment.user_id AS operator_id
            FROM conversations AS conversation
            JOIN conversation_assignments AS assignment
              ON assignment.tenant_id = conversation.tenant_id
             AND assignment.conversation_id = conversation.id
             AND assignment.user_id IS NOT NULL
             AND assignment.unassigned_at IS NULL
            JOIN contacts AS contact
              ON contact.tenant_id = conversation.tenant_id
             AND contact.id = conversation.contact_id
             AND NOT contact.is_blocked
            JOIN LATERAL (
                SELECT message.created_at
                FROM messages AS message
                WHERE message.tenant_id = conversation.tenant_id
                  AND message.conversation_id = conversation.id
                  AND message.direction = 'inbound'
                  AND message.author_kind = 'contact'
                  AND message.kind IN ('text', 'attachment')
                  AND message.sequence > COALESCE((
                      SELECT reply.sequence
                      FROM messages AS reply
                      WHERE reply.tenant_id = conversation.tenant_id
                        AND reply.conversation_id = conversation.id
                        AND reply.direction = 'outbound'
                        AND reply.author_kind IN ('operator', 'ai')
                        AND reply.kind IN ('text', 'attachment')
                        AND reply.status <> 'failed'
                        AND (reply.author_kind <> 'ai' OR NOT (
                            TRANSLATE(
                                REPLACE(REGEXP_REPLACE(LTRIM(reply.body, $3), $4, ' ', 'g'), '’', CHR(39)),
                                'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'
                            ) LIKE ANY($2::text[])
                        ))
                      ORDER BY reply.sequence DESC
                      LIMIT 1
                  ), 0)
                ORDER BY message.sequence
                LIMIT 1
            ) AS waiting ON true
            WHERE conversation.status <> 'resolved'
              AND ($1::uuid IS NULL OR conversation.id > $1)
              AND GREATEST(assignment.assigned_at, waiting.created_at,
                           conversation.last_reopened_at) <= now() - interval '30 minutes'
            ORDER BY conversation.id
            FOR UPDATE OF conversation SKIP LOCKED
            LIMIT 100
            "#,
        )
        .bind(after_id)
        .bind(&failure_patterns)
        .bind(format!("{}⚠️", provider_reply::PROVIDER_FAILURE_WHITESPACE))
        .bind(format!("[{}]+", provider_reply::PROVIDER_FAILURE_WHITESPACE))
        .fetch_all(&mut *transaction)
        .await?;
        for conversation in &conversations {
            if handoff_to_ai(
                &mut transaction,
                &conversation.scope,
                conversation.id,
                conversation.operator_id,
                None,
            )
            .await?
            .is_some()
            {
                returned += 1;
            }
        }
        transaction.commit().await?;
        if conversations.len() < 100 {
            break;
        }
        after_id = conversations.last().map(|conversation| conversation.id);
    }
    Ok(returned)
}

pub(super) async fn return_conversation_to_ai(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
) -> Result<Json<ReturnConversationToAiResponse>, AppError> {
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
            "resolved conversations cannot be returned to AI".to_owned(),
        ));
    }

    let assigned_operator = sqlx::query_scalar::<_, Uuid>(
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
    let assigned_operator = assigned_operator.ok_or_else(|| {
        AppError::Conflict("the conversation is not assigned to an operator".to_owned())
    })?;
    if assigned_operator != actor.actor_id {
        actor.require_session_admin()?;
    }
    let (response, event) = handoff_to_ai(
        &mut transaction,
        &scope,
        conversation_id,
        assigned_operator,
        Some(actor.actor_id),
    )
    .await?
    .ok_or_else(|| {
        AppError::Conflict("no active AI agent is available for this conversation".to_owned())
    })?;
    transaction.commit().await?;
    publish_best_effort(&state, &event).await;
    Ok(Json(response))
}

// The caller must hold the conversation row lock until this transaction commits.
async fn handoff_to_ai(
    transaction: &mut Transaction<'_, Postgres>,
    scope: &ConversationScope,
    conversation_id: Uuid,
    assigned_operator: Uuid,
    actor_id: Option<Uuid>,
) -> Result<Option<(ReturnConversationToAiResponse, RealtimeEvent)>, AppError> {
    let conversation_language = sqlx::query_scalar::<_, String>(
        r#"
        SELECT COALESCE(
            (
                SELECT session.language
                FROM widget_sessions AS session
                WHERE session.tenant_id = conversation.tenant_id
                  AND session.project_id = conversation.project_id
                  AND session.inbox_id = conversation.inbox_id
                  AND session.channel_connection_id = conversation.channel_connection_id
                  AND session.contact_id = conversation.contact_id
                ORDER BY session.created_at DESC, session.id DESC
                LIMIT 1
            ),
            NULLIF(btrim(conversation.widget_language), ''),
            NULLIF(btrim(widget.default_language), ''),
            'en'
        )
        FROM conversations AS conversation
        LEFT JOIN widget_configs AS widget
          ON widget.tenant_id = conversation.tenant_id
         AND widget.channel_connection_id = conversation.channel_connection_id
        WHERE conversation.tenant_id = $1 AND conversation.id = $2
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    let ai_profile = sqlx::query_as::<_, ResumableAiProfileRow>(
        r#"
        SELECT profile.id AS ai_profile_id,
               public_identity.display_name,
               '/public/v1/avatars/' || public_identity.avatar_public_id::text AS avatar_url,
               profile.language AS profile_languages
        FROM ai_profiles AS profile
        LEFT JOIN LATERAL (
            SELECT participant.joined_at
            FROM conversation_participants AS participant
            WHERE participant.tenant_id = profile.tenant_id
              AND participant.conversation_id = $2
              AND participant.participant_kind = 'ai'
              AND participant.ai_profile_id = profile.id
              AND participant.left_at IS NOT NULL
            ORDER BY participant.joined_at DESC, participant.id DESC
            LIMIT 1
        ) AS previous ON true
        JOIN ai_provider_connections AS provider
          ON provider.tenant_id = profile.tenant_id
         AND provider.id = profile.provider_connection_id
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
        JOIN LATERAL (
            SELECT identity.display_name,
                   identity_avatar.public_id AS avatar_public_id
            FROM ai_profile_public_identities AS identity
            LEFT JOIN ai_profile_public_identity_avatars AS identity_avatar
              ON identity_avatar.tenant_id = identity.tenant_id
             AND identity_avatar.ai_profile_id = identity.ai_profile_id
             AND identity_avatar.language = identity.language
            WHERE identity.tenant_id = profile.tenant_id
              AND identity.ai_profile_id = profile.id
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
        ) AS public_identity ON true
        WHERE profile.tenant_id = $1
          AND profile.project_id = $3
          AND (previous.joined_at IS NOT NULL OR profile.auto_join_new_conversations)
          AND profile.status = 'active'
          AND provider.status = 'active'
          AND provider.provider_kind IN ('openai', 'openai_compatible')
          AND connection.status = 'active'
          AND connection.deleted_at IS NULL
          AND inbox.status = 'active'
        ORDER BY previous.joined_at DESC NULLS LAST, channel_assignment.created_at, profile.id
        FOR SHARE OF channel_assignment, connection, inbox, profile, provider
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(scope.project_id)
    .bind(scope.channel_connection_id)
    .bind(scope.inbox_id)
    .bind(&conversation_language)
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .find(|profile| {
        provider_reply::profile_supports_widget_language(
            &profile.profile_languages,
            Some(&conversation_language),
        )
    });
    let Some(ai_profile) = ai_profile else {
        return Ok(None);
    };

    let now = Utc::now();
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
    .bind(assigned_operator)
    .bind(now)
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
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(assigned_operator)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO conversation_participants (
            id, tenant_id, conversation_id, participant_kind, ai_profile_id, joined_at
        ) VALUES ($1, $2, $3, 'ai', $4, $5)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(ai_profile.ai_profile_id)
    .bind(now)
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
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(now)
    .execute(&mut **transaction)
    .await?;

    let latest_message = sqlx::query_as::<_, (Uuid, i64, String, String, String)>(
        r#"
        SELECT id, sequence, direction, author_kind, kind
        FROM messages
        WHERE tenant_id = $1 AND conversation_id = $2
          AND direction IN ('inbound', 'outbound')
          AND author_kind IN ('contact', 'operator', 'ai')
          AND kind IN ('text', 'attachment')
          AND status <> 'failed'
          AND (author_kind <> 'ai' OR NOT (
              TRANSLATE(
                  REPLACE(REGEXP_REPLACE(LTRIM(body, $4), $5, ' ', 'g'), '’', CHR(39)),
                  'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'
              ) LIKE ANY($3::text[])
          ))
        ORDER BY sequence DESC
        LIMIT 1
        "#,
    )
    .bind(scope.tenant_id)
    .bind(conversation_id)
    .bind(
        provider_reply::PROVIDER_FAILURE_PREFIXES
            .iter()
            .map(|prefix| format!("{prefix}%"))
            .collect::<Vec<_>>(),
    )
    .bind(format!("{}⚠️", provider_reply::PROVIDER_FAILURE_WHITESPACE))
    .bind(format!(
        "[{}]+",
        provider_reply::PROVIDER_FAILURE_WHITESPACE
    ))
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some((message_id, sequence, direction, author_kind, kind)) = latest_message
        && direction == "inbound"
        && author_kind == "contact"
        && matches!(kind.as_str(), "text" | "attachment")
    {
        provider_reply::enqueue_for_contact_message(
            transaction,
            ContactMessageTrigger {
                tenant_id: scope.tenant_id,
                project_id: scope.project_id,
                inbox_id: scope.inbox_id,
                contact_id: scope.contact_id,
                channel_connection_id: scope.channel_connection_id,
                conversation_id,
                message_id,
                sequence,
                widget_language: Some(conversation_language.clone()),
            },
        )
        .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO audit_log (
            id, tenant_id, project_id, actor_id, action,
            resource_kind, resource_id, metadata, occurred_at
        ) VALUES (
            $1, $2, $3, $4, $8,
            'conversation', $5, $6, $7
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(actor_id)
    .bind(conversation_id)
    .bind(json!({
        "ai_profile_id": ai_profile.ai_profile_id,
        "previous_operator_id": assigned_operator,
        "reason": if actor_id.is_some() { "manual" } else { "operator_reply_timeout" },
    }))
    .bind(now)
    .bind(if actor_id.is_some() {
        "conversation.returned_to_ai"
    } else {
        "conversation.auto_returned_to_ai"
    })
    .execute(&mut **transaction)
    .await?;

    let event = RealtimeEvent {
        event_id: Uuid::now_v7(),
        tenant_id: scope.tenant_id,
        project_id: scope.project_id,
        inbox_id: scope.inbox_id,
        contact_id: Some(scope.contact_id),
        event_type: "conversation.ai_joined".to_owned(),
        aggregate_id: conversation_id,
        sequence: None,
        occurred_at: now,
        data: json!({ "conversation_id": conversation_id }),
    };
    insert_outbox(transaction, &event).await?;
    Ok(Some((
        ReturnConversationToAiResponse {
            ai_agent: ConversationAiAgentResponse {
                display_name: ai_profile.display_name,
                avatar_url: ai_profile.avatar_url,
                joined_at: now,
                active: true,
            },
            returned_at: now,
        },
        event,
    )))
}
