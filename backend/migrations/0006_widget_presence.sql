ALTER TABLE widget_configs
    ADD COLUMN notify_on_new_visitor boolean NOT NULL DEFAULT false;

ALTER TABLE widget_sessions
    ADD COLUMN presence_last_seen_at timestamptz;

CREATE INDEX widget_sessions_online_presence_idx
    ON widget_sessions (
        tenant_id, inbox_id, presence_last_seen_at DESC, id DESC
    )
    WHERE presence_last_seen_at IS NOT NULL
      AND revoked_at IS NULL;
