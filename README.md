# the workspace Customer Communication Platform

the workspace is a multi-tenant customer support platform with a Rust backend, a React operator workspace, and an embeddable Preact chat widget.

The current implementation includes:

- password and scoped access-token authentication;
- tenant, project, operator, Inbox, and channel administration;
- [project departments](docs/departments.md), customizable department menus, scoped employee access, and a director overview;
- [project notes](docs/notes.md), nested Markdown pages, typed tables with relations and saved views, and revocable public links;
- [task duration and reminders](docs/task-duration.md), start and end of a task, all-day spans, its own time zone and a reminder before it starts (standard edition);
- [company projects](docs/company-projects.md), company profiles, reporting lines, and organizational charts of employees and AI agents;
- [project appearance](docs/project-appearance.md): project logos for the light and dark theme and project colors that replace personal preferences (standard edition only);
- [custom channel drafts](docs/custom-ai-channels.md) with a name, icon, AI-agent or API source, and a planned action in the workspace;
- widget conversations, replies, resolution cycles, ratings, and realtime updates;
- visitor and operator presence;
- live visitor drafts visible to operators;
- quarantined widget attachments with per-widget and per-conversation controls;
- configurable widget languages, launcher, light and dark palettes, border, and footer;
- project-scoped AI provider settings, profiles, exact channel assignments, localized public identities, OpenAI-compatible replies, and knowledge bases;
- [reusable agent skills](docs/ai-skills.md), Markdown instruction imports, and project-scoped skill assignments;
- [Telegram voice messages](docs/voice-messages.md) with speech-to-text and text-to-speech model connections and optional spoken agent replies (standard edition only);
- [phone calls](docs/telephony.md) answered by agents through Asterisk AudioSocket, with live transcripts and operator transfer (standard edition only);
- support quality reporting;
- encrypted project SMTP settings and queued email or Telegram post-chat rating invitations;
- multiple Telegram Bot API connections with Inbox routing and queued operator replies;
- PostgreSQL migrations, an outbox worker, Redis realtime fan-out, and OpenAPI.

Telegram Business accounts, general inbound/outbound email conversation channels, OIDC/SSO, and agent tool execution are not complete yet. The Telegram Bot API integration currently supports private text conversations and structured post-chat ratings. See [BACKEND_ARCHITECTURE.md](./BACKEND_ARCHITECTURE.md) for the system boundaries and security model.

## Repository layout

```text
backend/
  data/                    Local GeoLite2 City/Country databases
  src/bin/api.rs          HTTP, WebSocket, authentication, and public widget API
  src/bin/worker.rs       Outbox delivery, retention jobs, and worker health
  src/                    Domain and infrastructure modules
  migrations/             SQLx migrations
  openapi/openapi.yaml    API contract
  Config.example.toml     Configuration template
frontend/
  src/main.tsx            React operator workspace
  src/widget-main.tsx     Preact widget iframe
  src/widget-loader.ts    Embeddable loader
```

The backend is one Cargo package with two binaries. The frontend is one npm project with separate admin, widget, and loader build modes.

## Prerequisites

- Rust stable with edition 2024 support;
- Node.js 22 or later;
- npm;
- PostgreSQL 15 or later;
- Redis for realtime fan-out. Redis may be disabled for a single API process.
- a private ClamAV daemon for deployed environments that enable chat attachments.

The local configuration used in this repository expects:

| Service | Address |
| --- | --- |
| PostgreSQL | `localhost:5432` |
| Redis | `localhost:6379` |
| API | `http://localhost:18080` |
| Worker health | `http://localhost:18081` |
| Admin | `http://localhost:15173` |
| Widget iframe | `http://localhost:15174/widget.html` |
| Widget loader | `http://127.0.0.1:15175/loader/widget-loader.js` |

## First-time setup

Install frontend dependencies:

```bash
cd frontend
npm install
cd ..
```

Create the local backend configuration if it does not exist:

```bash
cp backend/Config.example.toml backend/Config.toml
```

