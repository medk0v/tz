ALTER TABLE inboxes
    ADD CONSTRAINT inboxes_tenant_project_id_unique
        UNIQUE (tenant_id, project_id, id);

ALTER TABLE channel_connections
    ADD CONSTRAINT channel_connections_tenant_project_inbox_id_unique
        UNIQUE (tenant_id, project_id, inbox_id, id);

ALTER TABLE conversations
    ADD CONSTRAINT conversations_tenant_project_inbox_id_unique
        UNIQUE (tenant_id, project_id, inbox_id, id);

CREATE TABLE inbox_routing_queues (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    name text NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 120),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled')),
    created_by uuid REFERENCES users (id),
    updated_by uuid REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, inbox_id, id),
    FOREIGN KEY (tenant_id, project_id, inbox_id)
        REFERENCES inboxes (tenant_id, project_id, id)
);

CREATE UNIQUE INDEX inbox_routing_queues_name_unique_idx
    ON inbox_routing_queues (tenant_id, project_id, inbox_id, lower(name));

CREATE TABLE inbox_routing_policies (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    enabled boolean NOT NULL DEFAULT false,
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    assignment_strategy text NOT NULL DEFAULT 'manual'
        CHECK (assignment_strategy IN ('manual', 'least_active')),
    default_queue_id uuid,
    timezone text NOT NULL DEFAULT 'UTC'
        CHECK (char_length(btrim(timezone)) BETWEEN 1 AND 64),
    outside_hours_action text NOT NULL DEFAULT 'ai'
        CHECK (outside_hours_action IN ('ai', 'queue')),
    no_operator_action text NOT NULL DEFAULT 'ai'
        CHECK (no_operator_action IN ('ai', 'queue')),
    sla_clock text NOT NULL DEFAULT 'elapsed'
        CHECK (sla_clock IN ('elapsed', 'working_hours')),
    first_response_seconds integer
        CHECK (first_response_seconds BETWEEN 60 AND 604800),
    resolution_seconds integer
        CHECK (resolution_seconds BETWEEN 60 AND 2592000),
    unassigned_warning_seconds integer
        CHECK (unassigned_warning_seconds BETWEEN 60 AND 604800),
    escalation_queue_id uuid,
    telegram_new_visitor boolean NOT NULL DEFAULT false,
    telegram_new_message boolean NOT NULL DEFAULT false,
    telegram_operator_request boolean NOT NULL DEFAULT false,
    telegram_unassigned_warning boolean NOT NULL DEFAULT false,
    telegram_sla_breach boolean NOT NULL DEFAULT false,
    telegram_chat_ids text[] NOT NULL DEFAULT ARRAY[]::text[],
    created_by uuid REFERENCES users (id),
    updated_by uuid REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id, inbox_id),
    FOREIGN KEY (tenant_id, project_id, inbox_id)
        REFERENCES inboxes (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id, inbox_id, default_queue_id)
        REFERENCES inbox_routing_queues (tenant_id, project_id, inbox_id, id),
    FOREIGN KEY (tenant_id, project_id, inbox_id, escalation_queue_id)
        REFERENCES inbox_routing_queues (tenant_id, project_id, inbox_id, id),
    CHECK (
        assignment_strategy <> 'least_active'
        OR default_queue_id IS NOT NULL
    ),
    CHECK (
        outside_hours_action <> 'queue'
        OR default_queue_id IS NOT NULL
    ),
    CHECK (
        no_operator_action <> 'queue'
        OR default_queue_id IS NOT NULL
    ),
    CHECK (
        unassigned_warning_seconds IS NULL
        OR first_response_seconds IS NULL
        OR unassigned_warning_seconds < first_response_seconds
    ),
    CHECK (
        first_response_seconds IS NULL
        OR resolution_seconds IS NULL
        OR first_response_seconds < resolution_seconds
    ),
    CHECK (cardinality(telegram_chat_ids) BETWEEN 0 AND 32),
    CHECK (array_position(telegram_chat_ids, NULL) IS NULL),
    CHECK (
        NOT (
            telegram_new_visitor
            OR telegram_new_message
            OR telegram_operator_request
            OR telegram_unassigned_warning
            OR telegram_sla_breach
        )
        OR cardinality(telegram_chat_ids) > 0
    )
);

CREATE TABLE inbox_routing_queue_members (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    queue_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id),
    last_assigned_at timestamptz,
    created_by uuid REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id, inbox_id, queue_id, user_id),
    FOREIGN KEY (tenant_id, project_id, inbox_id, queue_id)
        REFERENCES inbox_routing_queues (tenant_id, project_id, inbox_id, id)
        ON DELETE CASCADE
);

CREATE INDEX inbox_routing_queue_members_assignment_idx
    ON inbox_routing_queue_members (
        tenant_id, project_id, inbox_id, queue_id,
        last_assigned_at NULLS FIRST, user_id
    );

