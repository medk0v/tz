WITH normalized AS (
    SELECT
        config.channel_connection_id,
        jsonb_object_agg(
            entry.key,
            entry.value || jsonb_build_object(
                'offline_message',
                CASE
                    WHEN jsonb_typeof(entry.value->'offline_message') = 'string'
                        AND char_length(btrim(entry.value->>'offline_message'))
                            BETWEEN 1 AND 500
                        THEN to_jsonb(btrim(entry.value->>'offline_message'))
                    WHEN split_part(lower(entry.key), '-', 1) = 'ru'
                        THEN to_jsonb(
                            'Операторы офлайн. Оставьте сообщение и контакты.'::text
                        )
                    ELSE to_jsonb(
                        'Operators are offline. Leave a message and your contact details.'::text
                    )
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
    updated_at = now()
FROM normalized
WHERE config.channel_connection_id = normalized.channel_connection_id
  AND config.translations IS DISTINCT FROM normalized.translations;
