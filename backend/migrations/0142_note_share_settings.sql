ALTER TABLE note_shares
    ADD COLUMN theme_palette text CHECK (theme_palette IN ('ocean', 'ink', 'graphite', 'forest', 'plum', 'copper', 'paper')),
    ADD COLUMN theme_mode text CHECK (theme_mode IN ('light', 'dark')),
    ADD COLUMN allow_theme_change boolean NOT NULL DEFAULT true,
    ADD COLUMN include_descendants boolean NOT NULL DEFAULT true,
    ADD COLUMN included_note_ids uuid[] CHECK (
        cardinality(included_note_ids) <= 5000 AND array_position(included_note_ids, NULL) IS NULL
    ),
    ADD COLUMN version integer NOT NULL DEFAULT 1 CHECK (version > 0);
