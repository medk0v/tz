-- Bind each new realtime ticket to the credential that authorized it.
-- Legacy tickets have no binding and are rejected by the API until they expire.
ALTER TABLE realtime_tickets
    ADD COLUMN credential_kind text,
    ADD COLUMN credential_id uuid,
    ADD CONSTRAINT realtime_tickets_credential_check CHECK (
        (credential_kind IS NULL AND credential_id IS NULL)
        OR (
            credential_kind IS NOT NULL
            AND credential_kind IN ('operator_session', 'access_token', 'widget_session')
            AND credential_id IS NOT NULL
        )
    );
