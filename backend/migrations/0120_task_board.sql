-- Kanban lists with editable statuses, task tags, employees as task participants
-- and task permissions separate from AI configuration.

CREATE TABLE task_lists (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    -- The default list has no stored name so every interface language can label it.
    name text CHECK (name IS NULL OR length(trim(name)) BETWEEN 1 AND 100),
    emoji text CHECK (emoji IS NULL OR length(emoji) BETWEEN 1 AND 16),
    color text CHECK (color IS NULL OR color ~ '^#[0-9a-f]{6}$'),
    is_default boolean NOT NULL DEFAULT false,
    position integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id) ON DELETE CASCADE,
    CHECK (is_default OR name IS NOT NULL)
);
CREATE UNIQUE INDEX task_lists_default_idx ON task_lists (tenant_id, project_id) WHERE is_default;

-- Board columns are the statuses of a list. The kind lets agents and the
-- checkbox move a card without knowing each list's custom names.
CREATE TABLE task_list_columns (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    list_id uuid NOT NULL,
    name text CHECK (name IS NULL OR length(trim(name)) BETWEEN 1 AND 60),
    preset_key text CHECK (preset_key IN ('backlog', 'in_progress', 'done')),
    emoji text CHECK (emoji IS NULL OR length(emoji) BETWEEN 1 AND 16),
    color text NOT NULL CHECK (color ~ '^#[0-9a-f]{6}$'),
    kind text NOT NULL CHECK (kind IN ('todo', 'in_progress', 'done')),
    position integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    UNIQUE (tenant_id, project_id, list_id, id),
    FOREIGN KEY (tenant_id, project_id, list_id)
        REFERENCES task_lists (tenant_id, project_id, id) ON DELETE CASCADE,
    CHECK (name IS NOT NULL OR preset_key IS NOT NULL)
);
CREATE INDEX task_list_columns_list_idx ON task_list_columns (list_id, position, id);

