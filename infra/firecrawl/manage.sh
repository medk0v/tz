#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$ROOT_DIR/.env"
OVERRIDE_FILE="$ROOT_DIR/compose.override.yaml"
RUNTIME_DIR="$ROOT_DIR/runtime"
FIRECRAWL_RELEASE="v2.11.162"
FIRECRAWL_COMMIT_PREFIX="7666c1f"
FIRECRAWL_REPOSITORY="https://github.com/firecrawl/firecrawl.git"
SOURCE_DIR="$RUNTIME_DIR/source-${FIRECRAWL_RELEASE#v}"
OPENCLAW_PLUGIN_VERSION="2026.7.1"
OPENCLAW_PLUGIN="@openclaw/firecrawl-plugin@$OPENCLAW_PLUGIN_VERSION"
OPENCLAW_DIR="$(cd "$ROOT_DIR/../openclaw" && pwd)"

fail() {
  echo "Firecrawl: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found"
}

prepare_local_state() {
  require_command docker
  require_command git
  require_command openssl

  mkdir -p "$RUNTIME_DIR"
  chmod 700 "$RUNTIME_DIR"

  if [[ ! -f "$ENV_FILE" ]]; then
    local postgres_password
    local previous_umask

    postgres_password="$(openssl rand -hex 32)" || fail "could not generate PostgreSQL password"
    [[ -n "$postgres_password" ]] || fail "generated PostgreSQL password is empty"

    previous_umask="$(umask)"
    umask 077
    {
      printf 'FIRECRAWL_RELEASE=%s\n' "$FIRECRAWL_RELEASE"
      printf 'USE_DB_AUTHENTICATION=false\n'
      printf 'POSTGRES_USER=firecrawl\n'
      printf 'POSTGRES_PASSWORD=%s\n' "$postgres_password"
      printf 'POSTGRES_DB=postgres\n'
      printf 'PORT=3002\n'
      printf 'INTERNAL_PORT=3002\n'
      printf 'NUM_WORKERS_PER_QUEUE=2\n'
      printf 'CRAWL_CONCURRENT_REQUESTS=2\n'
      printf 'MAX_CONCURRENT_JOBS=2\n'
      printf 'BROWSER_POOL_SIZE=2\n'
      printf 'BLOCK_MEDIA=true\n'
      printf 'ALLOW_LOCAL_WEBHOOKS=false\n'
      printf 'LOGGING_LEVEL=INFO\n'
      printf 'HARNESS_STARTUP_TIMEOUT_MS=120000\n'
      printf 'OPENAI_API_KEY=\n'
      printf 'OPENAI_BASE_URL=\n'
      printf 'MODEL_NAME=\n'
      printf 'MODEL_EMBEDDING_NAME=\n'
      printf 'OLLAMA_BASE_URL=\n'
      printf 'SEARXNG_ENDPOINT=\n'
      printf 'SEARXNG_CATEGORIES=\n'
      printf 'SEARXNG_ENGINES=\n'
      printf 'SUPABASE_URL=\n'
      printf 'SUPABASE_ANON_TOKEN=\n'
      printf 'SUPABASE_SERVICE_TOKEN=\n'
      printf 'SELF_HOSTED_WEBHOOK_URL=\n'
      printf 'SLACK_WEBHOOK_URL=\n'
      printf 'AUTUMN_SECRET_KEY=\n'
      printf 'TEST_API_KEY=\n'
      printf 'NUQ_BACKEND=\n'
      printf 'BULL_AUTH_KEY=\n'
      printf 'PROXY_SERVER=\n'
      printf 'PROXY_USERNAME=\n'
      printf 'PROXY_PASSWORD=\n'
    } >"$ENV_FILE"
    umask "$previous_umask"
    unset postgres_password
    echo "Firecrawl: created private .env"
  fi

  chmod 600 "$ENV_FILE"
}

ensure_source() {
  local actual_commit
  local actual_tag

  if [[ ! -d "$SOURCE_DIR/.git" ]]; then
    [[ ! -e "$SOURCE_DIR" ]] || fail "$SOURCE_DIR exists but is not a Git checkout"
    git clone --depth 1 --branch "$FIRECRAWL_RELEASE" --single-branch \
      "$FIRECRAWL_REPOSITORY" "$SOURCE_DIR"
  fi

  actual_commit="$(git -C "$SOURCE_DIR" rev-parse --short=7 HEAD)"
  actual_tag="$(git -C "$SOURCE_DIR" describe --tags --exact-match 2>/dev/null || true)"
  [[ "$actual_commit" == "$FIRECRAWL_COMMIT_PREFIX" ]] || \
    fail "unexpected Firecrawl commit $actual_commit; expected $FIRECRAWL_COMMIT_PREFIX"
  [[ "$actual_tag" == "$FIRECRAWL_RELEASE" ]] || \
    fail "unexpected Firecrawl tag ${actual_tag:-none}; expected $FIRECRAWL_RELEASE"
}

