CREATE TABLE api_integrations (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    integration_key text NOT NULL
        CHECK (integration_key ~ '^[a-z][a-z0-9_]{1,63}$'),
    description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    base_url text NOT NULL CHECK (length(base_url) BETWEEN 1 AND 2000),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled')),
    auth_kind text NOT NULL DEFAULT 'none'
        CHECK (auth_kind IN ('none', 'bearer', 'x_api_key')),
    encrypted_token bytea,
    token_nonce bytea,
    key_version text,
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    CHECK (
        (
            auth_kind = 'none'
            AND encrypted_token IS NULL
            AND token_nonce IS NULL
            AND key_version IS NULL
        )
        OR (
            auth_kind IN ('bearer', 'x_api_key')
            AND encrypted_token IS NOT NULL
            AND octet_length(encrypted_token) >= 24
            AND token_nonce IS NOT NULL
            AND octet_length(token_nonce) = 12
            AND key_version IS NOT NULL
            AND length(trim(key_version)) BETWEEN 1 AND 100
        )
    )
);

CREATE UNIQUE INDEX api_integrations_project_name_unique_idx
    ON api_integrations (tenant_id, project_id, lower(name));

CREATE UNIQUE INDEX api_integrations_project_key_unique_idx
    ON api_integrations (tenant_id, project_id, integration_key);

CREATE TABLE api_integration_actions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    integration_id uuid NOT NULL,
    action_key text NOT NULL
        CHECK (action_key ~ '^[a-z][a-z0-9_]{1,63}$'),
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    path_template text NOT NULL CHECK (length(path_template) BETWEEN 1 AND 2000),
    parameter_names text[] NOT NULL DEFAULT '{}',
    position smallint NOT NULL CHECK (position BETWEEN 0 AND 31),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, integration_id, action_key),
    UNIQUE (tenant_id, integration_id, position) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (tenant_id, project_id, integration_id)
        REFERENCES api_integrations (tenant_id, project_id, id) ON DELETE CASCADE,
    CHECK (cardinality(parameter_names) <= 32)
);

CREATE INDEX api_integration_actions_catalog_idx
    ON api_integration_actions (tenant_id, project_id, integration_id, position, id);

ALTER TABLE ai_profiles
    ADD CONSTRAINT ai_profiles_tenant_project_id_unique
    UNIQUE (tenant_id, project_id, id);

CREATE TABLE ai_profile_api_integrations (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    ai_profile_id uuid NOT NULL,
    integration_id uuid NOT NULL,
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, ai_profile_id, integration_id),
    FOREIGN KEY (tenant_id, project_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, project_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, integration_id)
        REFERENCES api_integrations (tenant_id, project_id, id) ON DELETE CASCADE
);

CREATE INDEX ai_profile_api_integrations_catalog_idx
    ON ai_profile_api_integrations (tenant_id, project_id, ai_profile_id, integration_id);
