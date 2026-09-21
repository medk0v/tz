-- A registration belongs to exactly one live operator credential. Existing
-- password-session registrations retain their foreign key and generation.
ALTER TABLE mobile_push_devices
    ALTER COLUMN session_id DROP NOT NULL,
    ADD COLUMN api_key_id uuid,
    ADD CONSTRAINT mobile_push_devices_api_key_fk
        FOREIGN KEY (tenant_id, api_key_id) REFERENCES api_keys (tenant_id, id)
        ON DELETE CASCADE,
    ADD CONSTRAINT mobile_push_devices_one_credential
        CHECK ((session_id IS NOT NULL) <> (api_key_id IS NOT NULL));

CREATE INDEX mobile_push_devices_api_key_idx ON mobile_push_devices (api_key_id)
    WHERE api_key_id IS NOT NULL;
