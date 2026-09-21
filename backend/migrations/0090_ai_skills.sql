CREATE TABLE ai_skills (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    instructions text NOT NULL CHECK (length(trim(instructions)) BETWEEN 1 AND 50000),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    FOREIGN KEY (tenant_id, project_id)
        REFERENCES projects (tenant_id, id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX ai_skills_project_name_unique_idx
    ON ai_skills (tenant_id, project_id, lower(name));

-- A shared profile can use different skills in each project where it runs.
CREATE TABLE ai_profile_skills (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    skill_id uuid NOT NULL,
    ai_profile_id uuid NOT NULL,
    PRIMARY KEY (tenant_id, project_id, skill_id, ai_profile_id),
    FOREIGN KEY (tenant_id, project_id, skill_id)
        REFERENCES ai_skills (tenant_id, project_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE
);

CREATE INDEX ai_profile_skills_profile_idx
    ON ai_profile_skills (tenant_id, project_id, ai_profile_id, skill_id);
