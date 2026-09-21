ALTER TABLE ai_profiles
    ADD COLUMN capability_http_get boolean NOT NULL DEFAULT false,
    ADD COLUMN capability_http_post boolean NOT NULL DEFAULT false,
    ADD COLUMN capability_shell boolean NOT NULL DEFAULT false;
