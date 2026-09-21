ALTER TABLE widget_configs
    DROP CONSTRAINT widget_configs_launcher_shape_check;

WITH normalized AS (
    SELECT
        channel_connection_id,
        launcher || jsonb_build_object(
            'attention_animation',
            CASE
                WHEN jsonb_typeof(launcher->'attention_animation') = 'string'
                    AND launcher->>'attention_animation' IN ('pulse', 'lift', 'sway', 'none')
                    THEN launcher->'attention_animation'
                ELSE '"pulse"'::jsonb
            END,
            'animation_interval_seconds',
            CASE
                WHEN jsonb_typeof(launcher->'animation_interval_seconds') = 'number'
                    THEN CASE
                        WHEN (launcher->>'animation_interval_seconds')::numeric BETWEEN 1 AND 300
                            AND (launcher->>'animation_interval_seconds')::numeric
                                = trunc((launcher->>'animation_interval_seconds')::numeric)
                            THEN launcher->'animation_interval_seconds'
                        ELSE '5'::jsonb
                    END
                ELSE '5'::jsonb
            END,
            'show_operator_message_cta',
            CASE
                WHEN jsonb_typeof(launcher->'show_operator_message_cta') = 'boolean'
                    THEN launcher->'show_operator_message_cta'
                ELSE 'false'::jsonb
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

ALTER TABLE widget_configs
    ALTER COLUMN launcher SET DEFAULT '{
        "launcher_type": "icon",
        "position": "bottom_right",
        "label": "Chat with us",
        "offset_x": 20,
        "offset_y": 20,
        "attention_animation": "pulse",
        "animation_interval_seconds": 5,
        "show_operator_message_cta": false
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
            'show_operator_message_cta'
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
    ) NOT VALID;

ALTER TABLE widget_configs
    VALIDATE CONSTRAINT widget_configs_launcher_shape_check;
