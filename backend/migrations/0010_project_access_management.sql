ALTER TABLE memberships
    ADD COLUMN revoked_at timestamptz;

CREATE INDEX memberships_active_user_scope_idx
    ON memberships (tenant_id, user_id, project_id)
    WHERE revoked_at IS NULL;
