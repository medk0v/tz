ALTER TABLE departments
    ADD COLUMN sidebar_categories jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (jsonb_typeof(sidebar_categories) = 'array');
