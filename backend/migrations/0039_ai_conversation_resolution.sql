ALTER TABLE ai_profiles
    ADD COLUMN can_resolve_conversations boolean NOT NULL DEFAULT false;

ALTER TABLE conversation_resolutions
    ADD COLUMN responsible_ai_profile_id uuid,
    ADD CONSTRAINT conversation_resolutions_responsible_ai_profile_fk
        FOREIGN KEY (tenant_id, responsible_ai_profile_id)
        REFERENCES ai_profiles (tenant_id, id)
        ON DELETE SET NULL (responsible_ai_profile_id),
    ADD CONSTRAINT conversation_resolutions_single_responsible_party_check
        CHECK (
            responsible_actor_id IS NULL
            OR responsible_ai_profile_id IS NULL
        );

CREATE INDEX conversation_resolutions_ai_quality_idx
    ON conversation_resolutions (
        tenant_id,
        project_id,
        responsible_ai_profile_id,
        resolved_at DESC
    )
    WHERE responsible_ai_profile_id IS NOT NULL;
