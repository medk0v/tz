//! Stored blacklist replies and isolated language adaptation.

use super::*;
use anyhow::Context as _;

#[derive(Deserialize, Serialize)]
struct ReplyJob {
    profile_id: Uuid,
    text: String,
    language: Option<String>,
}

pub(super) async fn prepare_reply(
    transaction: &mut Transaction<'_, Postgres>,
    scope: &ConversationScope,
    conversation_id: Uuid,
    inbound_message_id: Uuid,
    language: Option<&str>,
) -> Result<Option<RealtimeEvent>, AppError> {
    let profile = sqlx::query_as::<_, (Uuid, String, bool)>(
        r#"
        SELECT profile.id, profile.blacklist_reply_text, profile.blacklist_reply_match_language
        FROM ai_profiles AS profile
        JOIN ai_profile_channel_connections AS assignment
          ON assignment.tenant_id = profile.tenant_id AND assignment.ai_profile_id = profile.id
        WHERE profile.tenant_id = $1 AND profile.project_id = $2
          AND assignment.channel_connection_id = $3 AND profile.status = 'active'
        ORDER BY (
            SELECT max(participant.joined_at) FROM conversation_participants AS participant
            WHERE participant.tenant_id = profile.tenant_id
              AND participant.conversation_id = $4 AND participant.ai_profile_id = profile.id
              AND participant.left_at IS NULL
        ) DESC NULLS LAST,
        (position(split_part(lower(replace(COALESCE($5, ''), '_', '-')), '-', 1)
            IN lower(profile.language)) > 0) DESC,
        profile.auto_join_new_conversations DESC, assignment.created_at, profile.id
        LIMIT 1
        "#,
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.channel_connection_id)
    .bind(conversation_id)
    .bind(language)
    .fetch_optional(&mut **transaction)
    .await?;
    let body = if let Some((profile_id, text, match_language)) =
        profile.filter(|(_, text, _)| !text.trim().is_empty())
    {
        if match_language {
            sqlx::query(
                "INSERT INTO outbox_events (id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES ($1,$2,'blacklist_reply',$3,'blacklist.reply.requested',$4)",
            )
            .bind(Uuid::now_v7()).bind(scope.tenant_id).bind(inbound_message_id)
            .bind(serde_json::to_value(ReplyJob { profile_id, text, language: language.map(str::to_owned) }).map_err(AppError::internal)?)
            .execute(&mut **transaction).await?;
            return Ok(None);
        }
        text
    } else {
        let config = sqlx::query_scalar::<_, SqlJson<BlacklistReply>>(
            "SELECT blacklist_reply FROM channel_connections WHERE tenant_id = $1 AND project_id = $2 AND id = $3",
        )
        .bind(scope.tenant_id).bind(scope.project_id).bind(scope.channel_connection_id)
        .fetch_one(&mut **transaction).await?;
        config.resolve(language).to_owned()
    };
    insert_message(transaction, scope, conversation_id, Uuid::now_v7(), &body)
        .await
        .map(Some)
}

#[derive(FromRow)]
struct ClaimedJob {
    id: Uuid,
    tenant_id: Uuid,
    aggregate_id: Uuid,
    payload: Value,
    attempts: i32,
}

/// Processes one blacklist translation independently from ordinary agent replies.
///
/// # Errors
/// Returns a database error when claiming or recording delivery fails.
pub async fn process_once(state: &AppState, worker_id: Uuid) -> anyhow::Result<bool> {
    let job = sqlx::query_as::<_, ClaimedJob>(
        r#"
        WITH candidate AS (
            SELECT id FROM outbox_events
            WHERE aggregate_type = 'blacklist_reply' AND available_at <= now()
              AND (status = 'pending' OR (status = 'processing' AND locked_at < now() - interval '2 minutes'))
            ORDER BY available_at, id FOR UPDATE SKIP LOCKED LIMIT 1
        )
        UPDATE outbox_events AS event
        SET status = 'processing', locked_at = now(), locked_by = $1, attempts = attempts + 1
        FROM candidate WHERE event.id = candidate.id
        RETURNING event.id,event.tenant_id,event.aggregate_id,event.payload,event.attempts
        "#,
    ).bind(worker_id).fetch_optional(&state.db).await?;
    let Some(job) = job else {
        return Ok(false);
    };
    let result = process_job(state, &job, worker_id).await;
    sqlx::query(
        r#"
        UPDATE outbox_events
        SET status = $3, locked_at = NULL, locked_by = NULL,
            completed_at = CASE WHEN $3 = 'completed' THEN now() ELSE NULL END,
            available_at = now() + interval '10 seconds', last_error = $4
        WHERE id = $1 AND locked_by = $2
        "#,
    )
    .bind(job.id)
    .bind(worker_id)
    .bind(if result.is_ok() {
        "completed"
    } else if job.attempts >= state.config.outbox.max_attempts {
        "failed"
    } else {
        "pending"
    })
    .bind(
        result
            .as_ref()
            .err()
            .map(|_| "blacklist reply processing failed"),
    )
    .execute(&state.db)
    .await?;
    result?;
    Ok(true)
}

