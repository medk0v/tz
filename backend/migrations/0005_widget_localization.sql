ALTER TABLE widget_configs
    ADD COLUMN default_language text NOT NULL DEFAULT 'en'
        CHECK (default_language IN ('en', 'ru')),
    ADD COLUMN translations jsonb NOT NULL DEFAULT '{
        "en": {
            "greeting": "How can we help? Send us a message.",
            "launcher_label": "Chat with us"
        },
        "ru": {
            "greeting": "Чем мы можем помочь? Напишите нам.",
            "launcher_label": "Напишите нам"
        }
    }'::jsonb;

UPDATE widget_configs
SET translations = jsonb_build_object(
    'en', jsonb_build_object(
        'greeting', greeting,
        'launcher_label', launcher->>'label'
    ),
    'ru', jsonb_build_object(
        'greeting', 'Чем мы можем помочь? Напишите нам.',
        'launcher_label', 'Напишите нам'
    )
);

ALTER TABLE widget_configs
    ADD CONSTRAINT widget_configs_translations_shape_check CHECK (
        jsonb_typeof(translations) = 'object'
        AND translations ?& ARRAY['en', 'ru']
        AND jsonb_typeof(translations->'en') = 'object'
        AND jsonb_typeof(translations->'ru') = 'object'
        AND translations->'en' ?& ARRAY['greeting', 'launcher_label']
        AND translations->'ru' ?& ARRAY['greeting', 'launcher_label']
        AND (
            translations->'en'->'greeting' = 'null'::jsonb
            OR (
                jsonb_typeof(translations->'en'->'greeting') = 'string'
                AND char_length(translations->'en'->>'greeting') BETWEEN 1 AND 500
            )
        )
        AND (
            translations->'ru'->'greeting' = 'null'::jsonb
            OR (
                jsonb_typeof(translations->'ru'->'greeting') = 'string'
                AND char_length(translations->'ru'->>'greeting') BETWEEN 1 AND 500
            )
        )
        AND jsonb_typeof(translations->'en'->'launcher_label') = 'string'
        AND char_length(btrim(translations->'en'->>'launcher_label')) BETWEEN 1 AND 40
        AND jsonb_typeof(translations->'ru'->'launcher_label') = 'string'
        AND char_length(btrim(translations->'ru'->>'launcher_label')) BETWEEN 1 AND 40
    );

ALTER TABLE widget_sessions
    ADD COLUMN language text NOT NULL DEFAULT 'en'
        CHECK (language IN ('en', 'ru'));
