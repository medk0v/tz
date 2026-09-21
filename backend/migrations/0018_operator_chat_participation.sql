CREATE TABLE operator_chat_profiles (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id),
    display_name text NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 200),
    avatar_url text CHECK (avatar_url IS NULL OR length(avatar_url) BETWEEN 1 AND 2048),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id, user_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

WITH ranked_assignments AS (
    SELECT id,
           row_number() OVER (
               PARTITION BY tenant_id, conversation_id
               ORDER BY assigned_at DESC, id DESC
           ) AS position
    FROM conversation_assignments
    WHERE user_id IS NOT NULL AND unassigned_at IS NULL
)
UPDATE conversation_assignments AS assignment
SET unassigned_at = now()
FROM ranked_assignments AS ranked
WHERE assignment.id = ranked.id AND ranked.position > 1;

CREATE UNIQUE INDEX conversation_active_operator_assignment_idx
    ON conversation_assignments (tenant_id, conversation_id)
    WHERE user_id IS NOT NULL AND unassigned_at IS NULL;

WITH ranked_participants AS (
    SELECT id,
           row_number() OVER (
               PARTITION BY tenant_id, conversation_id, user_id
               ORDER BY joined_at DESC, id DESC
           ) AS position
    FROM conversation_participants
    WHERE participant_kind = 'operator' AND left_at IS NULL
)
UPDATE conversation_participants AS participant
SET left_at = now()
FROM ranked_participants AS ranked
WHERE participant.id = ranked.id AND ranked.position > 1;

CREATE UNIQUE INDEX conversation_active_operator_participant_idx
    ON conversation_participants (tenant_id, conversation_id, user_id)
    WHERE participant_kind = 'operator' AND left_at IS NULL;
