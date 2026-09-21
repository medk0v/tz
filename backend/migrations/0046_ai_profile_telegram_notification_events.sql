ALTER TABLE ai_profiles
    ADD COLUMN telegram_notify_on_new_visitor boolean NOT NULL DEFAULT false,
    ADD COLUMN telegram_notify_on_new_message boolean NOT NULL DEFAULT false,
    ADD COLUMN telegram_notify_on_operator_request boolean NOT NULL DEFAULT true;
