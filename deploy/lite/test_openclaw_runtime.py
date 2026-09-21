import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

DIRECTORY = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("openclaw_runtime", DIRECTORY / "render-openclaw-runtime.py")
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)
GATEWAY = "sha256:" + "a" * 64
BROWSER = "sha256:" + "b" * 64


class OpenClawRuntimeTests(unittest.TestCase):
    def test_lite_openai_runtime_override_preserves_main_models_and_exec_policy(self):
        commands = (DIRECTORY / "openclaw-runtime-commands.sh").read_text()
        writer = commands.split("\nconfigure_lite_agent_runtime() {\n", 1)[1].split(
            "\nverify_lite_agent_runtime() {\n", 1)[0]
        writer_js = writer.split("-e '", 1)[1].split("\n  ')", 1)[0]
        verifier = commands.split("\nverify_lite_agent_runtime() {\n", 1)[1].split(
            "\nconfigure_baked_firecrawl() {\n", 1)[0]
        verifier_js = verifier.split("-e '", 1)[1].split("\n  ' >/dev/null", 1)[0]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "openclaw.json"
            environment = {**os.environ, "OPENCLAW_CONFIG_PATH": str(path)}
            for existing_models in (None, {
                "openai/gpt-5.6-luna": {"alias": "Primary", "params": {"temperature": 0.2},
                                   "agentRuntime": {"id": "codex"}},
                "openai/gpt-4.1": {"alias": "Secondary"},
            }):
                with self.subTest(existing_models=existing_models):
                    config = {"agents": {"defaults": {"model": {"primary": "openai/gpt-5.6-luna"}}, "list": [
                        {"id": "main", "models": {"openai/gpt-5.6-luna": {"agentRuntime": {"id": "codex"}}}},
                        {"id": "support", "tools": {"exec": {"security": "allowlist", "host": "gateway"}}},
                    ]}, "tools": {"exec": {"security": "allowlist"}}}
                    if existing_models is not None:
                        config["agents"]["list"][1]["models"] = existing_models
                    path.write_text(json.dumps(config))
                    generated = subprocess.run(["node", "-e", writer_js], env=environment,
                                               check=True, capture_output=True, text=True)
                    changes = json.loads(generated.stdout)
                    self.assertEqual(len(changes), 1)
                    self.assertEqual(changes[0]["path"], "agents.list[1].models")
                    expected = json.loads(json.dumps(config))
                    expected_models = expected["agents"]["list"][1].setdefault("models", {})
                    expected_models.setdefault("openai/gpt-5.6-luna", {})["agentRuntime"] = {"id": "openclaw"}
                    config["agents"]["list"][1]["models"] = changes[0]["value"]
                    self.assertEqual(config, expected)
                    path.write_text(json.dumps(config))
                    subprocess.run(["node", "-e", verifier_js], env=environment,
                                   check=True, capture_output=True, text=True)
                    repeated = subprocess.run(["node", "-e", writer_js], env=environment,
                                              check=True, capture_output=True, text=True)
                    self.assertEqual(json.loads(repeated.stdout), changes)
                    for rejected_runtime in (None, "auto", "codex"):
                        config["agents"]["list"][1]["models"]["openai/gpt-5.6-luna"]["agentRuntime"] = (
                            {} if rejected_runtime is None else {"id": rejected_runtime})
                        path.write_text(json.dumps(config))
                        rejected = subprocess.run(["node", "-e", verifier_js], env=environment,
                                                  capture_output=True, text=True)
                        self.assertNotEqual(rejected.returncode, 0)
                        self.assertIn("Lite support OpenAI runtime must use openclaw", rejected.stderr)
            path.write_text(json.dumps({"agents": {"list": [{"id": "main"}]}}))
            rejected = subprocess.run(["node", "-e", writer_js], env=environment, capture_output=True, text=True)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("exactly one support agent is required", rejected.stderr)
        self.assertIn("    configure_gateway\n    configure_lite_agent_runtime\n", commands)
        for branch in ("start|restart)", "doctor)", "smoke)"):
            body = commands.split(branch, 1)[1].split("    ;;", 1)[0]
            self.assertIn("    verify_lite_agent_runtime\n", body)

    def test_build_context_copies_only_reviewed_inputs(self):
        source = runtime.ROOT / "infra/openclaw"
        original = (source / "Dockerfile").read_text()
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            runtime.render_build_context(source, output)
            self.assertTrue((output / "bin/support-shell-runner.mjs").is_file())
            self.assertTrue((output / "bin/support-test-tool.mjs").is_file())
            for path in output.rglob("*"):
                self.assertNotRegex(str(path.relative_to(output)), "(?i)tzomet")
                if path.is_file():
                    self.assertNotRegex(path.read_text(), "(?i)tzomet")
            dockerfile = (output / "Dockerfile").read_text()
            for line in dockerfile.splitlines():
                if line.startswith("COPY "):
                    for name in line.split()[1:-1]:
                        self.assertTrue((output / name).is_file(), name)
            self.assertIn('/run/support-agent-secrets', (output / "bin/support-shell.mjs").read_text())
            self.assertIn('"tz_test"', (output / "bin/support-test-tool.mjs").read_text())
        self.assertEqual((source / "Dockerfile").read_text(), original)

    def test_lite_approvals_replace_legacy_names_and_keep_other_agents_denied(self):
        source = (runtime.ROOT / "infra/openclaw/manage.sh").read_text()
        commands = (DIRECTORY / "openclaw-runtime-commands.sh").read_text()
        policy = runtime.runtime_script(source, commands, GATEWAY)
        writer = policy.split("\nbuild_exec_approvals_file() {\n")[1].split("\nverify_support_agent_config() {\n")[0]
        writer_js = writer.split("-e '", 1)[1].rsplit("'", 1)[0]
        verifier = policy.split("\nverify_support_agent_config() {\n")[1].split("\nverify_support_model() {\n")[0]
        verifier_js = verifier.split("-e '", 1)[1].split("\n    ' >/dev/null", 1)[0]
        names = ["telegram-notify", "resolve-conversation", "schedule-reminder", "public-http",
                 "browser", "knowledge-article", "integration", "shell"]
        neutral = [f"/usr/local/bin/support-{name}" for name in names]
        legacy = [f"/usr/local/bin/tz-{name}" for name in names]
        config = {"agents": {"list": [
            {"id": "main", "default": True},
            {"id": "support", "default": False,
             "workspace": "/home/node/.openclaw/workspace-support",
             "agentDir": "/home/node/.openclaw/agents/support/agent",
             "skills": [], "memorySearch": {"enabled": False}, "contextInjection": "never",
             "sandbox": {"mode": "off"},
             "tools": {"allow": ["exec"], "exec": {"host": "gateway", "security": "allowlist", "ask": "off", "safeBins": []},
                       "elevated": {"enabled": False}}},
            {"id": "other", "default": False},
            {"id": "tzomet", "default": False},
        ]}, "tools": {"exec": {"host": "gateway", "security": "allowlist", "ask": "off"},
                      "elevated": {"enabled": False}},
            "session": {"maintenance": {"mode": "enforce", "pruneAfter": "7d", "maxEntries": 500,
                        "resetArchiveRetention": "7d", "maxDiskBytes": "500mb", "highWaterBytes": "400mb"}}}
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            config_path = directory / "openclaw.json"
            approvals_path = directory / "exec-approvals.json"
            config_path.write_text(json.dumps(config))
            approvals_path.write_text(json.dumps({"agents": {
                "*": {"allowlist": [{"pattern": name} for name in legacy + neutral]},
                "other": {"allowlist": [{"pattern": "/usr/bin/true"}]},
                "tzomet": {"allowlist": [{"pattern": name} for name in legacy]},
            }}))
            environment = {**os.environ, "OPENCLAW_CONFIG_PATH": str(config_path), "OPENCLAW_STATE_DIR": str(directory)}
            generated = subprocess.run(["node", "-e", writer_js], env=environment, check=True, capture_output=True, text=True)
            approvals = json.loads(generated.stdout)
            active = approvals["agents"]["support"]
            self.assertEqual({entry["pattern"] for entry in active["allowlist"]}, set(neutral + ["/usr/local/bin/curl"]))
            self.assertEqual(active["security"], "allowlist")
            self.assertEqual(active["askFallback"], "deny")
            self.assertFalse(active["autoAllowSkills"])
            self.assertNotIn("*", approvals["agents"])
            self.assertEqual(approvals["agents"]["main"]["allowlist"], [])
            self.assertEqual(approvals["agents"]["tzomet"]["allowlist"], [])
            self.assertEqual(approvals["agents"]["other"]["allowlist"], [{"pattern": "/usr/bin/true"}])
            approvals_path.write_text(json.dumps(approvals))
            subprocess.run(["node", "-e", verifier_js], env=environment, check=True, capture_output=True, text=True)
            approvals["agents"]["other"]["allowlist"].append({"pattern": legacy[0]})
            approvals_path.write_text(json.dumps(approvals))
            rejected = subprocess.run(["node", "-e", verifier_js], env=environment, capture_output=True, text=True)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("wrapper remains approved for another agent", rejected.stderr)

    def test_lite_identity_keeps_conversation_policy_without_branding(self):
        compose = {"services": {name: {} for name in (
            "openclaw-gateway", "browser-runner", "shell-runner")}}
        with tempfile.TemporaryDirectory() as directory, patch.object(
            runtime.subprocess, "run", return_value=SimpleNamespace(stdout=json.dumps(compose))
        ):
            output = Path(directory)
            runtime.render(output, GATEWAY, BROWSER)
            identity = (output / "workspace/IDENTITY.md").read_text()
            self.assertNotRegex(identity.lower(), "tzomet|hinadex")
            self.assertIn("The application selects the client-facing name", identity)
            self.assertIn("application-managed public identity", identity)

    def test_runtime_keeps_exact_agent_policy_and_removes_build_dispatch(self):
        source = (runtime.ROOT / "infra/openclaw/manage.sh").read_text()
        commands = (DIRECTORY / "openclaw-runtime-commands.sh").read_text()
        result = runtime.runtime_script(source, commands, GATEWAY)
        self.assertNotRegex(result, "(?i)tzomet")
        neutral_source = source
        for function, following in (
            ("configure_gateway", "find_support_agent_index"),
            ("build_exec_approvals_file", "verify_support_agent_config"),
            ("verify_support_agent_config", "verify_support_model"),
        ):
            policy = neutral_source.split(f"\n{function}() {{\n")[1].split(f"\n{following}() {{\n")[0]
            expected = policy.replace("compose run -T", "compose run --pull never -T")
            self.assertIn(expected, result)
        self.assertNotIn("compose build", result)
        self.assertNotIn("plugins install", result)
        self.assertIn("up -d --no-build --pull never", result)
        self.assertIn('manifest.id !== "firecrawl"', result)
        self.assertIn("agents add support", result)
        self.assertIn("openclaw/support", result)
        self.assertIn("/home/node/.openclaw/workspace-support", result)
        self.assertNotIn("workspace-tzomet", result)
        self.assertNotIn("openclaw/tzomet", result)
        self.assertIn('install -d -o 1000 -g 1000 -m 0700 "$RUNTIME_DIR/state/workspace-support"', result)
        with tempfile.NamedTemporaryFile(mode="w", suffix=".sh") as script:
            script.write(result)
            script.flush()
            subprocess.run(["bash", "-n", script.name], check=True)

    def test_new_source_build_commands_are_rejected(self):
        source = (runtime.ROOT / "infra/openclaw/manage.sh").read_text()
        source = source.replace("configure_gateway() {", "configure_gateway() {\n  npm install unwanted")
        with self.assertRaisesRegex(ValueError, "runtime helper contains"):
            runtime.runtime_script(source, "", GATEWAY)

    def test_compose_requires_immutable_images_and_drops_build_context(self):
        source = {"services": {name: {"build": {"context": "source"}, "image": "mutable:latest"}
                              for name in ["openclaw-gateway", "browser-runner", "shell-runner"]}}
        output = runtime.runtime_compose(source, GATEWAY, BROWSER)
        for name, service in output["services"].items():
            self.assertNotIn("build", service)
            self.assertEqual(service["image"], BROWSER if name == "browser-runner" else GATEWAY)
            self.assertEqual(service["pull_policy"], "never")
            self.assertEqual(service["platform"], "linux/arm64")
        with self.assertRaisesRegex(ValueError, "immutable"):
            runtime.runtime_compose(source, "mutable:latest", BROWSER)

    def test_rendered_runtime_is_only_compose_policy_and_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            runtime.render(output, GATEWAY, BROWSER)
            files = {str(path.relative_to(output)) for path in output.rglob("*") if path.is_file()}
            self.assertEqual(files, {"manage.sh", "compose.yaml", "workspace/IDENTITY.md"})
            compose = json.loads((output / "compose.yaml").read_text())
            self.assertEqual(compose["services"]["openclaw-gateway"]["ports"][0]["host_ip"], "127.0.0.1")
            self.assertEqual(compose["services"]["shell-runner"]["network_mode"], "none")
            self.assertEqual(compose["services"]["browser-runner"]["network_mode"], "none")
            serialized = json.dumps(compose)
            self.assertNotRegex(serialized, "(?i)tzomet")
            self.assertEqual(compose["name"], "support-openclaw")
            self.assertEqual(compose["networks"]["support-ai"]["name"], "support-ai")
            self.assertNotIn(str(runtime.ROOT), serialized)
            self.assertNotIn("Dockerfile", serialized)

    def test_rendered_runtime_resolves_host_directories_as_bind_mounts(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            runtime.render(output, GATEWAY, BROWSER)
            # Compose v2 checks service env_file existence even with --no-env-resolution.
            (output / ".env").touch()
            for overrides in ({}, {
                "OPENCLAW_GRANT_HOST_DIR": "/srv/example/agent-secrets",
                "OPENCLAW_ACTION_HOST_DIR": "/srv/example/agent-actions",
                "OPENCLAW_SHELL_HOST_DIR": "/srv/example/shell",
            }):
                with self.subTest(overrides=overrides):
                    result = subprocess.run([
                        "docker", "compose", "--env-file", "/dev/null", "-f", str(output / "compose.yaml"),
                        "config", "--no-env-resolution", "--no-path-resolution", "--format", "json",
                    ], capture_output=True, text=True, cwd=output, env={
                        "PATH": os.environ["PATH"], "OPENCLAW_GATEWAY_TOKEN": "local-test-only", **overrides,
                    })
                    self.assertEqual(result.returncode, 0, result.stderr)
                    services = json.loads(result.stdout)["services"]
                    for service, variable, target, default, read_only in (
                        ("openclaw-gateway", "OPENCLAW_GRANT_HOST_DIR", "/run/support-agent-secrets", "./runtime/agent-secrets", True),
                        ("openclaw-gateway", "OPENCLAW_ACTION_HOST_DIR", "/run/support-agent-actions", "./runtime/agent-actions", False),
                        ("openclaw-gateway", "OPENCLAW_SHELL_HOST_DIR", "/run/support-shell", "./runtime/shell", True),
                        ("shell-runner", "OPENCLAW_SHELL_HOST_DIR", "/run/support-shell", "./runtime/shell", False),
                    ):
                        mount = next(item for item in services[service]["volumes"] if item["target"] == target)
                        self.assertEqual(mount["type"], "bind")
                        self.assertEqual(mount["source"], overrides.get(variable, default))
                        self.assertEqual(mount.get("read_only", False), read_only)

    def test_compose_failure_includes_the_actual_diagnostic(self):
        error = subprocess.CalledProcessError(1, ["docker", "compose"], stderr="undefined volume: invalid compose project")
        with tempfile.TemporaryDirectory() as directory, patch.object(runtime.subprocess, "run", side_effect=error):
            with self.assertRaisesRegex(ValueError, "undefined volume: invalid compose project"):
                runtime.render(Path(directory), GATEWAY, BROWSER)


if __name__ == "__main__":
    unittest.main()
