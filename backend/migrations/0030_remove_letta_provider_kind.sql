UPDATE outbox_events
SET status = 'completed',
    completed_at = now(),
    locked_at = NULL,
    locked_by = NULL,
    last_error = 'provider type removed'
WHERE aggregate_type = 'provider_reply'
  AND status IN ('pending', 'processing')
  AND payload ->> 'ai_profile_id' IN (
      SELECT profile.id::text
      FROM ai_profiles AS profile
      JOIN ai_provider_connections AS provider
        ON provider.tenant_id = profile.tenant_id
       AND provider.id = profile.provider_connection_id
      WHERE provider.provider_kind = 'letta'
  );

UPDATE conversation_participants
SET left_at = now()
WHERE participant_kind = 'ai'
  AND left_at IS NULL
  AND ai_profile_id IN (
      SELECT profile.id
      FROM ai_profiles AS profile
      JOIN ai_provider_connections AS provider
        ON provider.tenant_id = profile.tenant_id
       AND provider.id = profile.provider_connection_id
      WHERE provider.provider_kind = 'letta'
  );

UPDATE ai_profiles AS profile
SET provider_connection_id = NULL,
    status = 'disabled',
    updated_at = now()
FROM ai_provider_connections AS provider
WHERE provider.tenant_id = profile.tenant_id
  AND provider.id = profile.provider_connection_id
  AND provider.provider_kind = 'letta';

DELETE FROM ai_provider_connections
WHERE provider_kind = 'letta';

ALTER TABLE ai_provider_connections
    DROP CONSTRAINT ai_provider_connections_provider_kind_check;

ALTER TABLE ai_provider_connections
    ADD CONSTRAINT ai_provider_connections_provider_kind_check
        CHECK (provider_kind IN ('openai', 'anthropic', 'openai_compatible'));
