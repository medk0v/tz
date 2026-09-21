-- Custom AI channels store configuration only until an authenticated source
-- executor is available. They must never appear as a connected transport.
ALTER TABLE channel_connections
    DROP CONSTRAINT channel_connections_kind_check,
    ADD CONSTRAINT channel_connections_kind_check CHECK (kind IN (
        'widget', 'external_api', 'telegram_bot', 'telegram_business',
        'telegram_tdlib', 'gmail', 'imap_smtp', 'custom_ai'
    )),
    ADD CONSTRAINT channel_connections_custom_ai_status_check
        CHECK (kind <> 'custom_ai' OR status IN ('draft', 'disabled'));

CREATE TABLE custom_ai_channel_configs (
    channel_connection_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    icon text NOT NULL DEFAULT 'bot' CHECK (icon IN (
        'bot', 'globe', 'message-circle', 'mail', 'briefcase', 'users',
        'headset', 'send', 'linkedin', 'phone', 'workflow', 'layers'
    )),
    mode text NOT NULL CHECK (mode IN ('instructions', 'agent')),
    source_url text NOT NULL
        CHECK (length(source_url) BETWEEN 1 AND 4096 AND source_url LIKE 'https://%'),
    source_kind text NOT NULL CHECK (source_kind IN ('website', 'api')),
    instructions text NOT NULL DEFAULT '' CHECK (length(instructions) <= 50000),
    ai_profile_id uuid,
    FOREIGN KEY (tenant_id, project_id, inbox_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, project_id, inbox_id, id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, project_id, id)
        ON DELETE SET NULL (ai_profile_id),
    CHECK (mode <> 'instructions' OR (
        length(trim(instructions)) >= 10 AND ai_profile_id IS NULL
    ))
);

CREATE INDEX custom_ai_channel_configs_project_idx
    ON custom_ai_channel_configs (tenant_id, project_id, inbox_id);

CREATE INDEX custom_ai_channel_configs_profile_idx
    ON custom_ai_channel_configs (tenant_id, project_id, ai_profile_id)
    WHERE ai_profile_id IS NOT NULL;
