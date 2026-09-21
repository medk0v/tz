import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("ai_release", Path(__file__).with_name("lite-ai-release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
SHA = "a" * 40


class AIReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        self.config = json.dumps({"architecture": "arm64", "os": "linux"}).encode()
        self.image = "sha256:" + hashlib.sha256(self.config).hexdigest()
        self.images = {stack: {service: self.image for service in services}
                       for stack, services in release.SERVICES.items()}

    def compose(self, stack):
        return {"services": {service: {"image": self.image, "platform": "linux/arm64", "pull_policy": "never"}
                             for service in release.SERVICES[stack]}}

    def image_archive(self, config=None):
        config = self.config if config is None else config
        config_name = self.image[7:] + ".json"
        with tarfile.open(self.bundle / "images.tar", "w") as archive:
            for name, content in ((config_name, config), ("manifest.json", json.dumps([
                    {"Config": config_name, "RepoTags": None, "Layers": []}]).encode())):
                info = tarfile.TarInfo(name)
                info.size = len(content)
                archive.addfile(info, io.BytesIO(content))

    def package(self):
        self.image_archive()
        for stack in release.SERVICES:
            directory = self.bundle / stack
            directory.mkdir()
            (directory / "compose.yaml").write_text(json.dumps(self.compose(stack)))
            (directory / "manage.sh").write_text("#!/bin/sh\nexit 0\n")
        (self.bundle / "openclaw/workspace").mkdir()
        (self.bundle / "openclaw/workspace/IDENTITY.md").write_text("Runtime policy")
        (self.bundle / "firecrawl/env.example").write_text("POSTGRES_PASSWORD=\n")
        release.package(self.bundle, self.images, SHA, self.root / "output")
        self.archive = self.root / "output" / f"tz-lite-ai-{SHA}.tar.gz"
        self.checksum = Path(str(self.archive) + ".sha256")

    def test_runtime_bundle_round_trip(self):
        self.package()
        destination = self.root / "extracted"
        release.validate(self.archive, self.checksum, SHA, destination)
        self.assertEqual((destination / "images.tar").read_bytes(), (self.bundle / "images.tar").read_bytes())
        self.assertEqual(set(path.relative_to(destination).as_posix() for path in destination.rglob("*") if path.is_file()),
                         release.FILES | {"release.json"})

    def test_saved_images_must_match_id_and_arm64(self):
        self.image_archive()
        with (self.bundle / "images.tar").open("rb") as stream:
            release.check_image_archive(stream, {self.image})
        self.image_archive(json.dumps({"architecture": "amd64", "os": "linux"}).encode())
        with (self.bundle / "images.tar").open("rb") as stream, self.assertRaisesRegex(ValueError, "not linux/arm64"):
            release.check_image_archive(stream, {self.image})

    def test_runtime_rejects_build_tag_pull_and_public_port(self):
        for change in ({"build": {"context": "/source"}}, {"image": "redis:latest"},
                       {"pull_policy": "always"}, {"platform": "linux/amd64"},
                       {"ports": [{"published": "3002", "target": 3002}]}):
            with self.subTest(change=change):
                model = self.compose("firecrawl")
                model["services"]["api"].update(change)
                with self.assertRaises(ValueError):
                    release.check_compose(model, "firecrawl", self.images["firecrawl"])

    def test_runtime_rejects_legacy_namespace_in_compose_and_scripts(self):
        model = self.compose("openclaw")
        model["name"] = "tzomet-openclaw"
        with self.assertRaisesRegex(ValueError, "legacy namespace"):
            release.check_compose(model, "openclaw", self.images["openclaw"])
        self.package()
        (self.bundle / "openclaw/manage.sh").write_text("#!/bin/sh\ncd /var/lib/tzomet-ai\n")
        with self.assertRaisesRegex(ValueError, "legacy namespace"):
            release.package(self.bundle, self.images, SHA, self.root / "rejected")

    def test_firecrawl_render_removes_source_and_experimental_fdb(self):
        model = self.compose("firecrawl")
        model["services"]["api"].update(build={"context": "apps/api"},
                                             environment={"POSTGRES_PASSWORD": "${POSTGRES_PASSWORD}",
                                                          "FDB_CLUSTER_FILE": "${NUQ_BACKEND:+/var/fdb/fdb.cluster}"},
                                             volumes=[{"type": "volume", "source": "fdb-cluster-file"}])
        model["services"]["foundationdb"] = {"image": "foundationdb:mutable"}
        model["services"]["foundationdb-init"] = {"image": "foundationdb:mutable"}
        model["x-common-service"] = {"build": {"context": "apps/api"}}
        model["volumes"] = {"fdb-data": {}, "fdb-cluster-file": {}, "firecrawl-postgres": {"name": "builder_pg"}}
        model["networks"] = {"tzomet-ai": {"name": "tzomet-ai", "external": True}}
        model["services"]["api"]["networks"] = {"tzomet-ai": {"aliases": ["firecrawl-api"]}}
        rendered = release.render_firecrawl(model, self.images["firecrawl"])
        self.assertNotRegex(json.dumps(rendered), "(?i)tzomet")
        self.assertEqual(rendered["networks"]["tz-ai"], {"name": "tz-ai", "external": True})
        self.assertEqual(set(rendered["services"]), release.SERVICES["firecrawl"])
        self.assertEqual(set(rendered["volumes"]), {"firecrawl-postgres"})
        self.assertEqual(rendered["services"]["api"]["environment"]["POSTGRES_PASSWORD"], "${POSTGRES_PASSWORD}")
        self.assertNotIn("build", rendered["services"]["api"])
        self.assertNotIn("volumes", rendered["services"]["api"])
        self.assertNotIn("x-common-service", rendered)

    def test_archive_rejects_sources_links_and_tampering_before_extraction(self):
        self.package()
        original = self.archive.read_bytes()
        for name, type_ in (("../escape", tarfile.REGTYPE), ("Dockerfile", tarfile.REGTYPE),
                            ("openclaw/manage.sh", tarfile.SYMTYPE)):
            with self.subTest(name=name):
                self.archive.write_bytes(original)
                with tarfile.open(self.archive, "r:gz") as archive:
                    contents = [(member, archive.extractfile(member).read()) for member in archive]
                with tarfile.open(self.archive, "w:gz") as archive:
                    for member, content in contents:
                        archive.addfile(member, io.BytesIO(content))
                    extra = tarfile.TarInfo(name)
                    extra.type = type_
                    extra.linkname = "/etc/passwd"
                    archive.addfile(extra)
                self.checksum.write_text(f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  {self.archive.name}\n")
                destination = self.root / "extracted"
                with self.assertRaises(ValueError):
                    release.validate(self.archive, self.checksum, SHA, destination)
                self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
