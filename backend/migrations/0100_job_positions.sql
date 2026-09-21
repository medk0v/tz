CREATE TABLE job_positions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    department_id uuid,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    description text NOT NULL DEFAULT '' CHECK (length(description) <= 10000),
    instructions text NOT NULL DEFAULT '' CHECK (length(instructions) <= 50000),
    kpi_goal text NOT NULL DEFAULT '' CHECK (length(kpi_goal) <= 10000),
    draft_kpis jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(draft_kpis) = 'array' AND jsonb_array_length(draft_kpis) <= 20),
    approved_kpis jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(approved_kpis) = 'array' AND jsonb_array_length(approved_kpis) <= 20),
    approved_at timestamptz,
    approved_by uuid REFERENCES users (id),
    revision integer NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id, department_id) REFERENCES departments (tenant_id, project_id, id),
    CHECK ((approved_at IS NULL) = (approved_by IS NULL))
);

CREATE INDEX job_positions_project_idx ON job_positions (tenant_id, project_id, department_id);

ALTER TABLE memberships
    ADD COLUMN position_id uuid,
    ADD CONSTRAINT memberships_job_position_fk FOREIGN KEY (tenant_id, project_id, position_id)
        REFERENCES job_positions (tenant_id, project_id, id),
    ADD CONSTRAINT memberships_job_position_project_check CHECK (position_id IS NULL OR project_id IS NOT NULL);

ALTER TABLE ai_profiles
    ADD COLUMN position_id uuid,
    ADD CONSTRAINT ai_profiles_job_position_fk FOREIGN KEY (tenant_id, project_id, position_id)
        REFERENCES job_positions (tenant_id, project_id, id);

CREATE INDEX memberships_job_position_idx ON memberships (tenant_id, project_id, position_id)
    WHERE position_id IS NOT NULL;
CREATE INDEX ai_profiles_job_position_idx ON ai_profiles (tenant_id, project_id, position_id)
    WHERE position_id IS NOT NULL;
