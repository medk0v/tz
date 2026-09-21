ALTER TABLE users
    ADD COLUMN password_hash text,
    ADD COLUMN failed_login_attempts integer NOT NULL DEFAULT 0
        CHECK (failed_login_attempts >= 0),
    ADD COLUMN failed_login_window_started_at timestamptz,
    ADD COLUMN locked_until timestamptz;

ALTER TABLE memberships
    ADD CONSTRAINT memberships_role_check
        CHECK (role IN ('admin', 'manager', 'operator'));

ALTER TABLE api_keys
    ADD COLUMN role text NOT NULL DEFAULT 'operator'
        CHECK (role IN ('admin', 'manager', 'operator')),
    ADD COLUMN created_by_user_id uuid REFERENCES users (id),
    ADD COLUMN last_used_at timestamptz;

UPDATE api_keys AS api_key
SET role = membership.role
FROM memberships AS membership
WHERE api_key.actor_user_id = membership.user_id
  AND api_key.tenant_id = membership.tenant_id
  AND api_key.project_id IS NOT DISTINCT FROM membership.project_id;

UPDATE api_keys
SET expires_at = now() + INTERVAL '30 days'
WHERE expires_at IS NULL;

ALTER TABLE api_keys
    ALTER COLUMN expires_at SET NOT NULL;

CREATE TABLE operator_sessions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid,
    user_id uuid NOT NULL REFERENCES users (id),
    membership_id uuid NOT NULL,
    token_hash bytea NOT NULL UNIQUE,
    csrf_token_hash bytea NOT NULL,
    idle_expires_at timestamptz NOT NULL,
    absolute_expires_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, membership_id) REFERENCES memberships (tenant_id, id),
    CHECK (idle_expires_at <= absolute_expires_at)
);

CREATE INDEX operator_sessions_active_token_idx
    ON operator_sessions (token_hash, idle_expires_at, absolute_expires_at)
    WHERE revoked_at IS NULL;

CREATE INDEX operator_sessions_user_idx
    ON operator_sessions (tenant_id, user_id, created_at DESC);
