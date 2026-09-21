# the workspace production deployment

This document describes the standard deployment. The independent manual Lite
deployment for `tz.cyber-money.org` is documented in [lite/README.md](lite/README.md).

Production runs the two Rust binaries as separate systemd services, serves the
three frontend builds through nginx, and keeps PostgreSQL, Redis, ClamAV,
configuration, attachments, and encryption material outside release folders.

GitHub Actions creates an immutable archive only after CI succeeds on `master`.
The archive is uploaded with a dedicated SSH key whose server-side entry is
restricted to one upload path and one root-owned deployment command.

Repository Actions require exactly three secrets:

- `TZ_DEPLOY_HOST`: the production SSH hostname or IP address.
- `TZ_DEPLOY_USER`: the dedicated restricted deployment account.
- `TZ_DEPLOY_SSH_KEY`: the private half of the dedicated Ed25519 deployment
  key. Never reuse a personal or interactive server key.

The domain and pinned SSH host key are public deployment metadata and remain in
the workflow. The host and user are supplied through repository secrets.

The `Deploy production` workflow runs automatically after a successful CI run on
`master`, or manually through `workflow_dispatch`. It uploads both immutable
archives and invokes one restricted server-side release command. That command
stops the worker, updates and verifies OpenClaw and Firecrawl, activates the
backend/API (which applies migrations), and starts the worker only after the API
is ready. No repository variable or separate bootstrap step is required.

A failed or interrupted release leaves
`/var/lib/tz/deployment/worker-blocked` in place so a worker from one release
cannot run against another release's runtime. Fix the cause and rerun the same
release; the successful release command removes the marker automatically.

The workflow reuses the three deployment secrets. OpenClaw's Gateway token and
Firecrawl's PostgreSQL password are generated on the server and remain under
`/var/lib/tz-ai`; they are not GitHub secrets.

Server layout:

```text
/srv/tz/releases/<git-sha>/   immutable application releases
/srv/tz/current               active release symlink
/etc/tz/                      production config and encryption key
/var/lib/tz/attachments/      private uploaded objects
/var/lib/tz/geoip/            private GeoLite2 databases
/var/lib/tz/incoming/         restricted Actions upload directory
/var/lib/tz/deployment/       persistent fail-closed worker rollout marker
/var/backups/tz/              pre-deploy PostgreSQL dumps
/srv/tz-ai/releases/<sha>/    immutable OpenClaw/Firecrawl definitions
/srv/tz-ai/current            active AI definition symlink
/var/lib/tz-ai/               persistent private AI state, tokens, and Firecrawl data
/run/tz-ai/openclaw/          tmpfs grants, action markers, and shell socket
```

## Sentry error reporting

The API and worker initialize the Rust Sentry SDK when `SENTRY_DSN` is present
in `/etc/tz/tz.env`. Add a backend-project DSN without committing it:

```dotenv
SENTRY_DSN=https://public-key@example.ingest.sentry.io/project-id
```

Each immutable backend build embeds the Git SHA as its Sentry release and uses
the `production` environment. Runtime `SENTRY_RELEASE` and
`SENTRY_ENVIRONMENT` variables override those build values when needed. Only
error-level `tracing` events and panics are sent; default PII collection is
disabled.

Browser error reporting is optional and uses a separate project. Copy
`deploy/server/sentry-frontend.env.example` to
`/etc/tz/sentry-frontend.env`, replace every placeholder, make it owned by
`root`, and set mode `0600`. The deploy command passes only that allowlisted
file to the isolated frontend build. When all build credentials are present,
Vite uploads hidden source maps and deletes them from the deployed assets after
upload. `VITE_SENTRY_DSN` is public by design; `SENTRY_AUTH_TOKEN` is build-only
and must never use the `VITE_` prefix.

If `/etc/tz/sentry-frontend.env` is absent, frontend builds remain valid and
browser reporting stays disabled. If the file contains only a DSN, browser
events are sent but production stack traces remain minified because source maps
cannot be uploaded.

## Demo account and widget

The demo account is `demo@tz.ai`. Its generated, intentionally public
password is supplied by `GET /api/v1/auth/demo`; the login form's “Try” action
fills both fields. Backend migration `0077` creates the isolated demo tenant,
account, and widget; migration `0078` names the account and Inbox `DemoAccount`.
Deploy the backend and frontend together before enabling the demo host.

The static page `frontend/public/demo.html` is copied into the admin build and
served at `https://demo.tz.io/`. It embeds widget
`66f4db7a-c798-44ce-947c-a9fdac351d55` from `https://tz.io`; the seed allows
only `https://demo.tz.io` as its embedding origin. The demo vhost serves only
this page. API, widget iframe, and WebSocket requests use the primary host.
The page offers English and Romanian (`?lang=en` / `?lang=ro`), with matching
widget localization provisioned by migration `0079`. Light and dark themes
(`?theme=light` / `?theme=dark`) apply to both the page and widget. Theme changes
use the existing widget bridge without reloading the chat; language changes
reload the page with the selected locale. The URL retains both choices.

Demo sessions show the full workspace and allow browsing configuration. Saving,
deleting, connecting integrations, and running tasks are disabled; public replies
to their own widget conversations are allowed. The API compares the login session's client IP with the widget
visitor IP. Visitors behind the same public IP share this demo boundary. The
API must remain bound to loopback, with nginx overwriting `X-Tz-Client-IP`
and trusting `CF-Connecting-IP` only from the configured Cloudflare networks.

Point the demo DNS record at the production host. On an existing host, install
the HTTP challenge vhost, issue its separate certificate, then enable HTTPS.
Run the following from the reviewed release source directory on the server:

```bash
sudo install -o root -g root -m 0644 deploy/server/tz-demo.nginx.http.conf /etc/nginx/sites-available/tz-demo
sudo ln -sfn /etc/nginx/sites-available/tz-demo /etc/nginx/sites-enabled/tz-demo
sudo nginx -t
sudo systemctl reload nginx.service
sudo certbot certonly --webroot --webroot-path /var/www/letsencrypt --cert-name demo.tz.io -d demo.tz.io
sudo install -o root -g root -m 0644 deploy/server/tz-demo.nginx.conf /etc/nginx/sites-available/tz-demo
sudo install -o root -g root -m 0755 deploy/server/tz-certbot-deploy /etc/letsencrypt/renewal-hooks/deploy/tz-nginx
sudo nginx -t
sudo systemctl reload nginx.service
```

The certificate renewal hook reloads nginx for both the workspace certificates. Initial
server provisioning chooses the demo HTTP or HTTPS vhost based on whether its
certificate exists. Normal application releases replace the static page through
the existing release symlink; they do not install nginx configuration.

After deployment, verify the demo page and widget load over HTTPS, use “Try”
to sign in, and send a widget message from the same connection. Its conversation
must appear in that demo session, permit a reply, and reject configuration
changes. A second connection with a different public IP must not see or reply
to the first connection's conversation.
