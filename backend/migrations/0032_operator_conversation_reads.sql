ALTER TABLE conversations
    ADD COLUMN operator_unread_baseline_sequence bigint NOT NULL DEFAULT 0
        CHECK (operator_unread_baseline_sequence >= 0);

-- Read state did not exist before this migration. Treat the current history as
-- already read so deploying the feature does not flag every old conversation.
UPDATE conversations
SET operator_unread_baseline_sequence = last_message_sequence;

CREATE TABLE conversation_read_cursors (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    conversation_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    last_read_sequence bigint NOT NULL
        CHECK (last_read_sequence >= 0),
    read_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, conversation_id, actor_id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations (tenant_id, id)
        ON DELETE CASCADE
);

CREATE INDEX conversation_read_cursors_actor_idx
    ON conversation_read_cursors (tenant_id, actor_id, conversation_id)
    INCLUDE (last_read_sequence);
