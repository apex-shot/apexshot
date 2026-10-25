#!/usr/bin/env python3

from pathlib import Path
import os
import re
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent
INSTALLER = ROOT / "fedora-install.sh"
SPEC = ROOT.parent / "packaging" / "fedora" / "apexshot.spec"
MAIN_CALL = '\nmain "$@"\n'

HARNESS = """
set -euo pipefail
{script}
step() {{ :; }}
prime_sudo() {{ :; }}
run_spinner() {{ shift; "$@"; }}
SUDO=""
install_runtime_dependencies
"""


def installer_without_main():
    text = INSTALLER.read_text()
    if text.count(MAIN_CALL) != 1:
        raise AssertionError("fedora-install.sh must end with a single main call")
    return text.replace(MAIN_CALL, "\n")


class RuntimeDependencyTest(unittest.TestCase):
    """The installer must not force ffmpeg-free onto a system that already has ffmpeg."""

    def install_deps(self, ffmpeg_installed):
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            os.symlink(shutil.which("bash"), bin_dir / "bash")
            dnf = bin_dir / "dnf"
            dnf.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$APEXSHOT_TEST_DNF_LOG"\n')
            dnf.chmod(0o755)
            if ffmpeg_installed:
                for name in ("ffmpeg", "ffprobe"):
                    stub = bin_dir / name
                    stub.write_text("#!/bin/sh\nexit 0\n")
                    stub.chmod(0o755)
            harness = Path(tmp) / "harness.sh"
            harness.write_text(HARNESS.format(script=installer_without_main()))
            log = Path(tmp) / "dnf.log"
            log.touch()
            env = dict(
                os.environ,
                PATH=str(bin_dir),
                APEXSHOT_TEST_DNF_LOG=str(log),
            )
            result = subprocess.run(
                ["bash", str(harness)],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return log.read_text().split()

    def test_installs_ffmpeg_free_when_the_system_has_no_ffmpeg(self):
        self.assertIn("ffmpeg-free", self.install_deps(ffmpeg_installed=False))

    def test_leaves_an_existing_ffmpeg_alone(self):
        self.assertNotIn("ffmpeg-free", self.install_deps(ffmpeg_installed=True))


class SpecDependencyTest(unittest.TestCase):
    def test_ffmpeg_requirement_accepts_either_package(self):
        requires = [
            " ".join(line.split(":", 1)[1].split())
            for line in SPEC.read_text().splitlines()
            if re.match(r"^Requires:\s", line)
        ]
        self.assertNotIn("ffmpeg-free", requires)
        self.assertIn("(ffmpeg-free or ffmpeg)", requires)


if __name__ == "__main__":
    unittest.main()
