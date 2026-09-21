-- Model connections gain a type: chat models answer, speech-to-text models
-- transcribe voice messages, and text-to-speech models voice agent replies.
ALTER TABLE ai_provider_connections
    ADD COLUMN model_type text NOT NULL DEFAULT 'chat'
        CHECK (model_type IN ('chat', 'speech_to_text', 'text_to_speech')),
    ADD CONSTRAINT ai_provider_connections_audio_provider_check
        CHECK (model_type = 'chat' OR provider_kind IN ('openai', 'openai_compatible')),
    ADD CONSTRAINT ai_provider_connections_typed_identity_key
        UNIQUE (tenant_id, id, model_type);

-- Each profile reference is bound to the connection type it expects, so a
-- speech connection can never become an agent's chat model and vice versa.
ALTER TABLE ai_profiles
    ADD COLUMN chat_model_type text GENERATED ALWAYS AS ('chat') STORED,
    ADD COLUMN speech_to_text_connection_id uuid,
    ADD COLUMN speech_to_text_model text
        CHECK (speech_to_text_model IS NULL OR length(trim(speech_to_text_model)) BETWEEN 1 AND 200),
    ADD COLUMN speech_to_text_model_type text GENERATED ALWAYS AS ('speech_to_text') STORED,
    ADD COLUMN text_to_speech_connection_id uuid,
    ADD COLUMN text_to_speech_model text
        CHECK (text_to_speech_model IS NULL OR length(trim(text_to_speech_model)) BETWEEN 1 AND 200),
    ADD COLUMN text_to_speech_voice text
        CHECK (text_to_speech_voice IS NULL OR length(trim(text_to_speech_voice)) BETWEEN 1 AND 100),
    ADD COLUMN text_to_speech_model_type text GENERATED ALWAYS AS ('text_to_speech') STORED,
    ADD COLUMN voice_replies_enabled boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT ai_profiles_speech_to_text_settings_check
        CHECK (speech_to_text_connection_id IS NOT NULL OR speech_to_text_model IS NULL),
    ADD CONSTRAINT ai_profiles_text_to_speech_settings_check
        CHECK (
            text_to_speech_connection_id IS NOT NULL
            OR (text_to_speech_model IS NULL AND text_to_speech_voice IS NULL AND NOT voice_replies_enabled)
        );

ALTER TABLE ai_profiles
    DROP CONSTRAINT ai_profiles_tenant_id_provider_connection_id_fkey,
    ADD CONSTRAINT ai_profiles_chat_connection_fkey
        FOREIGN KEY (tenant_id, provider_connection_id, chat_model_type)
        REFERENCES ai_provider_connections (tenant_id, id, model_type),
    ADD CONSTRAINT ai_profiles_speech_to_text_connection_fkey
        FOREIGN KEY (tenant_id, speech_to_text_connection_id, speech_to_text_model_type)
        REFERENCES ai_provider_connections (tenant_id, id, model_type),
    ADD CONSTRAINT ai_profiles_text_to_speech_connection_fkey
        FOREIGN KEY (tenant_id, text_to_speech_connection_id, text_to_speech_model_type)
        REFERENCES ai_provider_connections (tenant_id, id, model_type);

CREATE INDEX ai_profiles_speech_to_text_connection_idx
    ON ai_profiles (tenant_id, speech_to_text_connection_id)
    WHERE speech_to_text_connection_id IS NOT NULL;

CREATE INDEX ai_profiles_text_to_speech_connection_idx
    ON ai_profiles (tenant_id, text_to_speech_connection_id)
    WHERE text_to_speech_connection_id IS NOT NULL;

-- Telegram voice notes keep their original Ogg recording next to the transcript.
ALTER TABLE message_attachments
    DROP CONSTRAINT message_attachments_content_type_check,
    ADD CONSTRAINT message_attachments_content_type_check
        CHECK (content_type IN (
            'image/jpeg', 'image/png', 'image/webp', 'application/pdf',
            'video/mp4', 'video/webm', 'audio/ogg'
        ));

-- Transcribed customer voice messages are stored as text; the flag keeps the
-- origin visible when the recording itself is not retained.
ALTER TABLE messages
    ADD COLUMN is_voice_message boolean NOT NULL DEFAULT false;
