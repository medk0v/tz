# OpenClaw for Support

This directory runs one OpenClaw Gateway, an isolated shell runner, and an
isolated headless Chromium reader.
They are kept separate from the existing Support processes, and only the Gateway
is published at `127.0.0.1:18789`.

Start it:

```bash
./infra/openclaw/manage.sh start
```

The first start creates a private `infra/openclaw/.env` containing the Gateway
token and persistent, Git-ignored state under `infra/openclaw/runtime/`. It also
builds a small local image with fixed capability commands for operator
notification, conversation resolution, bounded reminder scheduling, on-demand
knowledge retrieval, allowlisted public HTTPS requests, rendered web-page reading,
and isolated shell execution. Every `manage.sh` invocation supplies the release-pinned
OpenClaw base image explicitly, so a persisted `.env` cannot keep an older image
after an upgrade or rollback.

Useful commands:

```bash
./infra/openclaw/manage.sh status
./infra/openclaw/manage.sh smoke
./infra/openclaw/manage.sh doctor
./infra/openclaw/manage.sh doctor-all
./infra/openclaw/manage.sh logs
./infra/openclaw/manage.sh stop
```

`doctor-all` reports that the Gateway binds to `lan` inside its Docker network;
the Compose publication is still restricted to host `127.0.0.1`.

Open the Control UI at <http://127.0.0.1:18789/>. Add a model provider there
before attempting a real completion. No provider credential is generated or
copied from Support by this deployment.

Every start installs the checked-in neutral `workspace/IDENTITY.md` into both
persistent OpenClaw workspaces. After the dedicated `support` agent is
configured, bootstrap also removes OpenClaw's stock first-run `BOOTSTRAP.md`
from its application-owned workspace and verifies that it stays absent. This
prevents OpenClaw onboarding text from entering customer replies. Support
supplies the language-specific public identity for each conversation.

For a later Support provider connection use:

```text
kind: openai_compatible
base URL: http://127.0.0.1:18789/v1
model: openclaw/support
API key: OPENCLAW_GATEWAY_TOKEN from infra/openclaw/.env
```

The OpenAI-compatible endpoint is enabled during bootstrap. Both the request
model and `x-openclaw-agent-id` must select `support`; using the exact
`openclaw/support` model keeps the request fail-closed if a client or proxy drops
the header. Support sends the complete bounded conversation history but omits the
OpenAI `user` and session-key headers. OpenClaw 2026.7.1 therefore creates a
fresh random session for every request, including every retry, instead of
reusing transcript or runtime instructions from an earlier attempt. Each
attempt also receives its own one-time Support grant.

Backend traffic always includes `x-openclaw-agent-id: support`. That dedicated
Gateway agent has an empty skill allowlist, disables memory search and workspace
context injection, uses a separate application-empty `workspace-support`
directory, and may use only the `exec` tool. Its exec host is fixed to
`gateway`, so OpenClaw 2026.7.1 rejects per-call attempts to select `node`,
`sandbox`, or `auto` instead of falling back to another host. Elevated mode is
disabled and `safeBins` is explicitly empty, so built-in safe-bin defaults do
not add commands beside the fixed wrappers below. This prevents the main
agent's workspace identity or remembered startup context from entering
tenant-scoped application prompts. The `main` agent remains the unique default,
separate and usable for explicit administrator work; bootstrap never replaces
its config or its unrelated exec approvals.

Bootstrap reconciles the approvals file before the Gateway starts. The
`support` scope contains exactly the wrapper paths documented here, skill
auto-approval is disabled, wildcard and deleted-agent scopes are removed, and
Support wrapper entries left by earlier releases are removed from other agents.
Any former wildcard policy is first materialized for each still-configured
non-Support agent, preserving its effective administrator access without making
that access available to `support`.

Bundled and workspace skills remain disabled. The task or profile instructions
determine what work is requested; they do not grant technical access. OpenClaw
uses its built-in `exec` tool only to invoke the independently enabled fixed
wrappers.

