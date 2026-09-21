CREATE INDEX conversations_empty_expiration_idx
    ON conversations (created_at, id)
    WHERE status <> 'resolved' AND last_message_sequence = 0;
