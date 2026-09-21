ALTER TABLE conversation_participants
    ADD COLUMN ai_profile_id uuid REFERENCES ai_profiles (id) ON DELETE SET NULL;

WITH ranked_ai_participants AS (
    SELECT id,
           row_number() OVER (
               PARTITION BY tenant_id, conversation_id
               ORDER BY joined_at DESC, id DESC
           ) AS position
    FROM conversation_participants
    WHERE participant_kind = 'ai' AND left_at IS NULL
)
UPDATE conversation_participants AS participant
SET left_at = now()
FROM ranked_ai_participants AS ranked
WHERE participant.id = ranked.id AND ranked.position > 1;

CREATE UNIQUE INDEX conversation_active_ai_participant_idx
    ON conversation_participants (tenant_id, conversation_id)
    WHERE participant_kind = 'ai' AND left_at IS NULL;

CREATE TABLE ai_reply_jobs (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    conversation_id uuid NOT NULL,
    ai_profile_id uuid REFERENCES ai_profiles (id) ON DELETE SET NULL,
    ai_profile_name text NOT NULL CHECK (length(trim(ai_profile_name)) BETWEEN 1 AND 200),
    triggering_message_id uuid NOT NULL,
    triggering_sequence bigint NOT NULL CHECK (triggering_sequence > 0),
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'cancelled')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at timestamptz NOT NULL DEFAULT now(),
    locked_at timestamptz,
    locked_by uuid,
    last_error text CHECK (last_error IS NULL OR length(last_error) <= 2000),
    response_message_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, triggering_message_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, triggering_message_id)
        REFERENCES messages (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, response_message_id)
        REFERENCES messages (tenant_id, id)
);

CREATE INDEX ai_reply_jobs_claim_idx
    ON ai_reply_jobs (available_at, created_at, id)
    WHERE status = 'pending';

CREATE INDEX ai_reply_jobs_conversation_idx
    ON ai_reply_jobs (tenant_id, conversation_id, triggering_sequence, id);
