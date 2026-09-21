#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$ROOT_DIR/backend"
FRONTEND_DIR="$ROOT_DIR/frontend"
LOCAL_SECRETS_FILE="$BACKEND_DIR/.env.secrets.local"

PIDS=()
NAMES=()

fail() {
  echo "the workspace dev: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found."
}

require_free_port() {
  local port="$1"
  local service="$2"
  if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    fail "$service cannot start because port $port is already in use."
  fi
}

prepare_secret_encryption_key() {
  if [[ -z "${TZ_SECRETS_KEY:-}" && -f "$LOCAL_SECRETS_FILE" ]]; then
    # This file is generated locally by this script and ignored by Git.
    # shellcheck disable=SC1090
    source "$LOCAL_SECRETS_FILE"
  fi

  if [[ -z "${TZ_SECRETS_KEY:-}" ]]; then
    local generated_key
    local previous_umask
    generated_key="$(openssl rand -base64 32)" || fail "could not generate the local secret encryption key."
    previous_umask="$(umask)"
    umask 077
    if ! printf 'TZ_SECRETS_KEY=%q\n' "$generated_key" >"$LOCAL_SECRETS_FILE"; then
      umask "$previous_umask"
      fail "could not write backend/.env.secrets.local."
    fi
    umask "$previous_umask"
    chmod 600 "$LOCAL_SECRETS_FILE"
    TZ_SECRETS_KEY="$generated_key"
    unset generated_key
    echo "the workspace dev: generated backend/.env.secrets.local"
  fi

  export TZ_SECRETS_KEY
  if [[ ! "$TZ_SECRETS_KEY" =~ ^[A-Za-z0-9+/]{43}=$ ]]; then
    fail "TZ_SECRETS_KEY must be a base64-encoded 32-byte key."
  fi
}

start_service() {
  local name="$1"
  local directory="$2"
  shift 2

  echo "the workspace dev: starting $name"
  (
    cd "$directory"
    exec "$@"
  ) &
  PIDS+=("$!")
  NAMES+=("$name")
}

cleanup() {
  trap - EXIT INT TERM
  if ((${#PIDS[@]} > 0)); then
    echo
    echo "the workspace dev: stopping services"
    for pid in "${PIDS[@]}"; do
      if kill -0 "$pid" >/dev/null 2>&1; then
        kill "$pid" >/dev/null 2>&1 || true
      fi
    done
    for pid in "${PIDS[@]}"; do
      wait "$pid" >/dev/null 2>&1 || true
    done
  fi
}

interrupt() {
  cleanup
  exit 130
}

trap cleanup EXIT
trap interrupt INT TERM

require_command cargo
require_command npm
require_command lsof
require_command nc
require_command openssl

[[ -f "$BACKEND_DIR/Config.toml" ]] || fail "backend/Config.toml is missing. Copy backend/Config.example.toml first."
[[ -x "$FRONTEND_DIR/node_modules/.bin/vite" ]] || fail "frontend dependencies are missing. Run 'cd frontend && npm install'."

prepare_secret_encryption_key

nc -z 127.0.0.1 5432 >/dev/null 2>&1 || fail "PostgreSQL is not reachable on port 5432."
if ! nc -z 127.0.0.1 6379 >/dev/null 2>&1; then
  echo "the workspace dev: warning: Redis is not reachable on port 6379; startup may fail if Redis is required."
fi

require_free_port 18080 "API"
require_free_port 18081 "worker"
require_free_port 15173 "admin"
require_free_port 15174 "widget"
require_free_port 15175 "loader"

echo "the workspace dev: building widget and loader"
(
  cd "$FRONTEND_DIR"
  npm run build:widget
)

start_service "API" "$BACKEND_DIR" cargo run --bin api -- --config Config.toml
start_service "worker" "$BACKEND_DIR" cargo run --bin worker -- --config Config.toml
start_service "admin" "$FRONTEND_DIR" ./node_modules/.bin/vite --mode admin --port 15173 --host 127.0.0.1
start_service "widget" "$FRONTEND_DIR" ./node_modules/.bin/vite --mode widget --port 15174 --host 127.0.0.1
start_service "loader" "$FRONTEND_DIR" ./node_modules/.bin/vite preview --outDir dist --port 15175 --host 127.0.0.1

echo
echo "the workspace dev is running:"
echo "  Admin:  http://localhost:15173"
echo "  API:    http://localhost:18080/health/ready"
echo "  Worker: http://localhost:18081/health/live"
echo "  Widget: http://localhost:15174/widget.html"
echo "  Loader: http://127.0.0.1:15175/loader/widget-loader.js"
echo
echo "Press Ctrl+C to stop every process started by this script."

while true; do
  for index in "${!PIDS[@]}"; do
    pid="${PIDS[$index]}"
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      wait "$pid" || status=$?
      status="${status:-0}"
      echo "the workspace dev: ${NAMES[$index]} stopped with status $status." >&2
      exit "$status"
    fi
  done
  sleep 1
done