For the ports documented above, use these listener settings in `backend/Config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 18080
allowed-origins = [
  "http://localhost:15173",
  "http://127.0.0.1:15173",
  "http://localhost:15174",
  "http://127.0.0.1:15174"
]
trusted-proxies = []

[worker]
host = "0.0.0.0"
port = 18081
ai-task-concurrency = 4
ai-run-concurrency = 2

[pg]
url = "postgres://tz:tz@localhost:5432/tz"
max-connections = 10
migrate-on-start = true

[redis]
url = "redis://localhost:6379"
required = false

[geoip]
city-database-path = "data/GeoLite2-City.mmdb"
country-database-path = "data/GeoLite2-Country.mmdb"

[attachments]
enabled = true
storage-path = "data/attachments"
scan-mode = "disabled"
scan-timeout-seconds = 60
max-concurrent-uploads = 4
max-conversation-bytes = 268435456
max-tenant-bytes = 5368709120
retention-days = 30
```

`backend/Config.toml` is ignored by Git because it may contain local credentials. Do not commit production secrets.
The included GeoLite2 files are read locally and lazily; visitor enrichment does not call an external geolocation service.

## Secure chat attachments

Attachments are disabled when the `[attachments]` section is absent. The local example above explicitly uses `scan-mode = "disabled"`; this skips only malware scanning and is intended for an isolated developer machine. In every deployed environment keep `scan-mode = "required"` and configure a private `clamav-address`; loopback and private-network addresses are accepted, while public scanner addresses are rejected at startup. Scanner errors and timeouts then fail closed: the upload is rejected and the quarantined bytes are removed. The ClamAV stream limit must accept the largest supported file (50 MiB).

The channel editor controls the default visitor permission. An assigned operator using a password session can override that permission for an active conversation from its chat header; the widget receives the change over realtime. Operators assigned to the conversation can also send a file from the reply composer.

The accepted formats and per-file limits are deliberately narrow:

- JPEG, PNG, or WebP images up to 10 MiB;
- PDF documents up to 20 MiB;
- MP4 or WebM video up to 50 MiB.

The backend requires matching filename extension, declared MIME type, and file signature, rejects suspicious filenames and oversized image dimensions, computes a SHA-256 checksum, then scans the complete stream before publishing it. SVG, HTML, archives, Office documents, and executable formats are not accepted. Files use server-generated UUID object names, remain `0600` while quarantined, become read-only `0400` after scanning, live in private `0700` directories, and are never served as static public paths.

Downloads re-check the widget contact or operator tenant/project/Inbox scope, verify the stored size and SHA-256 against the scanned object, and are returned with an allowlisted content type, `Content-Disposition: attachment`, `nosniff`, a restrictive CSP, and private no-store caching. Uploads require UUID idempotency keys, use bounded concurrency, allow at most 100 retained files per conversation, and enforce the configured conversation and tenant byte quotas. The worker expires and removes objects after `retention-days` (30 by default), and reconciles UUID-named quarantine or object files left unreferenced for more than 24 hours after an interrupted process.

For deployment, keep attachment storage outside the web root, run the API and worker under the same dedicated least-privilege storage identity (or an equivalent explicit ACL), isolate ClamAV separately, keep the storage volume private, terminate TLS before the API, and apply an identity-aware request-rate limit at the reverse proxy in addition to the application concurrency and storage quotas. Newly created storage directories use mode `0700` and published objects use `0400`; if a configured directory already exists with group or world access, startup fails instead of changing permissions on a potentially shared path.

The frontend reads local URLs from `frontend/.env.local`:

```dotenv
VITE_API_BASE_URL=http://localhost:18080
VITE_ENTRY_MODE=landing
VITE_WIDGET_LOADER_URL=http://127.0.0.1:15175/loader/widget-loader.js
VITE_WIDGET_FRAME_URL=http://localhost:15174/widget.html
VITE_WIDGET_API_BASE_URL=http://localhost:18080
VITE_SENTRY_DSN=
VITE_SENTRY_ENVIRONMENT=development
```

