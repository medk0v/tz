-- Descriptive working roles never modify memberships, permission roles, or inbox teams.
CREATE TABLE company_governance (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    content jsonb NOT NULL CHECK (jsonb_typeof(content) = 'object'),
    updated_by uuid NOT NULL REFERENCES users (id),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);
