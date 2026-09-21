ALTER TABLE widget_configs
    DROP CONSTRAINT widget_configs_launcher_shape_check;

UPDATE widget_configs
SET launcher = jsonb_set(launcher, '{attention_animation}', '"lift"'::jsonb),
    updated_at = now()
WHERE launcher->>'show_greeting' = 'false'
  AND launcher->>'attention_animation' = 'pulse';

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
            WHEN jsonb_typeof(launcher->'show_greeting') = 'boolean'
                THEN (launcher->>'show_greeting')::boolean
                    OR launcher->>'attention_animation' <> 'pulse'
            ELSE false
        END
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
