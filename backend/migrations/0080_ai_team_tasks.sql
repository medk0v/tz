ALTER TABLE ai_tasks
    ADD COLUMN execution_mode text NOT NULL DEFAULT 'independent'
        CHECK (execution_mode IN ('independent', 'team')),
    ADD COLUMN coordinator_id uuid,
    ADD COLUMN expected_result text NOT NULL DEFAULT '' CHECK (length(expected_result) <= 5000),
    ADD COLUMN agent_roles jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(agent_roles) = 'array'),
    ADD CONSTRAINT ai_tasks_coordinator_fk FOREIGN KEY (tenant_id, coordinator_id)
        REFERENCES ai_profiles (tenant_id, id),
    ADD CONSTRAINT ai_tasks_coordinator_mode CHECK (
        (execution_mode = 'independent' AND coordinator_id IS NULL)
        OR (execution_mode = 'team' AND coordinator_id IS NOT NULL)
    ),
    ADD CONSTRAINT ai_tasks_project_identity UNIQUE (tenant_id, project_id, id);

-- Preserve existing schedule validation and add an explicit manual schedule.
ALTER TABLE ai_tasks DROP CONSTRAINT ai_tasks_schedule_check;
ALTER TABLE ai_tasks ADD CONSTRAINT ai_tasks_schedule_check CHECK (
    jsonb_typeof(schedule) = 'object'
    AND COALESCE(schedule->>'kind' IN ('manual', 'once', 'daily', 'weekly', 'monthly', 'yearly'), false)
);

CREATE TABLE ai_task_executions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    scheduled_for timestamptz,
    idempotency_key uuid,
    status text NOT NULL DEFAULT 'queued' CHECK (status IN
        ('queued','planning','running','reviewing','succeeded','needs_attention','failed','cancelled')),
    snapshot jsonb NOT NULL CHECK (jsonb_typeof(snapshot) = 'object'),
    result_text text CHECK (length(result_text) <= 50000),
    error text CHECK (length(error) <= 2000),
    rework_round integer NOT NULL DEFAULT 0 CHECK (rework_round BETWEEN 0 AND 1),
    deadline_at timestamptz NOT NULL DEFAULT now() + interval '1 hour',
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, id),
    UNIQUE (tenant_id, task_id, scheduled_for),
    UNIQUE (tenant_id, task_id, idempotency_key),
    FOREIGN KEY (tenant_id, project_id, task_id)
        REFERENCES ai_tasks (tenant_id, project_id, id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX ai_task_executions_active_task ON ai_task_executions (tenant_id, task_id)
    WHERE status IN ('queued','planning','running','reviewing','needs_attention');
CREATE INDEX ai_task_executions_history ON ai_task_executions (tenant_id, task_id, created_at DESC);

CREATE TABLE ai_task_execution_steps (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    step_key text NOT NULL CHECK (length(step_key) BETWEEN 1 AND 80),
    title text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    agent_id uuid NOT NULL,
    agent_name text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('planning','work','review')),
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','running','succeeded','failed','cancelled','blocked')),
    input_text text NOT NULL DEFAULT '' CHECK (length(input_text) <= 10000),
    result_text text CHECK (length(result_text) <= 50000),
    result jsonb,
    error text CHECK (length(error) <= 2000),
    may_have_effects boolean NOT NULL DEFAULT false,
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
    revision integer NOT NULL DEFAULT 0 CHECK (revision BETWEEN 0 AND 1),
    available_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    UNIQUE (tenant_id, project_id, execution_id, id),
    UNIQUE (execution_id, step_key),
    FOREIGN KEY (tenant_id, project_id, execution_id)
        REFERENCES ai_task_executions (tenant_id, project_id, id) ON DELETE CASCADE
);

-- Agent identity is snapshotted, so deleting a profile cannot erase run history.
CREATE TABLE ai_task_step_dependencies (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    step_id uuid NOT NULL,
    depends_on uuid NOT NULL,
    PRIMARY KEY (step_id, depends_on),
    CHECK (step_id <> depends_on),
    FOREIGN KEY (tenant_id, project_id, execution_id, step_id)
        REFERENCES ai_task_execution_steps (tenant_id, project_id, execution_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, execution_id, depends_on)
        REFERENCES ai_task_execution_steps (tenant_id, project_id, execution_id, id) ON DELETE CASCADE
);

CREATE TABLE ai_task_step_attempts (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    step_id uuid NOT NULL,
    attempt_number integer NOT NULL CHECK (attempt_number BETWEEN 1 AND 3),
    status text NOT NULL DEFAULT 'running'
        CHECK (status IN ('running','succeeded','failed','cancelled','unknown')),
    locked_by uuid NOT NULL,
    deadline_at timestamptz NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    error_code text,
    provider_kind text,
    model text,
    usage jsonb,
    UNIQUE (step_id, attempt_number),
    FOREIGN KEY (tenant_id, project_id, execution_id, step_id)
        REFERENCES ai_task_execution_steps (tenant_id, project_id, execution_id, id) ON DELETE CASCADE
);
CREATE INDEX ai_task_step_attempts_active ON ai_task_step_attempts (deadline_at) WHERE status = 'running';
CREATE INDEX ai_task_steps_queue ON ai_task_execution_steps (available_at, created_at) WHERE status = 'pending';

CREATE TABLE ai_task_execution_events (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    event_type text NOT NULL,
    message text NOT NULL CHECK (length(message) <= 2000),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id, execution_id)
        REFERENCES ai_task_executions (tenant_id, project_id, id) ON DELETE CASCADE
);

CREATE TABLE ai_task_manual_triggers (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    idempotency_key uuid NOT NULL,
    scheduled_for timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, task_id, idempotency_key),
    FOREIGN KEY (tenant_id, project_id, task_id)
        REFERENCES ai_tasks (tenant_id, project_id, id) ON DELETE CASCADE
);