`VITE_SENTRY_DSN` is optional. When set, the admin application and embedded
widget report uncaught browser errors without default PII; URL query strings and
fragments are removed before events leave the browser. Backend API and worker
errors use the separate `SENTRY_DSN` process environment variable. See
`deploy/README.md` for production release and source-map configuration.

`VITE_ENTRY_MODE` controls the unauthenticated entry experience for each deployment:

- `landing` serves the public the workspace product page at `/` and the operator sign-in/workspace at `/cabinet`;
- `login` opens the neutral sign-in form directly and removes the the workspace name, logo, favicon, and branded page title from the direct entry and operator shell.

In `landing` mode the homepage remains available at `/` even when an operator is signed in. Marketing login buttons open `/cabinet`, and browser Back/Forward navigation keeps the routes in sync. Project workspaces use `/cabinet/p/<project-uuid>` for conversations and append the page name for other sections, such as `/overview`, `/tasks`, `/ai`, or `/projects`. Project links can be opened in separate windows; each window sends its project with API requests, and department selection and operator presence are stored separately for each project within the session. Legacy links such as `/cabinet/overview` still work and acquire the selected project's URL after sign-in. Configure the production web server to return the admin `index.html` for `/cabinet` and `/cabinet/*` as well as `/`.

Tabs, open records and creation forms inside a section have nested paths too, so reloading, sharing a link or using browser Back/Forward returns to the same view. Examples: `/conversations/<inbox-uuid>/<conversation-uuid>`, `/team/positions/<position-uuid>/kpi`, `/ai/agents/<agent-uuid>/instructions`, `/ai/providers/new`, `/knowledge-base/<base-uuid>/<material-uuid>`, `/channels/widgets/<widget-uuid>/appearance`, `/processes/<process-uuid>/versions/<version-uuid>` and `/tasks/calendar/<task-uuid>/runs`, each after `/cabinet/p/<project-uuid>`. A link to a record that no longer exists, or that the operator cannot open, falls back to the nearest page that exists. Pages with unsaved changes ask before Back/Forward or a sidebar link discards them. Older `/ai?agent=<id>` and `/knowledge-base?base=<id>` links open the same record at its nested path.

Use `landing` for a public sales deployment and `login` for an internal or white-label deployment. The operator features remain the same in both modes.

In the standard edition, **Channels → Widget → Appearance → Animate chat opening**
controls the opening animation for each widget (`theme.opening_animation`, enabled
by default). The chat unfolds from the launcher corner and a single stroke follows
its outline; the stroke is brighter in dark mode. Reduced-motion users see the
panel immediately. `VITE_PRODUCT_EDITION=lite` disables this effect and hides the
checkbox. Set the same edition when building the admin preview, widget, and loader;
the default is `standard`.
`frontend/.env.lite.example` contains build-time values for
`tz.cyber-money.org`. Lite opens the neutral workspace directly and hides project
selection and project management. Its backend uses one fixed internal project
configured with `[product] edition = "lite"` and `project-id`; standard deployments
keep their existing multi-project behavior. See [Lite deployment](deploy/lite/README.md)
for the manual GitHub workflow and deployment of prebuilt ARM64 artifacts.

Project administrators using a password session can open **Administration → Users**
at `/cabinet/users` to create an account with a name, email, password, and project
role. They can also grant existing accounts access, change roles and department
access, edit public chat profiles, and revoke project access. Creating an account
does not alter an existing account with the same email. Choose the `admin` role
for full project access; custom permissions are configured through roles.

Project administrators can create, rename, configure, assign, and delete project
roles from **Administration → Roles & permissions**. A role keeps either the
manager or operator Inbox scope while its visible permissions remain editable.
Updates apply to current password sessions and existing project access tokens.
The system `admin` role is the only role with full access and the only role that
can manage roles; it cannot be edited, deleted, or assigned to an access token.
A role cannot be deleted while it still has active members or tokens. Access
tokens always exclude project administration, token administration, and visitor
network intelligence even when the corresponding password-session role has
those permissions.

