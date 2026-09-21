ALTER TABLE widget_configs
    DROP CONSTRAINT widget_configs_launcher_shape_check;

WITH normalized AS (
    SELECT
        channel_connection_id,
        launcher || jsonb_build_object(
            'proactive_invitation_enabled',
            CASE
                WHEN jsonb_typeof(launcher->'proactive_invitation_enabled') = 'boolean'
                    THEN launcher->'proactive_invitation_enabled'
                ELSE 'false'::jsonb
            END,
            'proactive_invitation_delay_seconds',
            CASE
                WHEN jsonb_typeof(launcher->'proactive_invitation_delay_seconds') = 'number'
                    THEN CASE
                        WHEN (launcher->>'proactive_invitation_delay_seconds')::numeric
                                BETWEEN 1 AND 300
                            AND (launcher->>'proactive_invitation_delay_seconds')::numeric
                                = trunc((launcher->>'proactive_invitation_delay_seconds')::numeric)
                            THEN launcher->'proactive_invitation_delay_seconds'
                        ELSE '15'::jsonb
                    END
                ELSE '15'::jsonb
            END
        ) AS launcher
    FROM widget_configs
)
UPDATE widget_configs AS config
SET launcher = normalized.launcher,
    updated_at = now()
FROM normalized
WHERE config.channel_connection_id = normalized.channel_connection_id
  AND config.launcher IS DISTINCT FROM normalized.launcher;

WITH normalized AS (
    SELECT
        config.channel_connection_id,
        jsonb_object_agg(
            entry.key,
            entry.value || jsonb_build_object(
                'proactive_invitation_message',
                CASE
                    WHEN jsonb_typeof(entry.value->'proactive_invitation_message') = 'string'
                        AND char_length(btrim(entry.value->>'proactive_invitation_message'))
                            BETWEEN 1 AND 240
                        THEN to_jsonb(btrim(entry.value->>'proactive_invitation_message'))
                    WHEN split_part(lower(entry.key), '-', 1) = 'ru'
                        THEN to_jsonb('Здравствуйте! Могу помочь?'::text)
                    ELSE to_jsonb('Hi! Can I help you?'::text)
                END
            )
        ) AS translations
    FROM widget_configs AS config
    CROSS JOIN LATERAL jsonb_each(config.translations) AS entry
    GROUP BY config.channel_connection_id
)
UPDATE widget_configs AS config
SET translations = normalized.translations,
    updated_at = now()
FROM normalized
WHERE config.channel_connection_id = normalized.channel_connection_id
  AND config.translations IS DISTINCT FROM normalized.translations;

ALTER TABLE widget_configs
    ALTER COLUMN launcher SET DEFAULT '{
        "launcher_type": "icon",
        "position": "bottom_right",
        "label": "Chat with us",
        "offset_x": 20,
        "offset_y": 20,
        "attention_animation": "pulse",
        "animation_interval_seconds": 5,
        "show_operator_message_cta": false,
        "proactive_invitation_enabled": false,
        "proactive_invitation_delay_seconds": 15
    }'::jsonb,
    ADD CONSTRAINT widget_configs_launcher_shape_check CHECK (
        jsonb_typeof(launcher) = 'object'
        AND launcher ?& ARRAY[
            'launcher_type',
            'position',
            'label',
            'offset_x',
            'offset_y',
            'attention_animation',
            'animation_interval_seconds',
            'show_operator_message_cta',
            'proactive_invitation_enabled',
            'proactive_invitation_delay_seconds'
        ]
        AND launcher->>'launcher_type' IN ('icon', 'text', 'icon_text')
        AND launcher->>'position' IN ('bottom_right', 'bottom_left')
        AND jsonb_typeof(launcher->'label') = 'string'
        AND char_length(btrim(launcher->>'label')) BETWEEN 1 AND 40
        AND jsonb_typeof(launcher->'offset_x') = 'number'
        AND (launcher->>'offset_x')::numeric BETWEEN 0 AND 120
        AND (launcher->>'offset_x')::numeric = trunc((launcher->>'offset_x')::numeric)
        AND jsonb_typeof(launcher->'offset_y') = 'number'
        AND (launcher->>'offset_y')::numeric BETWEEN 0 AND 120
        AND (launcher->>'offset_y')::numeric = trunc((launcher->>'offset_y')::numeric)
        AND jsonb_typeof(launcher->'attention_animation') = 'string'
        AND launcher->>'attention_animation' IN ('pulse', 'lift', 'sway', 'none')
        AND CASE
            WHEN jsonb_typeof(launcher->'animation_interval_seconds') = 'number'
                THEN (launcher->>'animation_interval_seconds')::numeric BETWEEN 1 AND 300
                    AND (launcher->>'animation_interval_seconds')::numeric
                        = trunc((launcher->>'animation_interval_seconds')::numeric)
            ELSE false
        END
        AND jsonb_typeof(launcher->'show_operator_message_cta') = 'boolean'
        AND jsonb_typeof(launcher->'proactive_invitation_enabled') = 'boolean'
        AND CASE
            WHEN jsonb_typeof(launcher->'proactive_invitation_delay_seconds') = 'number'
                THEN (launcher->>'proactive_invitation_delay_seconds')::numeric BETWEEN 1 AND 300
                    AND (launcher->>'proactive_invitation_delay_seconds')::numeric
                        = trunc((launcher->>'proactive_invitation_delay_seconds')::numeric)
            ELSE false
        END
    ) NOT VALID;

ALTER TABLE widget_configs
    VALIDATE CONSTRAINT widget_configs_launcher_shape_check;
