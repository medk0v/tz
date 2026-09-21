# Operator handoff during normal processing

The shared customer prompt suppresses unsolicited human handoff offers and operator
notifications when verified status and project policy confirm normal processing
within the allowed time. Waiting, frustration, repeated status questions and a
missing payout-sent marker alone do not justify a handoff. Explicit human requests
and verified exceptions requiring intervention under project policy remain valid.
The application does not hard-code a processing deadline or infer one from missing data.

Routine handoffs first collect the actionable context from the conversation, approved
records and focused follow-up questions. For payout-detail changes this includes every
affected order ID, replacement phone and bank for SBP, which orders the change applies
to, and the available payout status. The agent waits for the answer before notifying;
it does not repeat known questions or request credentials or unrelated personal data.
Urgent policy-required intervention, an insistence on immediate human help, or an
inability to supply details permits handoff with the known facts and explicit gaps.
Greetings never include an unsolicited operator invitation.

Conversation notification grants support `--summary '<HANDOFF_SUMMARY>'` (up to 2500
UTF-16 code units). The wrapper adds the trusted conversation reference and sends to
configured recipients only. Legacy calls without a summary still use the fixed text;
older grants cannot accept summaries. Deploy the updated backend and OpenClaw image
together. Telegram delivery is confirmed only by a successful wrapper result.

The rule applies to replies, drafts, follow-ups and agent scenarios through direct
providers and OpenClaw, including Lite. It takes effect after backend deployment.

`customer-operator-handoff-scenarios.json` can be imported using
`POST /api/v1/ai/profiles/{profile_id}/tests`. It uses synthetic order data in local
scenario articles and requires knowledge retrieval and operator notification capabilities.
The scenarios cover normal waiting, frustration, an explicit human request and a
policy-required overdue handoff, and intake for changes to two SBP payouts. Review
the final scenario's notification arguments for both IDs, replacement details and
explicitly unverified status. Local tests verify prompt propagation and scenario
assertions; only running the scenarios against the selected model and reviewing their
replies checks model compliance. This is a prompt rule, not a deterministic output filter.
