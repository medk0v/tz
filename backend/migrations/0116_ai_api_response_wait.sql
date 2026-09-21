ALTER TABLE ai_api_endpoints
    ADD COLUMN response_wait_seconds bigint NOT NULL DEFAULT 20
        CHECK (response_wait_seconds BETWEEN 0 AND 300);
