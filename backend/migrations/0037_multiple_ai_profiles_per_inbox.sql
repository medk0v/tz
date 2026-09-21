ALTER TABLE inbox_ai_profiles
    DROP CONSTRAINT inbox_ai_profiles_pkey;

ALTER TABLE inbox_ai_profiles
    ADD PRIMARY KEY (tenant_id, inbox_id, ai_profile_id);

UPDATE widget_configs
SET translations = (
    SELECT jsonb_object_agg(
        language,
        translation || jsonb_build_object(
            'offline_now',
            CASE split_part(lower(language), '-', 1)
                WHEN 'ru' THEN 'Оставьте сообщение'
                WHEN 'ro' THEN 'Lăsați un mesaj'
                WHEN 'hi' THEN 'संदेश छोड़ें'
                ELSE 'Leave a message'
            END
        )
    )
    FROM jsonb_each(translations) AS entry(language, translation)
);
