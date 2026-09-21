UPDATE widget_configs AS config
SET translations = localized.translations,
    updated_at = now()
FROM (
    SELECT source.channel_connection_id, jsonb_object_agg(
        language.code,
        language.translation || jsonb_build_object(
            'rating_prompt',
            CASE
                WHEN split_part(lower(language.code), '-', 1) = 'ru'
                    THEN 'Спасибо за обращение! Оцените, пожалуйста, качество поддержки.'
                ELSE 'Thanks for chatting with us. Please rate the support you received.'
            END,
            'rating_thanks',
            CASE
                WHEN split_part(lower(language.code), '-', 1) = 'ru'
                    THEN 'Спасибо! Ваш отзыв поможет нам стать лучше.'
                ELSE 'Thank you! Your feedback helps us improve.'
            END
        )
    ) AS translations
    FROM widget_configs AS source
    CROSS JOIN LATERAL jsonb_each(source.translations) AS language(code, translation)
    WHERE jsonb_typeof(source.translations) = 'object'
    GROUP BY source.channel_connection_id
) AS localized
WHERE config.channel_connection_id = localized.channel_connection_id;

CREATE INDEX conversation_resolutions_quality_idx
    ON conversation_resolutions (tenant_id, project_id, resolved_at DESC);

CREATE INDEX support_ratings_quality_idx
    ON support_ratings (tenant_id, project_id, created_at DESC);