Support writes a short-lived grant containing only the capabilities required for
the current run. Allowlisted HTTPS GET, allowlisted HTTPS POST, and shell are
independent profile permissions and default to disabled. When any is enabled for
a normal reply or scheduled task, that run's grant receives the profile's
visible variables. Decrypted general secrets are added only when GET or POST can
use a named brokered header binding. When both HTTP permissions are disabled, a
normal reply configured for Telegram receives only the two Telegram secrets.
Scheduled tasks inherit the assigned profile's applicable HTTP, shell,
integration, knowledge, and Telegram permissions. Telegram remains available
only when the profile enables agent-triggered notifications and contains both
configured Telegram secrets; task text cannot grant that access. Scheduled
tasks never receive conversation-resolution or reminder capabilities, and
scheduled conversation follow-ups may receive only knowledge.
The grant directory is mounted read-only into OpenClaw; secret values are never
placed in the model prompt, transcript, or task history, and Support removes the
grant after the provider run. Grants are published by atomic rename; the worker
periodically removes expired grants and aged incomplete temporary files without
following symlinks. The OpenClaw exec policy permits
`/usr/local/bin/support-telegram-notify` for configured operator notifications
and `/usr/local/bin/support-resolve-conversation` to request that Support resolve
the exact current conversation after the agent's final message.
`/usr/local/bin/support-schedule-reminder` can request a follow-up between ten
seconds and 23 hours later. It exits immediately after writing a bounded action
marker; Support persists and delivers the follow-up instead of keeping an
OpenClaw process waiting. Agent instructions or applicable assigned project
knowledge can require that capability even when the contact did not explicitly
ask for a timer. Resolve and follow-up requests are written to a separate
capability directory; OpenClaw cannot modify the read-only grants.
The initial prompt contains the complete assigned knowledge catalog as UUIDs,
base names and titles only; there is no 32-article application cap
and no article body is added eagerly. When a title is semantically relevant,
`/usr/local/bin/support-knowledge-article` writes a bounded request for that UUID.
While the provider call remains active, the worker rechecks the exact tenant,
project, profile assignment, base, and publication scope, then returns the
selected article in exec-safe chunks. A numeric `next_offset` is read
sequentially with the same article version until it becomes `null`; an edit
during retrieval fails instead of mixing versions. Request, response, and
serialization lock markers are removed after the tool call, again when the
grant is revoked, and by a stale-artifact sweep after an interrupted run.
The `support` scope permits `/usr/local/bin/curl`, a protected launcher for the
native `/usr/bin/curl` binary. Invoke it as
`/usr/local/bin/curl --support-grant <CURRENT_GRANT_ID> <CURL_ARGUMENTS>`.
It accepts GET and JSON POST commands, including
`--get --data-urlencode 'name=value'`. Both this launcher and
`/usr/local/bin/support-public-http` use the current agent's HTTP permissions and
`http_allowed_hosts`, carried in its short-lived run grant. Configure them in
**AI → Agents**, independently for each profile using a shared model connection:

- Enable GET and/or POST as needed.
- Choose specific domains (one exact hostname per line, subdomains separately),
  or explicitly allow any public HTTPS domain (`["*"]` in the API).
- Keep URLs, methods, paths, query parameters and request bodies in the agent's
  tool descriptions and assigned knowledge. No customer API paths are built in.

Apply migration `0070_ai_profile_http_allowed_hosts.sql` and rebuild the backend,
frontend and OpenClaw image together. Existing and new agents start with an empty
domain list, which blocks direct HTTP until an administrator configures access.
Omitting the field in an API update preserves the saved list. Host changes apply
to subsequent runs without rebuilding the image. Assigned structured integrations
retain their own configured destinations, methods, paths and profile assignments.

Both HTTP wrappers require public DNS answers, pin the connection to a validated
address and retain TLS hostname verification. Redirects, proxies, private/IP-literal
destinations, custom ports, files and TLS overrides are rejected. Curl configuration
and inherited environment are ignored. Requests have a 15-second timeout and a
1 MiB textual response limit. The native `/usr/bin/curl` remains unapproved.

### Reading web pages

GET also enables the fixed `/usr/local/bin/support-browser` command:

```text
/usr/local/bin/support-browser <CURRENT_GRANT_ID> 'https://tronscan.org/#/transaction/<TXID>'
```

