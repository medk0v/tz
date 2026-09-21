ALTER TABLE memberships
    DROP CONSTRAINT IF EXISTS memberships_role_check;

ALTER TABLE api_keys
    DROP CONSTRAINT IF EXISTS api_keys_role_check;

CREATE TABLE project_roles (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    id text NOT NULL CHECK (id ~ '^[a-z0-9][a-z0-9_-]{0,62}$'),
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    base_role text NOT NULL CHECK (base_role IN ('admin', 'manager', 'operator')),
    permissions text[] NOT NULL,
    is_system boolean NOT NULL DEFAULT false,
    updated_by_user_id uuid REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    CHECK (cardinality(permissions) BETWEEN 0 AND 10),
    CHECK (array_position(permissions, NULL) IS NULL),
    CHECK (
        permissions <@ ARRAY[
            'projects:read',
            'projects:manage',
            'conversations:read',
            'conversations:reply',
            'conversations:close',
            'contacts:read',
            'channels:read',
            'channels:manage',
            'access_tokens:manage',
            'visitor_network:read'
        ]::text[]
    ),
    CHECK (
        (id = 'admin' AND base_role = 'admin' AND is_system)
        OR (id <> 'admin' AND base_role IN ('manager', 'operator'))
    ),
    CHECK (
        base_role <> 'admin'
        OR (
            cardinality(permissions) = 10
            AND permissions @> ARRAY[
                'projects:read', 'projects:manage', 'conversations:read',
                'conversations:reply', 'conversations:close', 'contacts:read',
                'channels:read', 'channels:manage', 'access_tokens:manage',
                'visitor_network:read'
            ]::text[]
        )
    ),
    CHECK (
        base_role = 'admin'
        OR permissions <@ ARRAY[
            'projects:read', 'conversations:read', 'conversations:reply',
            'conversations:close', 'contacts:read', 'channels:read',
            'channels:manage', 'visitor_network:read'
        ]::text[]
    )
);

CREATE UNIQUE INDEX project_roles_name_unique_idx
    ON project_roles (tenant_id, project_id, lower(name));

INSERT INTO project_roles (
    tenant_id, project_id, id, name, base_role, permissions, is_system
)
SELECT project.tenant_id, project.id, preset.id, preset.name,
       preset.base_role, preset.permissions, preset.is_system
FROM projects AS project
CROSS JOIN (
    VALUES
        (
            'admin'::text,
            'Administrator'::text,
            'admin'::text,
            ARRAY[
                'projects:read',
                'projects:manage',
                'conversations:read',
                'conversations:reply',
                'conversations:close',
                'contacts:read',
                'channels:read',
                'channels:manage',
                'access_tokens:manage',
                'visitor_network:read'
            ]::text[],
            true
        ),
        (
            'manager'::text,
            'Manager'::text,
            'manager'::text,
            ARRAY[
                'projects:read',
                'conversations:read',
                'conversations:reply',
                'conversations:close',
                'contacts:read',
                'channels:read',
                'channels:manage',
                'visitor_network:read'
            ]::text[],
            false
        ),
        (
            'operator'::text,
            'Operator'::text,
            'operator'::text,
            ARRAY[
                'projects:read',
                'conversations:read',
                'conversations:reply',
                'conversations:close',
                'contacts:read',
                'channels:read'
            ]::text[],
            false
        )
) AS preset(id, name, base_role, permissions, is_system);

CREATE INDEX memberships_project_role_idx
    ON memberships (tenant_id, project_id, role)
    WHERE revoked_at IS NULL;

CREATE INDEX api_keys_project_role_idx
    ON api_keys (tenant_id, project_id, role)
    WHERE revoked_at IS NULL;
