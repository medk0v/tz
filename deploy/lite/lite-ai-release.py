#!/usr/bin/env python3
"""Validate and package the prebuilt Lite AI runtime (Python standard library only)."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import tarfile

FILES = {"images.tar", "openclaw/compose.yaml", "openclaw/manage.sh",
         "openclaw/workspace/IDENTITY.md", "firecrawl/compose.yaml", "firecrawl/manage.sh", "firecrawl/env.example"}
SERVICES = {"openclaw": {"openclaw-gateway", "shell-runner", "browser-runner"},
            "firecrawl": {"api", "playwright-service", "redis", "rabbitmq", "nuq-postgres"}}
IMAGE = re.compile(r"sha256:[0-9a-f]{64}")


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(stream):
    result = hashlib.sha256()
    for data in iter(lambda: stream.read(1024 * 1024), b""):
        result.update(data)
    return result.hexdigest()


def check_compose(model, stack, images):
    require(not re.search("tzomet", json.dumps(model), re.IGNORECASE), "legacy namespace in Lite compose")
    require(set(model) <= {"name", "services", "networks", "volumes"}, "unexpected runtime compose extension/input")
    require(set(model.get("services", {})) == SERVICES[stack], "unexpected runtime services")
    require(set(images) == SERVICES[stack], "unexpected image service list")
    require(not any(key in model for key in ("include", "configs", "secrets")), "external compose inputs forbidden")
    for name, service in model["services"].items():
        require(not any(key in service for key in ("build", "develop", "extends")), "runtime cannot build images")
        require(IMAGE.fullmatch(images[name]) and service.get("image") == images[name], "unpinned image")
        require(service.get("pull_policy") == "never", "runtime cannot pull images")
        require(service.get("platform") == "linux/arm64", "runtime platform must be ARM64")
        for port in service.get("ports", []):
            require(isinstance(port, dict) and port.get("host_ip") == "127.0.0.1", "AI ports must bind localhost")


def render_firecrawl(model, images):
    model = json.loads(json.dumps(model).replace("tzomet", "tz").replace("TZOMET", "TZ").replace("Tzomet", "TZ"))
    model = {key: value for key, value in model.items() if key in {"name", "services", "networks", "volumes"}}
    model["name"] = "tz-firecrawl"
    model["services"] = {name: value for name, value in model["services"].items()
                         if name in SERVICES["firecrawl"]}
    for name, service in model["services"].items():
        service.pop("build", None)
        service.update(image=images[name], platform="linux/arm64", pull_policy="never")
        require(not any(volume.get("type") == "bind" for volume in service.get("volumes", [])),
                "Firecrawl must not depend on source bind mounts")
    # Existing production starts the PostgreSQL queue, never experimental FoundationDB.
    api = model["services"]["api"]
    api.pop("volumes", None)
    api["environment"].pop("FDB_CLUSTER_FILE", None)
    api["environment"]["NUQ_BACKEND"] = ""
    model["volumes"] = {key: value for key, value in model.get("volumes", {}).items()
                        if key == "firecrawl-postgres"}
    # No build-host generated resource names in the runtime artifact.
    for kind in ("networks", "volumes"):
        for item in model.get(kind, {}).values():
            if item and not item.get("external"):
                item.pop("name", None)
    check_compose(model, "firecrawl", images)
    return model


def check_image_archive(stream, expected):
    """Check saved image config digests/architecture before invoking Docker load."""
    configs = {}
    manifest = None
    with tarfile.open(fileobj=stream, mode="r|") as image_tar:
        for count, member in enumerate(image_tar):
            parts = PurePosixPath(member.name).parts
            require(count < 100_000 and not member.name.startswith("/") and ".." not in parts,
                    "unsafe saved-image archive path")
            require(member.isfile() or member.isdir(), "saved-image archive contains a link/special file")
            if member.name == "manifest.json":
                require(member.size < 2 ** 20, "image manifest is too large")
                manifest = json.load(image_tar.extractfile(member))
            elif (member.name.removesuffix(".json").split("/")[-1] in {item[7:] for item in expected}):
                require(member.size < 8 * 2 ** 20, "image config is too large")
                content = image_tar.extractfile(member).read()
                image_id = "sha256:" + hashlib.sha256(content).hexdigest()
                config = json.loads(content)
                require(config.get("architecture") == "arm64" and config.get("os") == "linux",
                        "saved image is not linux/arm64")
                configs[member.name] = image_id
    require(isinstance(manifest, list) and len(manifest) > 0, "saved images have no Docker manifest")
    actual = {configs.get(item.get("Config")) for item in manifest}
    require(actual == expected, "saved image identities do not match release manifest")


def check_metadata(metadata, release):
    require(isinstance(metadata, dict) and metadata.get("format") == 1
            and metadata.get("release") == release and metadata.get("edition") == "lite"
            and metadata.get("architecture") == "aarch64", "AI release metadata mismatch")
    require(set(metadata.get("files", {})) == FILES, "AI file manifest mismatch")
    images = metadata.get("images", {})
    require(set(images) == set(SERVICES), "AI stack manifest mismatch")
    for stack in SERVICES:
        require(set(images[stack]) == SERVICES[stack] and
                all(isinstance(value, str) and IMAGE.fullmatch(value) for value in images[stack].values()),
                "AI image manifest mismatch")


def validate(archive, checksum_path, release, destination=None):
    require(re.fullmatch(r"[0-9a-f]{40}", release), "invalid release SHA")
    require(archive.is_file() and not archive.is_symlink() and checksum_path.is_file()
            and not checksum_path.is_symlink(), "unsafe AI input files")
    require(checksum_path.stat().st_size < 256, "oversized checksum")
    checksum = checksum_path.read_text().strip()
    match = re.fullmatch(r"([0-9a-f]{64})  tz-lite-ai-" + release + r"\.tar\.gz", checksum)
    require(match, "invalid AI checksum file")
    with archive.open("rb") as stream:
        require(sha(stream) == match[1], "AI archive checksum mismatch")
        stream.seek(0)
        with tarfile.open(fileobj=stream, mode="r:gz") as bundle:
            members = {}
            total = 0
            for member in bundle:
                require(member.name in FILES | {"release.json"} and member.isfile(),
                        "unexpected AI archive member/link/source")
                require(member.name not in members and not member.mode & 0o7000, "duplicate/unsafe AI member")
                require(member.size <= (80 * 2 ** 30 if member.name == "images.tar" else 8 * 2 ** 20),
                        "AI artifact too large")
                total += member.size
                require(total <= 81 * 2 ** 30, "AI release too large")
                members[member.name] = member
            require(set(members) == FILES | {"release.json"}, "incomplete AI runtime bundle")
            metadata = json.load(bundle.extractfile(members["release.json"]))
            check_metadata(metadata, release)
            for name, expected in metadata["files"].items():
                require(re.fullmatch(r"[0-9a-f]{64}", expected), "invalid AI file digest")
                require(sha(bundle.extractfile(members[name])) == expected, "AI member checksum mismatch")
                if name != "images.tar":
                    content = bundle.extractfile(members[name]).read().decode("utf-8")
                    require(not re.search("tzomet", content, re.IGNORECASE), "legacy namespace in Lite runtime")
            for stack in SERVICES:
                model = json.load(bundle.extractfile(members[f"{stack}/compose.yaml"]))
                check_compose(model, stack, metadata["images"][stack])
            expected_images = {value for stack in metadata["images"].values() for value in stack.values()}
            check_image_archive(bundle.extractfile(members["images.tar"]), expected_images)
            if destination:
                require(not destination.exists() and not destination.is_symlink(), "AI destination exists")
                destination.mkdir(mode=0o700)
                for name, member in members.items():
                    target = destination / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with bundle.extractfile(member) as source, target.open("xb") as output:
                        shutil.copyfileobj(source, output)
                    target.chmod(0o755 if name.endswith("manage.sh") else 0o644)
    return metadata


def package(directory, images, release, output):
    require(re.fullmatch(r"[0-9a-f]{40}", release), "invalid release SHA")
    hashes = {}
    for name in FILES:
        path = directory / name
        require(path.is_file() and not path.is_symlink(), f"missing runtime file: {name}")
        with path.open("rb") as stream:
            hashes[name] = sha(stream)
    metadata = {"format": 1, "release": release, "edition": "lite", "architecture": "aarch64",
                "images": images, "files": hashes}
    check_metadata(metadata, release)
    (directory / "release.json").write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"tz-lite-ai-{release}.tar.gz"
    require(not archive.exists(), "AI output release already exists")
    with tarfile.open(archive, "w:gz") as bundle:
        for name in sorted(FILES | {"release.json"}):
            path = directory / name
            info = bundle.gettarinfo(str(path), arcname=name)
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            info.mtime = 0
            info.mode = 0o755 if name.endswith("manage.sh") else 0o644
            with path.open("rb") as stream:
                bundle.addfile(info, stream)
    checksum = Path(str(archive) + ".sha256")
    with archive.open("rb") as stream:
        checksum.write_text(f"{sha(stream)}  {archive.name}\n")
    validate(archive, checksum, release)
    print(archive)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("validate")
    check.add_argument("archive", type=Path)
    check.add_argument("--checksum-file", type=Path, required=True)
    check.add_argument("--destination", type=Path)
    check.add_argument("--release", required=True)
    render = commands.add_parser("render-firecrawl")
    render.add_argument("--model", type=Path, required=True)
    render.add_argument("--images", type=Path, required=True)
    render.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "validate":
            validate(args.archive, args.checksum_file, args.release, args.destination)
        else:
            args.output.write_text(json.dumps(render_firecrawl(json.loads(args.model.read_text()),
                                                              json.loads(args.images.read_text())), indent=2) + "\n")
    except (ValueError, OSError, tarfile.TarError) as error:
        parser.exit(1, f"Lite AI release: {error}\n")


if __name__ == "__main__":
    main()
