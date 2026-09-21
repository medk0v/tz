CREATE TABLE reply_templates (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    title text NOT NULL CHECK (char_length(btrim(title)) BETWEEN 1 AND 120),
    body text NOT NULL CHECK (char_length(btrim(body)) BETWEEN 1 AND 10000),
    body_format text NOT NULL DEFAULT 'markdown' CHECK (body_format = 'markdown'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id, inbox_id)
        REFERENCES inboxes (tenant_id, project_id, id) ON DELETE CASCADE
);

CREATE INDEX reply_templates_inbox_title_idx
    ON reply_templates (tenant_id, project_id, inbox_id, lower(title), id);

ALTER TABLE project_roles
    DROP CONSTRAINT project_roles_permissions_cardinality_check,
    DROP CONSTRAINT project_roles_permissions_allowed_check,
    DROP CONSTRAINT project_roles_admin_permissions_check,
    DROP CONSTRAINT project_roles_non_admin_permissions_check;

-- Add the preset permission without changing grants on custom roles.
UPDATE project_roles
SET permissions = permissions || ARRAY['reply_templates:manage']::text[],
    updated_at = now()
WHERE id IN ('admin', 'manager')
  AND NOT permissions @> ARRAY['reply_templates:manage']::text[];

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_permissions_cardinality_check
        CHECK (cardinality(permissions) BETWEEN 0 AND 21),
    ADD CONSTRAINT project_roles_permissions_allowed_check
        CHECK (
            permissions <@ ARRAY[
                'projects:read', 'projects:manage', 'conversations:read',
                'conversations:reply', 'conversations:close', 'contacts:read',
                'channels:read', 'channels:manage', 'access_tokens:manage',
                'visitor_network:read', 'quality:read', 'quality:read_all',
                'reviews:read', 'routing:manage', 'teams:manage',
                'ai:manage', 'knowledge:manage', 'reply_templates:manage',
                'integrations:manage', 'roles:manage', 'system:read'
            ]::text[]
        ),
    ADD CONSTRAINT project_roles_admin_permissions_check
        CHECK (
            base_role <> 'admin'
            OR (
                cardinality(permissions) = 21
                AND permissions @> ARRAY[
                    'projects:read', 'projects:manage', 'conversations:read',
                    'conversations:reply', 'conversations:close', 'contacts:read',
                    'channels:read', 'channels:manage', 'access_tokens:manage',
                    'visitor_network:read', 'quality:read', 'quality:read_all',
                    'reviews:read', 'routing:manage', 'teams:manage',
                    'ai:manage', 'knowledge:manage', 'reply_templates:manage',
                    'integrations:manage', 'roles:manage', 'system:read'
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
                'reply_templates:manage', 'integrations:manage', 'system:read'
            ]::text[]
        );

-- Sessions read the live role or membership permissions on each request.
-- Preserve every existing membership/token grant while adding the new preset.
UPDATE memberships AS membership
SET permissions = membership.permissions || ARRAY['reply_templates:manage']::text[],
    updated_at = now()
WHERE membership.role IN ('admin', 'manager')
  AND NOT membership.permissions @> ARRAY['reply_templates:manage']::text[]
  AND (
      membership.project_id IS NULL
      OR EXISTS (
          SELECT 1 FROM project_roles AS role
          WHERE role.tenant_id = membership.tenant_id
            AND role.project_id = membership.project_id
            AND role.id = membership.role
            AND role.permissions @> ARRAY['reply_templates:manage']::text[]
      )
  );

UPDATE api_keys AS api_key
SET permissions = api_key.permissions || ARRAY['reply_templates:manage']::text[]
WHERE api_key.role IN ('admin', 'manager')
  AND NOT api_key.permissions @> ARRAY['reply_templates:manage']::text[]
  AND (
      api_key.project_id IS NULL
      OR EXISTS (
          SELECT 1 FROM project_roles AS role
          WHERE role.tenant_id = api_key.tenant_id
            AND role.project_id = api_key.project_id
            AND role.id = api_key.role
            AND role.permissions @> ARRAY['reply_templates:manage']::text[]
      )
  );
