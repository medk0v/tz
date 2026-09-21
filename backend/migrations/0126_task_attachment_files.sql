-- Keep validated media previews and allow other scanned files as binary downloads.
ALTER TABLE ai_task_attachments
    DROP CONSTRAINT ai_task_attachments_content_type_check,
    DROP CONSTRAINT ai_task_attachments_byte_size_check,
    ADD CONSTRAINT ai_task_attachments_content_type_check CHECK (
        content_type IN ('image/png', 'image/jpeg', 'image/webp', 'application/pdf',
                         'video/mp4', 'video/webm', 'application/octet-stream')
    ),
    ADD CONSTRAINT ai_task_attachments_byte_size_check CHECK (
        byte_size >= 1 AND byte_size <= CASE
            WHEN content_type IN ('image/png', 'image/jpeg', 'image/webp') THEN 10485760
            WHEN content_type IN ('video/mp4', 'video/webm') THEN 52428800
            ELSE 20971520
        END
    );
