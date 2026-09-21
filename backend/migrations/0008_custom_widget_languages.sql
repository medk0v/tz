ALTER TABLE widget_configs
    DROP CONSTRAINT IF EXISTS widget_configs_default_language_check,
    DROP CONSTRAINT IF EXISTS widget_configs_translations_shape_check;

ALTER TABLE widget_sessions
    DROP CONSTRAINT IF EXISTS widget_sessions_language_check;
