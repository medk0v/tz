ALTER TABLE notes ADD COLUMN sort_order integer NOT NULL DEFAULT 0 CHECK (sort_order >= 0);

WITH ordered AS (
    SELECT id, (row_number() OVER (
        PARTITION BY tenant_id, project_id, parent_id ORDER BY lower(title), id
    ) - 1)::integer AS sort_order
    FROM notes
)
UPDATE notes SET sort_order = ordered.sort_order FROM ordered WHERE notes.id = ordered.id;

CREATE INDEX notes_sibling_order_idx ON notes (tenant_id, project_id, parent_id, sort_order, id);
