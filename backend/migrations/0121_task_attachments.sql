-- Private screenshots uploaded while composing a task, then attached on save.
CREATE TABLE ai_task_attachments (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    task_id uuid,
    uploaded_by uuid NOT NULL,
    file_name text NOT NULL,
    content_type text NOT NULL CHECK (content_type IN ('image/png', 'image/jpeg', 'image/webp')),
    byte_size bigint NOT NULL CHECK (byte_size BETWEEN 1 AND 10485760),
    checksum_sha256 text NOT NULL,
    position integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, project_id, task_id) REFERENCES ai_tasks (tenant_id, project_id, id) ON DELETE CASCADE
);
CREATE INDEX ai_task_attachments_task_idx ON ai_task_attachments (tenant_id, task_id, position, id);
CREATE INDEX ai_task_attachments_drafts_idx ON ai_task_attachments (tenant_id, project_id, uploaded_by, created_at) WHERE task_id IS NULL;
