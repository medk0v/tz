ALTER TABLE widget_configs
    ADD COLUMN attachments_enabled boolean NOT NULL DEFAULT false;

ALTER TABLE conversations
    ADD COLUMN widget_attachments_enabled boolean;

ALTER TABLE message_attachments
    ADD COLUMN expires_at timestamptz NOT NULL DEFAULT (now() + interval '30 days'),
    ADD COLUMN purged_at timestamptz;

ALTER TABLE message_attachments
    DROP CONSTRAINT message_attachments_byte_size_check,
    DROP CONSTRAINT message_attachments_scan_status_check;

ALTER TABLE message_attachments
    ADD CONSTRAINT message_attachments_byte_size_check
        CHECK (byte_size BETWEEN 1 AND 52428800),
    ADD CONSTRAINT message_attachments_file_name_check
        CHECK (length(file_name) BETWEEN 1 AND 200),
    ADD CONSTRAINT message_attachments_content_type_check
        CHECK (content_type IN (
            'image/jpeg', 'image/png', 'image/webp', 'application/pdf',
            'video/mp4', 'video/webm'
        )),
    ADD CONSTRAINT message_attachments_object_key_check
        CHECK (object_key = id::text),
    ADD CONSTRAINT message_attachments_checksum_check
        CHECK (checksum_sha256 ~ '^[A-Za-z0-9_-]{43}$'),
    ADD CONSTRAINT message_attachments_scan_status_check
        CHECK (scan_status IN ('pending', 'clean', 'infected', 'failed', 'expired')),
    ADD CONSTRAINT message_attachments_expiry_check
        CHECK (expires_at > created_at);

CREATE UNIQUE INDEX message_attachments_one_per_message_idx
    ON message_attachments (tenant_id, message_id);

CREATE INDEX message_attachments_tenant_usage_idx
    ON message_attachments (tenant_id, scan_status, expires_at)
    INCLUDE (byte_size);

CREATE INDEX message_attachments_expiry_idx
    ON message_attachments (expires_at, id)
    WHERE scan_status IN ('clean', 'expired') AND purged_at IS NULL;
