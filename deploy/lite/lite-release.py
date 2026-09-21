#!/usr/bin/env python3
"""Package and validate source-free application releases using only the stdlib."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import struct
import tarfile
import tempfile


MACHINES = {"aarch64": 183, "x86_64": 62}
GEOIP_FILES = {"geoip/GeoLite2-City.mmdb", "geoip/GeoLite2-Country.mmdb"}
REQUIRED = {
    "bin/api", "bin/worker", "bin/bootstrap-admin", "frontend/admin/index.html",
    "frontend/widget/widget.html", "frontend/loader/widget-loader.js",
} | GEOIP_FILES
STATIC_SUFFIXES = {
    ".html", ".js", ".css", ".png", ".jpg", ".jpeg", ".svg", ".gif",
    ".webp", ".avif", ".ico", ".woff", ".woff2", ".ttf", ".otf",
    ".json", ".txt", ".xml", ".webmanifest", ".wasm", ".mp4", ".webm",
}
MAX_FILES = 20_000
MAX_BYTES = 4 * 1024 ** 3


def require(condition, message):
    if not condition:
        raise ValueError(message)


def valid_sha(value):
    require(re.fullmatch(r"[0-9a-f]{40}", value), "invalid release SHA")
    return value


def valid_path(name, directory=False):
    parts = name.split("/")
    require(all(re.fullmatch(r"[A-Za-z0-9_@+.,=-]+", part)
                and part not in {".", ".."} and not part.startswith(".")
                for part in parts), f"unsafe archive path: {name}")
    if directory:
        allowed = name in {"bin", "frontend", "geoip"} or (
            len(parts) >= 2 and parts[:2] in [
                ["frontend", "admin"], ["frontend", "widget"], ["frontend", "loader"]])
    else:
        allowed = name in ({"bin/api", "bin/worker", "bin/bootstrap-admin", "release.json"} | GEOIP_FILES) or (
            len(parts) >= 3 and parts[:2] in [
                ["frontend", "admin"], ["frontend", "widget"], ["frontend", "loader"]]
            and PurePosixPath(name).suffix.lower() in STATIC_SUFFIXES)
    require(allowed, f"file is outside the runtime allowlist: {name}")


def check_elf(header, architecture, name):
    require(len(header) >= 64 and header[:7] == b"\x7fELF\x02\x01\x01",
            f"{name} is not a 64-bit little-endian ELF executable")
    elf_type, machine, version = struct.unpack_from("<HHI", header, 16)
    require(elf_type in {2, 3} and version == 1 and machine == MACHINES[architecture],
            f"{name} does not target {architecture}")


def digest(stream):
    result = hashlib.sha256()
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        result.update(block)
    return result.hexdigest()


def regular_file(path):
    require(stat.S_ISREG(path.lstat().st_mode), f"not a regular file: {path}")


def package(args):
    valid_sha(args.release)
    files = {}
    for name in ("api", "worker", "bootstrap-admin"):
        path = args.backend_dir / name
        regular_file(path)
        with path.open("rb") as stream:
            check_elf(stream.read(64), args.arch, name)
        files[f"bin/{name}"] = path
    require(args.geoip_dir.is_dir() and not args.geoip_dir.is_symlink(), "missing/unsafe GeoIP directory")
    for name in sorted(GEOIP_FILES):
        path = args.geoip_dir / PurePosixPath(name).name
        regular_file(path)
        require(path.stat().st_size > 0, f"empty GeoIP database: {name}")
        files[name] = path
    for bundle in ("admin", "widget", "loader"):
        root = args.frontend_dir / bundle
        require(root.is_dir() and not root.is_symlink(), f"missing bundle: {bundle}")
        for path in sorted(root.rglob("*")):
            name = "frontend/" + path.relative_to(args.frontend_dir).as_posix()
            require(not path.is_symlink(), f"build contains a symlink: {path}")
            valid_path(name, path.is_dir())
            if path.is_dir():
                continue
            regular_file(path)
            files[name] = path
    require(REQUIRED <= files.keys(), "required runtime files are missing")
    hashes = {}
    for name, path in files.items():
        with path.open("rb") as stream:
            hashes[name] = digest(stream)
    metadata = {"format": 1, "release": args.release, "architecture": args.arch,
                "edition": "lite", "files": hashes}
    args.output_dir.mkdir(parents=True, exist_ok=True)
    archive = args.output_dir / f"tz-lite-{args.release}.tar.gz"
    checksum = Path(str(archive) + ".sha256")
    require(not archive.exists() and not checksum.exists(), "output release already exists")
    with tempfile.TemporaryDirectory(dir=args.output_dir) as temporary:
        temporary = Path(temporary)
        manifest = temporary / "release.json"
        manifest.write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n")
        with tarfile.open(temporary / archive.name, "w:gz", format=tarfile.USTAR_FORMAT) as output:
            for name, path in sorted({**files, "release.json": manifest}.items()):
                info = output.gettarinfo(str(path), arcname=name)
                info.uid = info.gid = 0
                info.uname = info.gname = "root"
                info.mtime = 0
                info.mode = 0o550 if name.startswith("bin/") else 0o644
                with path.open("rb") as stream:
                    output.addfile(info, stream)
        with (temporary / archive.name).open("rb") as stream:
            checksum_value = digest(stream)
        temporary_checksum = temporary / checksum.name
        temporary_checksum.write_text(f"{checksum_value}  {archive.name}\n")
        # Check the exact bundle produced by the builder before publishing it.
        validate(temporary / archive.name, args.release, args.arch, temporary_checksum)
        os.replace(temporary / archive.name, archive)
        os.replace(temporary_checksum, checksum)
    print(archive)
    print(checksum)


def validate(archive, release, architecture, checksum_file, destination=None):
    valid_sha(release)
    regular_file(archive)
    regular_file(checksum_file)
    require(checksum_file.stat().st_size <= 256, "checksum file is too large")
    checksum = checksum_file.read_text().strip()
    match = re.fullmatch(r"([0-9a-f]{64})  (tz-lite-[0-9a-f]{40}\.tar\.gz)", checksum)
    require(match and match[2] == f"tz-lite-{release}.tar.gz", "invalid checksum file")
    # The receiver copies incoming files into a root-only directory first.
    with archive.open("rb") as stream:
        require(digest(stream) == match[1], "archive checksum mismatch")
        stream.seek(0)
        with tarfile.open(fileobj=stream, mode="r:gz") as bundle:
            members = {}
            total_size = 0
            for member in bundle:
                name = member.name.removesuffix("/") if member.isdir() else member.name
                valid_path(name, member.isdir())
                require(member.isfile() or member.isdir(), f"links/special files are forbidden: {name}")
                require(name not in members, f"duplicate archive member: {name}")
                require(not member.mode & 0o7000, f"unsafe permission bits: {name}")
                total_size += member.size
                require(len(members) < MAX_FILES and total_size <= MAX_BYTES,
                        "archive exceeds runtime size limits")
                members[name] = member
            files = {name: member for name, member in members.items() if member.isfile()}
            for name in members:
                require(not any(str(parent) in files for parent in PurePosixPath(name).parents),
                        f"archive path has a file as its parent: {name}")
            require(REQUIRED | {"release.json"} <= files.keys(), "required runtime files are missing")
            for name in GEOIP_FILES:
                require(files[name].size > 0, f"empty GeoIP database: {name}")
            require(files["release.json"].size <= 4 * 1024 ** 2, "release manifest is too large")
            with bundle.extractfile(files["release.json"]) as manifest:
                metadata = json.load(manifest)
            require(isinstance(metadata, dict) and metadata.get("format") == 1
                    and metadata.get("release") == release
                    and metadata.get("architecture") == architecture
                    and metadata.get("edition") == "lite", "release metadata mismatch")
            hashes = metadata.get("files")
            require(isinstance(hashes, dict) and hashes.keys() == files.keys() - {"release.json"},
                    "manifest file list does not match archive")
            for name, expected in hashes.items():
                require(isinstance(expected, str) and re.fullmatch(r"[0-9a-f]{64}", expected),
                        f"invalid file digest: {name}")
                with bundle.extractfile(files[name]) as content:
                    if name.startswith("bin/"):
                        check_elf(content.read(64), architecture, name)
                        content.seek(0)
                    require(digest(content) == expected, f"file checksum mismatch: {name}")
            # Never use tar.extract: create only checked regular files in a new directory.
            if destination is not None:
                require(not destination.exists() and not destination.is_symlink(),
                        "extraction destination already exists")
                destination.mkdir(mode=0o700)
                for name, member in files.items():
                    target = destination / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with bundle.extractfile(member) as source, target.open("xb") as output:
                        shutil.copyfileobj(source, output)
                    target.chmod(0o550 if name.startswith("bin/") else 0o644)
                (destination / "RELEASE_SHA").write_text(release + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    pack = commands.add_parser("package")
    pack.add_argument("--backend-dir", type=Path, required=True)
    pack.add_argument("--geoip-dir", type=Path, required=True)
    pack.add_argument("--frontend-dir", type=Path, required=True)
    pack.add_argument("--output-dir", type=Path, required=True)
    check = commands.add_parser("validate")
    check.add_argument("archive", type=Path)
    check.add_argument("--checksum-file", type=Path, required=True)
    check.add_argument("--destination", type=Path)
    for command in (pack, check):
        command.add_argument("--release", required=True)
        command.add_argument("--arch", choices=MACHINES, required=True)
    args = parser.parse_args()
    try:
        if args.command == "package":
            package(args)
        else:
            validate(args.archive, args.release, args.arch, args.checksum_file, args.destination)
            print(f"Validated Lite {args.release} for {args.arch}")
    except (ValueError, OSError, tarfile.TarError) as error:
        parser.exit(1, f"Lite release: {error}\n")


if __name__ == "__main__":
    main()
