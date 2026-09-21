# Agent tests

The Tests tab lives in saved agent settings.
Apply migration `0067_ai_agent_tests.sql`, rebuild the backend/frontend, and rebuild the OpenClaw image (including `tz-test-tool.mjs`). No changes to agent instructions or published knowledge are required. Saved draft/disabled agents can be tested with an active provider, instructions and an assigned active channel; this does not activate the agent.

Create a scenario, paste questions one per line, or use **Добавить набор по базе знаний**. The generated suite includes one retrieval scenario for every currently assigned published article. It adds no fixed customer topics, API fixtures, business rules or timer durations. Configure any additional scenarios using the agent’s saved instructions and assigned knowledge.

Each step has a customer message, optional time advance and operator/conversation state changes. State changes happen before advancing virtual time; connecting an operator or closing the conversation cancels a pending timer, including at the due-time boundary. A due continuation uses the production reminder context and knowledge-only capabilities. No customer, conversation, outbox or timer records are created by the runner.

The scenario editor can also supply an artificial contact card:
`"contact": {"contact_id": "00000000-0000-0000-0000-000000000042", "display_name": "Sample Contact"}`.
The ID must be a UUID; the optional name is limited to 1000 UTF-8 bytes. These
values do not need a matching contact record. The runner adds the same
`tz_contact` block used by production replies to every customer reply and
scheduled continuation. It never reads or writes a real contact for this card.
Omitting `contact` or setting it to `null` preserves existing scenarios. An empty
or missing name becomes `null` in the model context. Use an ID with no name to
test ID matching, or a name with a different artificial ID to test name matching
independently. Keep prepared response wording in the scenario's knowledge
articles and assertions, so the agent must retrieve the matching article itself.

## Knowledge articles inside a test

`knowledge_articles` stores artificial articles directly in the scenario and its
versioned run snapshot. These articles are never published, inserted into the
knowledge tables, or made available to real conversations. For example:

```json
"knowledge_articles": [
  {
    "article_id": "00000000-0000-0000-0000-000000000043",
    "title": "Contact policy for this test",
    "body": "Use this prepared response when the contact matches the article.",
    "version": 1
  }
]
```

Omitting the list preserves existing scenarios. Each article needs a unique UUID,
a nonempty title of at most 300 characters, and a nonempty body of at most 200000
characters. Version defaults to 1 and must be positive. There may be at most 20
articles; the complete scenario remains limited to 512 KiB. An artificial article
ID cannot overlap an assigned published article ID; a collision stops the run
instead of replacing the real article.

The agent sees the artificial titles alongside its assigned article catalog and
reads their contents through the same `read_article` tool, including version and
chunk handling. Article bodies are never appended to the initial reply prompt.
Required and forbidden `read_article` assertions can use these artificial IDs and
versions. `source_articles` continues to reference real assigned articles; leave
it empty for a scenario that tests only artificial articles.

Edit an artificial article in the scenario editor. The production source editor
does not accept its ID, even after a successful read. Recommendations for these
articles target the scenario, not published knowledge. Historical runs retain
their original article bodies and versions.

## Assertions and results

- Required/forbidden facts are explicitly configured, case-sensitive literal phrases. These are strict text conditions, not an automatic factual-understanding judge. Write complete unambiguous phrases; assess their meaning separately.
- Required/forbidden actions match `tool` and `parameters` exactly. A fixture is available only after an authorized matching call, and has a bounded use count.
- Timer presence/remaining seconds, conversation state and reply presence are independent strict checks.
- After each reply, the same provider/model evaluates the actual answer against the expected meaning in a separate request. It receives the saved instructions, artificial contact card when configured, initial history, completed steps and observed tool trace, with no tool grant or execution broker. The expected meaning and previous grades are not included in the customer-answer prompt.
- The semantic result includes `pass`, `fail` or `manual_review`, a concrete Russian explanation and model metadata. Missing criteria, unavailable/invalid model review or a 60-second review timeout leave the step for manual review; an explicitly expected silence is checked directly. A failed review request preserves the actual reply and strict checks. All review requests share the run's nine-minute deadline.
- The UI displays the automatic explanation and allows a manual override with a separate explanation per step. Automatic and human verdicts are retained in review history. Passing a semantic review cannot override a strict failure or execution error. Existing runs are unchanged; rerun a scenario to obtain the automatic review.
- Every completed run, including a passed run, offers **Рекомендации по инструкциям**. This generates suggestions from the recorded instructions, scenario, knowledge and trace using the current provider, without tools. Suggestions distinguish agent instructions, knowledge articles, scenario expectations and execution problems. They do not change the run's verdict or save any proposed text. Advice for an outdated run is marked as such.
- **Использованные инструкции** opens the recorded main/tool instructions and actually read article fragments alongside an editor for their current source. **Редактировать источник** selects the source associated with a recommendation. Source edits are saved explicitly and preserve other unsaved profile fields; recommendations are generated on demand and are not retained as run history.
- Results distinguish passed, behavior error, execution error and manual review. Calls without a matching fixture or permitted live access, wrong parameters and forbidden capabilities are behavior errors; unavailable execution infrastructure is an execution error.

One run per profile executes at a time. Selected/all runs are queued in the open view; closing it stops dispatching further scenarios but an accepted server run finishes independently. Each run has a nine-minute deadline. Interrupted runs become execution errors after ten minutes, on the next list/launch request.

Each test has a separate bordered block. Its run history is hidden initially and opens when that test is launched or its eye button (after Play) is clicked. The eye button also hides the history. **Очистить историю** inside a test's history deletes all completed runs for that scenario, including records beyond the latest 50 displayed runs; active runs, the scenario, and other tests' history are preserved. The toolbar action still clears completed history for the entire agent.

