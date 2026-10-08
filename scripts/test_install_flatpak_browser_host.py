import contextlib
import importlib.util
import io
import json
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT_PATH = Path(__file__).with_name("install-flatpak-browser-host.py")
SPEC = importlib.util.spec_from_file_location("install_flatpak_browser_host", SCRIPT_PATH)
installer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(installer)


class FlatpakBrowserHostInstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="apexshot host bridge ")
        self.root = Path(self.temp_dir.name)
        self.home = self.root / "home with spaces"
        self.config = self.root / "config with spaces"
        self.home.mkdir()
        self.config.mkdir()
        self.env = {
            "HOME": str(self.home),
            "XDG_CONFIG_HOME": str(self.config),
        }

    def tearDown(self):
        self.temp_dir.cleanup()

    def make_fake_flatpak(self):
        executable = self.root / "host tools" / "flatpak executable"
        executable.parent.mkdir()
        executable.write_text(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CAPTURE_ARGS\"\ncat\n",
            encoding="utf-8",
        )
        executable.chmod(0o755)
        self.env["CAPTURE_ARGS"] = str(self.root / "flatpak-args.txt")
        return executable

    def test_installs_only_selected_browser_and_forwards_protocol_stdio(self):
        executable = self.make_fake_flatpak()
        manifests = installer.install_bridge(
            ("chrome",),
            installer.DEFAULT_EXTENSION_ID,
            False,
            self.env,
            executable,
        )

        self.assertEqual(len(manifests), 1)
        manifest_path = manifests[0]
        self.assertEqual(
            manifest_path,
            self.config / "google-chrome/NativeMessagingHosts" / installer.MANIFEST_NAME,
        )
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        launcher_path = self.home / ".local/bin" / installer.LAUNCHER_NAME
        self.assertEqual(manifest["name"], "io.github.codegoddy.apexshot")
        self.assertEqual(manifest["path"], str(launcher_path))
        self.assertEqual(
            manifest["allowed_origins"],
            [f"chrome-extension://{installer.DEFAULT_EXTENSION_ID}/"],
        )
        self.assertEqual(stat.S_IMODE(launcher_path.stat().st_mode), 0o755)
        self.assertIn("'" + str(executable) + "'", launcher_path.read_text(encoding="utf-8"))

        payload = b"\x00native-messaging-payload\xff"
        result = subprocess.run(
            [str(launcher_path), "chrome-extension://unforwarded.origin/"],
            input=payload,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=self.env,
            check=True,
        )
        self.assertEqual(result.stdout, payload)
        self.assertEqual(result.stderr, b"")
        self.assertEqual(
            (self.root / "flatpak-args.txt").read_text(encoding="utf-8").splitlines(),
            ["run", installer.APP_ID, "native-host"],
        )
        self.assertFalse(
            (self.config / "chromium/NativeMessagingHosts" / installer.MANIFEST_NAME).exists()
        )

    def test_both_browsers_are_registered_and_repeat_install_is_idempotent(self):
        executable = self.make_fake_flatpak()
        first = installer.install_bridge(
            ("chrome", "chromium"),
            installer.DEFAULT_EXTENSION_ID,
            False,
            self.env,
            executable,
        )
        second = installer.install_bridge(
            ("chrome", "chromium"),
            installer.DEFAULT_EXTENSION_ID,
            False,
            self.env,
            executable,
        )

        self.assertEqual(first, second)
        self.assertEqual(len(first), 2)
        self.assertTrue(all(path.exists() for path in first))

    def test_invalid_extension_ids_are_rejected_before_writing(self):
        for extension_id in ("short", "A" * 32, "q" * 32, "a" * 31 + "-"):
            with self.subTest(extension_id=extension_id):
                with self.assertRaises(installer.InstallError):
                    installer.install_bridge(
                        ("chrome",), extension_id, False, self.env, self.root / "flatpak"
                    )
        self.assertFalse((self.home / ".local/bin").exists())
        self.assertFalse((self.config / "google-chrome").exists())

    def test_native_registration_requires_replace_and_native_files_are_preserved(self):
        manifest_path = (
            self.config / "google-chrome/NativeMessagingHosts" / installer.MANIFEST_NAME
        )
        manifest_path.parent.mkdir(parents=True)
        native_manifest = {
            "name": installer.HOST_NAME,
            "description": "ApexShot native host",
            "path": str(self.home / ".local/bin/apexshot-native-host"),
            "type": "stdio",
            "allowed_origins": [f"chrome-extension://{installer.DEFAULT_EXTENSION_ID}/"],
        }
        manifest_path.write_text(json.dumps(native_manifest), encoding="utf-8")
        native_launcher = self.home / ".local/bin/apexshot-native-host"
        native_launcher.parent.mkdir(parents=True, exist_ok=True)
        native_launcher.write_text("native launcher stays intact\n", encoding="utf-8")
        executable = self.make_fake_flatpak()

        with self.assertRaisesRegex(installer.InstallError, "--replace"):
            installer.install_bridge(
                ("chrome",), installer.DEFAULT_EXTENSION_ID, False, self.env, executable
            )
        self.assertEqual(json.loads(manifest_path.read_text()), native_manifest)
        self.assertEqual(native_launcher.read_text(), "native launcher stays intact\n")

        installer.install_bridge(
            ("chrome",), installer.DEFAULT_EXTENSION_ID, True, self.env, executable
        )
        flatpak_manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        self.assertEqual(
            flatpak_manifest["path"],
            str(self.home / ".local/bin" / installer.LAUNCHER_NAME),
        )
        self.assertEqual(native_launcher.read_text(), "native launcher stays intact\n")

    def test_conflicting_bridge_launcher_requires_replace(self):
        executable = self.make_fake_flatpak()
        launcher = self.home / ".local/bin" / installer.LAUNCHER_NAME
        launcher.parent.mkdir(parents=True)
        launcher.write_text("unrelated launcher\n", encoding="utf-8")

        with self.assertRaisesRegex(installer.InstallError, "--replace"):
            installer.install_bridge(
                ("chrome",), installer.DEFAULT_EXTENSION_ID, False, self.env, executable
            )
        self.assertEqual(launcher.read_text(encoding="utf-8"), "unrelated launcher\n")

        installer.install_bridge(
            ("chrome",), installer.DEFAULT_EXTENSION_ID, True, self.env, executable
        )
        self.assertIn(str(executable), launcher.read_text(encoding="utf-8"))

    def test_installer_refuses_flatpak_environment_and_flatpak_info_marker(self):
        stderr = io.StringIO()
        with patch.dict(os.environ, {"FLATPAK_ID": installer.APP_ID}), contextlib.redirect_stderr(stderr):
            self.assertEqual(installer.main(["--browser", "chrome"]), 1)
        self.assertIn("host Python interpreter", stderr.getvalue())
        self.assertFalse((self.home / ".local/bin").exists())

        marker = self.root / ".flatpak-info"
        marker.write_text("sandbox marker", encoding="utf-8")
        stderr = io.StringIO()
        with patch.object(installer, "FLATPAK_INFO_PATH", marker), contextlib.redirect_stderr(stderr):
            self.assertEqual(installer.main(["--browser", "chrome"]), 1)
        self.assertFalse((self.home / ".local/bin").exists())

    def test_install_function_refuses_sandbox_before_creating_host_files(self):
        flatpak_path = self.root / "flatpak should not be used"
        flatpak_env = {**self.env, "FLATPAK_ID": installer.APP_ID}
        with self.assertRaisesRegex(installer.InstallError, "host Python interpreter"):
            installer.install_bridge(
                ("chrome",), installer.DEFAULT_EXTENSION_ID, False, flatpak_env, flatpak_path
            )
        self.assertFalse((self.home / ".local/bin").exists())
        self.assertFalse((self.config / "google-chrome").exists())

        marker = self.root / ".flatpak-info"
        marker.write_text("sandbox marker", encoding="utf-8")
        with patch.object(installer, "FLATPAK_INFO_PATH", marker):
            with self.assertRaisesRegex(installer.InstallError, "host Python interpreter"):
                installer.install_bridge(
                    ("chrome",), installer.DEFAULT_EXTENSION_ID, False, self.env, flatpak_path
                )
        self.assertFalse((self.home / ".local/bin").exists())
        self.assertFalse((self.config / "google-chrome").exists())


if __name__ == "__main__":
    unittest.main()