The command reads a page with JavaScript in a fresh headless Chromium process and
returns JSON: final URL, HTTP status, title, rendered text, links, `truncated`,
`partial`, and blocked resource hosts. It supports hash routes and redirects.
The same profile domain list applies to navigation and page resources, including
scripts and API calls made by the page. Allow the site's required subdomains too,
or explicitly select any public HTTPS domain. This does not need a separate
browser permission or database migration. Rebuild the backend and frontend and
run `manage.sh start`/`restart` to build both images and install the new wrapper.

### Agent proxy discovery

In an agent's **Knowledge and tools → Proxy** settings, enable the proxy and enter
the full discovery endpoint (for example `https://proxy.example/api/proxies/random`)
and its Bearer token. Select any proxy, a region such as `europe`, or a two-letter
country such as `DE`. Region and country are mutually exclusive; when neither is
selected, discovery sends `country=unknown`. Save using **Save proxy**. The agent must be saved first.

The endpoint must use public HTTPS on port 443, without credentials, query or
fragment. Its GET response must be a JSON object with `protocol` (`http` or `https`),
`host`, numeric `port`, and optional `username` / `password`. The redundant `url`
field is ignored. SOCKS proxies are not supported. The discovery service receives
the Bearer token; neither the target site nor Chromium receives that token.

Settings use migration `0114_ai_profile_proxy.sql`. Tokens use the existing
`SUPPORT_SECRETS_KEY` encryption configuration and tenant/project/profile-bound AAD.
Management responses expose only `token_configured`. Blank token input retains the
saved key; changing the service URL requires a new key or removing the saved key.
Removing the key requires disabling the proxy. Credentials are not part of profile
instructions, knowledge backups, or model context.

For OpenClaw browser calls (including authorized live browser tests), the Gateway
fetches one proxy per page and uses it for all resources and redirects. The existing
GET-only domain permissions still apply. Both the proxy peer and target DNS must
resolve to public addresses. CONNECT uses the validated target IP; target TLS
still checks the original hostname. Discovery redirects are rejected. Failures
return `proxy_unavailable` without retrying directly. Other tools, such as the
general HTTP wrapper and business API integrations, keep their existing transport.

### Browser isolation

