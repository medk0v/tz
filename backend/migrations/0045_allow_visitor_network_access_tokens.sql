-- Preserve visitor_network:read for existing tokens whose role grants it.
-- New tokens and role updates are handled by access_token_permissions_from().
UPDATE api_keys AS api_key
SET permissions = api_key.permissions || ARRAY['visitor_network:read']::text[]
FROM project_roles AS role
WHERE role.tenant_id = api_key.tenant_id
  AND role.project_id = api_key.project_id
  AND role.id = api_key.role
  AND role.permissions @> ARRAY['visitor_network:read']::text[]
  AND NOT api_key.permissions @> ARRAY['visitor_network:read']::text[];
