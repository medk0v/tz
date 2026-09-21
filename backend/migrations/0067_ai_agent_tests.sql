CREATE TABLE ai_test_scenarios (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    profile_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1,
    scenario jsonb NOT NULL,
    deleted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, profile_id, id),
    FOREIGN KEY (tenant_id, project_id, profile_id)
      REFERENCES ai_profiles (tenant_id, project_id, id) ON DELETE CASCADE
);
CREATE TABLE ai_test_scenario_versions (
    scenario_id uuid NOT NULL REFERENCES ai_test_scenarios(id) ON DELETE CASCADE,
    revision bigint NOT NULL,
    scenario jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (scenario_id, revision)
);
CREATE TABLE ai_test_runs (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    profile_id uuid NOT NULL,
    scenario_id uuid NOT NULL,
    scenario_revision bigint NOT NULL,
    mode text NOT NULL CHECK (mode IN ('fixtures', 'live_read_only')),
    status text NOT NULL CHECK (status IN ('running','passed','behavior_error','execution_error','manual_review')),
    fingerprint text NOT NULL,
    snapshot jsonb NOT NULL,
    result jsonb NOT NULL DEFAULT '{}',
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    FOREIGN KEY (tenant_id, project_id, profile_id, scenario_id)
      REFERENCES ai_test_scenarios (tenant_id, project_id, profile_id, id) ON DELETE CASCADE
);
CREATE INDEX ai_test_runs_profile_idx ON ai_test_runs(tenant_id, project_id, profile_id, created_at DESC);
CREATE UNIQUE INDEX ai_test_runs_one_active_idx ON ai_test_runs(profile_id) WHERE status = 'running';
