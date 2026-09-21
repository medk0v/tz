CREATE TABLE ai_profile_channel_connections (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    ai_profile_id uuid NOT NULL,
    channel_connection_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, ai_profile_id, channel_connection_id),
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id) ON DELETE CASCADE
);

CREATE INDEX ai_profile_channel_connections_channel_idx
    ON ai_profile_channel_connections (tenant_id, channel_connection_id, created_at, ai_profile_id);

CREATE INDEX conversation_participants_active_ai_profile_idx
    ON conversation_participants (tenant_id, ai_profile_id, conversation_id)
    WHERE participant_kind = 'ai' AND left_at IS NULL;

INSERT INTO ai_profile_channel_connections (
    tenant_id, ai_profile_id, channel_connection_id, created_at
)
SELECT assignment.tenant_id, assignment.ai_profile_id, connection.id, assignment.created_at
FROM inbox_ai_profiles AS assignment
JOIN ai_profiles AS profile
  ON profile.tenant_id = assignment.tenant_id
 AND profile.id = assignment.ai_profile_id
JOIN channel_connections AS connection
  ON connection.tenant_id = assignment.tenant_id
 AND connection.project_id = profile.project_id
 AND connection.inbox_id = assignment.inbox_id
 AND connection.deleted_at IS NULL
ON CONFLICT DO NOTHING;

CREATE TABLE ai_profile_public_identities (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    ai_profile_id uuid NOT NULL,
    language text NOT NULL CHECK (
        length(language) BETWEEN 2 AND 35
        AND language = lower(language)
        AND language = btrim(language)
    ),
    display_name text NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, ai_profile_id, language),
    FOREIGN KEY (tenant_id, ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id) ON DELETE CASCADE
);

CREATE TABLE ai_profile_public_identity_avatars (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    ai_profile_id uuid NOT NULL,
    language text NOT NULL,
    public_id uuid NOT NULL UNIQUE,
    media_type text NOT NULL CHECK (media_type IN ('image/jpeg', 'image/png', 'image/webp')),
    content bytea NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 2097152),
    sha256 bytea NOT NULL CHECK (octet_length(sha256) = 32),
    width integer NOT NULL CHECK (width BETWEEN 1 AND 4096),
    height integer NOT NULL CHECK (height BETWEEN 1 AND 4096),
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, ai_profile_id, language),
    FOREIGN KEY (tenant_id, ai_profile_id, language)
        REFERENCES ai_profile_public_identities (tenant_id, ai_profile_id, language)
        ON DELETE CASCADE
);

INSERT INTO ai_profile_public_identities (
    tenant_id, ai_profile_id, language, display_name
)
SELECT profile.tenant_id, profile.id, lower(btrim(language.value)), profile.name
FROM ai_profiles AS profile
CROSS JOIN LATERAL unnest(string_to_array(profile.language, ',')) AS language(value)
WHERE btrim(language.value) <> ''
ON CONFLICT DO NOTHING;

INSERT INTO ai_profile_public_identity_avatars (
    tenant_id, ai_profile_id, language, public_id, media_type, content,
    sha256, width, height, created_by, created_at, updated_at
)
SELECT identity.tenant_id, identity.ai_profile_id, identity.language,
       md5(avatar.public_id::text || ':' || identity.language)::uuid,
       avatar.media_type, avatar.content, avatar.sha256, avatar.width, avatar.height,
       avatar.created_by, avatar.created_at, avatar.updated_at
FROM ai_profile_public_identities AS identity
JOIN ai_profile_avatars AS avatar
  ON avatar.tenant_id = identity.tenant_id
 AND avatar.ai_profile_id = identity.ai_profile_id
ON CONFLICT DO NOTHING;

WITH left_participants AS (
    UPDATE conversation_participants AS participant
    SET left_at = now()
    FROM conversations AS conversation
    WHERE participant.participant_kind = 'ai'
      AND participant.left_at IS NULL
      AND conversation.tenant_id = participant.tenant_id
      AND conversation.id = participant.conversation_id
      AND (
          participant.ai_profile_id IS NULL
          OR NOT EXISTS (
              SELECT 1
              FROM ai_profile_channel_connections AS assignment
              JOIN channel_connections AS connection
                ON connection.tenant_id = assignment.tenant_id
               AND connection.id = assignment.channel_connection_id
              JOIN inboxes AS inbox
                ON inbox.tenant_id = connection.tenant_id
               AND inbox.project_id = connection.project_id
               AND inbox.id = connection.inbox_id
              JOIN ai_profiles AS profile
                ON profile.tenant_id = assignment.tenant_id
               AND profile.id = assignment.ai_profile_id
               AND profile.project_id = connection.project_id
              JOIN ai_provider_connections AS provider
                ON provider.tenant_id = profile.tenant_id
               AND provider.project_id = profile.project_id
               AND provider.id = profile.provider_connection_id
              WHERE assignment.tenant_id = participant.tenant_id
                AND assignment.ai_profile_id = participant.ai_profile_id
                AND assignment.channel_connection_id = conversation.channel_connection_id
                AND connection.inbox_id = conversation.inbox_id
                AND connection.status = 'active'
                AND connection.deleted_at IS NULL
                AND inbox.status = 'active'
                AND profile.status = 'active'
                AND provider.status = 'active'
                AND provider.provider_kind IN ('openai', 'openai_compatible')
                AND conversation.widget_language IS NOT NULL
                AND EXISTS (
                    SELECT 1
                    FROM unnest(string_to_array(profile.language, ','))
                        AS configured_language(value)
                    WHERE lower(btrim(configured_language.value))
                            = lower(replace(conversation.widget_language, '_', '-'))
                       OR (
                           split_part(
                               lower(btrim(configured_language.value)), '-', 1
                           ) = split_part(
                               lower(replace(conversation.widget_language, '_', '-')), '-', 1
                           )
                           AND (
                               position('-' IN btrim(configured_language.value)) = 0
                               OR position(
                                   '-' IN lower(replace(
                                       conversation.widget_language, '_', '-'
                                   ))
                               ) = 0
                           )
                       )
                )
          )
      )
    RETURNING participant.tenant_id, participant.conversation_id
)
UPDATE outbox_events AS event
SET status = 'completed', completed_at = now(),
    locked_at = NULL, locked_by = NULL,
    last_error = 'cancelled while migrating exact AI channel access'
WHERE event.aggregate_type = 'provider_reply'
  AND event.status IN ('pending', 'processing')
  AND EXISTS (
      SELECT 1
      FROM left_participants AS participant
      WHERE participant.tenant_id = event.tenant_id
        AND participant.conversation_id = event.aggregate_id
  );
