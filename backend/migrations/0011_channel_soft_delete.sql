ALTER TABLE channel_connections
    ADD COLUMN deleted_at timestamptz;

CREATE INDEX channel_connections_visible_idx
    ON channel_connections (tenant_id, project_id, name, id)
    WHERE deleted_at IS NULL;