## Tool fixtures

Tool names: `http`, `browser`, `integration`, `shell`, `notify_operator`, `resolve`, `schedule_reminder`. Assertions also accept `read_article`, `timer_fired`, and `timer_cancelled`.

`browser` uses parameters `{"url":"https://explorer.example/#/transaction/abc"}`
and returns rendered page fields (`ok`, `url`, `status`, `title`, `text`, `links`,
`partial`, `truncated`). A prepared response contains these fields as a JSON string.
Without a matching prepared response, the page opens in Chromium under the saved
agent's GET permission and allowed domains. A browser entry in `live_allowlist`
matches the exact initial call and URL; subsequent page requests retain the
production browser's network protections.

Example fixture for an API returning a missing item:

```json
[
  {
    "call": {
      "tool": "http",
      "parameters": {
        "method": "GET",
        "url": "https://api.example.com/v2/items/00000000-0000-0000-0000-000000000000"
      }
    },
    "response": "null",
    "uses": 1
  }
]
```

HTTP parameters are `method`, `url` and, for POST, `body` as serialized JSON. Integration parameters are `integration_key`, `action_key`, and `parameters`, matching the production integration request. Integration response stdout is `{ "ok": true, "status_code": 200, "content_type": "application/json", "body": "null" }`, or `{ "ok": false, "error": "integration_unavailable" }`. Shell parameters contain `command`; timer parameters contain `delay_seconds`. Notification and resolution parameters are `{}`.

Each authorized call first uses a matching prepared response with remaining uses,
even when the call also appears in `live_allowlist`. With no prepared response
and an empty per-scenario `live_allowlist`, browser and HTTP GET calls can run with
the saved agent's GET permission and allowed domains. A nonempty list restricts
live access to exact listed browser, HTTP or integration calls; unlisted calls
cannot fall back to live access. POST and integration
calls always require an exact list entry and the saved agent capability/assignment
for live execution. Direct HTTP also requires the agent's configured domain policy, shared
with both production HTTP wrappers (migration `0070_ai_profile_http_allowed_hosts.sql`).
Methods, URLs and parameters come from saved instructions/tool descriptions and
scenario calls; no customer-specific endpoint list is compiled into the server.
GET and POST permissions are independent. Live API calls may modify external data,
including POST calls. Shell commands never execute live. Integration
authentication uses the existing scoped executor and
token redaction; redirects/private destinations retain production protections.

Tests invoke the actual model, so fixed API responses do not guarantee identical
model wording. Notify/resolve/timer actions have isolated successful defaults and
can be given prepared failure responses.

Launch requests need only `scenario_id`. A legacy `mode` field from cached clients
is accepted and ignored; it does not change execution. The existing database
column is retained for compatibility and new runs store `live_read_only` there.

## Evidence and versions

Runs retain scenario revision, instructions/public identities, published article content and versions, provider/model settings, integration definitions, credential digests, Inbox routing settings, and context/tool implementation fingerprint. Articles are fetched from that snapshot only when the normal article tool requests them; the whole KB is never appended to the model prompt.

The trace shows article reads/versions/chunks, calls, responses and action order. Secrets and model reasoning fields/blocks are redacted before display/persistence. Full trace/snapshot is loaded on demand. Historical scenarios remain after deletion. Concurrent edits use revision checks. Results become stale when saved configuration, knowledge, scenario, routing or tool code changes.

The used-instructions editor keeps the run's immutable text beside the current saved text.
`GET /api/v1/ai/profiles/{profile_id}/test-runs/{run_id}/sources/{source_key}` returns
`{text, revision}`; article sources also return their current `title` and `status`.
Source keys are `instructions`, `tool_instructions`, or `article:{article_uuid}`.
`PATCH` to that path accepts `{text, expected_revision}` and returns the saved text
and new revision. A concurrent edit returns 409 rather than replacing newer work.
These explicit saves update only the chosen profile instruction field or article body;
article versions increment while publication status and other fields are preserved.
Published article edits affect subsequent runs; drafts still require publication.
Runs and recommendation generation never save source changes. The recommendations panel shows
each exact replacement as **Было → Станет**, naming the main instructions, tool instructions,
or article with its ID and version. These replacements use the current source text even when
the diagnostic evidence comes from an older run. **Применить рекомендации** saves the displayed
replacements only after the operator clicks it. Manual scenario/runtime advice without an exact
replacement is excluded. The button stays disabled when there are no exact replacements or
the source editor contains unsaved manual changes.

`POST /api/v1/ai/profiles/{profile_id}/test-runs/{run_id}/recommendations/apply` accepts
`{changes: [{source_key, expected_revision, original_text, suggested_text}]}` with 1–5 changes.
It verifies every revision and unique original fragment, rejects overlapping replacements,
and saves the entire batch in one transaction. A conflict changes nothing; the operator must
refresh recommendations and review the new replacements. Success returns
`{sources: [{source_key, text, revision, title?, status?}]}` and refreshes the open editors.
Other source text, profile fields, article publication state, and historical runs are preserved.

Source reads, manual saves, and recommendation application require AI management and complete
profile Inbox access. Profiles used by autonomous tasks or API endpoints require project-wide scope.
Article access additionally requires knowledge management and the existing knowledge-base
dependency scope. The article must still belong to an active base assigned to this profile
and have a successful recorded read with matching article ID and version in the run.
Removing the assignment or the required permission revokes current-source editing.

OpenClaw uses the same dedicated `openclaw/tz` route as production. The configured model, response model, and local runtime model configuration/digest are recorded when readable. If the runtime configuration is unavailable, the snapshot explicitly says so; changes hidden behind an upstream model alias cannot be reliably detected from that alias alone.