Access tokens can cover one selected project or all active projects in the
tenant. Only an administrator with tenant-wide access can issue or manage an
all-project token. The selected role determines module permissions; an all-project
token keeps using that role from the project where it was issued, including later
permission changes. A disabled role-source project disables its tokens.
The cabinet offers a project chooser for all-project tokens. API clients select
a project with `X-Tz-Project-Id`; omitting it selects the role-source project.
A single-project token rejects another project in this header. Existing tokens
retain their Inbox restrictions and are not upgraded to all-project tokens.

## Database migrations and development data

The API applies every pending migration on startup when `migrate-on-start = true`.

To run and inspect migrations manually:

```bash
cd backend
cargo sqlx migrate run --database-url postgres://tz:tz@localhost:5432/tz
cargo sqlx migrate info --database-url postgres://tz:tz@localhost:5432/tz
```

Apply the optional development seed after migrations:

```bash
psql postgres://tz:tz@localhost:5432/tz \
  -f backend/dev_seed.sql
```

The seed creates local-only sample data and an administrator account. Its credentials are defined in `backend/dev_seed.sql`; do not reuse them outside local development.

## Start the complete local stack

Keep PostgreSQL and Redis running. The recommended development command starts all five the workspace processes and stops them together on `Ctrl+C`:

```bash
./dev.sh
```

The script checks the required tools, `backend/Config.toml`, frontend dependencies, PostgreSQL, Redis, and ports `18080`, `18081`, `15173`, `15174`, and `15175`. It builds the widget loader before starting the processes. It never terminates a process that was already using one of those ports.

For individual logs or debugging, start each process in a separate terminal as described below.

### 1. API

```bash
cd backend
cargo run --bin api -- --config Config.toml
```

Starting the API also applies pending SQLx migrations.

### 2. Worker

```bash
cd backend
cargo run --bin worker -- --config Config.toml
```

The worker processes the transactional outbox, executes due AI task schedules, and automatically
resolves conversations after 24 hours of general inactivity. When the latest message is an
operator reply, it waits one hour, uses a compatible AI profile to generate a brief localized
closing message under the operator's identity, sends the rating prompt, and resolves the
conversation. New customer activity cancels that pending close. The worker also removes expired
visitor observations.
Every minute it also checks operator-owned conversations awaiting a response to the customer. After
30 minutes without an operator response, it transfers the conversation to an eligible AI and
queues a reply to the latest unanswered customer message. The window starts at the first
unanswered customer message or the latest assignment/reopening, whichever is later; customer
follow-ups and reading the chat do not reset it. If no compatible AI is available, the operator
stays assigned. Administrators can also take over another operator's chat or transfer it directly
to AI using the conversation header actions.
Each assigned profile executes independently with its current profile instructions. Every run stores
its immutable task-text snapshot, status, bounded output or error in project task history only; it
does not send a customer, channel, or Telegram message automatically. Schedule times are due-at
targets rather than exact start guarantees. After downtime the scheduler materializes at most one
missed occurrence and advances from the current database time. If any run from an earlier occurrence
is still pending or processing, the next due occurrence is coalesced instead of growing a backlog.
Disabled projects pause materialization and claiming; pending work resumes after re-enable, while a
run that passed the final active-project check may finish. Separate durable per-project cursors
round-robin both due-definition materialization and run claims, and claims first favor projects with
fewer active runs. The cursors survive worker restarts, so one project's older backlog cannot
indefinitely starve another project. `worker.ai-task-concurrency` controls up to four task
claim loops. `worker.ai-run-concurrency` bounds all concurrent durable agent runs across worker
processes (API calls, scheduled work, team steps and conversation replies; default 2, range 1–16).
Each agent's **Parallel runs** setting adds its own limit (default 2, range 1–16). Excess work stays
queued; messages in the same conversation remain sequential. Capacity is released after execution
cleanup, or after a bounded lease expires if a worker crashes. Each project may store at most 500 task definitions. The newest 100 terminal runs
per task plus all pending or processing runs are retained.

