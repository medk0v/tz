ALTER TABLE ai_provider_connections
    DROP CONSTRAINT ai_provider_connections_provider_kind_check;

ALTER TABLE ai_provider_connections
    ADD CONSTRAINT ai_provider_connections_provider_kind_check
        CHECK (provider_kind IN ('openai', 'anthropic', 'openai_compatible', 'letta'));
