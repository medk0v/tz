#!/usr/bin/env python3
"""Create private Lite runtime configuration once; validate and preserve on reruns."""
import argparse
import base64
import os
from pathlib import Path
import re
import secrets
import stat
import tomllib
import uuid


def private_text(path, create=None):
    if not path.exists() and not path.is_symlink() and create is not None:
        with open(path, "x", opener=lambda name, flags: os.open(name, flags, 0o600)) as stream:
            stream.write(create())
    if path.is_symlink() or not stat.S_ISREG(path.stat().st_mode):
        raise ValueError(f"unsafe configuration path: {path.name}")
    return path.read_text()


def prepare(template, directory):
    config_path = directory / "Config.toml"
    existing = config_path.exists() or config_path.is_symlink()
    values = {}
    for name in ("database", "redis"):
        text = private_text(directory / f"{name}.password", None if existing else lambda: secrets.token_hex(32) + "\n")
        value = text.strip()
        if re.fullmatch(r"[0-9a-f]{64}", value) is None:
            raise ValueError(f"invalid saved {name} credential; refusing to rotate it")
        values[name] = value
    environment = private_text(directory / "tz.env", None if existing else lambda:
                               "TZ_SECRETS_KEY=" + base64.b64encode(secrets.token_bytes(32)).decode() + "\n")
    key = next((line.partition("=")[2] for line in environment.splitlines()
                if line.startswith("TZ_SECRETS_KEY=")), "")
    if len(base64.b64decode(key, validate=True)) != 32:
        raise ValueError("invalid saved encryption key; refusing to rotate it")
    if existing:
        rendered = private_text(config_path)
    else:
        rendered = template.read_text().replace("__DATABASE_PASSWORD__", values["database"]) \
            .replace("__REDIS_PASSWORD__", values["redis"]).replace("__PROJECT_ID__", str(uuid.uuid4()))
    config = tomllib.loads(rendered)
    if not uuid.UUID(config.get("product", {}).get("project-id", "")).int:
        raise ValueError("existing configuration is not a fixed-project instance")
    expected_database = f"postgres://tz:{values['database']}@127.0.0.1:5432/tz"
    expected_redis = f"redis://tz:{values['redis']}@127.0.0.1:16379"
    if config["pg"]["url"] != expected_database or config["redis"]["url"] != expected_redis:
        raise ValueError("saved credentials do not match configuration; refusing to overwrite")
    if config["server"]["allowed-origins"] != ["https://tz.cyber-money.org"] \
            or config["telegram"]["webhook-base-url"] != "https://tz.cyber-money.org":
        raise ValueError("configuration domain does not match this Lite installation")
    if not existing:
        private_text(config_path, lambda: rendered)


def prepare_openclaw(directory):
    environment = private_text(directory / ".env", lambda:
                               "OPENCLAW_GATEWAY_TOKEN=" + secrets.token_hex(32) + "\nOPENCLAW_TZ=Europe/Istanbul\n")
    token = next((line.partition("=")[2] for line in environment.splitlines()
                  if line.startswith("OPENCLAW_GATEWAY_TOKEN=")), "")
    if re.fullmatch(r"[0-9a-f]{64}", token) is None:
        raise ValueError("invalid saved OpenClaw gateway token; refusing to rotate it")
    if not any(line.startswith("OPENCLAW_TZ=") and line.partition("=")[2] for line in environment.splitlines()):
        raise ValueError("saved OpenClaw environment is missing its timezone")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--template", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--openclaw-directory", type=Path, required=True)
    args = parser.parse_args()
    try:
        prepare(args.template, args.directory)
        prepare_openclaw(args.openclaw_directory)
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"Lite configuration: {error}\n")
