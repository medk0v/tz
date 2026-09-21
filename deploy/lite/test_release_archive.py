"""Archive trust-boundary tests; no deployment, service, or database access."""

import argparse
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("lite_release", Path(__file__).with_name("lite-release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
SHA = "a" * 40


def elf(machine=183):
    header = bytearray(64)
    header[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHI", header, 16, 3, machine, 1)
    return bytes(header)


class ReleaseArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / f"tz-lite-{SHA}.tar.gz"
        self.checksum = self.root / f"{self.archive.name}.sha256"
        self.payload = {name: elf() if name.startswith("bin/") else b"compiled asset"
                        for name in release.REQUIRED}

    def write_archive(self, payload=None, extra=(), metadata_overrides=None):
        payload = self.payload if payload is None else payload
        metadata = {"format": 1, "release": SHA, "architecture": "aarch64", "edition": "lite",
                    "files": {name: hashlib.sha256(content).hexdigest() for name, content in payload.items()}}
        metadata.update(metadata_overrides or {})
        with tarfile.open(self.archive, "w:gz") as bundle:
            for name, data in {**payload, "release.json": json.dumps(metadata).encode()}.items():
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                bundle.addfile(entry, io.BytesIO(data))
            for entry, content in extra:
                bundle.addfile(entry, io.BytesIO(content) if content is not None else None)
        self.checksum.write_text(f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  {self.archive.name}\n")

    def validate(self, **kwargs):
        return release.validate(self.archive, SHA, "aarch64", self.checksum, **kwargs)

    def test_valid_release_extracts_only_runtime_files(self):
        self.write_archive()
        destination = self.root / "extracted"
        self.validate(destination=destination)
        self.assertEqual((destination / "bin/api").read_bytes(), elf())
        self.assertEqual((destination / "bin/bootstrap-admin").stat().st_mode & 0o777, 0o550)
        self.assertEqual((destination / "RELEASE_SHA").read_text().strip(), SHA)
        self.assertFalse((destination / "backend").exists())
        for name in release.GEOIP_FILES:
            self.assertEqual((destination / name).read_bytes(), self.payload[name])

    def test_unsafe_paths_are_rejected_before_any_extraction(self):
        for name in ("../escape", "/tmp/escape", "frontend/admin/../secret.js",
                     "frontend/admin/.env", "backend/src/main.rs", "frontend/admin/main.ts",
                     "frontend/admin/source.js.map", "frontend//admin/a.js",
                     "geoip/other.mmdb", "geoip/README.md"):
            with self.subTest(name=name):
                extra = tarfile.TarInfo(name)
                self.write_archive(extra=[(extra, b"")])
                destination = self.root / "extracted"
                with self.assertRaises(ValueError):
                    self.validate(destination=destination)
                self.assertFalse(destination.exists())

    def test_links_and_special_files_are_rejected(self):
        for entry_type in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE):
            with self.subTest(type=entry_type):
                extra = tarfile.TarInfo("frontend/admin/linked.js")
                extra.type = entry_type
                extra.linkname = "/etc/passwd"
                self.write_archive(extra=[(extra, None)])
                with self.assertRaises(ValueError):
                    self.validate()

    def test_metadata_sha_edition_and_architecture_must_match(self):
        for override in ({"release": "b" * 40}, {"edition": "standard"}, {"architecture": "x86_64"}):
            with self.subTest(override=override):
                self.write_archive(metadata_overrides=override)
                with self.assertRaisesRegex(ValueError, "metadata mismatch"):
                    self.validate()

    def test_binary_architecture_must_match_even_if_manifest_claims_arm64(self):
        self.write_archive(payload={**self.payload, "bin/api": elf(62)})
        with self.assertRaisesRegex(ValueError, "does not target aarch64"):
            self.validate()

    def test_archive_and_member_checksums_are_both_checked(self):
        self.write_archive()
        self.checksum.write_text(f"{'0' * 64}  {self.archive.name}\n")
        with self.assertRaisesRegex(ValueError, "archive checksum mismatch"):
            self.validate()
        hashes = {name: "0" * 64 for name in self.payload}
        self.write_archive(metadata_overrides={"files": hashes})
        with self.assertRaisesRegex(ValueError, "file checksum mismatch"):
            self.validate()

    def test_duplicate_archive_path_is_rejected(self):
        entry = tarfile.TarInfo("bin/api")
        self.write_archive(extra=[(entry, b"")])
        with self.assertRaisesRegex(ValueError, "duplicate archive member"):
            self.validate()

    def test_unlisted_or_missing_runtime_files_are_rejected(self):
        incomplete = {name: data for name, data in self.payload.items() if name != "bin/worker"}
        self.write_archive(payload=incomplete)
        with self.assertRaisesRegex(ValueError, "required runtime files"):
            self.validate()
        extra = tarfile.TarInfo("frontend/admin/unlisted.js")
        self.write_archive(extra=[(extra, b"")])
        with self.assertRaisesRegex(ValueError, "manifest file list"):
            self.validate()

    def test_package_round_trip_and_rejects_source_symlink(self):
        for name, data in self.payload.items():
            path = self.root / "build" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        args = argparse.Namespace(backend_dir=self.root / "build/bin", frontend_dir=self.root / "build/frontend",
                                  geoip_dir=self.root / "build/geoip",
                                  release=SHA, arch="aarch64", output_dir=self.root / "output")
        release.package(args)
        destination = self.root / "round-trip"
        release.validate(args.output_dir / self.archive.name, SHA, "aarch64",
                         args.output_dir / self.checksum.name, destination)
        for name in release.GEOIP_FILES:
            self.assertEqual((destination / name).read_bytes(), self.payload[name])
        database = args.geoip_dir / "GeoLite2-Country.mmdb"
        database.unlink()
        with self.assertRaises(FileNotFoundError):
            release.package(args)
        database.symlink_to(self.root / "build/bin/api")
        with self.assertRaisesRegex(ValueError, "not a regular file"):
            release.package(args)
        database.unlink()
        database.write_bytes(self.payload["geoip/GeoLite2-Country.mmdb"])
        (self.root / "build/frontend/admin/hidden.js").symlink_to(self.root / "build/bin/api")
        args.output_dir = self.root / "other-output"
        with self.assertRaisesRegex(ValueError, "symlink"):
            release.package(args)

    def test_missing_or_empty_geoip_databases_are_rejected(self):
        for name in release.GEOIP_FILES:
            with self.subTest(name=name):
                self.write_archive(payload={key: data for key, data in self.payload.items() if key != name})
                with self.assertRaisesRegex(ValueError, "required runtime files"):
                    self.validate()
                self.write_archive(payload={**self.payload, name: b""})
                with self.assertRaisesRegex(ValueError, "empty GeoIP database"):
                    self.validate()


if __name__ == "__main__":
    unittest.main()
