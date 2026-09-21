-- Access tokens are operator identities, not anonymous API keys. Preserve
-- existing tokens whose issuer was recorded; tokens without a user identity
-- fail closed until an administrator issues a replacement token.
UPDATE api_keys
SET actor_user_id = created_by_user_id
WHERE actor_user_id IS NULL
  AND created_by_user_id IS NOT NULL;

CREATE INDEX api_keys_actor_user_active_idx
    ON api_keys (tenant_id, actor_user_id, project_id)
    WHERE revoked_at IS NULL;
