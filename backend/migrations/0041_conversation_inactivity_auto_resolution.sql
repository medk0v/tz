ALTER TABLE conversations
    ADD COLUMN last_message_at timestamptz;

WITH latest_messages AS (
    SELECT DISTINCT ON (message.tenant_id, message.conversation_id)
        message.tenant_id,
        message.conversation_id,
        message.created_at
    FROM messages AS message
    ORDER BY message.tenant_id, message.conversation_id, message.sequence DESC
)
UPDATE conversations AS conversation
SET last_message_at = latest.created_at
FROM latest_messages AS latest
WHERE latest.tenant_id = conversation.tenant_id
  AND latest.conversation_id = conversation.id;

DROP INDEX conversations_empty_expiration_idx;

CREATE INDEX conversations_inactivity_expiration_idx
    ON conversations (
        COALESCE(last_message_at, created_at),
        id
    )
    WHERE status <> 'resolved';
