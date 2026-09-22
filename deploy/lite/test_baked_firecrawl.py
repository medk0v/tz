import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

DIRECTORY = Path(__file__).resolve().parent
PACKAGE_PATH = Path("node_modules/@openclaw/firecrawl-plugin")


class BakedFirecrawlTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.state = self.root / "installer-state"
        self.project = self.state / "npm/projects/openclaw-firecrawl-plugin-example"
        self.plugin = self.project / PACKAGE_PATH
        self.plugin.mkdir(parents=True)
        self.metadata = {"name": "@openclaw/firecrawl-plugin", "version": "2026.7.1"}
        (self.plugin / "package.json").write_text(json.dumps(self.metadata))
        (self.plugin / "openclaw.plugin.json").write_text(json.dumps({"id": "firecrawl"}))
        self.destination = self.root / "baked/firecrawl"

    def bake(self):
        return subprocess.run([
            "node", str(DIRECTORY / "bake-firecrawl-plugin.mjs"), str(self.state), str(self.destination),
        ], capture_output=True, text=True)

    def test_baked_plugin_loads_hoisted_dependencies_after_installer_state_is_removed(self):
        (self.project / "package-lock.json").write_text('{"lockfileVersion":3}')
        dependency = self.project / "node_modules/.store/hoisted-dependency"
        dependency.mkdir(parents=True)
        (dependency / "index.js").write_text('module.exports = "hoisted";')
        (self.project / "node_modules/hoisted-dependency").symlink_to(".store/hoisted-dependency")
        core = self.root / "app"
        core.mkdir()
        (core / "index.js").write_text('module.exports = "openclaw-peer";')
        (self.project / "node_modules/openclaw").symlink_to(core)
        (self.plugin / "index.cjs").write_text(
            'console.log(require("hoisted-dependency") + ":" + require("openclaw"));')

        result = self.bake()
        self.assertEqual(result.returncode, 0, result.stderr)
        shutil.rmtree(self.state)
        result = subprocess.run(["node", str(self.destination / PACKAGE_PATH / "index.cjs")],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "hoisted:openclaw-peer")
        self.assertEqual((self.destination / "package-lock.json").read_text(), '{"lockfileVersion":3}')
        self.assertEqual((self.destination / "node_modules/openclaw").readlink(), core)
        self.assertEqual((self.destination / "node_modules/hoisted-dependency").readlink(),
                         Path(".store/hoisted-dependency"))

    def test_unexpected_package_or_version_is_rejected(self):
        for key, value in (("name", "unexpected-package"), ("version", "2026.8.1")):
            with self.subTest(key=key):
                (self.plugin / "package.json").write_text(json.dumps({**self.metadata, key: value}))
                result = self.bake()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("does not match", result.stderr)
                self.assertFalse(self.destination.exists())

    def test_unexpected_plugin_identity_is_rejected(self):
        (self.plugin / "openclaw.plugin.json").write_text('{"id":"unexpected"}')
        result = self.bake()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match", result.stderr)
        self.assertFalse(self.destination.exists())

    def test_ambiguous_install_is_rejected(self):
        shutil.copytree(self.project, self.project.with_name("second-project"))
        result = self.bake()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected one installed Firecrawl npm project, found 2", result.stderr)
        self.assertFalse(self.destination.exists())

    def test_missing_install_is_rejected(self):
        shutil.rmtree(self.plugin)
        result = self.bake()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected one installed Firecrawl npm project, found 0", result.stderr)
        self.assertFalse(self.destination.exists())


class RuntimeProjectNameTests(unittest.TestCase):
    """The development helper and the released runtime must name the same Compose
    project. A one-sided rename orphans the running stack and its named volumes."""

    ROOT = DIRECTORY.parents[1]

    def project_name(self, relative, flag):
        text = (self.ROOT / relative).read_text()
        names = re.findall(rf"{flag} ([A-Za-z0-9][A-Za-z0-9_.-]*)", text)
        self.assertEqual(len(names), 1, f"{relative} must name its project exactly once")
        return names[0]

    def test_firecrawl_project_name_agrees(self):
        self.assertEqual(
            self.project_name("infra/firecrawl/manage.sh", "--project-name"),
            self.project_name("deploy/lite/firecrawl-runtime", "--project-name"),
        )

    def test_openclaw_project_and_shared_network(self):
        compose = (self.ROOT / "infra/openclaw/compose.yaml").read_text()
        self.assertIn("name: tz-openclaw\n", compose)
        runtime = (self.ROOT / "deploy/lite/firecrawl-runtime").read_text()
        for source in (compose, runtime, (self.ROOT / "infra/openclaw/manage.sh").read_text()):
            self.assertIn("tz-ai", source)


if __name__ == "__main__":
    unittest.main()
