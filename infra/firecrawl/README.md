# Firecrawl for OpenClaw

This directory deploys the official self-hosted Firecrawl release `v2.11.162`
as a separate Compose project. The pinned upstream checkout and all runtime
secrets are Git-ignored.

Firecrawl uses its official API, Playwright, Redis, RabbitMQ, and PostgreSQL
services. FoundationDB is intentionally not started. Concurrency and memory
limits are reduced so the stack can coexist with the Support application.

Start and verify:

```bash
./infra/firecrawl/manage.sh start
./infra/firecrawl/manage.sh smoke
```

Connect the self-hosted API to OpenClaw through the private `support-ai` Docker
network:

```bash
./infra/firecrawl/manage.sh connect-openclaw
./infra/firecrawl/manage.sh smoke-openclaw
```

The OpenClaw plugin uses `http://firecrawl-api:3002`. Firecrawl publishes its
API only on host loopback for private diagnostics:

```text
http://127.0.0.1:3002
```

The verified integration covers `firecrawl_scrape` and Firecrawl-backed
`web_fetch`. Self-hosted `firecrawl_search` needs a search backend such as
SearXNG; it is intentionally not configured yet.

Other commands:

```bash
./infra/firecrawl/manage.sh status
./infra/firecrawl/manage.sh logs
./infra/firecrawl/manage.sh stop
```

`USE_DB_AUTHENTICATION=false` is intentional for this private deployment. The API
must remain on loopback and the private Docker network. The placeholder plugin
key `self-hosted-local` is not an authentication boundary.

Unused optional upstream integrations are explicitly set to empty values in the
private `.env`; this keeps Compose output quiet without enabling them.

Production keeps the checked-out pinned source, PostgreSQL volume, and private
environment under `/var/lib/support-ai/firecrawl`. Updating the versioned release
files does not delete or recreate that persistent data.

The management script waits until PostgreSQL has completed `initdb`, verifies
the three required NuQ queue tables, and reapplies the pinned upstream schema
before starting the API when a fresh volume did not finish its init hook.