CREATE TABLE inbox_routing_rules (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    position smallint NOT NULL CHECK (position BETWEEN 1 AND 64),
    enabled boolean NOT NULL DEFAULT true,
    channel_connection_id uuid,
    language text CHECK (
        language IS NULL
        OR (
            char_length(language) BETWEEN 2 AND 35
            AND language = lower(language)
            AND language = btrim(language)
            AND language ~ '^[a-z]{2,8}(-[a-z0-9]{1,8})*$'
        )
    ),
    queue_id uuid NOT NULL,
    created_by uuid REFERENCES users (id),
    updated_by uuid REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, inbox_id, id),
    UNIQUE (tenant_id, project_id, inbox_id, position),
    FOREIGN KEY (tenant_id, project_id, inbox_id)
        REFERENCES inboxes (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id, inbox_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, project_id, inbox_id, id),
    FOREIGN KEY (tenant_id, project_id, inbox_id, queue_id)
        REFERENCES inbox_routing_queues (tenant_id, project_id, inbox_id, id),
    CHECK (channel_connection_id IS NOT NULL OR language IS NOT NULL)
);

CREATE TABLE inbox_working_intervals (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    weekday smallint NOT NULL CHECK (weekday BETWEEN 1 AND 7),
    starts_at time without time zone NOT NULL,
    ends_at time without time zone NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, inbox_id, id),
    UNIQUE (tenant_id, project_id, inbox_id, weekday, starts_at, ends_at),
    FOREIGN KEY (tenant_id, project_id, inbox_id)
        REFERENCES inbox_routing_policies (tenant_id, project_id, inbox_id)
        ON DELETE CASCADE,
    CHECK (starts_at < ends_at)
);

CREATE INDEX inbox_working_intervals_lookup_idx
    ON inbox_working_intervals (
        tenant_id, project_id, inbox_id, weekday, starts_at
    );

CREATE TABLE inbox_telegram_credentials (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    encrypted_bot_token bytea NOT NULL,
    bot_token_nonce bytea NOT NULL CHECK (octet_length(bot_token_nonce) = 12),
    key_version text NOT NULL CHECK (char_length(btrim(key_version)) BETWEEN 1 AND 64),
    created_by uuid REFERENCES users (id),
    updated_by uuid REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id, inbox_id),
    FOREIGN KEY (tenant_id, project_id, inbox_id)
        REFERENCES inbox_routing_policies (tenant_id, project_id, inbox_id)
        ON DELETE CASCADE
);

CREATE TABLE inbox_routing_idempotency (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    response_body jsonb,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE (tenant_id, project_id, inbox_id, idempotency_key),
    FOREIGN KEY (tenant_id, project_id, inbox_id)
        REFERENCES inboxes (tenant_id, project_id, id)
        ON DELETE CASCADE,
    CHECK (
        (response_body IS NULL AND completed_at IS NULL)
        OR (response_body IS NOT NULL AND completed_at IS NOT NULL)
    )
);

CREATE INDEX inbox_routing_idempotency_expiry_idx
    ON inbox_routing_idempotency (expires_at);

CREATE TABLE conversation_routing_cycles (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    conversation_id uuid NOT NULL,
    cycle_number integer NOT NULL CHECK (cycle_number >= 0),
    policy_version bigint NOT NULL CHECK (policy_version > 0),
    rule_id uuid,
    queue_id uuid,
    started_at timestamptz NOT NULL DEFAULT now(),
    warning_due_at timestamptz,
    first_response_due_at timestamptz,
    resolution_due_at timestamptz,
    first_response_at timestamptz,
    warning_triggered_at timestamptz,
    breached_at timestamptz,
    resolution_breached_at timestamptz,
    closed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id, inbox_id, conversation_id, cycle_number),
    FOREIGN KEY (tenant_id, project_id, inbox_id, conversation_id)
        REFERENCES conversations (tenant_id, project_id, inbox_id, id)
);

CREATE INDEX conversation_routing_cycles_warning_due_idx
    ON conversation_routing_cycles (warning_due_at, conversation_id)
    WHERE warning_due_at IS NOT NULL
      AND warning_triggered_at IS NULL
      AND first_response_at IS NULL
      AND closed_at IS NULL;

CREATE INDEX conversation_routing_cycles_response_due_idx
    ON conversation_routing_cycles (first_response_due_at, conversation_id)
    WHERE first_response_due_at IS NOT NULL
      AND breached_at IS NULL
      AND first_response_at IS NULL
      AND closed_at IS NULL;

CREATE INDEX conversation_routing_cycles_resolution_due_idx
    ON conversation_routing_cycles (resolution_due_at, conversation_id)
    WHERE resolution_due_at IS NOT NULL
      AND resolution_breached_at IS NULL
      AND closed_at IS NULL;
