ALTER TABLE widget_sessions
    ADD COLUMN visitor_id_hash bytea,
    ADD COLUMN client_ip inet,
    ADD COLUMN client_ip_source text
        CHECK (client_ip_source IN ('peer', 'trusted_proxy')),
    ADD COLUMN user_agent text,
    ADD COLUMN accept_language text,
    ADD COLUMN client_hints jsonb,
    ADD COLUMN client_context jsonb,
    ADD COLUMN visitor_data_expires_at timestamptz,
    ADD CONSTRAINT widget_sessions_client_ip_source_pair_check CHECK (
        (client_ip IS NULL AND client_ip_source IS NULL)
        OR (client_ip IS NOT NULL AND client_ip_source IS NOT NULL)
    ),
    ADD CONSTRAINT widget_sessions_visitor_id_hash_length_check CHECK (
        visitor_id_hash IS NULL OR octet_length(visitor_id_hash) = 32
    ),
    ADD CONSTRAINT widget_sessions_client_hints_object_check CHECK (
        client_hints IS NULL OR jsonb_typeof(client_hints) = 'object'
    ),
    ADD CONSTRAINT widget_sessions_client_context_object_check CHECK (
        client_context IS NULL OR jsonb_typeof(client_context) = 'object'
    );

CREATE INDEX widget_sessions_contact_channel_created_idx
    ON widget_sessions (
        tenant_id, contact_id, channel_connection_id, created_at DESC, id DESC
    );

CREATE INDEX widget_sessions_visitor_data_expiry_idx
    ON widget_sessions (visitor_data_expires_at)
    WHERE visitor_data_expires_at IS NOT NULL;
