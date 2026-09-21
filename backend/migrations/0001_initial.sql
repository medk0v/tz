CREATE TABLE tenants (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (id)
);

CREATE TABLE projects (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    slug text NOT NULL CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,62}$'),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, slug)
);

CREATE TABLE users (
    id uuid PRIMARY KEY,
    email text NOT NULL,
    display_name text NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 200),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX users_email_unique_idx ON users (lower(email));

CREATE TABLE memberships (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid,
    user_id uuid NOT NULL REFERENCES users (id),
    role text NOT NULL,
    permissions text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE NULLS NOT DISTINCT (tenant_id, project_id, user_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE TABLE teams (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, project_id, name),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE TABLE team_members (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    team_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, team_id, user_id),
    FOREIGN KEY (tenant_id, team_id) REFERENCES teams (tenant_id, id)
);

CREATE TABLE inboxes (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, project_id, name),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE TABLE inbox_team_access (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    inbox_id uuid NOT NULL,
    team_id uuid NOT NULL,
    permissions text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, inbox_id, team_id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, team_id) REFERENCES teams (tenant_id, id)
);

CREATE TABLE channel_connections (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    public_id uuid NOT NULL UNIQUE,
    kind text NOT NULL CHECK (
        kind IN (
            'widget', 'external_api', 'telegram_bot', 'telegram_business',
            'telegram_tdlib', 'gmail', 'imap_smtp'
        )
    ),
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    status text NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'connecting', 'active', 'degraded', 'reauth_required', 'disabled')),
    config jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id)
);

CREATE TABLE widget_configs (
    channel_connection_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    allowed_origins text[] NOT NULL,
    theme jsonb NOT NULL DEFAULT '{}',
    greeting text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, channel_connection_id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id)
);

CREATE TABLE channel_secrets (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    channel_connection_id uuid NOT NULL,
    encrypted_payload bytea NOT NULL,
    key_version text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, channel_connection_id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id)
);

CREATE TABLE contacts (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    display_name text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE TABLE contact_identities (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    contact_id uuid NOT NULL,
    channel_kind text NOT NULL,
    external_id text NOT NULL,
    metadata jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, channel_kind, external_id),
    FOREIGN KEY (tenant_id, contact_id) REFERENCES contacts (tenant_id, id)
);

CREATE TABLE widget_sessions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    channel_connection_id uuid NOT NULL,
    contact_id uuid NOT NULL,
    token_hash bytea NOT NULL UNIQUE,
    origin text NOT NULL,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id),
    FOREIGN KEY (tenant_id, contact_id) REFERENCES contacts (tenant_id, id)
);

CREATE INDEX widget_sessions_expiry_idx
    ON widget_sessions (expires_at)
    WHERE revoked_at IS NULL;

CREATE TABLE conversations (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    channel_connection_id uuid NOT NULL,
    contact_id uuid NOT NULL,
    status text NOT NULL DEFAULT 'new'
        CHECK (status IN ('new', 'open', 'waiting_customer', 'resolved')),
    priority text NOT NULL DEFAULT 'normal'
        CHECK (priority IN ('low', 'normal', 'high', 'urgent')),
    subject text,
    last_message_sequence bigint NOT NULL DEFAULT 0,
    reopened_count integer NOT NULL DEFAULT 0 CHECK (reopened_count >= 0),
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    resolved_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id),
    FOREIGN KEY (tenant_id, contact_id) REFERENCES contacts (tenant_id, id)
);

CREATE INDEX conversations_inbox_queue_idx
    ON conversations (tenant_id, project_id, inbox_id, status, updated_at DESC);

CREATE INDEX conversations_contact_idx
    ON conversations (tenant_id, contact_id, updated_at DESC);

CREATE TABLE conversation_participants (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    conversation_id uuid NOT NULL,
    participant_kind text NOT NULL
        CHECK (participant_kind IN ('contact', 'operator', 'ai')),
    contact_id uuid,
    user_id uuid REFERENCES users (id),
    joined_at timestamptz NOT NULL DEFAULT now(),
    left_at timestamptz,
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations (tenant_id, id),
    FOREIGN KEY (tenant_id, contact_id) REFERENCES contacts (tenant_id, id),
    CHECK (
        (participant_kind = 'contact' AND contact_id IS NOT NULL AND user_id IS NULL)
        OR (participant_kind = 'operator' AND contact_id IS NULL AND user_id IS NOT NULL)
        OR (participant_kind = 'ai' AND contact_id IS NULL)
    )
);

CREATE TABLE conversation_assignments (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    conversation_id uuid NOT NULL,
    team_id uuid,
    user_id uuid REFERENCES users (id),
    assigned_by uuid REFERENCES users (id),
    assigned_at timestamptz NOT NULL DEFAULT now(),
    unassigned_at timestamptz,
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations (tenant_id, id),
    FOREIGN KEY (tenant_id, team_id) REFERENCES teams (tenant_id, id),
    CHECK (team_id IS NOT NULL OR user_id IS NOT NULL)
);

CREATE TABLE messages (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    conversation_id uuid NOT NULL,
    sequence bigint NOT NULL CHECK (sequence > 0),
    direction text NOT NULL CHECK (direction IN ('inbound', 'outbound', 'internal')),
    kind text NOT NULL CHECK (kind IN ('text', 'attachment', 'system')),
    author_kind text NOT NULL CHECK (author_kind IN ('contact', 'operator', 'ai', 'system')),
    author_id uuid,
    client_message_id uuid,
    body text NOT NULL DEFAULT '',
    status text NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'sending', 'sent', 'delivered', 'read', 'failed')),
    created_at timestamptz NOT NULL DEFAULT now(),
    edited_at timestamptz,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, conversation_id, sequence),
    UNIQUE NULLS NOT DISTINCT (tenant_id, conversation_id, client_message_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations (tenant_id, id)
);

