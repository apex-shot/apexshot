#!/usr/bin/env python3

from pathlib import Path
import os
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent
REPO = "apex-shot/apexshot"
RELEASES_URL = f"https://github.com/{REPO}/releases"
TAG = "v0.2.35"

FEDORA_RPM = "apexshot-0.2.35-1.fc42.x86_64.rpm"
OPENSUSE_RPM = "apexshot-0.2.35-0.x86_64.rpm"
DEBUGINFO_RPM = "apexshot-debuginfo-0.2.35-1.fc42.x86_64.rpm"
DEBUGSOURCE_RPM = "apexshot-debugsource-0.2.35-1.fc42.x86_64.rpm"
OPENSUSE_DEBUGINFO_RPM = "apexshot-debuginfo-0.2.35-0.x86_64.rpm"
SOURCE_RPM = "apexshot-0.2.35-1.fc42.src.rpm"

INSTALLER_STUBS = (
    "installed_apexshot_version() { :; }\n"
    'fetch_version() { VERSION="v0.2.35"; }\n'
)
MAIN_CALL = '\nmain "$@"\n'


def script_text(script):
    return (ROOT / script).read_text()


def resolver_source(script):
    match = re.search(r"^resolve_rpm_url\(\) \{\n.*?^\}\n", script_text(script), re.M | re.S)
    if match is None:
        raise AssertionError(f"resolve_rpm_url not found in {script}")
    return match.group(0)


def installer_harness(script, logged_functions):
    text = script_text(script)
    if text.count(MAIN_CALL) != 1:
        raise AssertionError(f"{script} must end with a single main call")
    logging_stubs = "".join(
        f'{name}() {{ echo {name} >> "$APEXSHOT_TEST_LOG"; }}\n' for name in logged_functions
    )
    return text.replace(MAIN_CALL, "\n") + "\n" + INSTALLER_STUBS + logging_stubs + MAIN_CALL


def release_page(*names):
    return "".join(
        f'<a href="/{REPO}/releases/download/{TAG}/{name}">{name}</a>\n'
        for name in names
    )


def expected_url(name):
    return f"https://github.com/{REPO}/releases/download/{TAG}/{name}"


def run_offline(tmp_path, page, command, extra_env):
    curl_stub = tmp_path / "curl"
    curl_stub.write_text('#!/bin/sh\ncat "$APEXSHOT_TEST_PAGE"\n')
    curl_stub.chmod(0o755)
    page_file = tmp_path / "page.html"
    page_file.write_text(page)
    env = dict(os.environ)
    env.update(
        PATH=f"{tmp_path}{os.pathsep}{os.environ['PATH']}",
        APEXSHOT_TEST_PAGE=str(page_file),
        HOME=str(tmp_path),
        **extra_env,
    )
    return subprocess.run(command, env=env, capture_output=True, text=True, check=False)


class ResolverCase(unittest.TestCase):
    script = ""

    def resolve(self, page, tag=TAG):
        harness = "set -euo pipefail\n" + resolver_source(self.script) + "resolve_rpm_url\n"
        extra_env = {"REPO": REPO, "RELEASES_URL": RELEASES_URL, "VERSION": tag}
        with tempfile.TemporaryDirectory() as tmp:
            return run_offline(Path(tmp), page, ["bash", "-c", harness], extra_env)

    def assertSelects(self, page, name):
        result = self.resolve(page)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected_url(name))

    def assertRefuses(self, page):
        result = self.resolve(page)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")


class FedoraResolverTest(ResolverCase):
    script = "fedora-install.sh"

    def test_selects_main_fedora_rpm_among_mixed_assets(self):
        page = release_page(SOURCE_RPM, DEBUGSOURCE_RPM, DEBUGINFO_RPM, OPENSUSE_RPM, FEDORA_RPM)
        self.assertSelects(page, FEDORA_RPM)

    def test_selects_plain_rpm_when_signature_sidecar_is_listed_first(self):
        self.assertSelects(release_page(FEDORA_RPM + ".sig", FEDORA_RPM), FEDORA_RPM)

    def test_refuses_signature_and_backup_sidecars_alone(self):
        for suffix in (".sig", ".backup"):
            with self.subTest(suffix=suffix):
                self.assertRefuses(release_page(FEDORA_RPM + suffix))

    def test_refuses_openSUSE_rpm_when_no_fedora_rpm_is_published(self):
        self.assertRefuses(release_page(OPENSUSE_RPM, DEBUGINFO_RPM))

    def test_requires_fedora_dist_tag(self):
        self.assertRefuses(release_page("apexshot-0.2.35-1.x86_64.rpm"))

    def test_rejects_other_version(self):
        self.assertRefuses(release_page("apexshot-0.2.34-1.fc42.x86_64.rpm"))

    def test_version_dots_match_literally(self):
        self.assertRefuses(release_page("apexshot-0x2x35-1.fc42.x86_64.rpm"))


