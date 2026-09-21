CREATE TABLE support_rating_telegram_invitations (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    inbox_id uuid NOT NULL,
    resolution_id uuid NOT NULL,
    channel_connection_id uuid NOT NULL,
    contact_id uuid NOT NULL,
    support_rating_id uuid,
    telegram_user_id text NOT NULL CHECK (char_length(telegram_user_id) BETWEEN 1 AND 32),
    chat_id text NOT NULL CHECK (char_length(chat_id) BETWEEN 1 AND 32),
    language text NOT NULL CHECK (char_length(language) BETWEEN 1 AND 35),
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'rating_recorded', 'awaiting_comment', 'completed')),
    prompt_message_id bigint,
    comment_prompt_message_id bigint,
    expires_at timestamptz NOT NULL,
    sent_at timestamptz,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, resolution_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, inbox_id) REFERENCES inboxes (tenant_id, id),
    FOREIGN KEY (tenant_id, resolution_id)
        REFERENCES conversation_resolutions (tenant_id, id),
    FOREIGN KEY (tenant_id, channel_connection_id)
        REFERENCES channel_connections (tenant_id, id),
    FOREIGN KEY (tenant_id, contact_id) REFERENCES contacts (tenant_id, id),
    FOREIGN KEY (tenant_id, support_rating_id)
        REFERENCES support_ratings (tenant_id, id)
);

CREATE INDEX support_rating_telegram_pending_idx
    ON support_rating_telegram_invitations (
        tenant_id, channel_connection_id, chat_id, expires_at DESC
    )
    WHERE completed_at IS NULL;
