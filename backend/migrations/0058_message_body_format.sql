ALTER TABLE messages
    ADD COLUMN body_format text NOT NULL DEFAULT 'plain'
        CHECK (body_format IN ('plain', 'markdown'));
