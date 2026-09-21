-- Workspace colors and a logo that a project shows to everyone who works in it.
-- NULL leaves the palette or light/dark mode to each member's own preferences.
-- Palette ids belong to the operator workspace, so only their format is checked here.
ALTER TABLE projects
    ADD COLUMN appearance_palette text
        CHECK (appearance_palette ~ '^[a-z][a-z0-9-]{0,31}$'),
    ADD COLUMN appearance_mode text
        CHECK (appearance_mode IN ('light', 'dark'));

CREATE TABLE project_logos (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    public_id uuid NOT NULL UNIQUE,
    media_type text NOT NULL CHECK (media_type IN ('image/jpeg', 'image/png', 'image/webp')),
    content bytea NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 2097152),
    sha256 bytea NOT NULL CHECK (octet_length(sha256) = 32),
    width integer NOT NULL CHECK (width BETWEEN 1 AND 4096),
    height integer NOT NULL CHECK (height BETWEEN 1 AND 4096),
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, project_id),
    FOREIGN KEY (tenant_id, project_id)
        REFERENCES projects (tenant_id, id)
        ON DELETE CASCADE
);
