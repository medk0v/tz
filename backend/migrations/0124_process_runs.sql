-- A launched process is an ordinary team task. It keeps an immutable link to the
-- approved version it was launched from and its own editable copy of the steps.
ALTER TABLE ai_tasks
    ADD COLUMN process_id uuid,
    ADD COLUMN process_version integer,
    ADD COLUMN plan_steps jsonb,
    -- Approved versions are immutable and never deleted, so the link needs no action.
    ADD CONSTRAINT ai_tasks_process_version_fk FOREIGN KEY (tenant_id, process_id, process_version)
        REFERENCES process_versions (tenant_id, process_id, version),
    ADD CONSTRAINT ai_tasks_process_marker CHECK ((process_id IS NULL) = (process_version IS NULL)),
    ADD CONSTRAINT ai_tasks_plan_steps CHECK (
        plan_steps IS NULL
        OR (execution_mode = 'team' AND CASE WHEN jsonb_typeof(plan_steps) = 'array'
                THEN jsonb_array_length(plan_steps) BETWEEN 1 AND 30 ELSE false END)
    ),
    ADD CONSTRAINT ai_tasks_process_plan CHECK (process_id IS NULL OR plan_steps IS NOT NULL);

-- Runs of one process, newest first.
CREATE INDEX ai_tasks_process_idx ON ai_tasks (tenant_id, process_id, created_at DESC)
    WHERE process_id IS NOT NULL;
