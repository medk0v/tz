CREATE TABLE knowledge_bases (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    default_language text NOT NULL DEFAULT 'en'
        CHECK (length(default_language) BETWEEN 2 AND 35),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'archived')),
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE UNIQUE INDEX knowledge_bases_project_name_unique_idx
    ON knowledge_bases (tenant_id, project_id, lower(name));

CREATE TABLE knowledge_articles (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    knowledge_base_id uuid NOT NULL,
    title text NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 300),
    body text NOT NULL DEFAULT '' CHECK (length(body) <= 200000),
    status text NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'published')),
    source_url text CHECK (source_url IS NULL OR length(source_url) <= 2000),
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_by uuid NOT NULL REFERENCES users (id),
    updated_by uuid NOT NULL REFERENCES users (id),
    published_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, knowledge_base_id)
        REFERENCES knowledge_bases (tenant_id, id) ON DELETE CASCADE,
    CHECK (
        (status = 'draft' AND published_at IS NULL)
        OR (status = 'published' AND published_at IS NOT NULL)
    )
);

CREATE INDEX knowledge_articles_base_updated_idx
    ON knowledge_articles (tenant_id, knowledge_base_id, updated_at DESC, id);

CREATE TABLE ai_provider_connections (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    provider_kind text NOT NULL
        CHECK (provider_kind IN ('openai', 'anthropic', 'openai_compatible')),
    base_url text NOT NULL CHECK (length(base_url) BETWEEN 1 AND 2000),
    default_model text NOT NULL CHECK (length(trim(default_model)) BETWEEN 1 AND 200),
    status text NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled')),
    encrypted_api_key bytea,
    api_key_nonce bytea,
    key_version text,
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    CHECK (
        (encrypted_api_key IS NULL AND api_key_nonce IS NULL AND key_version IS NULL)
        OR (encrypted_api_key IS NOT NULL AND api_key_nonce IS NOT NULL AND key_version IS NOT NULL)
    )
);

CREATE UNIQUE INDEX ai_provider_connections_project_name_unique_idx
    ON ai_provider_connections (tenant_id, project_id, lower(name));

CREATE TABLE ai_profiles (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    provider_connection_id uuid,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    status text NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'active', 'disabled')),
    mode text NOT NULL DEFAULT 'copilot' CHECK (mode = 'copilot'),
    model text CHECK (model IS NULL OR length(trim(model)) BETWEEN 1 AND 200),
    instructions text NOT NULL DEFAULT '' CHECK (length(instructions) <= 50000),
    language text NOT NULL DEFAULT 'ru' CHECK (length(language) BETWEEN 2 AND 35),
    temperature double precision NOT NULL DEFAULT 0.2
        CHECK (temperature >= 0 AND temperature <= 2),
    max_output_tokens integer NOT NULL DEFAULT 800
        CHECK (max_output_tokens BETWEEN 64 AND 32000),
    handoff_message text NOT NULL DEFAULT '' CHECK (length(handoff_message) <= 2000),
    allowed_tools text[] NOT NULL DEFAULT ARRAY['conversation_context']::text[],
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, provider_connection_id)
        REFERENCES ai_provider_connections (tenant_id, id)
);

CREATE UNIQUE INDEX ai_profiles_project_name_unique_idx
    ON ai_profiles (tenant_id, project_id, lower(name));

CREATE TABLE ai_profile_knowledge_bases (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    ai_profile_id uuid NOT NULL,
    knowledge_base_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, ai_profile_id, knowledge_base_id),
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, knowledge_base_id)
        REFERENCES knowledge_bases (tenant_id, id) ON DELETE CASCADE
);

CREATE TABLE inbox_ai_profiles (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    inbox_id uuid NOT NULL,
    ai_profile_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, inbox_id),
    FOREIGN KEY (tenant_id, inbox_id)
        REFERENCES inboxes (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE
);
