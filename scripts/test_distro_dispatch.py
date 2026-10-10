#!/usr/bin/env python3

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent


class DistroDispatchTests(unittest.TestCase):
    def dispatch(self, entrypoint, release, vendor_release=None, managers=()):
        with tempfile.TemporaryDirectory(prefix="apexshot-distro-test-") as temporary:
            directory = Path(temporary)
            binaries = directory / "bin"
            binaries.mkdir()
            for command in ("sh", "bash", "dirname", "tr", "cat"):
                executable = shutil.which(command)
                self.assertIsNotNone(executable)
                (binaries / command).symlink_to(executable)
            for command in managers:
                executable = binaries / command
                executable.write_text("#!/bin/sh\nexit 99\n")
                executable.chmod(0o755)

            local_release = directory / "os-release"
            vendor_path = directory / "vendor-os-release"
            if release is not None:
                local_release.write_text(release)
            if vendor_release is not None:
                vendor_path.write_text(vendor_release)

            source = (ROOT / entrypoint).read_text()
            source = source.replace("/etc/os-release", str(local_release))
            source = source.replace("/usr/lib/os-release", str(vendor_path))
            source = source.replace("/etc/steamos-release", str(directory / "steamos-release"))
            script = directory / entrypoint
            script.write_text(source)
            action = "install" if entrypoint == "install.sh" else "update"
            for family in ("arch", "ubuntu", "fedora"):
                (directory / f"{family}-{action}.sh").write_text(
                    f"printf '%s\\n' '{family}' \"$@\" \"${{APEXSHOT_SKIP_GNOME_EXTENSION:-}}\"\n"
                )

            environment = {
                "PATH": str(binaries),
                "HOME": str(directory),
                "XDG_CURRENT_DESKTOP": "KDE",
            }
            return subprocess.run(
                [str(binaries / "sh"), str(script), "--force"],
                env=environment,
                text=True,
                capture_output=True,
                timeout=10,
            )

    def test_known_derivatives_forward_arguments_without_installing(self):
        for entrypoint in ("install.sh", "update.sh"):
            for release, family in (
                ("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n", "ubuntu"),
                ("ID=elementary\n", "ubuntu"),
                ("ID=manjaro\nID_LIKE=arch\n", "arch"),
                ("ID=endeavouros\nID_LIKE=arch\n", "arch"),
                ("ID=almalinux\nID_LIKE=\"rhel centos fedora\"\n", "fedora"),
            ):
                with self.subTest(entrypoint=entrypoint, release=release):
                    result = self.dispatch(entrypoint, release)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.splitlines(), [family, "--force", "1"])

    def test_id_precedes_id_like_and_id_like_preserves_order(self):
        for entrypoint in ("install.sh", "update.sh"):
            for release, family in (
                ("ID=arch\nID_LIKE=debian\n", "arch"),
                ("ID=derivative\nID_LIKE=\"arch debian\"\n", "arch"),
                ("ID=derivative\nID_LIKE=\"debian arch\"\n", "ubuntu"),
            ):
                with self.subTest(entrypoint=entrypoint, release=release):
                    result = self.dispatch(entrypoint, release)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.splitlines()[0], family)

    def test_vendor_release_is_only_used_when_local_file_is_missing(self):
        for entrypoint in ("install.sh", "update.sh"):
            with self.subTest(entrypoint=entrypoint):
                result = self.dispatch(entrypoint, None, "ID=arch\n")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines()[0], "arch")
                result = self.dispatch(entrypoint, "ID=ubuntu\n", "ID=arch\n")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines()[0], "ubuntu")

    def test_nixos_does_not_use_foreign_package_managers(self):
        for entrypoint in ("install.sh", "update.sh"):
            with self.subTest(entrypoint=entrypoint):
                result = self.dispatch(
                    entrypoint, "ID=nixos\nID_LIKE=debian\n", managers=("apt", "dnf")
                )
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("NixOS configuration:", result.stderr)
                self.assertEqual(result.stdout, "")

    def test_unpackaged_distros_do_not_fall_back_to_foreign_managers(self):
        for entrypoint in ("install.sh", "update.sh"):
            for distro in ("alpine", "gentoo", "void"):
                with self.subTest(entrypoint=entrypoint, distro=distro):
                    result = self.dispatch(entrypoint, f"ID={distro}\n", managers=("apt",))
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertEqual(result.stdout, "")

    def test_opensuse_ids_do_not_select_a_fedora_installer(self):
        for entrypoint in ("install.sh", "update.sh"):
            for distro in ("opensuse-tumbleweed", "opensuse-leap", "sles"):
                with self.subTest(entrypoint=entrypoint, distro=distro):
                    result = self.dispatch(entrypoint, f"ID={distro}\n", managers=("dnf",))
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn("build-opensuse-rpm.sh", result.stderr)
                    self.assertEqual(result.stdout, "")

    def test_steamos_guard_still_overrides_arch(self):
        for entrypoint in ("install.sh", "update.sh"):
            with self.subTest(entrypoint=entrypoint):
                result = self.dispatch(entrypoint, "ID=steamos\nID_LIKE=arch\n")
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("SteamOS is not supported", result.stderr)


if __name__ == "__main__":
    unittest.main()
