-- Process responsibility is independent of system authorization.
ALTER TABLE project_roles
    DROP CONSTRAINT project_roles_permissions_cardinality_check,
    DROP CONSTRAINT project_roles_permissions_allowed_check,
    DROP CONSTRAINT project_roles_admin_permissions_check,
    DROP CONSTRAINT project_roles_non_admin_permissions_check;

-- Extend standard roles. Custom roles and existing API tokens keep their grants.
UPDATE project_roles
SET permissions = permissions || ARRAY[
            'processes:read',
            'processes:edit',
            'processes:approve'
        ]::text[], updated_at = now()
WHERE base_role = 'admin' OR (is_system AND id = 'manager');
UPDATE project_roles
SET permissions = permissions || ARRAY['processes:read']::text[], updated_at = now()
WHERE is_system AND id = 'operator';

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_permissions_cardinality_check
        CHECK (cardinality(permissions) BETWEEN 0 AND 25),
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
            'reply_templates:manage',
            'processes:read',
            'processes:edit',
            'processes:approve',
            'integrations:manage',
            'roles:manage',
            'system:read'
        ]::text[]),
    ADD CONSTRAINT project_roles_admin_permissions_check
        CHECK (base_role <> 'admin' OR (cardinality(permissions) = 25 AND permissions @> ARRAY[
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
            'reply_templates:manage',
            'processes:read',
            'processes:edit',
            'processes:approve',
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
            'reply_templates:manage',
            'processes:read',
            'processes:edit',
            'processes:approve',
            'integrations:manage',
            'system:read'
        ]::text[]),
    ADD CONSTRAINT project_roles_processes_read_check
        CHECK (NOT permissions && ARRAY['processes:edit', 'processes:approve']::text[]
               OR permissions @> ARRAY['processes:read']::text[]);

-- Discover generated names instead of assuming how PostgreSQL named the checks.
DO $$
DECLARE constraint_name text;
BEGIN
    FOR constraint_name IN
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'departments'::regclass AND contype = 'c'
          AND pg_get_constraintdef(oid) LIKE '%sidebar_items%'
          AND (pg_get_constraintdef(oid) LIKE '%cardinality%' OR pg_get_constraintdef(oid) LIKE '%<@%')
    LOOP
        EXECUTE format('ALTER TABLE departments DROP CONSTRAINT %I', constraint_name);
    END LOOP;
END $$;

ALTER TABLE departments
    ADD CONSTRAINT departments_sidebar_items_count CHECK (cardinality(sidebar_items) BETWEEN 1 AND 13),
    ADD CONSTRAINT departments_sidebar_items_allowed CHECK (sidebar_items <@ ARRAY[
            'conversations',
            'contacts',
            'online_visitors',
            'support_quality',
            'inbox_routing',
            'channels',
            'team',
            'ai',
            'tasks',
            'knowledge_base',
            'reply_templates',
            'integrations',
            'processes'
        ]::text[]),
    ALTER COLUMN sidebar_items SET DEFAULT ARRAY[
            'conversations',
            'contacts',
            'online_visitors',
            'support_quality',
            'inbox_routing',
            'channels',
            'team',
            'ai',
            'tasks',
            'knowledge_base',
            'reply_templates',
            'integrations',
            'processes'
        ]::text[];

-- Keep configured department menus intact; extend only the untouched standard menu.
UPDATE departments SET sidebar_items = array_append(sidebar_items, 'processes'), updated_at = now()
WHERE sidebar_items = ARRAY[
            'conversations',
            'contacts',
            'online_visitors',
            'support_quality',
            'inbox_routing',
            'channels',
            'team',
            'ai',
            'tasks',
            'knowledge_base',
            'reply_templates',
            'integrations'
        ]::text[];