Agents can also expose named API methods with saved instructions, typed JSON
input/output examples, a dedicated invocation key, and run history. API calls are
persisted and executed by the worker with the agent's configured permissions.
See [Agent API](docs/agent-api.md) for configuration, invocation, polling, and
deployment requirements.

### 3. Operator workspace

```bash
cd frontend
npm run dev:admin -- --port 15173
```

Open <http://localhost:15173>.

### 4. Widget iframe

```bash
cd frontend
npm run dev:widget -- --port 15174
```

The iframe is served from <http://localhost:15174/widget.html>. A standalone widget URL also needs a valid `widget_id` query parameter.

### 5. Embeddable widget loader

Build the widget and loader, then serve the complete `dist` directory:

```bash
cd frontend
npm run build:widget
./node_modules/.bin/vite preview --outDir dist --port 15175 --host 127.0.0.1
```

The loader is now available at <http://127.0.0.1:15175/loader/widget-loader.js>.

## Verify the running services

```bash
curl http://localhost:18080/health/live
curl http://localhost:18080/health/ready
curl http://localhost:18081/health/live
curl -I http://localhost:15173
curl -I http://localhost:15174/widget.html
curl -I http://127.0.0.1:15175/loader/widget-loader.js
```

API readiness reports PostgreSQL and Redis separately. The API and worker are different processes and both must remain running.

## Create and install a widget

1. Sign in to the operator workspace.
2. Open **Channels → Website widgets**.
3. Create a widget and select its Inbox.
4. Add the exact origin of the host website, for example `https://shop.example.com`.
5. Configure languages, the chat name, optional greeting, automatic invitation, launcher behavior, light and dark palettes, and rating text.
6. Save the widget.
7. Copy the generated installation code.

A local installation looks like this:

```html
<script
  src="http://127.0.0.1:15175/loader/widget-loader.js"
  data-src="http://localhost:15174/widget.html"
  data-api-base="http://localhost:18080/"
  data-widget-id="YOUR_WIDGET_ID"
  data-theme="light"
></script>
```

`data-widget-id` is public. Access is constrained by the channel's exact allowed origins and the backend's CORS configuration.

### Light and dark themes

Use `data-theme="light"` or `data-theme="dark"` when the host website has a fixed palette.

The widget editor also generates a complete automatic-theme example. It demonstrates how to:

- read `data-mode="dark"` from the host page;
- read a persisted `localStorage.darkMode` value;
- observe theme changes with `MutationObserver`;
- update the widget iframe theme without changing the workspace configuration.

Adapt the host-page checks to the website's actual theme implementation.

### Widget language

The loader resolves language in this order:

1. `data-language` on the loader script;
2. the host page's `<html lang>` value;
3. the widget's configured default language;
4. English or the first configured language as a fallback.

Language names are user-defined: an administrator may enter a standard tag such as `ru-MD` or any convenient identifier such as `Customer language`. Values are trimmed and matched case-insensitively, and the same identifier can be supplied through `data-language`. A regional tag such as `ru-MD` may still fall back to a configured `ru` translation.

### Automatic chat invitation

In **Channels → Website widgets → Launcher**, use the single automatic-invitation block to enable it, choose a delay from 1 to 300 seconds, and enter the text for every widget language. The timer runs only while at least one operator is online, restarting with the full configured delay when the chat changes from offline to online. The invitation appears only while the chat remains closed; it is hidden immediately if every operator goes offline. Opening or dismissing it prevents a repeat during that page load. The invitation is not stored as an operator or visitor message.

The same section configures the support or operator name shown in the header, greeting, and invitation. If the name is empty, the widget uses the localized generic name **Support**. Disable the greeting setting to open a new conversation with an empty message area and the composer only.

### Post-chat rating email

