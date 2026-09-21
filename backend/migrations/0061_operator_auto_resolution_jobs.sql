CREATE UNIQUE INDEX outbox_operator_auto_resolution_trigger_idx
    ON outbox_events (
        tenant_id,
        aggregate_id,
        (payload->>'triggering_sequence')
    )
    WHERE aggregate_type = 'provider_reply'
      AND event_type = 'provider.operator_auto_resolution.requested';
