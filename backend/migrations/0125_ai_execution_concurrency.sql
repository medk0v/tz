ALTER TABLE ai_profiles
    ADD COLUMN max_concurrent_runs integer NOT NULL DEFAULT 2
    CHECK (max_concurrent_runs BETWEEN 1 AND 16);

-- Runtime ownership outlives cancellation/deletion of the originating job.
-- Leases are released after cleanup; expiry recovers capacity after a crash.
CREATE TABLE ai_execution_leases (
    id uuid PRIMARY KEY,
    ai_profile_id uuid,
    conversation_id uuid,
    expires_at timestamptz NOT NULL
);
CREATE INDEX ai_execution_leases_profile ON ai_execution_leases (ai_profile_id);
CREATE INDEX ai_execution_leases_expiry ON ai_execution_leases (expires_at);
CREATE UNIQUE INDEX ai_execution_leases_conversation
    ON ai_execution_leases (conversation_id) WHERE conversation_id IS NOT NULL;
