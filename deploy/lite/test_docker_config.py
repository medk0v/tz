import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("docker_config", Path(__file__).with_name("configure-docker.py"))
docker_config = importlib.util.module_from_spec(spec)
spec.loader.exec_module(docker_config)


class DockerConfigTests(unittest.TestCase):
    def info(self, images=0, containers=0):
        return {"DriverStatus": [["driver-type", "io.containerd.snapshotter.v1"]],
                "Images": images, "Containers": containers}

    def test_empty_containerd_store_can_switch_without_losing_other_settings(self):
        original = {"log-driver": "local", "features": {"other-feature": True}}
        updated = docker_config.prepare_config(original, self.info())
        self.assertEqual(updated, {"log-driver": "local", "features": {
            "other-feature": True, "containerd-snapshotter": False}})
        self.assertNotIn("containerd-snapshotter", original["features"])

    def test_nonempty_containerd_store_is_never_switched_automatically(self):
        for info in (self.info(images=1), self.info(containers=1)):
            with self.subTest(info=info), self.assertRaisesRegex(ValueError, "nonempty"):
                docker_config.prepare_config({}, info)

    def test_existing_classic_store_and_settings_are_preserved(self):
        info = {"DriverStatus": [["Backing Filesystem", "extfs"]], "Images": 7, "Containers": 8}
        expected = {"features": {"containerd-snapshotter": False}}
        self.assertEqual(docker_config.prepare_config({}, info), expected)
        self.assertEqual(docker_config.prepare_config(expected, info), expected)

    def test_invalid_features_are_not_overwritten(self):
        with self.assertRaisesRegex(ValueError, "features"):
            docker_config.prepare_config({"features": []}, self.info())


if __name__ == "__main__":
    unittest.main()
