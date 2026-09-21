ALTER TABLE ai_api_endpoints
    ADD COLUMN cache_refresh_seconds bigint CHECK (cache_refresh_seconds BETWEEN 60 AND 86400),
    ADD COLUMN cache_next_refresh_at timestamptz NOT NULL DEFAULT now();

ALTER TABLE ai_api_runs ADD COLUMN cache_refresh boolean NOT NULL DEFAULT false;

CREATE TABLE ai_api_cache (
    endpoint_id uuid PRIMARY KEY REFERENCES ai_api_endpoints (id) ON DELETE CASCADE,
    endpoint_version bigint NOT NULL,
    key_id uuid NOT NULL,
    result jsonb NOT NULL CHECK (jsonb_typeof(result) = 'object'),
    updated_at timestamptz NOT NULL
);

CREATE INDEX ai_api_cache_due_idx ON ai_api_endpoints (cache_next_refresh_at)
    WHERE cache_refresh_seconds IS NOT NULL AND enabled AND deleted_at IS NULL;
