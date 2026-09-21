ALTER TABLE ai_profiles
    ADD COLUMN blacklist_reply_text text NOT NULL DEFAULT ''
        CHECK (char_length(blacklist_reply_text) <= 4000),
    ADD COLUMN blacklist_reply_match_language boolean NOT NULL DEFAULT true;
