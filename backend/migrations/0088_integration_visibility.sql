-- Integration ownership (including token encryption scope) remains stable.
ALTER TABLE api_integrations ADD COLUMN visibility jsonb NOT NULL
    DEFAULT '{"project_ids":[],"department_ids":[]}';

-- An integration can be granted to a visible agent owned by another project.
DO $$
DECLARE profile_fk text;
BEGIN
    SELECT conname INTO STRICT profile_fk FROM pg_constraint
    WHERE conrelid = 'ai_profile_api_integrations'::regclass
      AND confrelid = 'ai_profiles'::regclass AND contype = 'f';
    EXECUTE format('ALTER TABLE ai_profile_api_integrations DROP CONSTRAINT %I', profile_fk);
END $$;
ALTER TABLE ai_profile_api_integrations ADD CONSTRAINT api_integration_profile_fk
    FOREIGN KEY (tenant_id, ai_profile_id) REFERENCES ai_profiles (tenant_id, id)
    ON DELETE CASCADE;