CREATE TABLE task_tags (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 40),
    color text NOT NULL CHECK (color ~ '^#[0-9a-f]{6}$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX task_tags_name_idx ON task_tags (tenant_id, project_id, lower(name));

ALTER TABLE ai_tasks
    ADD COLUMN coordinator_user_id uuid REFERENCES users (id),
    ADD COLUMN list_id uuid,
    ADD COLUMN column_id uuid,
    ADD COLUMN position integer NOT NULL DEFAULT 0,
    ADD COLUMN due_at timestamptz,
    ADD COLUMN completed_at timestamptz,
    ADD CONSTRAINT ai_tasks_list_fk FOREIGN KEY (tenant_id, project_id, list_id)
        REFERENCES task_lists (tenant_id, project_id, id),
    ADD CONSTRAINT ai_tasks_column_fk FOREIGN KEY (tenant_id, project_id, list_id, column_id)
        REFERENCES task_list_columns (tenant_id, project_id, list_id, id),
    ADD CONSTRAINT ai_tasks_column_list CHECK (column_id IS NULL OR list_id IS NOT NULL),
    DROP CONSTRAINT ai_tasks_coordinator_mode,
    ADD CONSTRAINT ai_tasks_coordinator_mode CHECK (
        (execution_mode = 'independent' AND coordinator_id IS NULL AND coordinator_user_id IS NULL)
        OR (execution_mode = 'team' AND (coordinator_id IS NULL) <> (coordinator_user_id IS NULL))
    );
CREATE INDEX ai_tasks_board_idx ON ai_tasks (tenant_id, list_id, column_id, position, id);

CREATE TABLE ai_task_tags (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    tag_id uuid NOT NULL,
    PRIMARY KEY (task_id, tag_id),
    FOREIGN KEY (tenant_id, project_id, task_id)
        REFERENCES ai_tasks (tenant_id, project_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, tag_id)
        REFERENCES task_tags (tenant_id, project_id, id) ON DELETE CASCADE
);
CREATE INDEX ai_task_tags_tag_idx ON ai_task_tags (tag_id);

-- Employees assigned to a task: responsible people for independent tasks and
-- human members of a team.
CREATE TABLE ai_task_assignees (
    tenant_id uuid NOT NULL,
    task_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id),
    PRIMARY KEY (task_id, user_id),
    FOREIGN KEY (tenant_id, task_id) REFERENCES ai_tasks (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ai_task_assignees_user_idx ON ai_task_assignees (tenant_id, user_id);

-- A team step is performed either by an agent or by an employee. Participant
-- names stay snapshotted in agent_name so history survives later changes.
ALTER TABLE ai_task_execution_steps
    ALTER COLUMN agent_id DROP NOT NULL,
    ADD COLUMN assignee_user_id uuid,
    ADD CONSTRAINT ai_task_execution_steps_participant
        CHECK ((agent_id IS NULL) <> (assignee_user_id IS NULL));
CREATE INDEX ai_task_steps_assignee_idx ON ai_task_execution_steps (tenant_id, assignee_user_id, status)
    WHERE assignee_user_id IS NOT NULL;

-- Existing tasks start in the backlog of their project's default list.
INSERT INTO task_lists (id, tenant_id, project_id, is_default)
SELECT gen_random_uuid(), project.tenant_id, project.id, true
FROM projects AS project
WHERE EXISTS (
    SELECT 1 FROM ai_tasks AS task
    WHERE task.tenant_id = project.tenant_id AND task.project_id = project.id
);
INSERT INTO task_list_columns (id, tenant_id, project_id, list_id, preset_key, color, kind, position)
SELECT gen_random_uuid(), list.tenant_id, list.project_id, list.id, preset.key, preset.color, preset.kind, preset.position
FROM task_lists AS list
CROSS JOIN (VALUES
    ('backlog', '#8a94a6', 'todo', 0),
    ('in_progress', '#3b82f6', 'in_progress', 1),
    ('done', '#22a06b', 'done', 2)
) AS preset (key, color, kind, position);
UPDATE ai_tasks AS task
SET list_id = placement.list_id, column_id = placement.column_id, position = placement.position
FROM (
    SELECT task.id, list.id AS list_id, column_row.id AS column_id,
           (row_number() OVER (PARTITION BY list.id ORDER BY task.created_at DESC, task.id DESC))::integer - 1 AS position
    FROM ai_tasks AS task
    JOIN task_lists AS list
      ON list.tenant_id = task.tenant_id AND list.project_id = task.project_id AND list.is_default
    JOIN task_list_columns AS column_row
      ON column_row.list_id = list.id AND column_row.preset_key = 'backlog'
) AS placement
WHERE placement.id = task.id;

-- Task permissions: every role sees its own tasks; roles that managed tasks
-- through AI access keep managing them; board configuration starts with administrators.
ALTER TABLE project_roles
    DROP CONSTRAINT project_roles_permissions_cardinality_check,
    DROP CONSTRAINT project_roles_permissions_allowed_check,
    DROP CONSTRAINT project_roles_admin_permissions_check,
    DROP CONSTRAINT project_roles_non_admin_permissions_check;

UPDATE project_roles
SET permissions = permissions || ARRAY['tasks:own']::text[], updated_at = now()
WHERE NOT permissions @> ARRAY['tasks:own']::text[];
UPDATE project_roles
SET permissions = permissions || ARRAY['tasks:manage']::text[], updated_at = now()
WHERE (base_role = 'admin' OR permissions @> ARRAY['ai:manage']::text[])
  AND NOT permissions @> ARRAY['tasks:manage']::text[];
UPDATE project_roles
SET permissions = permissions || ARRAY['tasks:configure']::text[], updated_at = now()
WHERE base_role = 'admin' AND NOT permissions @> ARRAY['tasks:configure']::text[];

UPDATE memberships
SET permissions = permissions || ARRAY['tasks:own']::text[]
WHERE NOT permissions @> ARRAY['tasks:own']::text[];
UPDATE memberships
SET permissions = permissions || ARRAY['tasks:manage']::text[]
WHERE (role = 'admin' OR permissions @> ARRAY['ai:manage']::text[])
  AND NOT permissions @> ARRAY['tasks:manage']::text[];
UPDATE memberships
SET permissions = permissions || ARRAY['tasks:configure']::text[]
WHERE role = 'admin' AND NOT permissions @> ARRAY['tasks:configure']::text[];

-- Tokens with explicit grants that could manage tasks through AI access keep that ability.
UPDATE api_keys
SET permissions = permissions || ARRAY['tasks:own', 'tasks:manage']::text[]
WHERE permissions @> ARRAY['ai:manage']::text[]
  AND NOT permissions @> ARRAY['tasks:manage']::text[];

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_permissions_cardinality_check
        CHECK (cardinality(permissions) BETWEEN 0 AND 28),
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
            'tasks:own',
            'tasks:manage',
            'tasks:configure',
            'integrations:manage',
            'roles:manage',
            'system:read'
        ]::text[]),
    ADD CONSTRAINT project_roles_admin_permissions_check
        CHECK (base_role <> 'admin' OR (cardinality(permissions) = 28 AND permissions @> ARRAY[
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
            'reply_templates:manage',
            'processes:read',
            'processes:edit',
            'processes:approve',
            'tasks:own',
            'tasks:manage',
            'tasks:configure',
            'integrations:manage',
            'system:read'
        ]::text[]),
    ADD CONSTRAINT project_roles_tasks_scope_check
        CHECK ((NOT permissions @> ARRAY['tasks:manage']::text[] OR permissions @> ARRAY['tasks:own']::text[])
           AND (NOT permissions @> ARRAY['tasks:configure']::text[] OR permissions @> ARRAY['tasks:manage']::text[]));
