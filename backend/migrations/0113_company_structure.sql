ALTER TABLE ai_profiles
    ADD CONSTRAINT ai_profiles_project_id_unique UNIQUE (tenant_id, project_id, id);

CREATE TABLE company_positions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    title text NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
    job_position_id uuid,
    department_id uuid,
    reports_to_position_id uuid,
    is_department_head boolean NOT NULL DEFAULT false,
    occupant_kind text NOT NULL CHECK (occupant_kind IN ('vacant', 'human', 'agent')),
    occupant_name text CHECK (occupant_name IS NULL OR length(trim(occupant_name)) BETWEEN 1 AND 160),
    user_id uuid REFERENCES users (id),
    ai_profile_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    CONSTRAINT company_positions_job_position_fk FOREIGN KEY (tenant_id, project_id, job_position_id)
        REFERENCES job_positions (tenant_id, project_id, id),
    CONSTRAINT company_positions_department_fk FOREIGN KEY (tenant_id, project_id, department_id)
        REFERENCES departments (tenant_id, project_id, id),
    CONSTRAINT company_positions_manager_fk FOREIGN KEY (tenant_id, project_id, reports_to_position_id)
        REFERENCES company_positions (tenant_id, project_id, id),
    CONSTRAINT company_positions_agent_fk FOREIGN KEY (tenant_id, project_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, project_id, id),
    CHECK (reports_to_position_id IS DISTINCT FROM id),
    CHECK (NOT is_department_head OR department_id IS NOT NULL),
    CHECK (
        (occupant_kind = 'vacant' AND occupant_name IS NULL AND user_id IS NULL AND ai_profile_id IS NULL)
        OR (occupant_kind = 'human' AND ai_profile_id IS NULL AND (
            (user_id IS NULL AND occupant_name IS NOT NULL)
            OR (user_id IS NOT NULL AND occupant_name IS NULL)
        ))
        OR (occupant_kind = 'agent' AND occupant_name IS NULL AND user_id IS NULL AND ai_profile_id IS NOT NULL)
    )
);

CREATE UNIQUE INDEX company_positions_department_head_unique_idx
    ON company_positions (tenant_id, project_id, department_id) WHERE is_department_head;
CREATE INDEX company_positions_manager_idx
    ON company_positions (tenant_id, project_id, reports_to_position_id);
CREATE INDEX company_positions_agent_idx ON company_positions (ai_profile_id) WHERE ai_profile_id IS NOT NULL;
CREATE INDEX company_positions_job_position_idx ON company_positions (job_position_id) WHERE job_position_id IS NOT NULL;
CREATE INDEX company_positions_user_idx ON company_positions (user_id) WHERE user_id IS NOT NULL;
