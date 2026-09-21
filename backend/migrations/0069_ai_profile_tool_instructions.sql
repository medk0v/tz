ALTER TABLE ai_profiles ADD COLUMN tool_instructions text NOT NULL DEFAULT ''
    CHECK (char_length(tool_instructions) <= 50000);
