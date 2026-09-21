-- Phone channels connect a telephony gateway (first Asterisk through
-- AudioSocket) to an Inbox. Agents answer calls with their speech-to-text,
-- chat, and text-to-speech models.
ALTER TABLE channel_connections
    DROP CONSTRAINT channel_connections_kind_check,
    ADD CONSTRAINT channel_connections_kind_check CHECK (kind IN (
        'widget', 'external_api', 'telegram_bot', 'telegram_business',
        'telegram_tdlib', 'gmail', 'imap_smtp', 'custom_ai', 'phone'
    ));

CREATE TABLE phone_channel_configs (
    channel_connection_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    gateway text NOT NULL DEFAULT 'asterisk' CHECK (gateway IN ('asterisk')),
    phone_number text NOT NULL CHECK (phone_number ~ '^\+[1-9][0-9]{6,14}$'),
    greeting text NOT NULL CHECK (length(trim(greeting)) BETWEEN 1 AND 500),
    transfer_number text CHECK (
        transfer_number IS NULL OR transfer_number ~ '^\+?[0-9*#]{2,32}$'
    ),
    max_call_seconds integer NOT NULL DEFAULT 900
        CHECK (max_call_seconds BETWEEN 30 AND 7200),
    gateway_token_hash bytea NOT NULL CHECK (length(gateway_token_hash) = 32),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (gateway_token_hash),
    FOREIGN KEY (tenant_id, project_id, inbox_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, project_id, inbox_id, id)
        ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE UNIQUE INDEX phone_channel_configs_number_idx
    ON phone_channel_configs (tenant_id, phone_number);

-- One row per call. The call's transcript lives in its conversation.
CREATE TABLE phone_calls (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    channel_connection_id uuid NOT NULL,
    conversation_id uuid NOT NULL,
    contact_id uuid NOT NULL,
    ai_profile_id uuid,
    direction text NOT NULL CHECK (direction IN ('inbound')),
    status text NOT NULL CHECK (status IN (
        'ringing', 'active', 'completed', 'transferred', 'failed', 'unanswered'
    )),
    caller text NOT NULL CHECK (length(caller) BETWEEN 1 AND 64),
    callee text NOT NULL CHECK (length(callee) BETWEEN 1 AND 64),
    transfer_to text,
    end_reason text CHECK (end_reason IS NULL OR length(end_reason) <= 200),
    created_at timestamptz NOT NULL DEFAULT now(),
    answered_at timestamptz,
    ended_at timestamptz,
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, conversation_id) REFERENCES conversations (tenant_id, id),
    FOREIGN KEY (tenant_id, channel_connection_id) REFERENCES channel_connections (tenant_id, id),
    CHECK (status <> 'transferred' OR transfer_to IS NOT NULL)
);

CREATE INDEX phone_calls_channel_created_idx
    ON phone_calls (tenant_id, channel_connection_id, created_at DESC);

CREATE INDEX phone_calls_open_idx
    ON phone_calls (created_at)
    WHERE status IN ('ringing', 'active');
