CREATE TABLE ai_api_settings (
    ai_profile_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    enabled boolean NOT NULL DEFAULT false,
    key_id uuid,
    key_hash bytea,
    key_prefix text,
    key_created_by uuid REFERENCES users (id),
    key_created_at timestamptz,
    key_last_used_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, ai_profile_id) REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE,
    CHECK ((key_id IS NULL AND key_hash IS NULL AND key_prefix IS NULL)
        OR (key_id IS NOT NULL AND octet_length(key_hash) = 32 AND key_prefix IS NOT NULL))
);

CREATE TABLE ai_api_endpoints (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    ai_profile_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    slug text NOT NULL CHECK (slug ~ '^[a-z][a-z0-9_-]{0,63}$'),
    instructions text NOT NULL CHECK (length(trim(instructions)) BETWEEN 1 AND 50000),
    input_example jsonb NOT NULL CHECK (jsonb_typeof(input_example) = 'object'),
    output_example jsonb NOT NULL CHECK (jsonb_typeof(output_example) = 'object'),
    input_schema jsonb NOT NULL,
    output_schema jsonb NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    timeout_seconds bigint NOT NULL DEFAULT 180 CHECK (timeout_seconds BETWEEN 10 AND 300),
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, ai_profile_id) REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX ai_api_endpoint_slug_idx ON ai_api_endpoints (ai_profile_id, slug) WHERE deleted_at IS NULL;
CREATE INDEX ai_api_endpoint_profile_idx ON ai_api_endpoints (tenant_id, project_id, ai_profile_id);

CREATE TABLE ai_api_runs (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    ai_profile_id uuid NOT NULL,
    endpoint_id uuid NOT NULL,
    endpoint_name text NOT NULL,
    endpoint_slug text NOT NULL,
    endpoint_version bigint NOT NULL,
    key_id uuid,
    actor_id uuid REFERENCES users (id),
    instructions text NOT NULL,
    input_schema jsonb NOT NULL,
    output_schema jsonb NOT NULL,
    input jsonb NOT NULL CHECK (jsonb_typeof(input) = 'object'),
    result jsonb CHECK (result IS NULL OR jsonb_typeof(result) = 'object'),
    error_code text,
    error_message text,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'cancelled')),
    timeout_seconds bigint NOT NULL CHECK (timeout_seconds BETWEEN 10 AND 300),
    queue_deadline_at timestamptz NOT NULL DEFAULT now() + interval '10 minutes',
    deadline_at timestamptz,
    locked_by uuid,
    idempotency_key uuid,
    input_hash bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    completed_at timestamptz,
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, ai_profile_id) REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (endpoint_id) REFERENCES ai_api_endpoints (id) ON DELETE CASCADE,
    CHECK (key_id IS NOT NULL OR actor_id IS NOT NULL),
    CHECK ((status = 'processing' AND locked_by IS NOT NULL AND deadline_at IS NOT NULL)
        OR (status = 'cancelled' AND (locked_by IS NULL OR deadline_at IS NOT NULL))
        OR (status IN ('pending', 'completed', 'failed') AND locked_by IS NULL))
);

CREATE UNIQUE INDEX ai_api_run_idempotency_idx ON ai_api_runs (ai_profile_id, key_id, idempotency_key)
    WHERE key_id IS NOT NULL AND idempotency_key IS NOT NULL;
CREATE INDEX ai_api_run_queue_idx ON ai_api_runs (created_at, id) WHERE status = 'pending';
CREATE INDEX ai_api_run_processing_idx ON ai_api_runs (deadline_at, id) WHERE locked_by IS NOT NULL;
CREATE INDEX ai_api_run_project_queue_idx ON ai_api_runs (tenant_id, project_id) WHERE status IN ('pending', 'processing');
CREATE INDEX ai_api_run_history_idx ON ai_api_runs (tenant_id, project_id, ai_profile_id, created_at DESC);