In **Channels → Email**, configure the project SMTP relay, TLS mode, sender, and the public rating-page URL, then enable rating invitations. SMTP passwords are write-only and encrypted with the key configured in `[secrets]`. Only implicit TLS and required STARTTLS are accepted.

When a human operator or the configured support agent resolves a widget conversation, the workspace queues one invitation for that resolution cycle if the contact supplied an email address. Empty or inactive automatic closures do not create an invitation. The worker retries temporary SMTP failures through the transactional outbox. If feedback was already submitted in the widget, the email page displays the existing completion state instead of accepting a duplicate rating.

The email URL uses `/rate-chat#access=<opaque-token>`. The frontend removes the fragment immediately and sends the token only in `X-Support-Rating-Token`; the API stores only its hash after successful delivery. Configure the production web server to serve the admin `index.html` for `/rate-chat`, just as it does for `/cabinet`.

## Connect Telegram bots

Set the public the workspace origin before starting the API and worker:

```toml
[telegram]
webhook-base-url = "https://tz.io"
```

Then open **Channels → Telegram**, choose an Inbox, enter a connection name and a fresh token from `@BotFather`, and connect the bot. The worker verifies the bot and registers its webhook. Each Telegram user who sends a private text message to that bot appears as a conversation in the selected Inbox; an assigned operator's reply is queued and sent through the same bot.

The worker monitors Telegram webhook delivery. After a sustained delivery failure it removes the webhook without dropping pending updates, temporarily drains messages through `getUpdates`, and periodically restores the webhook. Telegram permits only one receiving mode at a time, so polling is used only as an automatic fallback.

When a human operator or the configured support agent resolves a Telegram conversation, the workspace sends a native 1–5 rating keyboard. The selected value is stored in the same support-quality record used by widget ratings. The customer can then reply to the comment prompt or skip it; unrelated messages continue to start or reopen the normal support flow.

Repeat this for as many distinct bots as required. Every bot is an independent channel connection and may point to a different Inbox. A Telegram bot can have only one active webhook, so the same bot token cannot be connected to the workspace twice. Tokens are write-only, encrypted at rest, and must never be pasted into logs, tickets, or chat messages.

## Using the widget on an external website

Localhost URLs work only when the website is opened on the same computer as the the workspace development processes. They do not work for real visitors because `localhost` then points to each visitor's own device.

For an external website, deploy all three public assets behind HTTPS:

```text
https://support.example.com/loader/widget-loader.js
https://support.example.com/widget/widget.html
https://support.example.com
```

Then configure the admin build with public URLs:

```dotenv
VITE_API_BASE_URL=https://support.example.com
VITE_WIDGET_LOADER_URL=https://support.example.com/loader/widget-loader.js
VITE_WIDGET_FRAME_URL=https://support.example.com/widget/widget.html
VITE_WIDGET_API_BASE_URL=https://support.example.com
```

Also add the exact external website origin in both places:

- the backend `[server].allowed-origins` list;
- the widget's **Allowed origins** setting in the operator workspace.

Origins must contain only scheme, host, and optional port. Do not include a path, query, fragment, or trailing slash.

## AI secret encryption key

AI provider credentials, API integration tokens, agent secret fields, SMTP passwords, and pending rating-link tokens are encrypted before storage. Set the environment variable named by `[secrets].encryption-key-env` before starting the API when any of these features is used:

```bash
export TZ_SECRETS_KEY="$(openssl rand -base64 32)"
```

The local `./dev.sh` launcher creates this key once in the Git-ignored `backend/.env.secrets.local` file and reuses it on later starts. Keep that file private and do not regenerate it while stored provider credentials or agent secrets still depend on it.

Use a persistent secret manager in production. Generating a different key on every restart makes previously stored credentials unreadable. Agent custom fields are visible configuration, while secret values are write-only through the management API and never returned to the browser. Storing database credentials does not itself enable database access; an explicit, authorized backend tool is still required.

