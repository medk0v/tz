CREATE TABLE note_databases (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 160),
    description text NOT NULL DEFAULT '' CHECK (length(description) <= 4000),
    icon text NOT NULL DEFAULT '' CHECK (length(icon) <= 32),
    version integer NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE TABLE note_database_fields (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    database_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 160),
    field_type text NOT NULL CHECK (field_type IN (
        'text', 'long_text', 'number', 'date', 'boolean', 'single_select',
        'multi_select', 'url', 'email', 'relation', 'rollup'
    )),
    position integer NOT NULL CHECK (position >= 0),
    version integer NOT NULL DEFAULT 1 CHECK (version > 0),
    config jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(config) = 'object'),
    relation_target_database_id uuid,
    relation_cardinality text CHECK (relation_cardinality IN ('one_to_one', 'one_to_many', 'many_to_one', 'many_to_many')),
    relation_owner_field_id uuid,
    inverse_field_id uuid,
    rollup_relation_field_id uuid,
    rollup_target_field_id uuid,
    rollup_operation text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    UNIQUE (tenant_id, project_id, database_id, id),
    FOREIGN KEY (tenant_id, project_id, database_id)
        REFERENCES note_databases (tenant_id, project_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, relation_target_database_id)
        REFERENCES note_databases (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id, relation_owner_field_id)
        REFERENCES note_database_fields (tenant_id, project_id, id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (tenant_id, project_id, inverse_field_id)
        REFERENCES note_database_fields (tenant_id, project_id, id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (tenant_id, project_id, database_id, rollup_relation_field_id)
        REFERENCES note_database_fields (tenant_id, project_id, database_id, id),
    FOREIGN KEY (tenant_id, project_id, rollup_target_field_id)
        REFERENCES note_database_fields (tenant_id, project_id, id),
    CHECK ((field_type = 'relation') = (relation_target_database_id IS NOT NULL AND relation_cardinality IS NOT NULL)),
    CHECK (field_type = 'relation' OR (relation_owner_field_id IS NULL AND inverse_field_id IS NULL)),
    CHECK ((field_type = 'rollup') = (rollup_relation_field_id IS NOT NULL AND rollup_operation IS NOT NULL)),
    CHECK (field_type = 'rollup' OR rollup_target_field_id IS NULL)
);
CREATE INDEX note_database_fields_order_idx ON note_database_fields (tenant_id, project_id, database_id, position, id);

CREATE TABLE note_database_records (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    database_id uuid NOT NULL,
    position integer NOT NULL CHECK (position >= 0),
    version integer NOT NULL DEFAULT 1 CHECK (version > 0),
    field_values jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(field_values) = 'object'),
    content_markdown text NOT NULL DEFAULT '' CHECK (length(content_markdown) <= 200000 AND octet_length(content_markdown) <= 800000),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    UNIQUE (tenant_id, project_id, database_id, id),
    FOREIGN KEY (tenant_id, project_id, database_id)
        REFERENCES note_databases (tenant_id, project_id, id) ON DELETE CASCADE
);
CREATE INDEX note_database_records_order_idx ON note_database_records (tenant_id, project_id, database_id, position, id);

CREATE TABLE note_record_relations (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    relation_field_id uuid NOT NULL,
    source_database_id uuid NOT NULL,
    target_database_id uuid NOT NULL,
    source_record_id uuid NOT NULL,
    target_record_id uuid NOT NULL,
    cardinality text NOT NULL CHECK (cardinality IN ('one_to_one', 'one_to_many', 'many_to_many')),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, relation_field_id, source_record_id, target_record_id),
    FOREIGN KEY (tenant_id, project_id, relation_field_id)
        REFERENCES note_database_fields (tenant_id, project_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, source_database_id, source_record_id)
        REFERENCES note_database_records (tenant_id, project_id, database_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, target_database_id, target_record_id)
        REFERENCES note_database_records (tenant_id, project_id, database_id, id) ON DELETE CASCADE,
    CHECK (source_record_id <> target_record_id)
);
CREATE UNIQUE INDEX note_record_relations_one_source_idx
    ON note_record_relations (tenant_id, project_id, relation_field_id, source_record_id)
    WHERE cardinality = 'one_to_one';
CREATE UNIQUE INDEX note_record_relations_one_target_idx
    ON note_record_relations (tenant_id, project_id, relation_field_id, target_record_id)
    WHERE cardinality IN ('one_to_one', 'one_to_many');

CREATE TABLE note_database_views (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    database_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 160),
    position integer NOT NULL CHECK (position >= 0),
    version integer NOT NULL DEFAULT 1 CHECK (version > 0),
    config jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(config) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, database_id, id),
    FOREIGN KEY (tenant_id, project_id, database_id)
        REFERENCES note_databases (tenant_id, project_id, id) ON DELETE CASCADE
);
CREATE INDEX note_database_views_order_idx ON note_database_views (tenant_id, project_id, database_id, position, id);

CREATE TABLE note_database_embeds (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    note_id uuid NOT NULL,
    database_id uuid NOT NULL,
    view_id uuid,
    position integer NOT NULL CHECK (position >= 0),
    version integer NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, note_id, id),
    FOREIGN KEY (tenant_id, project_id, note_id)
        REFERENCES notes (tenant_id, project_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, database_id)
        REFERENCES note_databases (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id, database_id, view_id)
        REFERENCES note_database_views (tenant_id, project_id, database_id, id)
);
CREATE INDEX note_database_embeds_note_idx ON note_database_embeds (tenant_id, project_id, note_id, position, id);
