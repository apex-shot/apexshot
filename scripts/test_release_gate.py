#!/usr/bin/env python3
import base64
import tempfile
import unittest
from pathlib import Path

import release_gate


class StudioConfigTests(unittest.TestCase):
    def setUp(self):
        self.public_key = base64.urlsafe_b64encode(bytes(range(32))).decode().rstrip("=")

    def test_accepts_synthetic_canonical_public_key_and_pinned_timestamp(self):
        self.assertEqual(release_gate.validate_studio_config(self.public_key, "1798761600"), 1798761600)

    def test_rejects_missing_malformed_noncanonical_key_and_bad_timestamp(self):
        for key, timestamp in (
            ("", "1798761600"),
            ("not-a-key", "1798761600"),
            (self.public_key + "=", "1798761600"),
            (self.public_key, ""),
            (self.public_key, "1.25"),
            (self.public_key, str(2**63)),
        ):
            with self.subTest(key=key, timestamp=timestamp):
                with self.assertRaises(release_gate.ReleaseGateError):
                    release_gate.validate_studio_config(key, timestamp)


class PromotionTests(unittest.TestCase):
    def test_promotion_requires_authorized_actor_main_stable_tag_and_confirmation(self):
        release_gate.authorize_promotion("codegoddy", "refs/heads/main", "v1.2.3", "PUBLISH v1.2.3")
        for actor, ref, tag, confirmation in (
            ("other-user", "refs/heads/main", "v1.2.3", "PUBLISH v1.2.3"),
            ("codegoddy", "refs/heads/feature", "v1.2.3", "PUBLISH v1.2.3"),
            ("codegoddy", "refs/heads/main", "v1.2.3-rc.1", "PUBLISH v1.2.3-rc.1"),
            ("codegoddy", "refs/heads/main", "v1.2.3", "PUBLISH v1.2.4"),
        ):
            with self.subTest(actor=actor, ref=ref, tag=tag, confirmation=confirmation):
                with self.assertRaises(release_gate.ReleaseGateError):
                    release_gate.authorize_promotion(actor, ref, tag, confirmation)

    def test_release_marker_must_advance(self):
        release_gate.validate_marker_advances(200, 100)
        for candidate in (100, 99):
            with self.subTest(candidate=candidate):
                with self.assertRaises(release_gate.ReleaseGateError):
                    release_gate.validate_marker_advances(candidate, 100)

    def test_studio_key_fingerprint_must_remain_unchanged_until_migration_exists(self):
        fingerprint = "a" * 64
        release_gate.validate_key_continuity(fingerprint, fingerprint)
        for candidate, previous in (
            ("b" * 64, fingerprint),
            ("bad", fingerprint),
            (fingerprint, "bad"),
        ):
            with self.subTest(candidate=candidate, previous=previous):
                with self.assertRaises(release_gate.ReleaseGateError):
                    release_gate.validate_key_continuity(candidate, previous)