ensure_shared_network() {
  if ! docker network inspect tz-ai >/dev/null 2>&1; then
    docker network create --driver bridge tz-ai >/dev/null
    echo "Firecrawl: created shared Docker network tz-ai"
  fi
}

compose() {
  docker compose \
    --project-name tz-firecrawl \
    --project-directory "$SOURCE_DIR" \
    --env-file "$ENV_FILE" \
    -f "$SOURCE_DIR/docker-compose.yaml" \
    -f "$OVERRIDE_FILE" \
    "$@"
}

openclaw_compose() {
  docker compose \
    --project-directory "$OPENCLAW_DIR" \
    --env-file "$OPENCLAW_DIR/.env" \
    -f "$OPENCLAW_DIR/compose.yaml" \
    "$@"
}

wait_until_ready() {
  local attempt

  require_command curl
  for attempt in $(seq 1 90); do
    if curl -fsS http://127.0.0.1:3002/v0/health/readiness >/dev/null 2>&1; then
      echo "Firecrawl: API is ready at http://127.0.0.1:3002/"
      return 0
    fi
    sleep 2
  done

  compose ps --all
  fail "API did not become ready within 180 seconds"
}

wait_until_postgres_ready() {
  local attempt
  local container_id
  local health_state

  container_id="$(compose ps -q nuq-postgres)"
  [[ -n "$container_id" ]] || fail "PostgreSQL container is not running"

  for attempt in $(seq 1 60); do
    health_state="$(docker inspect --format '{{.State.Health.Status}}' "$container_id" 2>/dev/null || true)"
    if [[ "$health_state" == "healthy" ]] \
      && docker exec "$container_id" sh -c '[ "$(cat /proc/1/comm)" = "postgres" ]'; then
      return 0
    fi
    sleep 2
  done

  compose ps --all
  fail "PostgreSQL did not finish initialization within 120 seconds"
}

nuq_schema_is_ready() {
  printf '%s\n' \
    "SELECT to_regclass('nuq.queue_scrape') IS NOT NULL AND to_regclass('nuq.queue_crawl_finished') IS NOT NULL AND to_regclass('nuq.group_crawl') IS NOT NULL;" \
    | compose exec -T nuq-postgres sh -eu -c \
      'psql -X -A -t -q -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB"' \
    | grep -qx t
}

ensure_nuq_schema() {
  if nuq_schema_is_ready; then
    return 0
  fi

  echo "Firecrawl: applying the pinned NuQ PostgreSQL schema"
  compose exec -T nuq-postgres sh -eu -c \
    'psql -X -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" --file /docker-entrypoint-initdb.d/010-nuq.sql' \
    >/dev/null
  nuq_schema_is_ready || fail "NuQ PostgreSQL schema is incomplete"
}

smoke_test() {
  local response

  require_command curl
  require_command jq

  curl -fsS http://127.0.0.1:3002/v0/health/readiness >/dev/null
  response="$(curl --fail-with-body --silent --show-error --max-time 90 \
    -X POST http://127.0.0.1:3002/v2/scrape \
    -H 'Content-Type: application/json' \
    -d '{"url":"https://example.com","formats":["markdown"],"timeout":60000}')"

  printf '%s\n' "$response" | jq -e '{
    success,
    sourceURL: .data.metadata.sourceURL,
    markdownPreview: (.data.markdown[0:180])
  } | select(.success == true)'
}

