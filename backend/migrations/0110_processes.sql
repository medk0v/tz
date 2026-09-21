-- Process responsibilities describe work; they never grant application access.
CREATE TABLE company_processes (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants(id),
    project_id uuid NOT NULL,
    visibility jsonb NOT NULL DEFAULT '{"project_ids":[],"department_ids":[]}',
    edit_department_ids uuid[] NOT NULL DEFAULT '{}',
    approve_department_ids uuid[] NOT NULL DEFAULT '{}',
    draft jsonb,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    archived boolean NOT NULL DEFAULT false,
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects(tenant_id, id),
    CHECK (draft IS NULL OR jsonb_typeof(draft) = 'object'),
    CHECK (cardinality(edit_department_ids) <= 100 AND array_position(edit_department_ids,NULL) IS NULL),
    CHECK (cardinality(approve_department_ids) <= 100 AND array_position(approve_department_ids,NULL) IS NULL)
);
CREATE INDEX company_processes_tenant_idx ON company_processes(tenant_id, updated_at DESC);

CREATE TABLE process_versions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    process_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    content jsonb NOT NULL CHECK (jsonb_typeof(content) = 'object'),
    approved_by uuid NOT NULL,
    approved_by_name text NOT NULL,
    approved_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, process_id, version),
    FOREIGN KEY (tenant_id, process_id) REFERENCES company_processes(tenant_id, id)
);

CREATE FUNCTION preserve_process_version() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'approved process versions are immutable';
END;
$$;
CREATE TRIGGER process_versions_immutable BEFORE UPDATE OR DELETE ON process_versions
FOR EACH ROW EXECUTE FUNCTION preserve_process_version();