async fn process_job(state: &AppState, job: &ClaimedJob, worker_id: Uuid) -> anyhow::Result<()> {
    let payload: ReplyJob = serde_json::from_value(job.payload.clone())?;
    let source = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid, Uuid, String)>(
        r#"
        SELECT conversation.project_id, conversation.inbox_id, conversation.contact_id,
               conversation.channel_connection_id, conversation.id, message.body
        FROM messages AS message
        JOIN conversations AS conversation ON conversation.tenant_id = message.tenant_id AND conversation.id = message.conversation_id
        JOIN contacts AS contact ON contact.tenant_id = conversation.tenant_id AND contact.id = conversation.contact_id
        WHERE message.tenant_id = $1 AND message.id = $2 AND message.direction = 'inbound' AND contact.is_blocked
        "#,
    ).bind(job.tenant_id).bind(job.aggregate_id).fetch_optional(&state.db).await?;
    let Some((project_id, inbox_id, contact_id, channel_connection_id, conversation_id, body)) =
        source
    else {
        return Ok(());
    };
    let scope = ConversationScope {
        tenant_id: job.tenant_id,
        project_id,
        inbox_id,
        contact_id,
        channel_connection_id,
    };
    // The only model operation is translation; unavailable providers retain the saved message.
    let customer_message = match body.as_str() {
        "[Telegram media or service message]" | "[Telegram button interaction]" => "",
        text => text,
    };
    let translated = provider_reply::translate_blacklist_reply(
        state,
        job.tenant_id,
        project_id,
        payload.profile_id,
        job.id,
        &payload.text,
        customer_message,
        payload.language.as_deref(),
    )
    .await
    .unwrap_or_else(|_| payload.text.clone());
    let mut transaction = state.db.begin().await?;
    if !lock_contact_block_status(&mut transaction, &scope).await? {
        return Ok(());
    }
    let active = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT conversation.id FROM conversations AS conversation
        JOIN channel_connections AS channel ON channel.tenant_id = conversation.tenant_id AND channel.id = conversation.channel_connection_id
        WHERE conversation.tenant_id = $1 AND conversation.id = $2
          AND channel.status = 'active' AND channel.deleted_at IS NULL
        FOR UPDATE OF conversation
        "#,
    ).bind(job.tenant_id).bind(conversation_id).fetch_optional(&mut *transaction).await?;
    if active.is_none() {
        return Ok(());
    }
    let owns_job = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM outbox_events WHERE id = $1 AND locked_by = $2 AND status = 'processing' FOR UPDATE",
    ).bind(job.id).bind(worker_id).fetch_optional(&mut *transaction).await?;
    anyhow::ensure!(owns_job.is_some(), "blacklist reply lease expired");
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE tenant_id = $1 AND conversation_id = $2 AND client_message_id = $3)",
    ).bind(job.tenant_id).bind(conversation_id).bind(job.id).fetch_one(&mut *transaction).await?;
    if exists {
        return Ok(());
    }
    let event = insert_message(
        &mut transaction,
        &scope,
        conversation_id,
        job.id,
        &translated,
    )
    .await?;
    transaction
        .commit()
        .await
        .context("could not persist blacklist reply")?;
    publish_best_effort(state, &event).await;
    Ok(())
}

async fn insert_message(
    transaction: &mut Transaction<'_, Postgres>,
    scope: &ConversationScope,
    conversation_id: Uuid,
    message_id: Uuid,
    body: &str,
) -> Result<RealtimeEvent, AppError> {
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
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO messages (
            id, tenant_id, project_id, inbox_id, conversation_id, sequence,
            direction, kind, author_kind, client_message_id, body, body_format, status, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, 'outbound', 'text', 'system', $1, $7, 'plain', 'queued', $8)
        "#,
    )
    .bind(message_id)
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(scope.inbox_id)
    .bind(conversation_id)
    .bind(sequence)
    .bind(body)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    let delivery_required = telegram_bot::enqueue_outbound_if_needed(
        transaction,
        scope.tenant_id,
        scope.channel_connection_id,
        message_id,
    )
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
            "direction": "outbound",
            "author_kind": "system",
            "channel_connection_id": scope.channel_connection_id,
            "delivery_required": delivery_required,
        }),
    };
    insert_outbox(transaction, &event).await?;
    Ok(event)
}
