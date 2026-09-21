ALTER TABLE widget_configs
    ADD COLUMN launcher jsonb NOT NULL DEFAULT '{
        "launcher_type": "icon",
        "position": "bottom_right",
        "label": "Chat with us",
        "offset_x": 20,
        "offset_y": 20
    }'::jsonb,
    ADD CONSTRAINT widget_configs_launcher_shape_check CHECK (
        jsonb_typeof(launcher) = 'object'
        AND launcher ?& ARRAY[
            'launcher_type', 'position', 'label', 'offset_x', 'offset_y'
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
    );
