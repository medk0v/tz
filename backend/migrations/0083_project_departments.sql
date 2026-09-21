CREATE TABLE departments (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    icon text NOT NULL DEFAULT 'headset' CHECK (length(icon) BETWEEN 1 AND 32),
    sidebar_items text[] NOT NULL DEFAULT ARRAY[
        'conversations', 'contacts', 'online_visitors', 'support_quality',
        'inbox_routing', 'channels', 'ai', 'tasks', 'knowledge_base',
        'reply_templates', 'integrations'
    ],
    default_page text NOT NULL DEFAULT 'conversations',
    position integer NOT NULL DEFAULT 0 CHECK (position BETWEEN 0 AND 999),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    CHECK (cardinality(sidebar_items) BETWEEN 1 AND 11),
    CHECK (array_position(sidebar_items, NULL) IS NULL),
    CHECK (sidebar_items <@ ARRAY[
        'conversations', 'contacts', 'online_visitors', 'support_quality',
        'inbox_routing', 'channels', 'ai', 'tasks', 'knowledge_base',
        'reply_templates', 'integrations'
    ]::text[]),
    CHECK (default_page = ANY(sidebar_items))
);

CREATE UNIQUE INDEX departments_name_unique_idx
    ON departments (tenant_id, project_id, lower(name));

ALTER TABLE projects
    ADD COLUMN default_department_id uuid,
    ADD COLUMN director_enabled boolean NOT NULL DEFAULT true,
    ADD CONSTRAINT projects_default_department_fk
        FOREIGN KEY (tenant_id, id, default_department_id)
        REFERENCES departments (tenant_id, project_id, id);

ALTER TABLE inboxes
    ADD COLUMN department_id uuid,
    ADD CONSTRAINT inboxes_department_fk
        FOREIGN KEY (tenant_id, project_id, department_id)
        REFERENCES departments (tenant_id, project_id, id);

CREATE INDEX inboxes_department_idx ON inboxes (tenant_id, project_id, department_id);

ALTER TABLE memberships
    ADD COLUMN department_id uuid,
    ADD COLUMN director_access boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT memberships_department_fk
        FOREIGN KEY (tenant_id, project_id, department_id)
        REFERENCES departments (tenant_id, project_id, id),
    ADD CONSTRAINT memberships_department_scope_check CHECK (
        department_id IS NULL OR (project_id IS NOT NULL AND role <> 'admin' AND NOT director_access)
    );

ALTER TABLE operator_sessions
    ADD COLUMN department_id uuid,
    ADD COLUMN director_mode boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT operator_sessions_department_fk
        FOREIGN KEY (tenant_id, project_id, department_id)
        REFERENCES departments (tenant_id, project_id, id),
    ADD CONSTRAINT operator_sessions_department_context_check CHECK (
        (department_id IS NULL OR project_id IS NOT NULL)
        AND (NOT director_mode OR (project_id IS NOT NULL AND department_id IS NULL))
    );

INSERT INTO departments (id, tenant_id, project_id, name)
SELECT gen_random_uuid(), tenant_id, id, 'Support' FROM projects;

UPDATE projects AS project
SET default_department_id = department.id
FROM departments AS department
WHERE department.tenant_id = project.tenant_id AND department.project_id = project.id;

UPDATE inboxes AS inbox
SET department_id = project.default_department_id
FROM projects AS project
WHERE project.tenant_id = inbox.tenant_id AND project.id = inbox.project_id;
