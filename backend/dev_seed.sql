-- Idempotent local demo data. The token below is intentionally development-only.
BEGIN;

INSERT INTO tenants (id, name)
VALUES ('11111111-1111-1111-1111-111111111111', 'Tzomet Local')
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = now();

INSERT INTO projects (id, tenant_id, name, slug, status)
VALUES (
    '22222222-2222-2222-2222-222222222222',
    '11111111-1111-1111-1111-111111111111',
    'Local support',
    'local-support',
    'active'
)
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, status = EXCLUDED.status, updated_at = now();

INSERT INTO users (id, email, display_name, status, password_hash)
VALUES (
    '33333333-3333-3333-3333-333333333333',
    'admin@tzomet.local',
    'Local Administrator',
    'active',
    '$argon2id$v=19$m=65536,t=4,p=1$VlFQbFhQdTdXclRUcnRvUA$1/DI8gntCfvU5r3yAYUC/1bK2qldIX0UEHCxsnSEDaU'
)
ON CONFLICT (id) DO UPDATE
SET email = EXCLUDED.email,
    display_name = EXCLUDED.display_name,
    status = EXCLUDED.status,
    password_hash = EXCLUDED.password_hash,
    updated_at = now();

INSERT INTO memberships (id, tenant_id, project_id, user_id, role, permissions)
VALUES (
    'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    '11111111-1111-1111-1111-111111111111',
    '22222222-2222-2222-2222-222222222222',
    '33333333-3333-3333-3333-333333333333',
    'admin',
    ARRAY[
        'conversations:read', 'conversations:reply', 'conversations:close',
        'contacts:read', 'channels:read', 'channels:manage',
        'access_tokens:manage', 'visitor_network:read'
    ]
)
ON CONFLICT (tenant_id, project_id, user_id) DO UPDATE
SET role = EXCLUDED.role, permissions = EXCLUDED.permissions, updated_at = now();

INSERT INTO inboxes (id, tenant_id, project_id, name, status)
VALUES (
    '44444444-4444-4444-4444-444444444444',
    '11111111-1111-1111-1111-111111111111',
    '22222222-2222-2222-2222-222222222222',
    'Customer support',
    'active'
)
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, status = EXCLUDED.status, updated_at = now();

INSERT INTO channel_connections (
    id, tenant_id, project_id, inbox_id, public_id, kind, name, status
)
VALUES (
    '55555555-5555-5555-5555-555555555555',
    '11111111-1111-1111-1111-111111111111',
    '22222222-2222-2222-2222-222222222222',
    '44444444-4444-4444-4444-444444444444',
    '66666666-6666-6666-6666-666666666666',
    'widget',
    'Website widget',
    'active'
)
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, status = EXCLUDED.status, updated_at = now();

INSERT INTO widget_configs (
    channel_connection_id, tenant_id, allowed_origins, greeting, theme,
    default_language, translations, notify_on_new_visitor
)
VALUES (
    '55555555-5555-5555-5555-555555555555',
    '11111111-1111-1111-1111-111111111111',
    ARRAY[
        'http://localhost:15174',
        'http://127.0.0.1:15174'
    ],
    'Hello! How can we help?',
    '{"accent_color":"#F05A28","accent_text_color":"#FFFFFF","surface_color":"#FFFFFF","text_color":"#1B1B1D","border_radius":20}'::jsonb,
    'en',
    '{"en":{"greeting":"Hello! How can we help?","launcher_label":"Chat with us","rating_prompt":"Thanks for chatting with us. Please rate the support you received.","rating_thanks":"Thank you! Your feedback helps us improve."},"ru":{"greeting":"Здравствуйте! Чем мы можем помочь?","launcher_label":"Напишите нам","rating_prompt":"Спасибо за обращение! Оцените, пожалуйста, качество поддержки.","rating_thanks":"Спасибо! Ваш отзыв поможет нам стать лучше."}}'::jsonb,
    true
)
ON CONFLICT (channel_connection_id) DO UPDATE
SET allowed_origins = EXCLUDED.allowed_origins,
    greeting = EXCLUDED.greeting,
    theme = EXCLUDED.theme,
    default_language = EXCLUDED.default_language,
    translations = EXCLUDED.translations,
    notify_on_new_visitor = EXCLUDED.notify_on_new_visitor,
    updated_at = now();

INSERT INTO api_keys (
    id, tenant_id, project_id, actor_user_id, name, token_hash,
    role, permissions, inbox_scope, expires_at
)
VALUES (
    '77777777-7777-7777-7777-777777777777',
    '11111111-1111-1111-1111-111111111111',
    '22222222-2222-2222-2222-222222222222',
    '33333333-3333-3333-3333-333333333333',
    'Local operator',
    decode('6198c57a957da1807f8f6e8257770eb2b76eb895579229a38c810d1f5e323022', 'hex'),
    'admin',
    ARRAY[
        'conversations:read', 'conversations:reply', 'conversations:close',
        'contacts:read', 'channels:read', 'channels:manage'
    ],
    ARRAY['44444444-4444-4444-4444-444444444444'::uuid],
    now() + INTERVAL '365 days'
)
ON CONFLICT (id) DO UPDATE
SET token_hash = EXCLUDED.token_hash,
    role = EXCLUDED.role,
    permissions = EXCLUDED.permissions,
    inbox_scope = EXCLUDED.inbox_scope,
    expires_at = EXCLUDED.expires_at,
    revoked_at = NULL;

COMMIT;
