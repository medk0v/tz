CREATE UNIQUE INDEX channel_connections_active_telegram_bot_user_idx
    ON channel_connections ((config ->> 'bot_user_id'))
    WHERE kind = 'telegram_bot'
      AND deleted_at IS NULL
      AND config ? 'bot_user_id';
