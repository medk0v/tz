DROP TABLE IF EXISTS ai_reply_jobs;
DROP TABLE IF EXISTS agent_tool_calls;
DROP TABLE IF EXISTS agent_resource_grants;
DROP TABLE IF EXISTS agent_runs;

ALTER TABLE ai_profiles
    DROP COLUMN IF EXISTS allowed_tools;
