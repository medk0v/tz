CREATE TABLE mobile_push_devices (
    id uuid PRIMARY KEY,
    installation_id uuid NOT NULL UNIQUE,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id),
    session_id uuid NOT NULL,
    token text NOT NULL CHECK (length(token) BETWEEN 1 AND 4096),
    token_hash bytea NOT NULL UNIQUE,
    registered_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, session_id) REFERENCES operator_sessions (tenant_id, id)
        ON DELETE CASCADE
);

CREATE INDEX mobile_push_devices_project_idx
    ON mobile_push_devices (tenant_id, project_id, registered_at);
CREATE INDEX mobile_push_devices_session_idx ON mobile_push_devices (session_id);

-- One independent delivery per original event and registration generation.
CREATE UNIQUE INDEX mobile_push_outbox_source_device_idx
    ON outbox_events (aggregate_id, (payload ->> 'event_id'))
    WHERE aggregate_type = 'mobile_push';

CREATE INDEX mobile_push_outbox_lease_idx ON outbox_events (locked_at)
    WHERE aggregate_type = 'mobile_push' AND status = 'processing';
