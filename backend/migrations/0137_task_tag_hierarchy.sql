ALTER TABLE task_tags
    ADD COLUMN parent_tag_id uuid,
    ADD CONSTRAINT task_tags_parent_not_self CHECK (parent_tag_id <> id),
    ADD CONSTRAINT task_tags_parent_fk
        FOREIGN KEY (tenant_id, project_id, parent_tag_id)
        REFERENCES task_tags (tenant_id, project_id, id)
        ON DELETE SET NULL (parent_tag_id);

CREATE INDEX task_tags_parent_idx ON task_tags (tenant_id, project_id, parent_tag_id);
