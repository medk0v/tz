ALTER TABLE conversations
    ADD COLUMN last_reopened_at timestamptz;

DROP INDEX conversations_inactivity_expiration_idx;

CREATE INDEX conversations_inactivity_expiration_idx
    ON conversations (
        GREATEST(COALESCE(last_message_at, created_at), last_reopened_at),
        id
    )
    WHERE status <> 'resolved';