CREATE INDEX messages_history_idx
    ON messages (tenant_id, conversation_id, sequence);

CREATE TABLE message_deliveries (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    message_id uuid NOT NULL,
    channel_connection_id uuid NOT NULL,
    provider_message_id text,
    status text NOT NULL
        CHECK (status IN ('queued', 'sending', 'sent', 'delivered', 'read', 'failed')),
    last_error text,
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, message_id, channel_connection_id),
    FOREIGN KEY (tenant_id, message_id) REFERENCES messages (tenant_id, id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id)
);

CREATE TABLE message_attachments (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    message_id uuid NOT NULL,
    object_key text NOT NULL,
    file_name text NOT NULL,
    content_type text NOT NULL,
    byte_size bigint NOT NULL CHECK (byte_size >= 0),
    checksum_sha256 text NOT NULL,
    scan_status text NOT NULL DEFAULT 'pending'
        CHECK (scan_status IN ('pending', 'clean', 'infected', 'failed')),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, object_key),
    FOREIGN KEY (tenant_id, message_id) REFERENCES messages (tenant_id, id)
);

CREATE TABLE conversation_resolutions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    conversation_id uuid NOT NULL,
    cycle_number integer NOT NULL CHECK (cycle_number > 0),
    responsible_actor_id uuid,
    responsible_team_id uuid,
    note text,
    resolved_at timestamptz NOT NULL,
    reopened_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, conversation_id, cycle_number),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations (tenant_id, id),
    FOREIGN KEY (tenant_id, responsible_team_id) REFERENCES teams (tenant_id, id)
);

CREATE TABLE support_ratings (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    resolution_id uuid NOT NULL,
    rating smallint NOT NULL CHECK (rating BETWEEN 1 AND 5),
    comment text,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, resolution_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, resolution_id)
        REFERENCES conversation_resolutions (tenant_id, id)
);

CREATE TABLE support_rating_reasons (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    support_rating_id uuid NOT NULL,
    reason text NOT NULL CHECK (length(trim(reason)) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, support_rating_id, reason),
    FOREIGN KEY (tenant_id, support_rating_id)
        REFERENCES support_ratings (tenant_id, id)
);

CREATE TABLE outbox_events (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    aggregate_type text NOT NULL,
    aggregate_id uuid NOT NULL,
    event_type text NOT NULL,
    payload jsonb NOT NULL,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at timestamptz NOT NULL DEFAULT now(),
    locked_at timestamptz,
    locked_by uuid,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE (tenant_id, id)
);

CREATE INDEX outbox_claim_idx
    ON outbox_events (status, available_at, id)
    WHERE status = 'pending';

CREATE TABLE idempotency_keys (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    scope text NOT NULL,
    idempotency_key text NOT NULL,
    request_hash bytea NOT NULL,
    response_status integer,
    response_body jsonb,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, scope, idempotency_key)
);

CREATE TABLE inbound_events (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    channel_connection_id uuid NOT NULL,
    provider_event_id text NOT NULL,
    payload_hash bytea NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    processed_at timestamptz,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, channel_connection_id, provider_event_id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id)
);

CREATE TABLE audit_log (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid,
    actor_id uuid,
    action text NOT NULL,
    resource_kind text NOT NULL,
    resource_id uuid,
    metadata jsonb NOT NULL DEFAULT '{}',
    occurred_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE INDEX audit_log_tenant_time_idx
    ON audit_log (tenant_id, occurred_at DESC);

CREATE TABLE api_keys (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid,
    actor_user_id uuid REFERENCES users (id),
    name text NOT NULL,
    token_hash bytea NOT NULL UNIQUE,
    permissions text[] NOT NULL DEFAULT '{}',
    inbox_scope uuid[],
    expires_at timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE TABLE realtime_tickets (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid,
    actor_id uuid,
    contact_id uuid,
    inbox_scope uuid[],
    token_hash bytea NOT NULL UNIQUE,
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, contact_id) REFERENCES contacts (tenant_id, id),
    CHECK (actor_id IS NOT NULL OR contact_id IS NOT NULL)
);

CREATE INDEX realtime_tickets_expiry_idx
    ON realtime_tickets (expires_at)
    WHERE consumed_at IS NULL;

CREATE TABLE agent_runs (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    conversation_id uuid,
    policy_version text NOT NULL,
    status text NOT NULL DEFAULT 'created'
        CHECK (status IN ('created', 'running', 'completed', 'failed', 'denied')),
    started_at timestamptz,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations (tenant_id, id)
);

CREATE TABLE agent_resource_grants (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    agent_run_id uuid NOT NULL,
    resource_kind text NOT NULL
        CHECK (resource_kind IN ('order', 'conversation', 'contact')),
    resource_id uuid NOT NULL,
    allowed_fields text[] NOT NULL,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, agent_run_id, resource_kind, resource_id),
    FOREIGN KEY (tenant_id, agent_run_id) REFERENCES agent_runs (tenant_id, id)
);

CREATE TABLE agent_tool_calls (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    agent_run_id uuid NOT NULL,
    tool_name text NOT NULL,
    resource_kind text,
    resource_id uuid,
    requested_fields text[] NOT NULL DEFAULT '{}',
    decision text NOT NULL CHECK (decision IN ('allowed', 'denied')),
    denial_reason text,
    result_metadata jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, agent_run_id) REFERENCES agent_runs (tenant_id, id)
);
