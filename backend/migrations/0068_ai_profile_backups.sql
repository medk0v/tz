CREATE TABLE ai_profile_backups (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    ai_profile_id uuid NOT NULL,
    snapshot jsonb NOT NULL CHECK (jsonb_typeof(snapshot) = 'object'),
    reason text NOT NULL CHECK (reason IN ('manual', 'before_restore')),
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE
);

CREATE INDEX ai_profile_backups_profile_created_idx
    ON ai_profile_backups (tenant_id, project_id, ai_profile_id, created_at DESC, id DESC);
