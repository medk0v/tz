ALTER TABLE conversations
    ADD COLUMN widget_language text,
    ADD CONSTRAINT conversations_widget_language_check CHECK (
        widget_language IS NULL
        OR (
            char_length(widget_language) BETWEEN 1 AND 35
            AND widget_language = lower(widget_language)
            AND widget_language = btrim(widget_language)
        )
    );

CREATE TABLE project_email_settings (
    project_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    enabled boolean NOT NULL DEFAULT false,
    smtp_host text NOT NULL CHECK (char_length(btrim(smtp_host)) BETWEEN 1 AND 253),
    smtp_port integer NOT NULL CHECK (smtp_port BETWEEN 1 AND 65535),
    smtp_security text NOT NULL CHECK (smtp_security IN ('tls', 'starttls')),
    smtp_username text,
    encrypted_smtp_password bytea,
    smtp_password_nonce bytea,
    key_version text,
    from_name text NOT NULL CHECK (char_length(btrim(from_name)) BETWEEN 1 AND 120),
    from_email text NOT NULL CHECK (char_length(btrim(from_email)) BETWEEN 3 AND 320),
    reply_to_email text CHECK (
        reply_to_email IS NULL OR char_length(btrim(reply_to_email)) BETWEEN 3 AND 320
    ),
    rating_page_url text NOT NULL CHECK (char_length(btrim(rating_page_url)) BETWEEN 1 AND 2000),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    CHECK (
        (
            smtp_username IS NULL
            AND encrypted_smtp_password IS NULL
            AND smtp_password_nonce IS NULL
            AND key_version IS NULL
        )
        OR (
            smtp_username IS NOT NULL
            AND encrypted_smtp_password IS NOT NULL
            AND octet_length(smtp_password_nonce) = 12
            AND key_version IS NOT NULL
        )
    )
);

CREATE TABLE support_rating_email_invitations (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    resolution_id uuid NOT NULL,
    recipient_email text NOT NULL CHECK (char_length(recipient_email) BETWEEN 3 AND 320),
    language text NOT NULL CHECK (char_length(language) BETWEEN 1 AND 35),
    access_token_hash bytea NOT NULL UNIQUE CHECK (octet_length(access_token_hash) = 32),
    encrypted_access_token bytea,
    access_token_nonce bytea,
    key_version text,
    expires_at timestamptz NOT NULL,
    queued_at timestamptz NOT NULL DEFAULT now(),
    sent_at timestamptz,
    consumed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, resolution_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, resolution_id)
        REFERENCES conversation_resolutions (tenant_id, id),
    CHECK (
        (
            encrypted_access_token IS NULL
            AND access_token_nonce IS NULL
            AND key_version IS NULL
        )
        OR (
            encrypted_access_token IS NOT NULL
            AND octet_length(access_token_nonce) = 12
            AND key_version IS NOT NULL
        )
    )
);

CREATE INDEX support_rating_email_invitation_expiry_idx
    ON support_rating_email_invitations (expires_at)
    WHERE consumed_at IS NULL;
