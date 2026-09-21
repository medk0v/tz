UPDATE channel_connections
SET config = config - 'bot_user_id' - 'bot_username',
    updated_at = now()
WHERE kind = 'telegram_bot'
  AND deleted_at IS NOT NULL
  AND config ? 'bot_user_id';

DROP INDEX channel_connections_active_telegram_bot_user_idx;

CREATE UNIQUE INDEX channel_connections_telegram_bot_user_idx
    ON channel_connections ((config ->> 'bot_user_id'))
    WHERE kind = 'telegram_bot'
      AND config ? 'bot_user_id';
