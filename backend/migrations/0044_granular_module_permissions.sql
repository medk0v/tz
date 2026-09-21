DO $$
DECLARE
    constraint_name text;
BEGIN
    FOR constraint_name IN
        SELECT conname
        FROM pg_constraint
        WHERE conrelid = 'project_roles'::regclass
          AND contype = 'c'
          AND pg_get_constraintdef(oid) ILIKE '%permissions%'
    LOOP
        EXECUTE format('ALTER TABLE project_roles DROP CONSTRAINT %I', constraint_name);
    END LOOP;
END $$;

UPDATE project_roles
SET permissions = permissions || ARRAY['quality:read']::text[]
WHERE permissions @> ARRAY['conversations:read']::text[]
  AND NOT permissions @> ARRAY['quality:read']::text[];

UPDATE project_roles
SET permissions = permissions || ARRAY[
    'reviews:read',
    'routing:manage',
    'teams:manage',
    'ai:manage',
    'knowledge:manage',
    'integrations:manage'
]::text[]
WHERE base_role IN ('admin', 'manager');

UPDATE project_roles
SET permissions = permissions || ARRAY['roles:manage', 'system:read']::text[]
WHERE base_role = 'admin';

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_permissions_cardinality_check
        CHECK (cardinality(permissions) BETWEEN 0 AND 19),
    ADD CONSTRAINT project_roles_permissions_no_null_check
        CHECK (array_position(permissions, NULL) IS NULL),
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
                cardinality(permissions) = 19
                AND permissions @> ARRAY[
                    'projects:read', 'projects:manage', 'conversations:read',
                    'conversations:reply', 'conversations:close', 'contacts:read',
                    'channels:read', 'channels:manage', 'access_tokens:manage',
                    'visitor_network:read', 'quality:read', 'reviews:read',
                    'routing:manage', 'teams:manage', 'ai:manage',
                    'knowledge:manage', 'integrations:manage', 'roles:manage',
                    'system:read'
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
                'reviews:read', 'routing:manage', 'teams:manage', 'ai:manage',
                'knowledge:manage', 'integrations:manage', 'system:read'
            ]::text[]
        );

UPDATE memberships AS membership
SET permissions = role.permissions,
    updated_at = now()
FROM project_roles AS role
WHERE role.tenant_id = membership.tenant_id
  AND role.project_id = membership.project_id
  AND role.id = membership.role;

UPDATE api_keys AS api_key
SET permissions = ARRAY(
    SELECT permission
    FROM unnest(role.permissions) AS permission
    WHERE permission <> ALL (ARRAY[
        'projects:manage',
        'access_tokens:manage',
        'visitor_network:read',
        'roles:manage'
    ]::text[])
)
FROM project_roles AS role
WHERE role.tenant_id = api_key.tenant_id
  AND role.project_id = api_key.project_id
  AND role.id = api_key.role;
