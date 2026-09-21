CREATE TABLE note_shares (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    note_id uuid,
    token_hash bytea NOT NULL CHECK (octet_length(token_hash) = 32),
    custom_slug text CHECK (
        length(custom_slug) BETWEEN 3 AND 64
        AND custom_slug ~ '^[a-z0-9][a-z0-9-]*[a-z0-9]$'
    ),
    password_hash text,
    can_edit boolean NOT NULL DEFAULT false,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    unlock_window_start timestamptz NOT NULL DEFAULT now(),
    unlock_attempts integer NOT NULL DEFAULT 0 CHECK (unlock_attempts BETWEEN 0 AND 10),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id, note_id)
        REFERENCES notes (tenant_id, project_id, id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX note_shares_active_slug_idx ON note_shares (custom_slug)
    WHERE custom_slug IS NOT NULL AND revoked_at IS NULL;
CREATE UNIQUE INDEX note_shares_active_token_idx ON note_shares (token_hash)
    WHERE revoked_at IS NULL;
CREATE INDEX note_shares_project_idx ON note_shares (tenant_id, project_id, created_at DESC)
    WHERE revoked_at IS NULL;

CREATE TABLE note_share_access_tokens (
    token_hash bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    share_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '12 hours',
    FOREIGN KEY (tenant_id, project_id, share_id)
        REFERENCES note_shares (tenant_id, project_id, id) ON DELETE CASCADE,
    CHECK (expires_at > created_at)
);

CREATE INDEX note_share_access_tokens_share_idx
    ON note_share_access_tokens (tenant_id, project_id, share_id, expires_at);