smoke_openclaw_plugin() {
  local container_id

  [[ -f "$OPENCLAW_DIR/.env" ]] || fail "OpenClaw .env is missing; start OpenClaw first"
  container_id="$(openclaw_compose ps -q openclaw-gateway)"
  [[ -n "$container_id" ]] || fail "OpenClaw gateway container is not running"

  docker exec "$container_id" node -e '
    const request = {
      tool: "firecrawl_scrape",
      args: {
        url: "https://example.com",
        extractMode: "markdown",
        maxChars: 180,
        onlyMainContent: true,
        timeoutSeconds: 60,
      },
    };

    fetch("http://127.0.0.1:18789/tools/invoke", {
      method: "POST",
      headers: {
        Authorization: "Bearer " + process.env.OPENCLAW_GATEWAY_TOKEN,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(request),
    })
      .then(async (response) => {
        const body = await response.json();
        const textItem = Array.isArray(body?.result?.content)
          ? body.result.content.find((item) => item?.type === "text")
          : undefined;
        let result;

        try {
          result = JSON.parse(textItem?.text ?? "null");
        } catch {
          result = null;
        }

        console.log(JSON.stringify({
          httpStatus: response.status,
          ok: body?.ok,
          url: result?.url,
          scrapeStatus: result?.status,
        }, null, 2));

        if (!response.ok || body?.ok !== true || result?.url !== "https://example.com" || result?.status !== 200) {
          process.exit(1);
        }
      })
      .catch((error) => {
        console.error(error.message);
        process.exit(1);
      });
  '
}

connect_openclaw() {
  local container_id
  local settings

  [[ -f "$OPENCLAW_DIR/.env" ]] || fail "OpenClaw .env is missing; start OpenClaw first"
  require_command jq

  ensure_shared_network
  openclaw_compose up -d openclaw-gateway >/dev/null
  container_id="$(openclaw_compose ps -q openclaw-gateway)"
  [[ -n "$container_id" ]] || fail "OpenClaw gateway container is not running"

  if ! docker exec "$container_id" node dist/index.js plugins inspect firecrawl --json 2>/dev/null \
    | jq -e --arg version "$OPENCLAW_PLUGIN_VERSION" '.plugin.version == $version' >/dev/null; then
    docker exec "$container_id" node dist/index.js plugins install --pin "$OPENCLAW_PLUGIN"
  fi

  settings='[
    {"path":"plugins.entries.firecrawl.enabled","value":true},
    {"path":"plugins.entries.firecrawl.config.webFetch","value":{
      "apiKey":"self-hosted-local",
      "baseUrl":"http://firecrawl-api:3002",
      "onlyMainContent":true,
      "maxAgeMs":172800000,
      "timeoutSeconds":60
    }},
    {"path":"tools.web.fetch.provider","value":"firecrawl"}
  ]'

  docker exec "$container_id" node dist/index.js config set --batch-json "$settings" >/dev/null
  openclaw_compose restart openclaw-gateway >/dev/null

  for _ in $(seq 1 30); do
    if curl -fsS http://127.0.0.1:18789/healthz >/dev/null 2>&1; then
      break
    fi
    sleep 2
  done

  container_id="$(openclaw_compose ps -q openclaw-gateway)"
  docker exec "$container_id" node -e \
    "fetch('http://firecrawl-api:3002/v0/health/readiness').then((response)=>process.exit(response.ok?0:1)).catch(()=>process.exit(1))"
  docker exec "$container_id" node dist/index.js plugins inspect firecrawl --json
  smoke_openclaw_plugin
}

usage() {
  cat <<'EOF'
Usage: ./manage.sh <command>

Commands:
  start             Clone the pinned source, build, and start Firecrawl
  stop              Stop Firecrawl containers; persistent PostgreSQL remains
  status            Show Firecrawl container status
  logs              Follow API and Playwright logs
  smoke             Run readiness and a real example.com scrape
  smoke-openclaw    Scrape example.com through OpenClaw /tools/invoke
  connect-openclaw  Install/configure the official plugin and private endpoint
EOF
}

command="${1:-}"

case "$command" in
  start)
    prepare_local_state
    ensure_source
    ensure_shared_network
    compose build api playwright-service nuq-postgres
    compose up -d playwright-service redis rabbitmq nuq-postgres
    wait_until_postgres_ready
    ensure_nuq_schema
    compose up -d api
    wait_until_ready
    ;;
  stop)
    prepare_local_state
    ensure_source
    compose down
    ;;
  status)
    prepare_local_state
    ensure_source
    compose ps --all
    ;;
  logs)
    prepare_local_state
    ensure_source
    compose logs -f --tail=200 api playwright-service
    ;;
  smoke)
    prepare_local_state
    smoke_test
    ;;
  smoke-openclaw)
    prepare_local_state
    smoke_openclaw_plugin
    ;;
  connect-openclaw)
    prepare_local_state
    ensure_source
    connect_openclaw
    ;;
  *)
    usage
    [[ -n "$command" ]] || exit 0
    exit 2
    ;;
esac
