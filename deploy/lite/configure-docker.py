#!/usr/bin/env python3
"""Use the classic Docker image store required by Lite's config-digest image IDs."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


def uses_containerd(info):
    return ["driver-type", "io.containerd.snapshotter.v1"] in info.get("DriverStatus", [])


def prepare_config(config, info):
    if uses_containerd(info) and (info["Images"] or info["Containers"]):
        raise ValueError("refusing to switch a nonempty Docker image store; review existing images/containers first")
    features = config.get("features", {})
    if not isinstance(features, dict):
        raise ValueError("invalid Docker features configuration")
    return {**config, "features": {**features, "containerd-snapshotter": False}}


def docker_info():
    return json.loads(subprocess.check_output(["docker", "info", "--format", "{{json .}}"], text=True))


def main():
    if os.geteuid() != 0:
        raise ValueError("must run as root")
    path = Path("/etc/docker/daemon.json")
    if path.is_symlink():
        raise ValueError("Docker daemon configuration must not be a symlink")
    previous = path.read_bytes() if path.exists() else None
    config = json.loads(previous) if previous is not None else {}
    info = docker_info()
    updated = prepare_config(config, info)
    if updated != config:
        path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=".tz-lite-") as temporary:
            json.dump(updated, temporary, indent=2)
            temporary.write("\n")
            temporary.flush()
            subprocess.run(["dockerd", "--validate", "--config-file", temporary.name], check=True)
            path.write_text(Path(temporary.name).read_text())
            path.chmod(0o600)
    if uses_containerd(info):
        try:
            subprocess.run(["systemctl", "restart", "docker.service"], check=True)
            if uses_containerd(docker_info()):
                raise ValueError("Docker still uses the containerd image store")
        except (ValueError, OSError, subprocess.CalledProcessError):
            if previous is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(previous)
            subprocess.run(["systemctl", "restart", "docker.service"], check=False)
            raise
    print("Lite Docker: classic image store ready")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Lite Docker: {error}")
