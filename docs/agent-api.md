# Agent API

An AI agent can expose named HTTP methods. Each method stores an instruction,
example input, and example output. A caller supplies input data; the worker runs
the saved task with the agent's configured model, knowledge, and tool permissions.
The API returns a validated JSON result.

## Configure an agent

1. Open **AI → Agents**, save the agent, and open **API**.
2. Enable API access and add a method.
3. Set its name, URL slug, task instructions, input example, output example, and
   execution timeout. Choose **Response mode**: **Wait for a fresh result** or
   **Return saved result**. In live mode, configure **Response wait, seconds**
   (0–300). New methods in the editor suggest 180; existing methods keep 20.
   Save the method before testing it.
4. Use **Test** to submit an input and inspect its result or error.
5. A project administrator using a browser session can create an invocation key.
   Copy it immediately: only its prefix is displayed afterward. Store the key on
   the calling server, outside browser bundles and public URLs.

API configuration, test calls, and history require project-wide `ai:manage`.
Inbox-scoped identities cannot manage an agent's API. Invocation keys grant only
access to their agent's methods and calls made with that key; they are not operator
access tokens. Rotating or revoking a key invalidates the previous key.

For browser-based work, configure the agent's OpenClaw provider, HTTP GET access,
allowed domains, and tool instructions. The API switch does not grant additional
tools or network access. Tool permissions and allowed hosts are captured when
a run starts; edits apply to future runs. Disable API access or revoke the key
to cancel an existing API run. Other explicitly configured agent capabilities remain
available to the saved task. Calls do not create an Inbox conversation.

## Example: check an address

The feature is generic: cryptocurrency checks are one possible task. For a SOL
checker, choose a slug such as `check-sol-address`, then define the exact meaning
of “empty” in the task. Checking a native balance, checking token holdings, and
checking transaction history are different task definitions.

Example task:

> Check the supplied Solana address using the source described in this agent's
> tool instructions. For this method, empty means the native SOL balance is zero.
> Return address_empty as a boolean. If the address is invalid, the source is
> unavailable, or the relevant data cannot be verified, report a verification
> failure rather than guessing a result.

Input example:

```json
{"address": "SOL_ADDRESS"}
```

Output example:

```json
{"address_empty": true}
```

Examples define the required keys and JSON types, not constant answers. All
object properties in an example are required, and additional properties are
rejected. Nested objects and arrays are supported; an array example needs an
item from which to infer its element type. A boolean example accepts both `true`
and `false`, but not the string `"true"`.

## Call a method

The public route is `POST /{agent_id}/api/{slug}`. Replace the sample agent ID,
invocation key, and address below. Use a new idempotency key for each logical
operation and retain it when retrying that same operation.

```bash
curl --request POST \
  'https://tz.io/AGENT_ID/api/check-sol-address' \
  --header 'Authorization: Bearer INVOCATION_KEY' \
  --header 'Content-Type: application/json' \
  --header 'Idempotency-Key: 7b792dd9-2ee5-4f42-b7e2-989070598209' \
  --data '{"address":"SOL_ADDRESS"}'
```

The request body contains only the configured input. The caller cannot replace
the saved task, model, permissions, or result format. Reusing an idempotency key
with different input returns a conflict.

Live calls wait through queueing and execution, then return
`200 application/json` with the result itself:

```json
{"address_empty": true}
```

Public POST calls never return `202`, `pending`, or a polling URL. Legacy
`response_wait_seconds` and `wait_seconds` values do not shorten this wait.
Execution failures return `502`, execution or queue timeouts return `504`, and
cancellation returns `409`, with an `error` object and `run_id`. The worker must
be running to process runs and enforce their deadlines. Client and reverse-proxy
timeouts must allow for both queueing and execution. If the connection is lost,
retry the same input with the same idempotency key to reuse the existing run.

The newest 100 calls appear in the editor; run results and idempotency records
are retained for seven days. The run history endpoint remains available for diagnostics.

Each run records its endpoint definition at submission time. Editing a method
does not rewrite an already submitted task or its expected output. Changing
the input or output structure changes the caller contract; use a new method slug
when existing integrations must keep the previous contract.

## Cached results (for example, USD/EUR)

Select **Response mode → Return saved result** in the method editor and set **Refresh interval,
seconds** (60–86400; the editor defaults to 300). Save the method. The agent,
provider, API and method must be active, with an invocation key configured.
The worker starts the first refresh automatically; it must remain running.

For a fixed USD/EUR method, save `{}` as the input example. The worker uses this
saved JSON for every refresh, and callers must submit exactly the same JSON.
Different input returns 422 instead of another currency pair's cached result.
Only enable this mode for tasks intended to run repeatedly on a schedule.