the workspace keeps encrypted provider connections for OpenAI, Anthropic, and OpenAI-compatible endpoints,
including base URL, API key, default model, profile override, and output limit. The
reply worker dispatches `openai` and `openai_compatible` connections through
`POST {base_url}/chat/completions`. Anthropic connections remain available as configuration but are
not dispatched by this compatibility path.

Project-wide AI tasks assign one to 32 active executable profiles to one-time or recurring text.
Each selected profile runs the text independently using its current instructions, and its output or
error is retained only in project task history. Tasks do not deliver results to a customer,
conversation channel, or Telegram automatically.
Direct scheduled-task providers must use HTTPS and resolve only to public network addresses; DNS
answers are checked and pinned for each execution to prevent rebinding to loopback, private,
link-local, metadata, multicast, or otherwise non-public destinations. The exact configured local
OpenClaw endpoint is the sole exception. OpenClaw-targeting model names fail closed unless that
configured endpoint matches, and all the workspace OpenClaw traffic selects the isolated `tz` Gateway
agent explicitly. Each profile independently enables allowlisted HTTPS GET, allowlisted HTTPS POST,
and isolated shell execution; all three capabilities default to disabled, and instructions cannot
expand them. When any is enabled, visible profile variables enter only the short-lived run grant;
decrypted secrets are added only for brokered HTTP header bindings (or an explicitly configured
Telegram action). The isolated, networkless shell process receives only shell-safe visible variables
as prefixed temporary environment variables and never receives profile secrets. the workspace never embeds
stored values in the model prompt or
conversation/task history. Scheduled POST retries reuse the run ID as an `Idempotency-Key`; the
receiving endpoint must enforce that key if duplicate side effects are unacceptable.

The **Integrations** page manages project-scoped external business APIs separately from the workspace
access tokens and model-provider connections. A connection stores an HTTPS base URL, optional
Bearer or `X-API-Key` token, one or more named GET operations, and explicit AI-profile assignments.
Operation paths may contain fixed snake-case placeholders in their path or query string. Tokens are
encrypted, write-only, and never enter the model prompt or OpenClaw grant. An assigned OpenClaw
profile receives only the connection/action catalog; when it selects an action, the backend
rechecks the current tenant, project, active assignment, exact parameters, fixed URL boundary, and
public DNS answers before adding authentication and issuing the request. Redirects, private or
metadata destinations, arbitrary URLs/headers/methods, binary responses, and write operations are
not permitted. External response bodies are bounded and treated as untrusted data rather than
instructions. A profile may have at most 32 API connections, and each connection may expose at
most 32 operations.

Assign an active profile to one or more exact channel connections and enable
**Automatically join new conversations** to add the profile only to conversations received through
those channels. Newly connected channels are not granted automatically. AI management lists only
channels inside the actor's active project and Inbox scope; an Inbox-scoped actor can manage only
profiles whose complete, non-empty channel assignment is inside that scope. Provider mutations are
project-wide and therefore require an actor without an Inbox scope. For every configured reply
language, the profile has a client-facing public name
and optional avatar; the widget selects that identity from the resolved session language, and the
reply worker supplies the same authoritative name to the model while the internal profile name
remains an administration label. The canonical `{{public_display_name}}` placeholder and legacy
`{{Имя для клиента}}` placeholder are resolved before the provider request. The profile joins only
when one of its configured languages is compatible with the resolved widget-session language. A
primary tag matches its regional form (`ru` and `ru-RU`), while different languages and different
explicit regional variants do not match. For example, a `ru` profile does not join an `en` widget
session, while `ru, en`
supports both. Missing, invalid, or incompatible language data fails closed: no AI participant or
provider request is created. Every inbound contact message handled by a compatible profile queues a
durable provider request; the reply is stored as an ordinary outbound AI message. the workspace sends an
`Idempotency-Key` with the triggering message UUID. Before calling the provider, the worker checks
the profile language again, so a queued reply is suppressed if the profile configuration changed.
If an operator joins while the request is running, the workspace rechecks ownership and suppresses a late
AI reply.

