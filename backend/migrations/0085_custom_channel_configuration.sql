-- Extend saved channel drafts without activating a transport or granting
-- permission to create the configured result inside Tzomet.
ALTER TABLE custom_ai_channel_configs
    ADD COLUMN connection_type text NOT NULL DEFAULT 'ai_agent',
    ADD COLUMN destination text NOT NULL DEFAULT 'conversations',
    ALTER COLUMN source_url SET DEFAULT '',
    DROP CONSTRAINT custom_ai_channel_configs_source_url_check,
    DROP CONSTRAINT custom_ai_channel_configs_check,
    ADD CONSTRAINT custom_ai_channel_configs_connection_type_check
        CHECK (connection_type IN ('ai_agent', 'external_api')),
    ADD CONSTRAINT custom_ai_channel_configs_destination_check
        CHECK (destination IN ('conversations', 'tasks', 'custom')),
    ADD CONSTRAINT custom_ai_channel_configs_source_url_check
        CHECK (length(source_url) <= 4096 AND (
            source_url = '' OR source_url LIKE 'https://%'
        )),
    ADD CONSTRAINT custom_ai_channel_configs_instruction_profile_check
        CHECK (mode <> 'instructions' OR ai_profile_id IS NULL),
    ADD CONSTRAINT custom_ai_channel_configs_required_instructions_check
        CHECK (NOT (
            (connection_type = 'ai_agent' AND mode = 'instructions')
            OR destination = 'custom'
        ) OR length(trim(instructions)) >= 10),
    ADD CONSTRAINT custom_ai_channel_configs_external_api_check
        CHECK (connection_type <> 'external_api' OR (
            mode = 'instructions' AND ai_profile_id IS NULL AND source_kind = 'api'
        ));
