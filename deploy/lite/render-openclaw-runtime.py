#!/usr/bin/env python3
"""Builder-only: retain reviewed agent policies, remove all source-build operations."""
import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
IMMUTABLE_IMAGE = re.compile(r"sha256:[0-9a-f]{64}\Z")


def render_build_context(source, output):
    """Copy only reviewed image inputs, adapting module paths and imports together."""
    paths = [source / name for name in (
        "Dockerfile", "Dockerfile.browser", "browser/package.json", "browser/package-lock.json")]
    paths.extend(path for path in (source / "bin").glob("*.mjs")
                 if not path.name.endswith((".test.mjs", ".smoke.mjs", "-test-runner.mjs")))
    for path in paths:
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"unsafe or missing OpenClaw build input: {path.name}")
        target = output / path.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(path.read_text())
        target.chmod(path.stat().st_mode & 0o777)


def runtime_script(source, commands, gateway_image):
    if not IMMUTABLE_IMAGE.fullmatch(gateway_image):
        raise ValueError("gateway image must be an immutable image ID")
    if source.count("\nusage() {\n") != 1:
        raise ValueError("OpenClaw command boundary changed; review runtime extraction")
    policy = source.split("\nusage() {\n", 1)[0]
    pinned = 'OPENCLAW_RELEASE_IMAGE="ghcr.io/openclaw/openclaw:2026.7.1-2"'
    if policy.count(pinned) != 1:
        raise ValueError("OpenClaw release pin changed; review runtime extraction")
    policy = policy.replace(pinned, f'OPENCLAW_RELEASE_IMAGE="{gateway_image}"')
    # No image selection from state or shell environment: generated Compose has IDs.
    compose_start = '  OPENCLAW_IMAGE="$OPENCLAW_RELEASE_IMAGE" docker compose \\\n'
    if policy.count(compose_start) != 1:
        raise ValueError("OpenClaw compose wrapper changed; review runtime extraction")
    policy = policy.replace(compose_start, '  docker compose \\\n')
    policy = policy.replace("compose run -T --rm --no-deps", "compose run --pull never -T --rm --no-deps")
    result = policy + "\n\n" + commands
    if re.search("tzomet", result, re.IGNORECASE):
        raise ValueError("runtime contains a legacy namespace")
    forbidden = r"\b(?:npm|npx|git)\s|\bplugins\s+install\b|\b(?:compose|docker)\s+(?:build|buildx|pull)\b"
    if re.search(forbidden, result):
        raise ValueError("runtime helper contains an install, source, build, or pull operation")
    for function in ("configure_gateway", "build_exec_approvals_file", "verify_support_agent_config", "verify_workspace_policy"):
        if policy.count(f"\n{function}() {{\n") != 1:
            raise ValueError(f"required policy function changed: {function}")
    return result


def runtime_compose(source, gateway_image, browser_image):
    if not all(IMMUTABLE_IMAGE.fullmatch(image) for image in (gateway_image, browser_image)):
        raise ValueError("images must use immutable IDs")
    services = source.get("services", {})
    if set(services) != {"openclaw-gateway", "browser-runner", "shell-runner"}:
        raise ValueError("OpenClaw services changed; review runtime adaptation")
    for name, service in services.items():
        service.pop("build", None)
        service["image"] = browser_image if name == "browser-runner" else gateway_image
        service["pull_policy"] = "never"
        service["platform"] = "linux/arm64"
        if "develop" in service:
            raise ValueError("runtime services must not carry development actions")
    return source


def render(output, gateway_image, browser_image):
    source = ROOT / "infra/openclaw"
    commands = (Path(__file__).parent / "openclaw-runtime-commands.sh").read_text()
    script = runtime_script((source / "manage.sh").read_text(), commands, gateway_image)
    try:
        result = subprocess.run([
            "docker", "compose", "--env-file", "/dev/null", "-f", str(source / "compose.yaml"),
            "config", "--no-interpolate", "--no-env-resolution", "--no-path-resolution", "--format", "json",
        ], check=True, capture_output=True, text=True)
    except subprocess.CalledProcessError as error:
        raise ValueError(f"could not render OpenClaw Compose: {(error.stderr or '').strip() or error}") from error
    compose = runtime_compose(json.loads(result.stdout), gateway_image, browser_image)
    output.mkdir(parents=True, exist_ok=True)
    (output / "workspace").mkdir(exist_ok=True)
    (output / "manage.sh").write_text(script)
    (output / "manage.sh").chmod(0o755)
    (output / "compose.yaml").write_text(json.dumps(compose, indent=2) + "\n")
    identity = (source / "workspace/IDENTITY.md").read_text()
    if re.search(r"tzomet|hinadex", identity, re.IGNORECASE):
        raise ValueError("the public identity must not contain product branding")
    (output / "workspace/IDENTITY.md").write_text(identity)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--gateway-image", required=True)
    parser.add_argument("--browser-image", required=True)
    args = parser.parse_args()
    try:
        render(args.output, args.gateway_image, args.browser_image)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"OpenClaw runtime: {error}\n")
