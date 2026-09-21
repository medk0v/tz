ALTER TABLE widget_configs
    DROP CONSTRAINT widget_configs_launcher_shape_check;

WITH normalized AS (
    SELECT
        config.channel_connection_id,
        jsonb_object_agg(
            entry.key,
            entry.value || jsonb_build_object(
                'support_name',
                CASE
                    WHEN jsonb_typeof(config.launcher->'support_name') = 'string'
                        AND char_length(btrim(config.launcher->>'support_name')) BETWEEN 1 AND 80
                        THEN to_jsonb(btrim(config.launcher->>'support_name'))
                    WHEN jsonb_typeof(entry.value->'support_name') = 'string'
                        AND char_length(btrim(entry.value->>'support_name')) BETWEEN 1 AND 80
                        THEN to_jsonb(btrim(entry.value->>'support_name'))
                    ELSE to_jsonb('Support'::text)
                END
            )
        ) AS translations
    FROM widget_configs AS config
    CROSS JOIN LATERAL jsonb_each(config.translations) AS entry
    WHERE jsonb_typeof(config.translations) = 'object'
      AND jsonb_typeof(entry.value) = 'object'
    GROUP BY config.channel_connection_id
)
UPDATE widget_configs AS config
SET translations = normalized.translations,
    launcher = config.launcher - 'support_name',
    updated_at = now()
FROM normalized
WHERE config.channel_connection_id = normalized.channel_connection_id
  AND (
      config.translations IS DISTINCT FROM normalized.translations
      OR config.launcher ? 'support_name'
  );

ALTER TABLE widget_configs
    ALTER COLUMN launcher SET DEFAULT '{
        "launcher_type": "icon",
        "position": "bottom_right",
        "label": "Chat with us",
        "show_greeting": true,
        "show_operator_profile": false,
        "offset_x": 20,
        "offset_y": 20,
        "attention_animation": "pulse",
        "animation_interval_seconds": 5,
        "proactive_invitation_enabled": false,
        "proactive_invitation_delay_seconds": 15
    }'::jsonb,
    ADD CONSTRAINT widget_configs_launcher_shape_check CHECK (
        jsonb_typeof(launcher) = 'object'
        AND launcher ?& ARRAY[
            'launcher_type',
            'position',
            'label',
            'show_greeting',
            'show_operator_profile',
            'offset_x',
            'offset_y',
            'attention_animation',
            'animation_interval_seconds',
            'proactive_invitation_enabled',
            'proactive_invitation_delay_seconds'
        ]
        AND NOT (launcher ?| ARRAY['show_operator_message_cta', 'support_name'])
        AND launcher->>'launcher_type' IN ('icon', 'text', 'icon_text')
        AND launcher->>'position' IN ('bottom_right', 'bottom_left')
        AND jsonb_typeof(launcher->'label') = 'string'
        AND char_length(btrim(launcher->>'label')) BETWEEN 1 AND 40
        AND jsonb_typeof(launcher->'show_greeting') = 'boolean'
        AND jsonb_typeof(launcher->'show_operator_profile') = 'boolean'
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
