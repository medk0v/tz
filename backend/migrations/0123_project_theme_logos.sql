-- A project keeps one logo for the light theme and another for the dark theme.
-- A logo uploaded before this change becomes the light-theme logo.
ALTER TABLE project_logos
    ADD COLUMN theme text NOT NULL DEFAULT 'light' CHECK (theme IN ('light', 'dark'));
ALTER TABLE project_logos ALTER COLUMN theme DROP DEFAULT;
ALTER TABLE project_logos DROP CONSTRAINT project_logos_pkey;
ALTER TABLE project_logos ADD PRIMARY KEY (tenant_id, project_id, theme);
