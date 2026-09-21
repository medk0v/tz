-- Give every access token its own technical user so token authentication can
-- use the same assignment, participant, message, and presence model as a
-- password-authenticated operator without impersonating the token creator.
INSERT INTO users (id, email, display_name, status)
SELECT md5('tzomet-access-token:' || api_key.id::text)::uuid,
       'access-token-' || md5('tzomet-access-token:' || api_key.id::text) || '@internal.tzomet',
       api_key.name,
       'active'
FROM api_keys AS api_key
ON CONFLICT (id) DO NOTHING;

INSERT INTO memberships (id, tenant_id, project_id, user_id, role, permissions)
SELECT md5('tzomet-access-membership:' || api_key.id::text)::uuid,
       api_key.tenant_id,
       api_key.project_id,
       md5('tzomet-access-token:' || api_key.id::text)::uuid,
       api_key.role,
       api_key.permissions
FROM api_keys AS api_key
ON CONFLICT (tenant_id, project_id, user_id) DO NOTHING;

UPDATE api_keys AS api_key
SET actor_user_id = md5('tzomet-access-token:' || api_key.id::text)::uuid
WHERE api_key.actor_user_id IS DISTINCT FROM
      md5('tzomet-access-token:' || api_key.id::text)::uuid;
