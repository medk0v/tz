-- Existing sessions keep the standard lifetime; only an explicit password login
-- with remember_me opts into the longer, separately configured lifetime.
ALTER TABLE operator_sessions
    ADD COLUMN remember_me boolean NOT NULL DEFAULT false;
