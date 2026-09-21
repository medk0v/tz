# Appended to the verified policy functions by render-openclaw-runtime.py.

require_preloaded_images() {
  local image_id images
  images="$(compose config --images)" || fail "could not resolve runtime image IDs"
  [[ -n "$images" ]] || fail "runtime image list is empty"
  while IFS= read -r image_id; do
    [[ "$image_id" =~ ^sha256:[0-9a-f]{64}$ ]] || fail "runtime image must be immutable"
    [[ "$(docker image inspect --format '{{.Architecture}}' "$image_id")" == arm64 ]] \
      || fail "preloaded ARM64 image is missing"
  done <<< "$images"
}

configure_lite_agent_runtime() {
  local settings
  # OpenAI defaults to Codex, whose local execution cannot enforce our allowlist.
  # Keep the embedded runtime override scoped to the support agent and model.
  settings="$(compose run --pull never -T --rm --no-deps --entrypoint node openclaw-gateway -e '
    const fs = require("node:fs");
    const config = JSON.parse(fs.readFileSync(process.env.OPENCLAW_CONFIG_PATH, "utf8"));
    const matches = (config.agents?.list ?? []).map((agent, index) => ({ agent, index }))
      .filter(({ agent }) => agent.id === "support");
    if (matches.length !== 1) throw new Error("exactly one support agent is required");
    const { agent, index } = matches[0];
    const model = "openai/gpt-5.6-luna";
    const models = { ...agent.models, [model]: {
      ...agent.models?.[model],
      agentRuntime: { ...agent.models?.[model]?.agentRuntime, id: "openclaw" },
    } };
    process.stdout.write(JSON.stringify([{ path: `agents.list[${index}].models`, value: models }]));
  ')" || fail "could not select the Lite support agent runtime"
  run_cli_before_gateway config set --batch-json "$settings" >/dev/null
}

verify_lite_agent_runtime() {
  compose run --pull never -T --rm --no-deps --entrypoint node openclaw-gateway -e '
    const fs = require("node:fs");
    const config = JSON.parse(fs.readFileSync(process.env.OPENCLAW_CONFIG_PATH, "utf8"));
    const matches = (config.agents?.list ?? []).filter(agent => agent.id === "support");
    if (matches.length !== 1
        || matches[0].models?.["openai/gpt-5.6-luna"]?.agentRuntime?.id !== "openclaw") {
      throw new Error("Lite support OpenAI runtime must use openclaw");
    }
  ' >/dev/null || fail "Lite support agent runtime drifted"
}

configure_baked_firecrawl() {
  compose run --pull never -T --rm --no-deps --entrypoint node openclaw-gateway -e '
    const fs = require("node:fs");
    const root = "/opt/tz-plugins/firecrawl/node_modules/@openclaw/firecrawl-plugin";
    const p = JSON.parse(fs.readFileSync(root + "/package.json", "utf8"));
    const manifest = JSON.parse(fs.readFileSync(root + "/openclaw.plugin.json", "utf8"));
    if(p.name !== "@openclaw/firecrawl-plugin" || p.version !== "2026.7.1" || manifest.id !== "firecrawl") process.exit(1);
  '
  run_cli_before_gateway config set --batch-json '[
    {"path":"plugins.load.paths","value":["/opt/tz-plugins/firecrawl/node_modules/@openclaw/firecrawl-plugin"]},
    {"path":"plugins.entries.firecrawl.enabled","value":true},
    {"path":"plugins.entries.firecrawl.config.webFetch","value":{
      "apiKey":"self-hosted-local","baseUrl":"http://firecrawl-api:3002",
      "onlyMainContent":true,"maxAgeMs":172800000,"timeoutSeconds":60
    }},
    {"path":"tools.web.fetch.provider","value":"firecrawl"}
  ]' >/dev/null
}

verify_baked_firecrawl() {
  run_cli_before_gateway plugins inspect firecrawl --json \
    | jq -e '.plugin.version == "2026.7.1"' >/dev/null \
    || fail "baked Firecrawl plugin did not load at its pinned version"
}

smoke_firecrawl() {
  local container_id
  container_id="$(compose ps -q openclaw-gateway)"
  [[ -n "$container_id" ]] || fail "OpenClaw gateway is not running"
  docker exec "$container_id" node -e '
    fetch("http://firecrawl-api:3002/v0/health/readiness")
      .then(response => process.exit(response.ok ? 0 : 1)).catch(() => process.exit(1));
  '
  docker exec "$container_id" node -e '
    fetch("http://127.0.0.1:18789/tools/invoke", {
      method:"POST",
      headers:{Authorization:"Bearer " + process.env.OPENCLAW_GATEWAY_TOKEN,"Content-Type":"application/json"},
      body:JSON.stringify({tool:"firecrawl_scrape",args:{url:"https://example.com",extractMode:"markdown",maxChars:180,onlyMainContent:true,timeoutSeconds:60}})
    }).then(async response => {
      const body = await response.json();
      const text = body?.result?.content?.find(item => item.type === "text")?.text;
      const result = JSON.parse(text ?? "null");
      if (!response.ok || body?.ok !== true || result?.url !== "https://example.com" || result?.status !== 200) process.exit(1);
      console.log("OpenClaw Lite: Firecrawl scrape succeeded");
    }).catch(error => { console.error(error.message); process.exit(1); });
  '
}

command="${1:-}"
case "$command" in
  start|restart)
    require_command curl
    require_command jq
    prepare_local_state
    # The existing receiver owns the state root but may only know the old workspace.
    install -d -o 1000 -g 1000 -m 0700 "$RUNTIME_DIR/state/workspace-support"
    install_workspace_policy
    ensure_shared_network
    require_preloaded_images
    compose stop openclaw-gateway shell-runner browser-runner >/dev/null 2>&1 || true
    configure_gateway
    configure_lite_agent_runtime
    configure_baked_firecrawl
    install_workspace_policy
    verify_tz_agent_config
    verify_lite_agent_runtime
    verify_baked_firecrawl
    compose up -d --no-build --pull never shell-runner browser-runner openclaw-gateway
    wait_until_live
    ;;
  stop)
    compose down
    ;;
  doctor)
    require_preloaded_images
    verify_tz_agent_config
    verify_lite_agent_runtime
    verify_tz_model
    verify_baked_firecrawl
    run_cli_before_gateway doctor --lint --json --severity-min error
    ;;
  smoke)
    require_preloaded_images
    smoke_test
    verify_lite_agent_runtime
    verify_baked_firecrawl
    ;;
  connect-firecrawl)
    require_preloaded_images
    configure_baked_firecrawl
    compose restart openclaw-gateway >/dev/null
    wait_until_live
    verify_baked_firecrawl
    smoke_firecrawl
    ;;
  status)
    compose ps
    ;;
  *)
    echo 'Usage: manage.sh start|restart|stop|doctor|smoke|connect-firecrawl|status' >&2
    exit 64
    ;;
esac
