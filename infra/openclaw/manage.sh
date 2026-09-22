#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$ROOT_DIR/.env"
COMPOSE_FILE="$ROOT_DIR/compose.yaml"
RUNTIME_DIR="$ROOT_DIR/runtime"
WORKSPACE_IDENTITY_TEMPLATE="$ROOT_DIR/workspace/IDENTITY.md"
MAIN_WORKSPACE_DIR="$RUNTIME_DIR/workspace"
SUPPORT_WORKSPACE_DIR="$RUNTIME_DIR/state/workspace-support"
OPENCLAW_RELEASE_IMAGE="ghcr.io/openclaw/openclaw:2026.7.1-2"

fail() {
  echo "OpenClaw: $*" >&2
  exit 1
}

install_workspace_policy() {
  require_command cmp
  require_command install
  [[ -f "$WORKSPACE_IDENTITY_TEMPLATE" ]] \
    || fail "Support workspace identity policy is missing"
  [[ -d "$MAIN_WORKSPACE_DIR" && ! -L "$MAIN_WORKSPACE_DIR" ]] \
    || fail "main OpenClaw workspace is missing or unsafe"
  [[ -d "$SUPPORT_WORKSPACE_DIR" && ! -L "$SUPPORT_WORKSPACE_DIR" ]] \
    || fail "dedicated Support workspace is missing or unsafe"
  install -m 0644 \
    "$WORKSPACE_IDENTITY_TEMPLATE" \
    "$MAIN_WORKSPACE_DIR/IDENTITY.md"
  install -m 0644 \
    "$WORKSPACE_IDENTITY_TEMPLATE" \
    "$SUPPORT_WORKSPACE_DIR/IDENTITY.md"
  rm -f -- "$SUPPORT_WORKSPACE_DIR/BOOTSTRAP.md"
}

verify_workspace_policy() {
  [[ ! -e "$SUPPORT_WORKSPACE_DIR/BOOTSTRAP.md" ]] \
    || fail "dedicated Support workspace still has pending OpenClaw bootstrap"
  cmp -s "$WORKSPACE_IDENTITY_TEMPLATE" "$SUPPORT_WORKSPACE_DIR/IDENTITY.md" \
    || fail "dedicated Support workspace identity policy drifted"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found"
}

configured_env_value() {
  local key="$1"
  [[ -f "$ENV_FILE" ]] || return 0
  awk -F= -v key="$key" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' "$ENV_FILE"
}

validate_host_directory() {
  local value="$1"
  local pattern='^(/|\./)[A-Za-z0-9._/-]+$'
  [[ "$value" =~ $pattern ]] \
    || fail "unsafe OpenClaw host directory"
}

