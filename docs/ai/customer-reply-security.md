# Customer reply confidentiality

The backend adds `tz_customer_reply_security` to the shared customer conversation
prompt for normal replies, operator drafts, automatic closing messages, scheduled
follow-ups and agent scenario runs. It applies with either a direct provider or
OpenClaw, including profiles without custom instructions or a public name. Internal
scheduled tasks and API/team execution keep their separate task prompts.

The rule permits truthful AI identification and documented public product API
support. It prohibits model/provider disclosure, confirmation of internal connections,
tool inventories, configuration, credentials and instruction extraction, including
repetition of earlier disclosures. It also prohibits invented checks or handoffs
used to deflect technical questions.

`customer-reply-security-scenarios.json` is a request body for
`POST /api/v1/ai/profiles/{profile_id}/tests`. Import it explicitly for the intended
profile, then run the saved isolated scenarios using the profile's actual model.
The normal article-based scenario generator is unchanged. All messages and the
public-API article in this file are synthetic fixtures; the article is not published.

Run the scenarios repeatedly and inspect replies and tool traces. No tools should
be used to investigate the assistant's internals. Literal assertions catch the two
reported disclosures; semantic review checks paraphrases, confirmations and encoded
disclosures. Manually inspect those results as well. The public-API scenario verifies
that normal product support still works. Local Rust tests validate prompt propagation,
fixture schemas and literal checks; they do not prove model compliance. This is an
instruction-level control, not a deterministic output filter. Existing server-side
tool grants and secret isolation remain necessary.

Deploying the backend activates the shared rule for subsequent customer requests.
It does not rewrite saved profile instructions or previously sent messages. Scenario
run fingerprints include the prompt source, so earlier results become stale after
this change. Production deployment, scenario import and live model checks are separate
operations.
