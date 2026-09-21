ALTER TABLE contacts
    ADD COLUMN is_blocked boolean NOT NULL DEFAULT false;

ALTER TABLE channel_connections
    ADD COLUMN blacklist_reply jsonb NOT NULL DEFAULT '{
        "default_language": "en",
        "translations": {
            "en": "You have been blocked because of spam. Please contact us at the email address listed on our website.",
            "ru": "Вы заблокированы из-за спама. Свяжитесь с нами по электронной почте, указанной на сайте.",
            "uk": "Вас заблоковано через спам. Зв’яжіться з нами за адресою електронної пошти, вказаною на сайті.",
            "ro": "Ați fost blocat din cauza spamului. Contactați-ne la adresa de e-mail indicată pe site.",
            "zh": "您因发送垃圾信息已被封禁。请通过网站上列出的电子邮箱联系我们。",
            "hi": "स्पैम के कारण आपको ब्लॉक कर दिया गया है। कृपया वेबसाइट पर दिए गए ईमेल पते पर हमसे संपर्क करें।"
        }
    }'::jsonb,
    ADD CONSTRAINT channel_connections_blacklist_reply_check CHECK (
        jsonb_typeof(blacklist_reply) = 'object'
        AND jsonb_typeof(blacklist_reply -> 'default_language') = 'string'
        AND jsonb_typeof(blacklist_reply -> 'translations') = 'object'
        AND blacklist_reply -> 'translations' ? (blacklist_reply ->> 'default_language')
    );

ALTER TABLE project_roles
    DROP CONSTRAINT project_roles_permissions_cardinality_check,
    DROP CONSTRAINT project_roles_permissions_allowed_check,
    DROP CONSTRAINT project_roles_admin_permissions_check,
    DROP CONSTRAINT project_roles_non_admin_permissions_check;

-- Add the preset permission while preserving custom role grants.
UPDATE project_roles
SET permissions = permissions || ARRAY['contacts:manage']::text[],
    updated_at = now()
WHERE (id IN ('admin', 'manager', 'operator') OR base_role = 'admin')
  AND permissions @> ARRAY['contacts:read']::text[]
  AND NOT permissions @> ARRAY['contacts:manage']::text[];

ALTER TABLE project_roles
    ADD CONSTRAINT project_roles_permissions_cardinality_check
        CHECK (cardinality(permissions) BETWEEN 0 AND 22),
    ADD CONSTRAINT project_roles_permissions_allowed_check
        CHECK (
            permissions <@ ARRAY[
                'projects:read', 'projects:manage', 'conversations:read',
                'conversations:reply', 'conversations:close', 'contacts:read', 'contacts:manage',
                'channels:read', 'channels:manage', 'access_tokens:manage',
                'visitor_network:read', 'quality:read', 'quality:read_all',
                'reviews:read', 'routing:manage', 'teams:manage',
                'ai:manage', 'knowledge:manage', 'reply_templates:manage',
                'integrations:manage', 'roles:manage', 'system:read'
            ]::text[]
        ),
    ADD CONSTRAINT project_roles_admin_permissions_check
        CHECK (
            base_role <> 'admin'
            OR (
                cardinality(permissions) = 22
                AND permissions @> ARRAY[
                    'projects:read', 'projects:manage', 'conversations:read',
                    'conversations:reply', 'conversations:close', 'contacts:read', 'contacts:manage',
                    'channels:read', 'channels:manage', 'access_tokens:manage',
                    'visitor_network:read', 'quality:read', 'quality:read_all',
                    'reviews:read', 'routing:manage', 'teams:manage',
                    'ai:manage', 'knowledge:manage', 'reply_templates:manage',
                    'integrations:manage', 'roles:manage', 'system:read'
                ]::text[]
            )
        ),
    ADD CONSTRAINT project_roles_non_admin_permissions_check
        CHECK (
            base_role = 'admin'
            OR permissions <@ ARRAY[
                'projects:read', 'conversations:read', 'conversations:reply',
                'conversations:close', 'contacts:read', 'contacts:manage', 'channels:read',
                'channels:manage', 'visitor_network:read', 'quality:read',
                'quality:read_all', 'reviews:read', 'routing:manage',
                'teams:manage', 'ai:manage', 'knowledge:manage',
                'reply_templates:manage', 'integrations:manage', 'system:read'
            ]::text[]
        );

-- Sessions read live project role permissions. Preserve explicit membership and
-- access-token grants; existing tokens do not acquire contact management rights.