set_env_value() {
  local key="$1"
  local value="$2"
  local env_target="$ENV_FILE"
  local link_target
  local stage

  if [[ -L "$env_target" ]]; then
    link_target="$(readlink "$env_target")"
    if [[ "$link_target" == /* ]]; then
      env_target="$link_target"
    else
      env_target="$(cd "$(dirname "$env_target")" && pwd -P)/$link_target"
    fi
  fi
  [[ -f "$env_target" && ! -L "$env_target" ]] \
    || fail "OpenClaw .env target is missing or unsafe"
  stage="$(mktemp "$(dirname "$env_target")/.openclaw-env.XXXXXX")"
  if ! awk -F= -v key="$key" -v value="$value" '
      BEGIN { replaced = 0 }
      $1 == key {
        if (replaced == 0) print key "=" value
        replaced = 1
        next
      }
      { print }
      END { if (replaced == 0) print key "=" value }
    ' "$env_target" >"$stage"; then
    rm -f -- "$stage"
    fail "could not update OpenClaw .env"
  fi
  chmod 600 "$stage"
  mv -f -- "$stage" "$env_target"
}

prepare_local_state() {
  local grant_host_dir
  local action_host_dir
  local shell_host_dir
  local grant_gid

  require_command docker
  require_command openssl

  grant_host_dir="${OPENCLAW_GRANT_HOST_DIR:-$(configured_env_value OPENCLAW_GRANT_HOST_DIR)}"
  action_host_dir="${OPENCLAW_ACTION_HOST_DIR:-$(configured_env_value OPENCLAW_ACTION_HOST_DIR)}"
  shell_host_dir="${OPENCLAW_SHELL_HOST_DIR:-$(configured_env_value OPENCLAW_SHELL_HOST_DIR)}"
  grant_host_dir="${grant_host_dir:-$RUNTIME_DIR/agent-secrets}"
  action_host_dir="${action_host_dir:-$RUNTIME_DIR/agent-actions}"
  shell_host_dir="${shell_host_dir:-$RUNTIME_DIR/shell}"
  grant_gid="${OPENCLAW_GRANT_GID:-$(configured_env_value OPENCLAW_GRANT_GID)}"
  grant_gid="${grant_gid:-$(id -g)}"
  validate_host_directory "$grant_host_dir"
  validate_host_directory "$action_host_dir"
  validate_host_directory "$shell_host_dir"
  [[ "$grant_gid" =~ ^[0-9]+$ && "$grant_gid" != "65534" ]] \
    || fail "invalid OpenClaw grant GID"

  mkdir -p \
    "$RUNTIME_DIR/state" \
    "$RUNTIME_DIR/state/workspace-support" \
    "$RUNTIME_DIR/workspace" \
    "$RUNTIME_DIR/auth" \
    "$grant_host_dir" \
    "$action_host_dir" \
    "$shell_host_dir"
  chmod 750 "$RUNTIME_DIR"
  chmod 700 \
    "$RUNTIME_DIR/state" \
    "$RUNTIME_DIR/state/workspace-support" \
    "$RUNTIME_DIR/workspace" \
    "$RUNTIME_DIR/auth"
  chmod 2750 "$grant_host_dir"
  chmod 2770 "$action_host_dir"
  chmod 2770 "$shell_host_dir"

  if [[ ! -f "$ENV_FILE" ]]; then
    local gateway_token
    local previous_umask

    gateway_token="$(openssl rand -hex 32)" || fail "could not generate a gateway token"
    [[ -n "$gateway_token" ]] || fail "generated gateway token is empty"

    previous_umask="$(umask)"
    umask 077
    {
      printf 'OPENCLAW_IMAGE=%s\n' "$OPENCLAW_RELEASE_IMAGE"
      printf 'OPENCLAW_GATEWAY_TOKEN=%s\n' "$gateway_token"
      printf 'OPENCLAW_TZ=Europe/Istanbul\n'
      printf 'OPENCLAW_GRANT_GID=%s\n' "$grant_gid"
      printf 'OPENCLAW_GRANT_HOST_DIR=%s\n' "$grant_host_dir"
      printf 'OPENCLAW_ACTION_HOST_DIR=%s\n' "$action_host_dir"
      printf 'OPENCLAW_SHELL_HOST_DIR=%s\n' "$shell_host_dir"
    } >"$ENV_FILE"
    umask "$previous_umask"
    unset gateway_token
    echo "OpenClaw: created private .env"
  fi

  set_env_value OPENCLAW_GRANT_GID "$grant_gid"
  set_env_value OPENCLAW_GRANT_HOST_DIR "$grant_host_dir"
  set_env_value OPENCLAW_ACTION_HOST_DIR "$action_host_dir"
  set_env_value OPENCLAW_SHELL_HOST_DIR "$shell_host_dir"

  chmod 600 "$ENV_FILE"
}

compose() {
  OPENCLAW_IMAGE="$OPENCLAW_RELEASE_IMAGE" docker compose \
    --project-directory "$ROOT_DIR" \
    --env-file "$ENV_FILE" \
    -f "$COMPOSE_FILE" \
    "$@"
}

ensure_shared_network() {
  if ! docker network inspect tz-ai >/dev/null 2>&1; then
    docker network create --driver bridge tz-ai >/dev/null
    echo "OpenClaw: created shared Docker network tz-ai"
  fi
}

run_cli_before_gateway() {
  compose run -T --rm --no-deps \
    --entrypoint node \
    openclaw-gateway \
    dist/index.js "$@"
}

configure_gateway() {
  local agent_index
  local agent_settings
  local approvals_file
  local default_settings
  local settings

  settings='[
    {"path":"gateway.mode","value":"local"},
    {"path":"gateway.bind","value":"lan"},
    {"path":"gateway.auth.mode","value":"token"},
    {"path":"gateway.controlUi.allowedOrigins","value":["http://localhost:18789","http://127.0.0.1:18789"]},
    {"path":"gateway.http.endpoints.chatCompletions.enabled","value":true},
    {"path":"skills.allowBundled","value":[]},
    {"path":"agents.defaults.skills","value":[]},
    {"path":"session.maintenance","value":{"mode":"enforce","pruneAfter":"7d","maxEntries":500,"resetArchiveRetention":"7d","maxDiskBytes":"500mb","highWaterBytes":"400mb"}},
    {"path":"tools.fs.workspaceOnly","value":true},
    {"path":"tools.elevated.enabled","value":false}
  ]'

  run_cli_before_gateway config set --batch-json "$settings" >/dev/null

  if ! agent_index="$(find_support_agent_index)"; then
    run_cli_before_gateway agents add support \
      --workspace /home/node/.openclaw/workspace-support \
      --agent-dir /home/node/.openclaw/agents/support/agent \
      --non-interactive >/dev/null
    agent_index="$(find_support_agent_index)" \
      || fail "could not locate the newly configured support agent"
  fi

  default_settings="$(build_agent_default_settings)" \
    || fail "could not make main the unique default OpenClaw agent"
  run_cli_before_gateway config set --batch-json "$default_settings" >/dev/null

  agent_settings="[
    {\"path\":\"agents.list[$agent_index].workspace\",\"value\":\"/home/node/.openclaw/workspace-support\"},
    {\"path\":\"agents.list[$agent_index].agentDir\",\"value\":\"/home/node/.openclaw/agents/support/agent\"},
    {\"path\":\"agents.list[$agent_index].skills\",\"value\":[]},
    {\"path\":\"agents.list[$agent_index].memorySearch\",\"value\":{\"enabled\":false}},
    {\"path\":\"agents.list[$agent_index].contextInjection\",\"value\":\"never\"},
    {\"path\":\"agents.list[$agent_index].sandbox\",\"value\":{\"mode\":\"off\"}},
    {\"path\":\"agents.list[$agent_index].tools\",\"value\":{\"allow\":[\"exec\"],\"exec\":{\"host\":\"gateway\",\"security\":\"allowlist\",\"ask\":\"off\",\"safeBins\":[]},\"elevated\":{\"enabled\":false}}}
  ]"
  run_cli_before_gateway config set --batch-json "$agent_settings" >/dev/null

  # Keep unattended execution explicit: only fixed Support capability wrappers
  # are allowed without an interactive approval.
  run_cli_before_gateway exec-policy set \
    --host gateway \
    --security allowlist \
    --ask off \
    --ask-fallback deny >/dev/null

  approvals_file="$(build_exec_approvals_file)" \
    || fail "could not reconcile the OpenClaw exec approvals file"
  printf '%s' "$approvals_file" \
    | run_cli_before_gateway approvals set --stdin >/dev/null
  unset approvals_file

  run_cli_before_gateway sessions cleanup \
    --agent support \
    --enforce \
    --json >/dev/null
}

find_support_agent_index() {
  compose run -T --rm --no-deps \
    --entrypoint node \
    openclaw-gateway \
    -e '
      const fs = require("node:fs");
      const config = JSON.parse(fs.readFileSync(process.env.OPENCLAW_CONFIG_PATH, "utf8"));
      const entries = Array.isArray(config.agents?.list) ? config.agents.list : [];
      const matches = entries
        .map((entry, index) => ({ entry, index }))
        .filter(({ entry }) => entry?.id === "support");
      if (matches.length !== 1) process.exit(1);
      process.stdout.write(String(matches[0].index));
    '
}

build_agent_default_settings() {
  compose run -T --rm --no-deps \
    --entrypoint node \
    openclaw-gateway \
    -e '
      const fs = require("node:fs");
      const config = JSON.parse(fs.readFileSync(process.env.OPENCLAW_CONFIG_PATH, "utf8"));
      const entries = Array.isArray(config.agents?.list) ? config.agents.list : [];
      if (entries.filter((entry) => entry?.id === "main").length !== 1
          || entries.filter((entry) => entry?.id === "support").length !== 1) {
        process.exit(1);
      }
      process.stdout.write(JSON.stringify(entries.map((entry, index) => ({
        path: `agents.list[${index}].default`,
        value: entry.id === "main",
      }))));
    '
}

build_exec_approvals_file() {
  compose run -T --rm --no-deps \
    --entrypoint node \
    openclaw-gateway \
    -e '
      const fs = require("node:fs");
      const path = require("node:path");
      const config = JSON.parse(fs.readFileSync(process.env.OPENCLAW_CONFIG_PATH, "utf8"));
      const approvalsPath = path.join(process.env.OPENCLAW_STATE_DIR, "exec-approvals.json");
      const approvals = JSON.parse(fs.readFileSync(approvalsPath, "utf8"));
      const configuredAgentIds = new Set(
        (Array.isArray(config.agents?.list) ? config.agents.list : [])
          .map((entry) => entry?.id)
          .filter((id) => typeof id === "string" && id.length > 0),
      );
      const managedPatterns = [
        "/usr/local/bin/curl",
        "/usr/local/bin/support-telegram-notify",
        "/usr/local/bin/support-resolve-conversation",
        "/usr/local/bin/support-schedule-reminder",
        "/usr/local/bin/support-public-http",
        "/usr/local/bin/support-browser",
        "/usr/local/bin/support-knowledge-article",
        "/usr/local/bin/support-integration",
        "/usr/local/bin/support-shell",
      ];
      const retiredManagedPatterns = [
        "/usr/local/bin/tz-telegram-notify",
        "/usr/local/bin/tz-resolve-conversation",
        "/usr/local/bin/tz-schedule-reminder",
        "/usr/local/bin/tz-public-http",
        "/usr/local/bin/tz-browser",
        "/usr/local/bin/tz-knowledge-article",
        "/usr/local/bin/tz-integration",
        "/usr/local/bin/tz-shell",
        "/usr/local/bin/support-order-status",
      ];
      const managedSet = new Set([...managedPatterns, ...retiredManagedPatterns]);
      const agents = approvals.agents && typeof approvals.agents === "object"
        && !Array.isArray(approvals.agents) ? { ...approvals.agents } : {};
      const wildcard = agents["*"] && typeof agents["*"] === "object"
        && !Array.isArray(agents["*"]) ? agents["*"] : {};
      const inheritedFields = ["security", "ask", "askFallback", "autoAllowSkills"];
      const normalizeEntries = (entries) => {
        const result = [];
        const seen = new Set();
        for (const entry of Array.isArray(entries) ? entries : []) {
          const pattern = entry && typeof entry === "object" ? entry.pattern : undefined;
          if (typeof pattern !== "string" || pattern.length === 0
              || managedSet.has(pattern) || seen.has(pattern)) continue;
          seen.add(pattern);
          result.push(entry);
        }
        return result;
      };

      for (const agentId of configuredAgentIds) {
        if (agentId === "support") continue;
        const current = agents[agentId] && typeof agents[agentId] === "object"
          && !Array.isArray(agents[agentId]) ? { ...agents[agentId] } : {};
        for (const field of inheritedFields) {
          if (!Object.hasOwn(current, field) && Object.hasOwn(wildcard, field)) {
            current[field] = wildcard[field];
          }
        }
        current.allowlist = normalizeEntries([
          ...(Array.isArray(current.allowlist) ? current.allowlist : []),
          ...(Array.isArray(wildcard.allowlist) ? wildcard.allowlist : []),
        ]);
        agents[agentId] = current;
      }

      for (const agentId of Object.keys(agents)) {
        if (agentId === "*" || !configuredAgentIds.has(agentId)) delete agents[agentId];
      }
      agents.support = {
        security: "allowlist",
        ask: "off",
        askFallback: "deny",
        autoAllowSkills: false,
        allowlist: managedPatterns.map((pattern) => ({ pattern })),
      };
      approvals.version = 1;
      approvals.defaults = {
        security: "allowlist",
        ask: "off",
        askFallback: "deny",
        autoAllowSkills: false,
      };
      approvals.agents = agents;
      process.stdout.write(JSON.stringify(approvals));
    '
}

verify_support_agent_config() {
  verify_workspace_policy
  compose run -T --rm --no-deps \
    --entrypoint node \
    openclaw-gateway \
    -e '
      const fs = require("node:fs");
      const path = require("node:path");
      const { isDeepStrictEqual } = require("node:util");
      const reject = (message) => {
        process.stderr.write(`OpenClaw verifier: ${message}\n`);
        process.exit(1);
      };
      const config = JSON.parse(
        fs.readFileSync(process.env.OPENCLAW_CONFIG_PATH, "utf8"),
      );
      const approvals = JSON.parse(
        fs.readFileSync(
          path.join(process.env.OPENCLAW_STATE_DIR, "exec-approvals.json"),
          "utf8",
        ),
      );
      const entries = Array.isArray(config.agents?.list) ? config.agents.list : [];
      const ids = entries.map((entry) => entry?.id);
      if (new Set(ids).size !== ids.length) reject("agent ids are not unique");
      if (!ids.includes("main")) reject("the main administrator agent is missing");
      const defaultAgents = entries.filter((entry) => entry?.default === true);
      if (defaultAgents.length !== 1 || defaultAgents[0].id !== "main") {
        reject("main is not the unique default agent");
      }
      const matches = entries.filter((entry) => entry?.id === "support");
      if (matches.length !== 1) reject("exactly one support agent is required");
      const support = matches[0];
      const exactKeys = (value, keys) => value && typeof value === "object"
        && !Array.isArray(value)
        && isDeepStrictEqual(Object.keys(value).sort(), [...keys].sort());

      if (support.default !== false
          || support.workspace !== "/home/node/.openclaw/workspace-support"
          || support.agentDir !== "/home/node/.openclaw/agents/support/agent"
          || !isDeepStrictEqual(support.skills, [])
          || !isDeepStrictEqual(support.memorySearch, { enabled: false })
          || support.contextInjection !== "never"
          || !isDeepStrictEqual(support.sandbox, { mode: "off" })) {
        reject("support isolation, memory, or context policy drifted");
      }
      if (!exactKeys(support.tools, ["allow", "exec", "elevated"])
          || !isDeepStrictEqual(support.tools.allow, ["exec"])
          || !isDeepStrictEqual(support.tools.exec, {
            host: "gateway",
            security: "allowlist",
            ask: "off",
            safeBins: [],
          })
          || !isDeepStrictEqual(support.tools.elevated, { enabled: false })) {
        reject("support tool or exec-host policy drifted");
      }
      if (config.tools?.exec?.host !== "gateway"
          || config.tools.exec.security !== "allowlist"
          || config.tools.exec.ask !== "off"
          || config.tools?.elevated?.enabled !== false) {
        reject("global gateway exec policy drifted");
      }
      if (!isDeepStrictEqual(config.session?.maintenance, {
        mode: "enforce",
        pruneAfter: "7d",
        maxEntries: 500,
        resetArchiveRetention: "7d",
        maxDiskBytes: "500mb",
        highWaterBytes: "400mb",
      })) reject("session maintenance policy drifted");

      const expectedPatterns = [
        "/usr/local/bin/curl",
        "/usr/local/bin/support-telegram-notify",
        "/usr/local/bin/support-resolve-conversation",
        "/usr/local/bin/support-schedule-reminder",
        "/usr/local/bin/support-public-http",
        "/usr/local/bin/support-browser",
        "/usr/local/bin/support-knowledge-article",
        "/usr/local/bin/support-integration",
        "/usr/local/bin/support-shell",
      ];
      const retiredPatterns = [
        "/usr/local/bin/tz-telegram-notify",
        "/usr/local/bin/tz-resolve-conversation",
        "/usr/local/bin/tz-schedule-reminder",
        "/usr/local/bin/tz-public-http",
        "/usr/local/bin/tz-browser",
        "/usr/local/bin/tz-knowledge-article",
        "/usr/local/bin/tz-integration",
        "/usr/local/bin/tz-shell",
        "/usr/local/bin/support-order-status",
      ];
      const approvalAgents = approvals.agents && typeof approvals.agents === "object"
        && !Array.isArray(approvals.agents) ? approvals.agents : {};
      if (Object.hasOwn(approvalAgents, "*")) reject("wildcard exec approvals are forbidden");
      for (const approvalAgentId of Object.keys(approvalAgents)) {
        if (!ids.includes(approvalAgentId)) reject("stale exec approval agent scope found");
      }
      const expectedDefaults = {
        security: "allowlist",
        ask: "off",
        askFallback: "deny",
        autoAllowSkills: false,
      };
      if (approvals.version !== 1
          || !isDeepStrictEqual(approvals.defaults, expectedDefaults)) {
        reject("exec approval defaults drifted");
      }
      const supportApprovals = approvalAgents.support;
      if (!exactKeys(supportApprovals, [
        "security", "ask", "askFallback", "autoAllowSkills", "allowlist",
      ])) reject("support exec approval policy has unexpected fields");
      for (const [key, value] of Object.entries(expectedDefaults)) {
        if (supportApprovals[key] !== value) reject(`support exec approval ${key} drifted`);
      }
      const patterns = Array.isArray(supportApprovals.allowlist)
        ? supportApprovals.allowlist.map((entry) => entry?.pattern)
        : [];
      if (new Set(patterns).size !== patterns.length
          || !isDeepStrictEqual([...patterns].sort(), [...expectedPatterns].sort())) {
        reject("support exec allowlist is not the exact wrapper set");
      }
      for (const [approvalAgentId, policy] of Object.entries(approvalAgents)) {
        if (approvalAgentId === "support") continue;
        const otherPatterns = Array.isArray(policy?.allowlist)
          ? policy.allowlist.map((entry) => entry?.pattern)
          : [];
        if (otherPatterns.some((pattern) => (
          expectedPatterns.includes(pattern) || retiredPatterns.includes(pattern)
        ))) {
          reject("a Support wrapper remains approved for another agent");
        }
      }
    ' >/dev/null \
    || fail "dedicated support agent configuration is missing or unsafe"
}

verify_support_model() {
  local gateway_token
  local models

  require_command curl
  require_command jq
  gateway_token="$(sed -n 's/^OPENCLAW_GATEWAY_TOKEN=//p' "$ENV_FILE" | tail -n 1)"
  [[ -n "$gateway_token" ]] || fail "OPENCLAW_GATEWAY_TOKEN is missing"
  models="$(curl -fsS http://127.0.0.1:18789/v1/models \
    -H "Authorization: Bearer $gateway_token")" \
    || fail "could not read the authenticated OpenClaw model list"
  printf '%s' "$models" \
    | jq -e 'any(.data[]?; .id == "openclaw/support")' >/dev/null \
    || fail "OpenClaw model list does not expose exact model openclaw/support"
  unset gateway_token models
}

wait_until_live() {
  local attempt
  for attempt in $(seq 1 30); do
    if curl -fsS http://127.0.0.1:18789/healthz >/dev/null 2>&1; then
      echo "OpenClaw: gateway is live at http://127.0.0.1:18789/"
      return 0
    fi
    sleep 2
  done

  compose ps
  fail "gateway did not become live within 60 seconds"
}

smoke_test() {
  require_command curl
  verify_support_agent_config
  curl -fsS http://127.0.0.1:18789/healthz >/dev/null
  verify_support_model
  echo "OpenClaw: dedicated openclaw/support model is available"
}

usage() {
  cat <<'EOF'
Usage: ./manage.sh <command>

Commands:
  start     Prepare state, configure OpenClaw, and start the gateway
  stop      Stop and remove the gateway container (persistent state remains)
  restart   Restart the gateway container
  status    Show container status
  logs      Follow gateway logs
  smoke     Check health and the authenticated OpenAI-compatible model list
  doctor    Run read-only diagnostics and fail only on error-level findings
  doctor-all
             Run read-only diagnostics including expected Docker warnings
EOF
}

command="${1:-}"

case "$command" in
  start)
    require_command curl
    prepare_local_state
    install_workspace_policy
    ensure_shared_network
    compose build --pull openclaw-gateway browser-runner
    compose stop openclaw-gateway shell-runner browser-runner >/dev/null 2>&1 || true
    configure_gateway
    install_workspace_policy
    verify_support_agent_config
    compose up -d shell-runner browser-runner openclaw-gateway
    wait_until_live
    ;;
  stop)
    prepare_local_state
    compose down
    ;;
  restart)
    prepare_local_state
    install_workspace_policy
    ensure_shared_network
    compose build --pull openclaw-gateway browser-runner
    compose stop openclaw-gateway shell-runner browser-runner >/dev/null 2>&1 || true
    configure_gateway
    install_workspace_policy
    verify_support_agent_config
    compose up -d shell-runner browser-runner openclaw-gateway
    wait_until_live
    ;;
  status)
    prepare_local_state
    compose ps
    ;;
  logs)
    prepare_local_state
    compose logs -f --tail=200 openclaw-gateway
    ;;
  smoke)
    prepare_local_state
    smoke_test
    ;;
  doctor)
    prepare_local_state
    verify_support_agent_config
    verify_support_model
    run_cli_before_gateway doctor --lint --json --severity-min error
    ;;
  doctor-all)
    prepare_local_state
    verify_support_agent_config
    verify_support_model
    run_cli_before_gateway doctor --lint --json
    ;;
  *)
    usage
    [[ -n "$command" ]] || exit 0
    exit 2
    ;;
esac
