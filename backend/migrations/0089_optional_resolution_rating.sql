ALTER TABLE conversation_resolutions
    ADD COLUMN rating_requested boolean NOT NULL DEFAULT true;
