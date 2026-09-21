CREATE TABLE telegram_bot_receivers (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    channel_connection_id uuid NOT NULL,
    receive_mode text NOT NULL DEFAULT 'webhook'
        CHECK (receive_mode IN ('webhook', 'polling')),
    polling_offset bigint,
    webhook_failure_started_at timestamptz,
    last_webhook_check_at timestamptz,
    next_webhook_retry_at timestamptz,
    lease_owner uuid,
    lease_expires_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, channel_connection_id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id)
);

INSERT INTO telegram_bot_receivers (tenant_id, channel_connection_id)
SELECT tenant_id, id
FROM channel_connections
WHERE kind = 'telegram_bot'
ON CONFLICT (tenant_id, channel_connection_id) DO NOTHING;

CREATE INDEX telegram_bot_receivers_due_idx
    ON telegram_bot_receivers (receive_mode, last_webhook_check_at, next_webhook_retry_at);
