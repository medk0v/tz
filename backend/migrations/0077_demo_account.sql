-- Public demo resources live in a separate tenant. The only stored credential
-- is an Argon2id hash; the deliberately public password is in demo.rs.
ALTER TABLE users ADD COLUMN is_demo boolean NOT NULL DEFAULT false;
ALTER TABLE operator_sessions ADD COLUMN login_ip inet;

CREATE TABLE demo_login_attempts (
    client_ip inet PRIMARY KEY,
    attempts integer NOT NULL CHECK (attempts BETWEEN 1 AND 1000000),
    window_started_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX demo_login_attempts_expiry_idx ON demo_login_attempts (window_started_at);

-- SQLx applies this migration exactly once, inside a transaction. Any reserved
-- UUID or email collision aborts provisioning instead of adopting existing data.
INSERT INTO tenants (id, name)
VALUES ('c11ebede-f63c-4315-ab9f-b7c42e5515b6', 'Tzomet Demo');

INSERT INTO projects (id, tenant_id, name, slug)
VALUES (
    '17690801-2854-4974-a377-284bfd23990d',
    'c11ebede-f63c-4315-ab9f-b7c42e5515b6', 'Tzomet Demo', 'demo'
);

INSERT INTO users (id, email, display_name, password_hash, is_demo)
VALUES (
    'eee36b29-db1e-41a6-bf9d-fc185aad1cad', 'demo@tzomet.ai', 'Demo',
    '$argon2id$v=19$m=19456,t=2,p=1$OjJMnPP9EGHH35IgyUxb7g$5n4IHCORB6mRRSN7qy1b2mPApm3ZsZ34m7dqi6SLh0U',
    true
);

INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
VALUES (
    'c11ebede-f63c-4315-ab9f-b7c42e5515b6', '17690801-2854-4974-a377-284bfd23990d',
    'demo', 'Demo', 'operator',
    ARRAY['conversations:read', 'conversations:reply']
);

INSERT INTO memberships (id, tenant_id, project_id, user_id, role, permissions)
VALUES (
    '409c71a1-7212-4fea-a220-d7fcb28ae20c',
    'c11ebede-f63c-4315-ab9f-b7c42e5515b6', '17690801-2854-4974-a377-284bfd23990d',
    'eee36b29-db1e-41a6-bf9d-fc185aad1cad', 'demo',
    ARRAY['conversations:read', 'conversations:reply']
);

INSERT INTO inboxes (id, tenant_id, project_id, name)
VALUES (
    'de678595-1e2e-4574-826e-a29db5b8157e',
    'c11ebede-f63c-4315-ab9f-b7c42e5515b6', '17690801-2854-4974-a377-284bfd23990d',
    'Demo Widget'
);

INSERT INTO channel_connections (
    id, tenant_id, project_id, inbox_id, public_id, kind, name, status
)
VALUES (
    '9d299e1f-b99f-41da-9330-863d94385058',
    'c11ebede-f63c-4315-ab9f-b7c42e5515b6', '17690801-2854-4974-a377-284bfd23990d',
    'de678595-1e2e-4574-826e-a29db5b8157e', '66f4db7a-c798-44ce-947c-a9fdac351d55',
    'widget', 'Tzomet Widget Example', 'active'
);

INSERT INTO widget_configs (channel_connection_id, tenant_id, allowed_origins, attachments_enabled)
VALUES (
    '9d299e1f-b99f-41da-9330-863d94385058', 'c11ebede-f63c-4315-ab9f-b7c42e5515b6',
    ARRAY['https://demo.tzomet.io'], false
);

CREATE INDEX widget_sessions_demo_ip_idx
    ON widget_sessions (tenant_id, channel_connection_id, client_ip, contact_id)
    WHERE channel_connection_id = '9d299e1f-b99f-41da-9330-863d94385058';
