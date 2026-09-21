ALTER TABLE contacts
    ADD COLUMN email text,
    ADD CONSTRAINT contacts_email_shape_check CHECK (
        email IS NULL
        OR (
            char_length(email) BETWEEN 3 AND 320
            AND email = btrim(email)
            AND email = lower(email)
            AND email ~ '^[^@[:space:]]+@[^@[:space:]]+$'
        )
    );

UPDATE contacts
SET display_name = NULL,
    updated_at = now()
WHERE display_name = 'Anonymous visitor';

CREATE INDEX contacts_email_lookup_idx
    ON contacts (tenant_id, lower(email))
    WHERE email IS NOT NULL;