class OpenSuseResolverTest(ResolverCase):
    script = "opensuse-install.sh"

    def test_selects_main_opensuse_rpm_among_mixed_assets(self):
        page = release_page(FEDORA_RPM, DEBUGSOURCE_RPM, DEBUGINFO_RPM, SOURCE_RPM, OPENSUSE_RPM)
        self.assertSelects(page, OPENSUSE_RPM)

    def test_selects_plain_rpm_when_signature_sidecar_is_listed_first(self):
        self.assertSelects(release_page(OPENSUSE_RPM + ".sig", OPENSUSE_RPM), OPENSUSE_RPM)

    def test_refuses_signature_and_backup_sidecars_alone(self):
        for suffix in (".sig", ".backup"):
            with self.subTest(suffix=suffix):
                self.assertRefuses(release_page(OPENSUSE_RPM + suffix))

    def test_refuses_fedora_rpm_when_no_opensuse_rpm_is_published(self):
        self.assertRefuses(release_page(FEDORA_RPM, DEBUGINFO_RPM))

    def test_rejects_other_version(self):
        self.assertRefuses(release_page("apexshot-0.2.34-0.x86_64.rpm"))

    def test_refuses_debug_and_source_packages_alone(self):
        page = release_page(DEBUGINFO_RPM, OPENSUSE_DEBUGINFO_RPM, DEBUGSOURCE_RPM, SOURCE_RPM)
        self.assertRefuses(page)


class InstallerOrderCases:
    script = ""
    foreign_page = ""

    def run_installer(self, logged_functions, args=(), page=""):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            harness = tmp_path / "installer.sh"
            harness.write_text(installer_harness(self.script, logged_functions))
            log = tmp_path / "calls.log"
            log.touch()
            result = run_offline(
                tmp_path,
                page,
                ["bash", str(harness), *args],
                {"APEXSHOT_TEST_LOG": str(log)},
            )
            calls = log.read_text().split()
        return result, calls

    def test_asset_is_resolved_before_dependencies_are_installed(self):
        result, calls = self.run_installer(
            [
                "header",
                "check_prereqs",
                "download_rpm",
                "install_runtime_dependencies",
                "install_rpm",
                "post_install_launch",
                "summary",
            ]
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(
            calls,
            [
                "header",
                "check_prereqs",
                "download_rpm",
                "install_runtime_dependencies",
                "install_rpm",
                "post_install_launch",
                "summary",
            ],
        )

    def test_missing_asset_fails_before_dependencies_are_installed(self):
        result, calls = self.run_installer(
            [
                "header",
                "check_prereqs",
                "install_runtime_dependencies",
                "install_rpm",
                "post_install_launch",
                "summary",
            ],
            page=self.foreign_page,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("Could not find the", result.stdout + result.stderr)
        self.assertEqual(calls, ["header", "check_prereqs"])

    def test_from_source_refuses_before_dependencies_are_installed(self):
        result, calls = self.run_installer(
            [
                "header",
                "check_prereqs",
                "download_rpm",
                "install_runtime_dependencies",
                "install_rpm",
                "post_install_launch",
                "summary",
            ],
            args=["--from-source"],
        )
        self.assertEqual(result.returncode, 1)
        self.assertEqual(calls, ["header", "check_prereqs"])


class FedoraInstallerOrderTest(InstallerOrderCases, unittest.TestCase):
    script = "fedora-install.sh"
    foreign_page = release_page(OPENSUSE_RPM, DEBUGINFO_RPM)


class OpenSuseInstallerOrderTest(InstallerOrderCases, unittest.TestCase):
    script = "opensuse-install.sh"
    foreign_page = release_page(FEDORA_RPM, DEBUGINFO_RPM)


if __name__ == "__main__":
    unittest.main()
