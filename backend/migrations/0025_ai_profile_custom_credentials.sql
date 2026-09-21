ALTER TABLE ai_profiles
    ADD COLUMN custom_fields jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD CONSTRAINT ai_profiles_custom_fields_object_check
        CHECK (jsonb_typeof(custom_fields) = 'object');

CREATE TABLE ai_profile_secrets (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    ai_profile_id uuid NOT NULL,
    secret_key text NOT NULL
        CHECK (secret_key ~ '^[A-Za-z0-9_][A-Za-z0-9_.-]{0,79}$'),
    encrypted_value bytea NOT NULL CHECK (octet_length(encrypted_value) >= 17),
    nonce bytea NOT NULL CHECK (octet_length(nonce) = 12),
    key_version text NOT NULL CHECK (length(trim(key_version)) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, ai_profile_id, secret_key),
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE
);

CREATE INDEX ai_profile_secrets_profile_idx
    ON ai_profile_secrets (tenant_id, ai_profile_id);
