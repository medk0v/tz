import importlib.util
from pathlib import Path
import tempfile
import tomllib
import unittest

DIRECTORY = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("lite_config", DIRECTORY / "render-config.py")
config_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(config_module)


class LiteConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def render(self):
        config_module.prepare(DIRECTORY / "Config.lite.toml.template", self.directory)

    def test_creates_private_credentials_and_preserves_configuration_on_rerun(self):
        self.render()
        original = {path.name: path.read_bytes() for path in self.directory.iterdir()}
        self.render()
        self.assertEqual(original, {path.name: path.read_bytes() for path in self.directory.iterdir()})
        config = tomllib.loads(original["Config.toml"].decode())
        self.assertEqual(config["server"]["allowed-origins"], ["https://tz.cyber-money.org"])
        self.assertEqual(config["telegram"]["webhook-base-url"], "https://tz.cyber-money.org")
        self.assertTrue(config["pg"]["url"].startswith("postgres://tz:"))
        self.assertTrue(config["pg"]["url"].endswith("/tz"))
        self.assertTrue(config["redis"]["url"].startswith("redis://tz:"))
        self.assertEqual(config["secrets"]["encryption-key-env"], "TZ_SECRETS_KEY")
        self.assertTrue(original["tz.env"].startswith(b"TZ_SECRETS_KEY="))
        self.assertEqual(config["openclaw"]["grant-directory"], "/run/tz-ai/openclaw/agent-secrets")
        self.assertEqual(config["attachments"]["storage-path"], "/var/lib/tz/attachments")
        self.assertNotIn("tzomet", original["Config.toml"].decode().lower())
        self.assertNotIn("__", original["Config.toml"].decode())
        for path in self.directory.iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_does_not_rotate_a_missing_key_for_an_existing_installation(self):
        self.render()
        original = (self.directory / "Config.toml").read_bytes()
        (self.directory / "tz.env").unlink()
        with self.assertRaises(FileNotFoundError):
            self.render()
        self.assertFalse((self.directory / "tz.env").exists())
        self.assertEqual((self.directory / "Config.toml").read_bytes(), original)

    def test_rejects_credentials_that_disagree_with_the_database_url(self):
        self.render()
        original = (self.directory / "Config.toml").read_bytes()
        (self.directory / "database.password").write_text("0" * 64 + "\n")
        with self.assertRaisesRegex(ValueError, "do not match"):
            self.render()
        self.assertEqual((self.directory / "Config.toml").read_bytes(), original)

    def test_rejects_a_configuration_without_a_fixed_project(self):
        self.render()
        path = self.directory / "Config.toml"
        without_project = path.read_text().replace(
            'project-id = "', 'project-id = "00000000-0000-0000-0000-000000000000" # ')
        path.write_text(without_project)
        with self.assertRaisesRegex(ValueError, "not a fixed-project"):
            self.render()
        self.assertEqual(path.read_text(), without_project)

    def test_rejects_secret_symlinks(self):
        target = self.directory / "unrelated"
        target.write_text("0" * 64 + "\n")
        (self.directory / "database.password").symlink_to(target)
        with self.assertRaisesRegex(ValueError, "unsafe configuration path"):
            self.render()

    def test_preserves_the_private_openclaw_token_and_timezone(self):
        config_module.prepare_openclaw(self.directory)
        path = self.directory / ".env"
        original = path.read_bytes()
        config_module.prepare_openclaw(self.directory)
        self.assertEqual(path.read_bytes(), original)
        self.assertIn(b"OPENCLAW_TZ=Europe/Istanbul\n", original)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_rejects_an_invalid_openclaw_token_without_replacing_it(self):
        path = self.directory / ".env"
        original = "OPENCLAW_GATEWAY_TOKEN=\nOPENCLAW_TZ=Europe/Istanbul\n"
        path.write_text(original)
        with self.assertRaisesRegex(ValueError, "refusing to rotate"):
            config_module.prepare_openclaw(self.directory)
        self.assertEqual(path.read_text(), original)


if __name__ == "__main__":
    unittest.main()