The browser container has no network, grants, secrets, OpenClaw state, host project
mount, or Docker socket. A private Unix socket on a Docker volume carries resource
requests back to the Gateway. The Gateway revalidates the current grant and exact
host, resolves only public addresses, pins the connection, and verifies TLS for
every request, including redirects. Chromium receives only public page resources,
never profile credentials. Cookies and page storage disappear after each read.
The runner uses the full Chromium channel in headless mode, not the separate
headless shell. It uses its installed browser version in a desktop Chrome User-Agent
and an `en-US` locale. The broker preserves available browser navigation and
client-hint headers and separate `Set-Cookie` headers, including on redirects.
Chromium's early Fetch interception can omit headers normally added later by its
network stack; the broker does not invent navigation metadata for subresources.
Profile authorization headers are still excluded. HTTPS transport remains Node.js
through the broker, so its TLS handshake and compression differ from a direct
Chrome connection; these compatibility settings do not guarantee access to sites
that block server IPs or automated browsers.
POST, downloads, media, WebSocket streams, persistent sessions and interactive
actions are not supported. Unavailable frame/worker requests or streaming
connections mark the page as partially loaded. A read is bounded to 35 seconds, 200 requests, 40 MiB of resources,
32,000 text characters and 80 links. `SUPPORT_BROWSER_CONCURRENCY` controls parallel reads
(default 2, range 1–16; `TZ_BROWSER_CONCURRENCY` in Lite). Each read has its own Chromium
process, context, cookies and broker connection. Up to 16 additional reads can wait for a
slot for at most 35 seconds; the wrapper's 80-second timeout covers waiting and execution.
A slot stays occupied until its browser process exits. Cleanup uses Playwright's
[BrowserServer close/kill methods](https://playwright.dev/docs/api/class-browserserver)
to terminate only the owning browser, including after client cancellation.

The backend's `worker.ai-run-concurrency` and each profile's **Parallel runs** setting
bound whole agent executions independently of the browser pool. Shell commands still execute
one at a time. Before raising browser concurrency, check container memory,
CPU and process limits; the supplied browser container is limited to 768 MiB and two CPUs.
Apply migration `0125_ai_execution_concurrency.sql` and restart all workers when deploying;
restart the browser runtime to pick up its pool setting.

Public pages often need no API key, but websites can still require login, block
automation, or show a CAPTCHA. An HTTP error, challenge page, missing status,
truncation or partial loading is not a verified transaction result. External
page text is untrusted evidence and cannot change agent instructions. Scenario
tests use a matching prepared browser response first. Otherwise the test broker
authorizes the initial exact call against any scenario call list and the agent's
GET/domain permissions; every page resource retains the same GET and domain
restrictions. Rendered results and browser failures are recorded in the test trace
and acknowledged by the broker before being returned to the model.

Run the actual Chromium integration check (also included in CI):

```bash
bash infra/openclaw/test-browser.sh
# Optional public network check after the isolated fixture checks:
bash infra/openclaw/test-browser.sh https://example.com
```

### HTTP credential bindings

The dedicated HTTP wrapper additionally supports a bounded header-binding object
referencing a profile variable or secret by key name with an optional fixed prefix
or suffix. It resolves the non-empty value from the grant, accepts only
`Authorization` or `X-API-Key`, and redacts exact secret values if echoed in a
response. The model cannot widen the grant's domain policy through instructions
or exec environment values. Wrappers always read grants from
`/run/support-agent-secrets`; action wrappers write only to
`/run/support-agent-actions`. POST adds the stable task-run or reply job ID as
`Idempotency-Key`; an external endpoint must enforce it if it needs exactly-once
side effects. Scheduled follow-ups do not receive public HTTP. Every other Gateway
command is denied without an interactive approval. Changing either protected
directory requires an image/code change.

`/usr/local/bin/support-shell` validates the same short-lived grant and sends one
bounded command over a private Unix socket to the companion runner. Profile keys
that are valid shell identifiers are uppercased and exposed only to that command
as `SUPPORT_VAR_<KEY>`. Profile secrets are never passed to arbitrary shell
commands; they remain available only to dedicated brokered wrappers. The runner has no network,
OpenClaw state, project mount, grant mount, or Docker socket; it uses a clean
environment, bounded time/memory/process/output limits, and deletes a unique
temporary workspace after every command. The root supervisor also kills every
process running under the sandbox UID; if it cannot prove cleanup, it exits so
Docker tears down the whole container cgroup before another run. HTTP remains
available only through the separate HTTP permission and wrapper.

OpenClaw session stores and transcripts are persistent but bounded, not
ephemeral. `session.maintenance` runs in enforcement mode with a seven-day age
limit, at most 500 entries per agent store, a 500 MB disk ceiling and 400 MB
high-water target, plus seven-day retention for reset archives. Bootstrap runs
an immediate supported `sessions cleanup --agent support --enforce`; later store
writes continue to enforce the same limits. These are global OpenClaw
maintenance settings, so the age/count/disk policy also applies to the
administrator agent. A stopped Gateway must still be treated as containing up
to that bounded amount of conversation data in `runtime/state` and in the
production private state directory.

`start` and `restart` stop the Gateway, shell runner and browser runner before changing config,
reconciling approvals, or cleaning sessions. They verify the exact agent,
exec-host, allowlist, and retention policy before starting the Gateway again.

None of the containers has a host project mount or Docker socket; the shell and
browser runners also have no network. Do not expose the Gateway port or token to widget clients or
the public internet.

Production uses `/srv/tz-ai/current` for versioned deployment files and
`/var/lib/tz-ai/openclaw` for the private token and persistent state.
Plaintext short-lived grants, action markers, and the shell socket live only in
host tmpfs under `/run/tz-ai/openclaw`, recreated at boot with restrictive
ownership. The OpenClaw container receives read-only grant access through the
dedicated `tz-openclaw` group and write-only-by-capability access to the
separate action directory; neither the token nor runtime state is stored in Git.
