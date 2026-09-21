-- Notes have independent read/write grants. Extend administrator and untouched
-- built-in presets; customized roles and existing tokens retain their scopes.
-- Manager/operator presets are editable (is_system = false), so compare grants.
ALTER TABLE project_roles
    DROP CONSTRAINT project_roles_permissions_cardinality_check,
    DROP CONSTRAINT project_roles_permissions_allowed_check,
    DROP CONSTRAINT project_roles_admin_permissions_check,
    DROP CONSTRAINT project_roles_non_admin_permissions_check;

UPDATE project_roles
SET permissions = permissions || ARRAY['notes:read', 'notes:write']::text[], updated_at = now()
WHERE base_role = 'admin'
    OR (id = 'manager' AND base_role = 'manager'
        AND cardinality(permissions) = 23
        AND permissions @> ARRAY[
            'projects:read', 'conversations:read', 'conversations:reply', 'conversations:close', 'contacts:read', 'contacts:manage', 'channels:read', 'channels:manage', 'visitor_network:read', 'quality:read', 'quality:read_all', 'reviews:read', 'routing:manage', 'teams:manage', 'ai:manage', 'knowledge:manage', 'reply_templates:manage', 'processes:read', 'processes:edit', 'processes:approve', 'tasks:own', 'tasks:manage', 'integrations:manage'
        ]::text[])
    OR (id = 'operator' AND base_role = 'operator'
        AND cardinality(permissions) = 10
        AND permissions @> ARRAY[
            'projects:read', 'conversations:read', 'conversations:reply', 'conversations:close', 'contacts:read', 'contacts:manage', 'channels:read', 'quality:read', 'processes:read', 'tasks:own'
        ]::text[]);

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_permissions_cardinality_check
        CHECK (cardinality(permissions) BETWEEN 0 AND 30),
    ADD CONSTRAINT project_roles_permissions_allowed_check
        CHECK (permissions <@ ARRAY[
            'projects:read',
            'projects:manage',
            'conversations:read',
            'conversations:reply',
            'conversations:close',
            'contacts:read',
            'contacts:manage',
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
            'notes:read',
            'notes:write',
            'reply_templates:manage',
            'processes:read',
            'processes:edit',
            'processes:approve',
            'tasks:own',
            'tasks:manage',
            'tasks:configure',
            'integrations:manage',
            'roles:manage',
            'system:read'
        ]::text[]),
    ADD CONSTRAINT project_roles_admin_permissions_check
        CHECK (base_role <> 'admin' OR (cardinality(permissions) = 30 AND permissions @> ARRAY[
            'projects:read',
            'projects:manage',
            'conversations:read',
            'conversations:reply',
            'conversations:close',
            'contacts:read',
            'contacts:manage',
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
            'notes:read',
            'notes:write',
            'reply_templates:manage',
            'processes:read',
            'processes:edit',
            'processes:approve',
            'tasks:own',
            'tasks:manage',
            'tasks:configure',
            'integrations:manage',
            'roles:manage',
            'system:read'
        ]::text[])),
    ADD CONSTRAINT project_roles_non_admin_permissions_check
        CHECK (base_role = 'admin' OR permissions <@ ARRAY[
            'projects:read',
            'conversations:read',
            'conversations:reply',
            'conversations:close',
            'contacts:read',
            'contacts:manage',
            'channels:read',
            'channels:manage',
            'visitor_network:read',
            'quality:read',
            'quality:read_all',
            'reviews:read',
            'routing:manage',
            'teams:manage',
            'ai:manage',
            'knowledge:manage',
            'notes:read',
            'notes:write',
            'reply_templates:manage',
            'processes:read',
            'processes:edit',
            'processes:approve',
            'tasks:own',
            'tasks:manage',
            'tasks:configure',
            'integrations:manage',
            'system:read'
        ]::text[]);

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_notes_scope_check
        CHECK (NOT permissions @> ARRAY['notes:write']::text[]
               OR permissions @> ARRAY['notes:read']::text[]);
