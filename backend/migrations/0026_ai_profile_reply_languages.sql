ALTER TABLE ai_profiles
    DROP CONSTRAINT ai_profiles_language_check,
    ADD CONSTRAINT ai_profiles_language_check
        CHECK (length(language) BETWEEN 2 AND 255);