class StagedAssetTests(unittest.TestCase):
    def setUp(self):
        self.public_key = base64.urlsafe_b64encode(bytes(range(32))).decode().rstrip("=")
        self.source_commit = "a" * 40

    def build_artifacts(self, root):
        files = {
            "debian-gnome/target/debian/apexshot_1.2.3-1_amd64.deb": b"deb",
            "debian-gnome/gnome-extension/apexshot-gnome-integration.zip": b"gnome",
            "opensuse-rpms/target/RPMS/x86_64/apexshot-1.2.3-0.x86_64.rpm": b"suse",
            "opensuse-rpms/target/SRPMS/apexshot-1.2.3-0.src.rpm": b"suse source",
            "opensuse-rpms/target/RPMS/x86_64/apexshot-debuginfo-1.2.3-0.x86_64.rpm": b"suse debug",
            "fedora-rpms/target/RPMS/x86_64/apexshot-1.2.3-1.fc42.x86_64.rpm": b"fedora",
            "fedora-rpms/target/RPMS/x86_64/apexshot-debuginfo-1.2.3-1.fc42.x86_64.rpm": b"fedora debug",
            "fedora-rpms/target/RPMS/x86_64/apexshot-debugsource-1.2.3-1.fc42.x86_64.rpm": b"fedora debug source",
            "fedora-rpms/target/SRPMS/apexshot-1.2.3-1.fc42.src.rpm": b"fedora source",
            "arch-package/packaging/arch/apexshot-1.2.3-1-x86_64.pkg.tar.zst": b"arch",
            "arch-package/packaging/arch/apexshot-debug-1.2.3-1-x86_64.pkg.tar.zst": b"arch debug",
        }
        for relative, contents in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(contents)
        return files

    def test_full_package_set_including_debug_variants_is_checksummed_and_verified(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build_root = root / "build"
            asset_dir = root / "assets"
            files = self.build_artifacts(build_root)
            release_gate.prepare_assets(
                build_root, asset_dir, "v1.2.3", self.public_key, "1798761600", self.source_commit
            )

            metadata = release_gate.validate_assets(asset_dir, require_stable=True)
            self.assertEqual(metadata["tag"], "v1.2.3")
            self.assertEqual(metadata["release_timestamp"], 1798761600)
            expected_names = {Path(path).name for path in files}
            checksum_names = {
                line.split("  ", 1)[1]
                for line in (asset_dir / release_gate.CHECKSUMS_NAME).read_text(encoding="utf-8").splitlines()
            }
            self.assertEqual(checksum_names, expected_names | {release_gate.METADATA_NAME})
            self.assertIn(
                "apexshot-debuginfo-1.2.3-1.fc42.x86_64.rpm",
                metadata["assets_by_artifact"]["fedora-rpms"],
            )

    def test_missing_package_group_duplicate_name_or_tampered_file_blocks_stage(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build_root = root / "build"
            self.build_artifacts(build_root)
            (build_root / "arch-package" / "packaging/arch/apexshot-1.2.3-1-x86_64.pkg.tar.zst").unlink()
            with self.assertRaises(release_gate.ReleaseGateError):
                release_gate.validate_build_artifacts(build_root, "1.2.3")

            build_root = root / "duplicate-build"
            self.build_artifacts(build_root)
            duplicate = build_root / "arch-package/apexshot_1.2.3-1_amd64.deb"
            duplicate.parent.mkdir(parents=True, exist_ok=True)
            duplicate.write_bytes(b"collision")
            with self.assertRaises(release_gate.ReleaseGateError):
                release_gate.prepare_assets(
                    build_root,
                    root / "duplicate-assets",
                    "v1.2.3",
                    self.public_key,
                    "1798761600",
                    self.source_commit,
                )

            build_root = root / "valid-build"
            self.build_artifacts(build_root)
            asset_dir = root / "valid-assets"
            release_gate.prepare_assets(
                build_root, asset_dir, "v1.2.3", self.public_key, "1798761600", self.source_commit
            )
            (asset_dir / "apexshot_1.2.3-1_amd64.deb").write_bytes(b"tampered")
            with self.assertRaises(release_gate.ReleaseGateError):
                release_gate.validate_assets(asset_dir, require_stable=True)

    def test_debug_rpms_cannot_stand_in_for_the_primary_application_packages(self):
        primary_packages = (
            "opensuse-rpms/target/RPMS/x86_64/apexshot-1.2.3-0.x86_64.rpm",
            "fedora-rpms/target/RPMS/x86_64/apexshot-1.2.3-1.fc42.x86_64.rpm",
            "arch-package/packaging/arch/apexshot-1.2.3-1-x86_64.pkg.tar.zst",
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for index, relative in enumerate(primary_packages):
                with self.subTest(package=relative):
                    build_root = root / f"build-{index}"
                    self.build_artifacts(build_root)
                    (build_root / relative).unlink()
                    debug_pattern = "*debug*.pkg.tar.zst" if relative.startswith("arch-package") else "*debuginfo*.rpm"
                    self.assertTrue(any((build_root / relative).parent.glob(debug_pattern)))
                    with self.assertRaises(release_gate.ReleaseGateError):
                        release_gate.validate_build_artifacts(build_root, "1.2.3")

    def test_debian_primary_matches_the_upstream_version_and_valid_revision(self):
        with tempfile.TemporaryDirectory() as temporary:
            build_root = Path(temporary) / "build"
            self.build_artifacts(build_root)
            package = build_root / "debian-gnome/target/debian/apexshot_1.2.3-1_amd64.deb"
            self.assertEqual(package.name, "apexshot_1.2.3-1_amd64.deb")
            release_gate.validate_build_artifacts(build_root, "1.2.3")

            wrong_version = package.with_name("apexshot_9.9.9-1_amd64.deb")
            package.rename(wrong_version)
            with self.assertRaises(release_gate.ReleaseGateError):
                release_gate.validate_build_artifacts(build_root, "1.2.3")

            wrong_version.unlink()
            debug_only = package.with_name("apexshot-dbgsym_1.2.3-1_amd64.deb")
            debug_only.write_bytes(b"debug symbols")
            with self.assertRaises(release_gate.ReleaseGateError):
                release_gate.validate_build_artifacts(build_root, "1.2.3")

    def test_promotion_rejects_a_draft_built_with_changed_public_config(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build_root = root / "build"
            self.build_artifacts(build_root)
            asset_dir = root / "assets"
            release_gate.prepare_assets(
                build_root, asset_dir, "v1.2.3", self.public_key, "1798761600", self.source_commit
            )
            changed_key = base64.urlsafe_b64encode(bytes(reversed(range(32)))).decode().rstrip("=")
            with self.assertRaises(release_gate.ReleaseGateError):
                release_gate.validate_assets(
                    asset_dir,
                    require_stable=True,
                    verify_key=changed_key,
                    release_timestamp="1798761600",
                )

    def test_promotion_rejects_assets_from_a_different_commit_than_the_tag(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build_root = root / "build"
            self.build_artifacts(build_root)
            asset_dir = root / "assets"
            release_gate.prepare_assets(
                build_root, asset_dir, "v1.2.3", self.public_key, "1798761600", self.source_commit
            )
            with self.assertRaises(release_gate.ReleaseGateError):
                release_gate.validate_assets(
                    asset_dir,
                    require_stable=True,
                    expected_source_commit="b" * 40,
                )


class WorkflowSourceTests(unittest.TestCase):
    def test_build_workflow_stages_artifacts_and_cannot_publish_or_update_aur(self):
        workflow = (Path(__file__).parent.parent / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertNotIn("softprops/action-gh-release", workflow)
        self.assertIn("name: Stage complete draft release", workflow)
        self.assertIn("--draft", workflow)
        self.assertIn("name: debian-gnome", workflow)
        self.assertIn("name: opensuse-rpms", workflow)
        self.assertIn("name: fedora-rpms", workflow)
        self.assertIn("name: arch-package", workflow)
        self.assertNotIn("Publish AUR package", workflow)
        stage = workflow.split("name: Stage complete draft release", 1)[1]
        self.assertIn(
            "APEXSHOT_STUDIO_RELEASE_TIMESTAMP: ${{ needs.studio_release_config.outputs.release_timestamp }}",
            stage,
        )
        self.assertNotIn("vars.APEXSHOT_STUDIO_RELEASE_TIMESTAMP", stage)
        self.assertIn("stable-release-${{ github.repository }}-${{ github.ref_name }}", stage)
        self.assertIn("$GITHUB_SHA", stage)

    def test_promotion_workflow_gates_publish_and_aur_on_manual_authorized_validation(self):
        workflow = (Path(__file__).parent.parent / ".github/workflows/promote-release.yml").read_text(encoding="utf-8")
        self.assertIn("workflow_dispatch:", workflow)
        self.assertIn("environment: stable-release", workflow)
        self.assertIn("needs: [authorize]", workflow)
        self.assertIn("validate-assets staged-assets", workflow)
        self.assertIn("--require-stable", workflow)
        self.assertIn("--verify-key", workflow)
        self.assertIn("needs: [promote]", workflow)
        self.assertIn("validate-marker-advances", workflow)
        self.assertIn("validate-key-continuity", workflow)
        self.assertIn("AUR_SSH_KNOWN_HOSTS", workflow)
        self.assertNotIn("ssh-keyscan", workflow)
        self.assertIn("stable-release-${{ github.repository }}-${{ inputs.tag }}", workflow)
        self.assertIn("--source-commit", workflow)
        self.assertLess(
            workflow.index("name: Install release verification tools"),
            workflow.index("name: Confirm the stable release is published"),
        )

    def test_local_flatpak_build_uses_validated_public_config_without_becoming_a_submission_manifest(
        self,
    ):
        root = Path(__file__).parent.parent
        workflow = (root / ".github/workflows/flatpak.yml").read_text(encoding="utf-8")
        manifest = (root / "flatpak/org.apexshot.ApexShot.yml").read_text(encoding="utf-8")
        readme = (root / "flatpak/README.md").read_text(encoding="utf-8")
        helper = (root / "flatpak/prepare-local-build-config.sh").read_text(encoding="utf-8")

        self.assertIn("vars.APEXSHOT_STUDIO_VERIFY_KEY", workflow)
        self.assertIn("vars.APEXSHOT_STUDIO_RELEASE_TIMESTAMP", workflow)
        self.assertIn("bash flatpak/prepare-local-build-config.sh", workflow)
        self.assertIn("flatpak/.studio-build-config.env", manifest)
        self.assertIn('scripts/release_gate.py" validate-config', helper)
        source = manifest.split("path: ..", 1)[1]
        for private_path in (".env", ".env.*", "AGENTS.local.md", ".agents", ".claude", ".codex"):
            self.assertIn(f"- {private_path}", source)
        self.assertNotIn(".studio-build-config.env", source)
        self.assertIn("not a submission manifest", readme)
        self.assertIn("created with AI assistance", readme)


if __name__ == "__main__":
    unittest.main()
