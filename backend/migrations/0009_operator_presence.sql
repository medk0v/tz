ALTER TABLE operator_sessions
    ADD COLUMN presence_last_seen_at timestamptz;

ALTER TABLE api_keys
    ADD COLUMN presence_last_seen_at timestamptz;

CREATE INDEX operator_sessions_presence_idx
    ON operator_sessions (tenant_id, presence_last_seen_at DESC)
    WHERE revoked_at IS NULL AND presence_last_seen_at IS NOT NULL;

CREATE INDEX api_keys_presence_idx
    ON api_keys (tenant_id, presence_last_seen_at DESC)
    WHERE revoked_at IS NULL
      AND actor_user_id IS NOT NULL
      AND presence_last_seen_at IS NOT NULL;
