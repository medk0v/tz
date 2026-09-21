-- A password session can keep multiple project windows open independently.
CREATE TABLE operator_session_projects (
    session_id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    membership_id uuid NOT NULL,
    department_id uuid,
    director_mode boolean NOT NULL DEFAULT false,
    presence_last_seen_at timestamptz,
    PRIMARY KEY (session_id, project_id),
    FOREIGN KEY (tenant_id, session_id) REFERENCES operator_sessions (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, membership_id) REFERENCES memberships (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id, department_id) REFERENCES departments (tenant_id, project_id, id),
    CHECK (NOT director_mode OR department_id IS NULL)
);

CREATE INDEX operator_session_projects_presence_idx
    ON operator_session_projects (tenant_id, project_id, presence_last_seen_at)
    WHERE presence_last_seen_at IS NOT NULL;

INSERT INTO operator_session_projects (
    session_id, tenant_id, project_id, membership_id, department_id,
    director_mode, presence_last_seen_at
)
SELECT id, tenant_id, project_id, membership_id, department_id,
       director_mode, presence_last_seen_at
FROM operator_sessions
WHERE project_id IS NOT NULL AND revoked_at IS NULL;
