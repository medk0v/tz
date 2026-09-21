ALTER TABLE project_roles
    DROP CONSTRAINT project_roles_permissions_cardinality_check,
    DROP CONSTRAINT project_roles_permissions_allowed_check,
    DROP CONSTRAINT project_roles_admin_permissions_check,
    DROP CONSTRAINT project_roles_non_admin_permissions_check;

UPDATE project_roles
SET permissions = permissions || ARRAY['quality:read_all']::text[],
    updated_at = now()
WHERE base_role IN ('admin', 'manager')
  AND permissions @> ARRAY['quality:read']::text[]
  AND NOT permissions @> ARRAY['quality:read_all']::text[];

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_permissions_cardinality_check
        CHECK (cardinality(permissions) BETWEEN 0 AND 20),
    ADD CONSTRAINT project_roles_permissions_allowed_check
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
                'visitor_network:read',
                'quality:read',
                'quality:read_all',
                'reviews:read',
                'routing:manage',
                'teams:manage',
                'ai:manage',
                'knowledge:manage',
                'integrations:manage',
                'roles:manage',
                'system:read'
            ]::text[]
        ),
    ADD CONSTRAINT project_roles_admin_permissions_check
        CHECK (
            base_role <> 'admin'
            OR (
                cardinality(permissions) = 20
                AND permissions @> ARRAY[
                    'projects:read', 'projects:manage', 'conversations:read',
                    'conversations:reply', 'conversations:close', 'contacts:read',
                    'channels:read', 'channels:manage', 'access_tokens:manage',
                    'visitor_network:read', 'quality:read', 'quality:read_all',
                    'reviews:read', 'routing:manage', 'teams:manage',
                    'ai:manage', 'knowledge:manage', 'integrations:manage',
                    'roles:manage', 'system:read'
                ]::text[]
            )
        ),
    ADD CONSTRAINT project_roles_non_admin_permissions_check
        CHECK (
            base_role = 'admin'
            OR permissions <@ ARRAY[
                'projects:read', 'conversations:read', 'conversations:reply',
                'conversations:close', 'contacts:read', 'channels:read',
                'channels:manage', 'visitor_network:read', 'quality:read',
                'quality:read_all', 'reviews:read', 'routing:manage',
                'teams:manage', 'ai:manage', 'knowledge:manage',
                'integrations:manage', 'system:read'
            ]::text[]
        );

UPDATE memberships AS membership
SET permissions = role.permissions,
    updated_at = now()
FROM project_roles AS role
WHERE role.tenant_id = membership.tenant_id
  AND role.project_id = membership.project_id
  AND role.id = membership.role;

UPDATE memberships
SET permissions = permissions || ARRAY['quality:read_all']::text[],
    updated_at = now()
WHERE project_id IS NULL
  AND role = 'admin'
  AND permissions @> ARRAY['quality:read']::text[]
  AND NOT permissions @> ARRAY['quality:read_all']::text[];

UPDATE api_keys AS api_key
SET permissions = ARRAY(
    SELECT permission
    FROM unnest(role.permissions) AS permission
    WHERE permission <> ALL (ARRAY[
        'projects:manage',
        'access_tokens:manage',
        'roles:manage'
    ]::text[])
)
FROM project_roles AS role
WHERE role.tenant_id = api_key.tenant_id
  AND role.project_id = api_key.project_id
  AND role.id = api_key.role;
