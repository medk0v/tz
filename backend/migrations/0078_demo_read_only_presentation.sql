-- Preserve the restricted demo role and tenant. UI permissions are exposed by
-- the server, while its demo route policy continues to deny configuration writes.
UPDATE users
SET display_name = 'DemoAccount', updated_at = now()
WHERE id = 'eee36b29-db1e-41a6-bf9d-fc185aad1cad'
  AND is_demo
  AND lower(email) = 'demo@tzomet.ai';

UPDATE inboxes
SET name = 'DemoAccount', updated_at = now()
WHERE id = 'de678595-1e2e-4574-826e-a29db5b8157e'
  AND tenant_id = 'c11ebede-f63c-4315-ab9f-b7c42e5515b6'
  AND project_id = '17690801-2854-4974-a377-284bfd23990d';

UPDATE operator_chat_profiles
SET display_name = 'DemoAccount', updated_at = now()
WHERE user_id = 'eee36b29-db1e-41a6-bf9d-fc185aad1cad'
  AND tenant_id = 'c11ebede-f63c-4315-ab9f-b7c42e5515b6'
  AND project_id = '17690801-2854-4974-a377-284bfd23990d';
