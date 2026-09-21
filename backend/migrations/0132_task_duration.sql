-- A task's deadline becomes a span: due_at keeps its meaning as the moment work
-- must be finished, starts_at is when it may begin. A task with no start behaves
-- exactly as before. all_day spans whole days in time_zone, the IANA zone the
-- span was entered in -- a timestamptz stores an instant, not the zone that
-- produced it, so "the whole of 3 March in Istanbul" needs both.
ALTER TABLE ai_tasks
    ADD COLUMN starts_at timestamptz,
    ADD COLUMN all_day boolean NOT NULL DEFAULT false,
    ADD COLUMN time_zone text,
    -- A backstop only: an inverted span is rejected in Rust with a 400, because
    -- an update patches each column on its own and would surface this as a 500.
    ADD CONSTRAINT ai_tasks_duration CHECK (starts_at IS NULL OR due_at IS NULL OR starts_at <= due_at),
    ADD CONSTRAINT ai_tasks_time_zone CHECK (
        time_zone IS NULL OR char_length(btrim(time_zone)) BETWEEN 1 AND 100
    );
