WITH normalized AS (
    SELECT
        config.channel_connection_id,
        jsonb_object_agg(
            entry.key,
            defaults.payload || entry.value
        ) AS translations
    FROM widget_configs AS config
    CROSS JOIN LATERAL jsonb_each(config.translations) AS entry
    CROSS JOIN LATERAL (
        SELECT CASE
            WHEN split_part(lower(entry.key), '-', 1) = 'ru' THEN jsonb_build_object(
                'online_now', 'Сейчас в сети',
                'message_placeholder', 'Напишите сообщение…',
                'contact_title', 'Представьтесь',
                'contact_description', 'Необязательно. Оставьте имя и email, чтобы мы могли связаться с вами по этому диалогу.',
                'contact_name', 'Имя',
                'contact_name_placeholder', 'Ваше имя',
                'contact_email', 'Email',
                'contact_email_placeholder', 'you@example.com',
                'contact_save', 'Сохранить',
                'contact_saving', 'Сохраняем…',
                'contact_skip', 'Продолжить анонимно',
                'contact_error', 'Не удалось сохранить контактные данные.'
            )
            ELSE jsonb_build_object(
                'online_now', 'Online now',
                'message_placeholder', 'Write a message…',
                'contact_title', 'Introduce yourself',
                'contact_description', 'Optional. Leave your name and email so we can contact you about this conversation.',
                'contact_name', 'Name',
                'contact_name_placeholder', 'Your name',
                'contact_email', 'Email',
                'contact_email_placeholder', 'you@example.com',
                'contact_save', 'Save details',
                'contact_saving', 'Saving…',
                'contact_skip', 'Continue anonymously',
                'contact_error', 'Could not save your contact details.'
            )
        END AS payload
    ) AS defaults
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
