"""Verify the exact Lite install archive without touching host services."""
import hashlib
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tarfile
import tempfile
import unittest


DIRECTORY = Path(__file__).resolve().parent


class LiteProvisionBundleTests(unittest.TestCase):
    def test_packaged_runtime_uses_its_own_namespace(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            subprocess.run([sys.executable, str(DIRECTORY / "package-provision"), str(output)],
                           check=True, capture_output=True, text=True)
            archive = output / "tz-lite-install.tar.gz"
            checksum = output / "tz-lite-install.tar.gz.sha256"
            self.assertEqual(checksum.read_text(),
                             f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
            with tarfile.open(archive) as bundle:
                contents = {}
                for member in bundle:
                    self.assertTrue(member.isfile())
                    self.assertTrue(member.name.startswith("tz-lite-install/"))
                    self.assertNotIn("tzomet", member.name.lower())
                    contents[member.name.removeprefix("tz-lite-install/")] = \
                        bundle.extractfile(member).read().decode()
            for name, content in contents.items():
                if name in {"provision", "lite-ai-release.py"}:
                    # These guards must recognize and refuse legacy hosts/artifacts.
                    continue
                with self.subTest(file=name):
                    self.assertNotIn("tzomet", content.lower())
            api = contents["shared/tz-api.service"]
            self.assertIn("User=tz\n", api)
            self.assertIn("EnvironmentFile=/etc/tz/tz.env\n", api)
            self.assertIn("WorkingDirectory=/srv/tz/current\n", api)
            worker = contents["shared/tz-worker.service"]
            self.assertIn("ConditionPathExists=!/var/lib/tz/deployment/worker-blocked\n", worker)
            self.assertIn("X-Tz-Client-IP", contents["shared/tz-proxy.conf"])
            self.assertIn("/run/tz-ai/openclaw/agent-secrets", contents["shared/tz-ai-runtime.conf"])
            self.assertIn("ForceCommand /usr/local/sbin/tz-lite-github-ssh", contents["sshd-deploy.conf"])
            self.assertIn("user tz on", contents["provision"])
            self.assertIn("&tz.events", contents["provision"])

    def run_install_guard(self, root, account=""):
        script = (DIRECTORY / "provision").read_text()
        guard = script[script.index("# Refuse legacy installs"):script.index("deploy_key=")]
        guard = re.sub(r"/(?:etc|srv|var|usr)/", lambda match: f"{root}{match[0]}", guard)
        return subprocess.run(["bash", "-c", "set -eu\n"
                               'fail() { echo "$*" >&2; exit 1; }\n'
                               f'getent() {{ [[ "$2" == {shlex.quote(account)} ]]; }}\n'
                               + guard], capture_output=True, text=True)

    def test_install_guard_rejects_legacy_state_without_following_a_symlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.assertEqual(self.run_install_guard(root).returncode, 0)
            marker = root / "etc/tzomet-lite-installation"
            marker.parent.mkdir()
            marker.symlink_to(root / "missing")
            result = self.run_install_guard(root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("namespace migration", result.stderr)

    def test_install_guard_rejects_the_legacy_application_account(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = self.run_install_guard(Path(temporary), account="tzomet")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("legacy application account", result.stderr)


if __name__ == "__main__":
    unittest.main()
