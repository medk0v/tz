CREATE TABLE ai_profile_proxy_settings (
    ai_profile_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    enabled boolean NOT NULL DEFAULT false,
    service_url text NOT NULL DEFAULT '',
    region text,
    country text,
    encrypted_token bytea,
    token_nonce bytea,
    key_version text,
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, ai_profile_id) REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE,
    CHECK (region IS NULL OR country IS NULL),
    CHECK (region IS NULL OR region ~ '^[a-z][a-z0-9_-]{0,63}$'),
    CHECK (country IS NULL OR country ~ '^[A-Z]{2}$'),
    CHECK ((encrypted_token IS NULL AND token_nonce IS NULL AND key_version IS NULL)
        OR (encrypted_token IS NOT NULL AND token_nonce IS NOT NULL AND octet_length(token_nonce) = 12 AND key_version IS NOT NULL)),
    CHECK (NOT enabled OR (service_url <> '' AND encrypted_token IS NOT NULL))
);