```bash
curl --request POST 'https://tz.io/AGENT_ID/api/usd-eur' \
  --header 'Authorization: Bearer INVOCATION_KEY' \
  --header 'Content-Type: application/json' \
  --data '{}'
```

After the first successful refresh, the response immediately returns HTTP 200:

```json
{
  "result": {"from": "USD", "to": "EUR", "amount": 1, "rate": 0.85},
  "updated_at": "2026-09-16T12:00:00Z",
  "stale": false
}
```

The number and timestamp above are illustrative. `result` contains the actual
validated agent output. `updated_at` is the last successful refresh completion
time, not the source's market timestamp (keep `rate_time` in the agent result if
needed). `stale` is true when that refresh is older than the configured interval.
Refresh scheduling shares the existing worker queue and capacity, so it can be
late. A failed refresh preserves the previous result and timestamp. Clients must
check freshness according to their needs; there is no automatic expiry of the
last successful snapshot.

Before the first success, the API immediately returns HTTP 503 with error code
`cache_warming` and `Retry-After: 5`. Reads never enqueue work and need neither
polling nor an idempotency key; `wait_seconds` does not delay them. The worker
attempts the next refresh at the interval even after a failure. At most one
pending/running cache refresh per method is admitted.

Editing a method clears its snapshot and schedules another refresh. Key rotation
requires a fresh snapshot under the new key. Disabling API access, the method,
agent, project or provider prevents refreshes and access to cached results.
**Test method** remains a separate live test and does not publish into the cache.
Refresh runs appear in the normal run history. The saved snapshot is independent
of the seven-day run-history retention.

## Execution and failures

The API process persists a run and the worker executes it. Both processes must
be running. Closing the caller's HTTP connection does not create a second run
or discard its history. Admission is limited to 30 new calls per minute per agent and 100 outstanding
calls per project. Replaying an existing idempotency key does not consume a
new admission slot. API calls, cache refreshes, scheduled tasks, team steps and conversation
replies share `worker.ai-run-concurrency` across worker processes (default 2, range 1–16).
Each profile's `max_concurrent_runs` (default 2, range 1–16) also applies. Configure it in
**Agent → Settings → Parallel runs**. Calls above either limit remain pending without consuming
an execution attempt. Changing a limit affects new admissions; existing runs finish normally.
Conversation replies additionally reserve the conversation so its messages never execute together.
Cancelled work retains its capacity until the runtime grant is revoked and execution finalizes;
bounded leases recover capacity after a worker crash.

Queue waiting is bounded to ten minutes. The configured execution timeout is
between 10 and 300 seconds, with a default of 180 seconds. A failed or interrupted call
is not automatically repeated, because the agent may already have performed a
configured action. Use idempotency keys to recover the original run after a
client-side network interruption.

Successful results are parsed as JSON and checked against the stored output
schema. Oversized or malformed model output fails instead of being truncated
into an apparent success. A failed verification, inaccessible page, or unavailable
provider is an execution error, not a positive or negative business result.
Admin test calls can execute a saved method before public API access is enabled
and without an invocation key. They still require an active agent, project, and
compatible provider.

Schema validation verifies structure; task correctness still depends on the
configured procedure and the evidence obtained by the agent. Test representative
positive, negative, and unavailable-source cases before connecting a caller.

The public API never returns raw provider errors, task instructions, provider
credentials, or runtime grants. Disabling API access or a method prevents new
calls and cancels outstanding work. Already performed external actions cannot
be undone by cancellation.

## Deployment

Apply migrations through `0116_ai_api_response_wait.sql`, update the API and worker together, and build the admin
frontend. The supplied Nginx configuration includes the agent URL prefix; a
deployment using a custom proxy must also forward `/{agent_id}/api/` to the
backend instead of the SPA fallback. Browser-based tasks additionally require
the separately configured OpenClaw browser runtime.

The supplied Standard and Lite HTTPS templates allow 330 seconds on agent API
routes, leaving headroom for the maximum 300-second response wait. Apply the
updated Nginx template separately from application deployment, run `nginx -t`,
and reload Nginx. Client HTTP timeouts and every upstream proxy must exceed the
selected wait. A client timeout does not cancel the durable run; retain its
idempotency key when reconnecting.

If the domain is proxied through Cloudflare, its current default
[Proxy Read Timeout is 125 seconds](https://developers.cloudflare.com/fundamentals/reference/connection-limits/).
Use a shorter wait with margin (for example 110 seconds), or configure an
appropriate supported ingress for longer waits. Increasing Nginx's timeout
alone does not change Cloudflare's limit. Production ingress must be checked
separately; local tests do not establish its deployed settings.

Management routes live under `/api/v1/ai/profiles/{profile_id}/api`; the complete
static contract is in `backend/openapi/openapi.yaml`. Endpoint-specific input
and output schemas are returned with each saved method.
