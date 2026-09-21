ALTER TABLE ai_tasks
    ADD COLUMN next_run_at timestamptz;

CREATE INDEX ai_tasks_due_idx
    ON ai_tasks (next_run_at, id)
    WHERE next_run_at IS NOT NULL;

CREATE TABLE ai_task_agents (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    task_id uuid NOT NULL,
    ai_profile_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, task_id, ai_profile_id),
    FOREIGN KEY (tenant_id, task_id)
        REFERENCES ai_tasks (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id)
);

CREATE INDEX ai_task_agents_profile_idx
    ON ai_task_agents (tenant_id, ai_profile_id, task_id);

CREATE SEQUENCE ai_task_project_scheduler_sequence;

CREATE TABLE ai_task_project_scheduler_state (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    last_materialized_sequence bigint CHECK (last_materialized_sequence > 0),
    last_claim_sequence bigint CHECK (last_claim_sequence > 0),
    PRIMARY KEY (tenant_id, project_id),
    FOREIGN KEY (tenant_id, project_id)
        REFERENCES projects (tenant_id, id) ON DELETE CASCADE,
    CHECK (last_materialized_sequence IS NOT NULL OR last_claim_sequence IS NOT NULL)
);

CREATE TABLE ai_task_runs (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    ai_profile_id uuid REFERENCES ai_profiles (id) ON DELETE SET NULL,
    agent_name text NOT NULL CHECK (length(trim(agent_name)) BETWEEN 1 AND 200),
    task_text text NOT NULL CHECK (length(trim(task_text)) BETWEEN 1 AND 50000),
    scheduled_for timestamptz NOT NULL,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'cancelled')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at timestamptz NOT NULL DEFAULT now(),
    locked_at timestamptz,
    locked_by uuid,
    output text CHECK (output IS NULL OR length(output) <= 10000),
    error text CHECK (error IS NULL OR length(error) <= 2000),
    started_at timestamptz,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, task_id, ai_profile_id, scheduled_for),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, task_id)
        REFERENCES ai_tasks (tenant_id, id) ON DELETE CASCADE,
    CHECK (
        (status = 'processing' AND locked_at IS NOT NULL AND locked_by IS NOT NULL)
        OR (status <> 'processing' AND locked_at IS NULL AND locked_by IS NULL)
    )
);

CREATE INDEX ai_task_runs_claim_idx
    ON ai_task_runs (available_at, scheduled_for, id)
    WHERE status IN ('pending', 'processing');

CREATE INDEX ai_task_runs_stale_idx
    ON ai_task_runs (locked_at, id)
    WHERE status = 'processing';

CREATE INDEX ai_task_runs_processing_project_idx
    ON ai_task_runs (tenant_id, project_id, id)
    WHERE status = 'processing';

CREATE INDEX ai_task_runs_history_idx
    ON ai_task_runs (tenant_id, task_id, scheduled_for DESC, id DESC);
