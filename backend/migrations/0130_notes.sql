CREATE TABLE notes (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    parent_id uuid,
    title text NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
    body text NOT NULL DEFAULT '' CHECK (length(body) <= 200000 AND octet_length(body) <= 800000),
    icon text NOT NULL DEFAULT '' CHECK (length(icon) <= 32),
    is_favorite boolean NOT NULL DEFAULT false,
    version integer NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id, parent_id) REFERENCES notes (tenant_id, project_id, id),
    CHECK (parent_id IS DISTINCT FROM id)
);

CREATE INDEX notes_parent_idx ON notes (tenant_id, project_id, parent_id);
CREATE INDEX notes_updated_idx ON notes (tenant_id, project_id, updated_at DESC, id);

ALTER TABLE departments
    DROP CONSTRAINT departments_sidebar_items_count,
    DROP CONSTRAINT departments_sidebar_items_allowed,
    ADD CONSTRAINT departments_sidebar_items_count CHECK (cardinality(sidebar_items) BETWEEN 1 AND 14),
    ADD CONSTRAINT departments_sidebar_items_allowed CHECK (sidebar_items <@ ARRAY[
        'conversations', 'contacts', 'online_visitors', 'support_quality',
        'inbox_routing', 'channels', 'team', 'ai', 'tasks', 'knowledge_base',
        'reply_templates', 'integrations', 'processes', 'notes'
    ]::text[]),
    ALTER COLUMN sidebar_items SET DEFAULT ARRAY[
        'conversations', 'contacts', 'online_visitors', 'support_quality',
        'inbox_routing', 'channels', 'team', 'ai', 'tasks', 'knowledge_base',
        'reply_templates', 'integrations', 'processes', 'notes'
    ]::text[];

-- Extend menus that show every existing section, keeping their current order.
-- Preserve configured subsets (including deliberately hidden sections).
UPDATE departments SET sidebar_items = array_append(sidebar_items, 'notes'), updated_at = now()
WHERE cardinality(sidebar_items) = 13 AND sidebar_items @> ARRAY[
    'conversations', 'contacts', 'online_visitors', 'support_quality',
    'inbox_routing', 'channels', 'team', 'ai', 'tasks', 'knowledge_base',
    'reply_templates', 'integrations', 'processes'
]::text[];