For a profile connected through the local OpenClaw adapter, the reply worker adds a metadata-only
catalog of every non-empty published article from every active knowledge base explicitly assigned
to that profile. There is no application-level article-count limit, and the catalog contains each
article UUID, knowledge-base name, and title but no body or source URL. It follows the
profile instructions, which remain authoritative. When a contact request may relate to a title,
including through a synonym or differently worded intent, the agent requests that article by UUID.
The worker then rechecks tenant, project, profile assignment, active/published status, and returns
only bounded, version-stable chunks of the selected body. Draft articles, archived or unassigned
bases, and records outside the current scope are never returned. Articles may contain reference
data, instructions, procedures, and response wording, used according to the saved profile
instructions. Article bodies are not placed into every conversation context.

## OpenAPI client generation

Regenerate TypeScript API types after changing `backend/openapi/openapi.yaml`:

```bash
cd frontend
npm run generate:client
```

The WebSocket endpoint accepts only short-lived, one-time realtime tickets. Never place a long-lived operator or widget token in the WebSocket URL.

## Production build

```bash
cd frontend
npm run build
```

Build output:

```text
frontend/dist/admin/
frontend/dist/widget/
frontend/dist/loader/widget-loader.js
```

Serve the admin, iframe, and loader from their configured public URLs. Run `api` and `worker` as separate supervised processes.

For HTTPS deployments set `cookie-secure = true` and configure only trusted reverse-proxy networks. Never use a wildcard trusted-proxy CIDR.

## Checks

Backend:

```bash
cd backend
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --all-targets
```

Frontend:

```bash
cd frontend
npm run lint
npm run test
npm run build
```

## Troubleshooting

### The admin page does not open

- Confirm that the admin Vite process is listening on port `15173`.
- Confirm that `VITE_API_BASE_URL` points to `http://localhost:18080`.
- Check `http://localhost:18080/health/ready`.

### The widget does not appear on a local host page

- Confirm that the loader returns JavaScript from port `15175`.
- Confirm that the iframe is running on port `15174`.
- Confirm that the API is running on port `18080`.
- Confirm that the script contains the correct widget ID.
- Add the host page's exact origin to the widget settings.

### The widget does not appear on an external website

- Do not use `localhost` or `127.0.0.1` in production installation code.
- Confirm that loader, iframe, and API URLs are public HTTPS URLs.
- Add the website origin to backend CORS and the widget's Allowed origins.
- Check the browser console and Network panel for blocked loader, iframe, CORS, or session requests.

### Widget settings open to a blank page

- Reload the admin page to load the latest frontend bundle.
- Check the browser console for a React runtime error.
- Run `npm run test:admin` before deploying admin changes.

### The API starts but messages are not processed

- Confirm that the worker is also running.
- Check worker health on port `18081`.
- Confirm PostgreSQL and Redis connectivity.

## Security and data handling

- PostgreSQL is the durable source of truth.
- Redis is non-authoritative realtime infrastructure.
- Messages and outbox events are committed atomically.
- Widget sessions use opaque tokens stored only as hashes on the backend.
- Visitor identity uses a first-party random UUID whose digest is stored server-side.
- Raw visitor network and browser observations expire according to `[widget].visitor-data-retention-days`.
- Approximate country/city and parsed browser/OS fields are derived only while the retained IP and User-Agent are available; they are not stored as permanent profile fields.
- Network-level visitor intelligence requires an audited password session with the appropriate permission; access tokens cannot read it.
- Do not mount the widget on password reset, credential, or other sensitive pages whose URL or title may contain secrets.
- Agent capabilities execute in the configured external agent runtime. For the local OpenClaw
  adapter, the workspace hands off a short-lived capability grant and OpenClaw performs only the enabled
  fixed-wrapper actions through its built-in, allowlisted `exec`. Shell commands are delegated to a
  separate networkless runner with bounded resources and a discarded per-command filesystem.
