-- Keep credential ownership stable while sharing model connections across workspaces.
ALTER TABLE ai_provider_connections
    ADD COLUMN visibility jsonb NOT NULL DEFAULT '{"project_ids":[],"department_ids":[]}';
