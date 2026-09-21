-- Keep project history and foreign keys while permanently removing workspace access.
ALTER TABLE projects
    ADD COLUMN deleted_at timestamptz,
    ADD CONSTRAINT projects_deleted_disabled CHECK (deleted_at IS NULL OR status = 'disabled');

ALTER TABLE projects DROP CONSTRAINT projects_tenant_id_slug_key;
CREATE UNIQUE INDEX projects_live_slug_key ON projects (tenant_id, slug) WHERE deleted_at IS NULL;
