UPDATE widget_configs
SET translations = (
    SELECT jsonb_object_agg(
        language,
        CASE
            WHEN split_part(lower(language), '-', 1) = 'zh'
                 AND translation ->> 'offline_now' = 'Leave a message'
                THEN translation || jsonb_build_object('offline_now', '请留言')
            ELSE translation
        END
    )
    FROM jsonb_each(translations) AS entry(language, translation)
);
