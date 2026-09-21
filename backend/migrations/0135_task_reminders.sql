-- An "on time" reminder of a task: how long before it opens the people working
-- on it are told. The offset counts back from the span's start, or from the
-- deadline when the task has no start. NULL is off.
ALTER TABLE ai_tasks
    ADD COLUMN reminder_minutes integer
        CHECK (reminder_minutes IS NULL OR reminder_minutes BETWEEN 0 AND 40320),
    -- The moment already delivered, not a flag: rescheduling the task moves the
    -- moment, which differs from this one, and the reminder arms itself again.
    ADD COLUMN reminder_sent_at timestamptz;

-- One delivered reminder per task, per recipient, per moment. The row keeps no
-- copy of the task: a task has no title column -- it is the first line of its
-- text -- and its span moves, so the bell reads both through the join that also
-- re-checks whether the reader may still see the task at all.
CREATE TABLE task_reminders (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    task_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users (id),
    -- Both what the bell shows and the key that makes delivery idempotent: it is
    -- a pure function of the task's current span and offset, so a second pass
    -- over the same task computes the same key and conflicts away.
    fires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    read_at timestamptz,
    UNIQUE (tenant_id, project_id, id),
    UNIQUE (tenant_id, task_id, user_id, fires_at),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id, task_id)
        REFERENCES ai_tasks (tenant_id, project_id, id) ON DELETE CASCADE
);

-- The bell: one person's unread reminders, newest first.
CREATE INDEX task_reminders_unread_idx
    ON task_reminders (tenant_id, user_id, fires_at DESC, id)
    WHERE read_at IS NULL;

-- The materialiser scans only tasks that actually want a reminder.
CREATE INDEX ai_tasks_reminder_idx
    ON ai_tasks (tenant_id, id)
    WHERE reminder_minutes IS NOT NULL AND completed_at IS NULL;
