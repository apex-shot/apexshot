#!/usr/bin/env python3
"""Validate staged Studio releases without publishing them."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import shlex
import shutil
import sys
from pathlib import Path


PROMOTION_ACTOR = "codegoddy"
ARTIFACT_DIRECTORIES = {
    "debian-gnome": "Debian package and GNOME extension",
    "opensuse-rpms": "openSUSE RPMs",
    "fedora-rpms": "Fedora RPMs",
    "arch-package": "Arch package",
}
METADATA_NAME = "STUDIO_RELEASE_METADATA.json"
CHECKSUMS_NAME = "SHA256SUMS"


class ReleaseGateError(ValueError):
    pass


def is_stable_tag(tag: str) -> bool:
    return re.fullmatch(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", tag) is not None


def validate_studio_config(verify_key: str, release_timestamp: str) -> int:
    if re.fullmatch(r"[A-Za-z0-9_-]{43}", verify_key) is None:
        raise ReleaseGateError("APEXSHOT_STUDIO_VERIFY_KEY must be unpadded base64url for 32 bytes")
    try:
        key_bytes = base64.urlsafe_b64decode(verify_key + "=")
    except (ValueError, base64.binascii.Error) as error:
        raise ReleaseGateError("APEXSHOT_STUDIO_VERIFY_KEY is malformed") from error
    if len(key_bytes) != 32 or base64.urlsafe_b64encode(key_bytes).decode().rstrip("=") != verify_key:
        raise ReleaseGateError("APEXSHOT_STUDIO_VERIFY_KEY must be canonical unpadded base64url for 32 bytes")
    if re.fullmatch(r"-?(?:0|[1-9][0-9]*)", release_timestamp) is None:
        raise ReleaseGateError("APEXSHOT_STUDIO_RELEASE_TIMESTAMP must be a pinned Unix timestamp")
    timestamp = int(release_timestamp)
    if not 0 < timestamp <= 2**63 - 1:
        raise ReleaseGateError("APEXSHOT_STUDIO_RELEASE_TIMESTAMP must be a positive i64 Unix timestamp")
    return timestamp


def public_key_fingerprint(verify_key: str) -> str:
    decoded = base64.urlsafe_b64decode(verify_key + "=")
    return hashlib.sha256(decoded).hexdigest()


def version_from_tag(tag: str) -> str:
    if not tag.startswith("v") or len(tag) == 1:
        raise ReleaseGateError("Release tag must start with v and include a package version")
    return tag[1:]


def authorize_promotion(actor: str, ref: str, tag: str, confirmation: str) -> None:
    if actor != PROMOTION_ACTOR:
        raise ReleaseGateError("Only the configured release maintainer may promote a stable release")
    if ref != "refs/heads/main":
        raise ReleaseGateError("Stable promotion must be dispatched from main")
    if not is_stable_tag(tag):
        raise ReleaseGateError("Only a stable vMAJOR.MINOR.PATCH tag can be promoted")
    if confirmation != f"PUBLISH {tag}":
        raise ReleaseGateError("Confirmation must exactly match: PUBLISH <tag>")


def _files_under(directory: Path) -> list[Path]:
    if not directory.is_dir():
        return []
    files = []
    for path in directory.rglob("*"):
        if path.is_symlink():
            raise ReleaseGateError(f"Release artifact must not contain symlinks: {path}")
        if path.is_file():
            if path.stat().st_size == 0:
                raise ReleaseGateError(f"Release artifact is empty: {path}")
            files.append(path)
    return files


def _has_primary_rpm(paths: list[Path], version: str) -> bool:
    prefix = f"apexshot-{version}-"
    return any(
        path.name.startswith(prefix)
        and path.name.endswith(".x86_64.rpm")
        and not path.name.endswith(".src.rpm")
        for path in paths
    )


def _has_primary_deb(paths: list[Path], version: str) -> bool:
    pattern = re.compile(rf"apexshot_{re.escape(version)}(?:-[0-9]+)?_amd64\.deb")
    return any(pattern.fullmatch(path.name) for path in paths)


def _has_source_rpm(paths: list[Path], version: str) -> bool:
    prefix = f"apexshot-{version}-"
    return any(path.name.startswith(prefix) and path.name.endswith(".src.rpm") for path in paths)


def _has_primary_arch_package(paths: list[Path], version: str) -> bool:
    prefix = f"apexshot-{version}-"
    return any(
        path.name.startswith(prefix) and path.name.endswith("-x86_64.pkg.tar.zst")
        for path in paths
    )


def is_commit_id(value: object) -> bool:
    return isinstance(value, str) and re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", value) is not None


def validate_build_artifacts(build_root: Path, version: str) -> dict[str, list[Path]]:
    result = {}
    for directory, description in ARTIFACT_DIRECTORIES.items():
        paths = _files_under(build_root / directory)
        if not paths:
            raise ReleaseGateError(f"Missing {description} artifact")
        result[directory] = paths

    debian_files = [path for path in result["debian-gnome"] if path.suffix == ".deb"]
    gnome_files = [path for path in result["debian-gnome"] if path.name == "apexshot-gnome-integration.zip"]
    if not _has_primary_deb(debian_files, version) or not gnome_files:
        raise ReleaseGateError("Staged Debian build must include the matching amd64 package and GNOME extension zip")

    for directory in ("opensuse-rpms", "fedora-rpms"):
        paths = result[directory]
        if not _has_source_rpm(paths, version):
            raise ReleaseGateError(f"Staged {directory} build is missing the matching source RPM")
        if not _has_primary_rpm(paths, version):
            raise ReleaseGateError(f"Staged {directory} build is missing the matching x86_64 ApexShot RPM")

    if not _has_primary_arch_package(result["arch-package"], version):
        raise ReleaseGateError("Staged Arch build is missing the matching x86_64 ApexShot package")
    return result


def prepare_assets(
    build_root: Path,
    asset_dir: Path,
    tag: str,
    verify_key: str,
    release_timestamp: str,
    source_commit: str,
) -> None:
    version = version_from_tag(tag)
    validate_build_artifacts(build_root, version)
    timestamp = validate_studio_config(verify_key, release_timestamp)
    if not is_commit_id(source_commit):
        raise ReleaseGateError("Source commit must be a full lowercase Git object id")

    asset_dir.mkdir(parents=True, exist_ok=True)
    by_artifact: dict[str, list[str]] = {}
    used_names = set()
    for directory in ARTIFACT_DIRECTORIES:
        names = []
        for source in sorted(_files_under(build_root / directory)):
            if source.name in used_names:
                raise ReleaseGateError(f"Duplicate release asset filename: {source.name}")
            destination = asset_dir / source.name
            shutil.copy2(source, destination)
            used_names.add(source.name)
            names.append(source.name)
        by_artifact[directory] = names

    metadata = {
        "schema_version": 2,
        "tag": tag,
        "source_commit": source_commit,
        "release_timestamp": timestamp,
        "verify_key_sha256": public_key_fingerprint(verify_key),
        "assets_by_artifact": by_artifact,
    }
    (asset_dir / METADATA_NAME).write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    write_checksums(asset_dir)
    validate_assets(asset_dir)


def write_checksums(asset_dir: Path) -> None:
    assets = sorted(
        path for path in asset_dir.iterdir() if path.is_file() and path.name != CHECKSUMS_NAME
    )
    if not assets:
        raise ReleaseGateError("Cannot create a checksum list without release assets")
    lines = []
    for path in assets:
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        lines.append(f"{digest}  {path.name}")
    (asset_dir / CHECKSUMS_NAME).write_text("\n".join(lines) + "\n", encoding="utf-8")


def validate_assets(
    asset_dir: Path,
    require_stable: bool = False,
    verify_key: str | None = None,
    release_timestamp: str | None = None,
    expected_source_commit: str | None = None,
) -> dict:
    metadata_path = asset_dir / METADATA_NAME
    checksums_path = asset_dir / CHECKSUMS_NAME
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ReleaseGateError("Missing or invalid Studio release metadata") from error
    if metadata.get("schema_version") != 2:
        raise ReleaseGateError("Unsupported Studio release metadata schema")
    tag = metadata.get("tag")
    source_commit = metadata.get("source_commit")
    timestamp = metadata.get("release_timestamp")
    if not isinstance(tag, str) or not isinstance(timestamp, int) or not 0 < timestamp <= 2**63 - 1:
        raise ReleaseGateError("Studio release metadata has an invalid tag or timestamp")
    if not is_commit_id(source_commit):
        raise ReleaseGateError("Studio release metadata has an invalid source commit")
    if expected_source_commit is not None:
        if not is_commit_id(expected_source_commit) or source_commit != expected_source_commit:
            raise ReleaseGateError("Staged binaries do not match the requested tag's source commit")
    fingerprint = metadata.get("verify_key_sha256")
    if not isinstance(fingerprint, str) or re.fullmatch(r"[0-9a-f]{64}", fingerprint) is None:
        raise ReleaseGateError("Studio release metadata has an invalid verification-key fingerprint")
    if verify_key is not None or release_timestamp is not None:
        if verify_key is None or release_timestamp is None:
            raise ReleaseGateError("Both the current verification key and release marker are required")
        current_timestamp = validate_studio_config(verify_key, release_timestamp)
        if fingerprint != public_key_fingerprint(verify_key) or timestamp != current_timestamp:
            raise ReleaseGateError("Staged binaries do not match the current pinned Studio build configuration")
    if require_stable and not is_stable_tag(tag):
        raise ReleaseGateError("Only a stable vMAJOR.MINOR.PATCH tag can be promoted")

    artifact_assets = metadata.get("assets_by_artifact")
    if not isinstance(artifact_assets, dict) or set(artifact_assets) != set(ARTIFACT_DIRECTORIES):
        raise ReleaseGateError("Studio release metadata is missing required package groups")
    expected_names = {METADATA_NAME}
    for artifact, names in artifact_assets.items():
        if not isinstance(names, list) or not all(isinstance(name, str) for name in names):
            raise ReleaseGateError(f"Invalid asset list for {artifact}")
        if any(Path(name).name != name or name in (CHECKSUMS_NAME, METADATA_NAME) for name in names):
            raise ReleaseGateError(f"Invalid asset filename in {artifact}")
        expected_names.update(names)

    all_names = {path.name for path in asset_dir.iterdir() if path.is_file()}
    if all_names != expected_names | {CHECKSUMS_NAME}:
        raise ReleaseGateError("Release assets do not exactly match the staged asset manifest")

    grouped = {key: [asset_dir / name for name in names] for key, names in artifact_assets.items()}
    version = version_from_tag(tag)
    debian_files = [path for path in grouped["debian-gnome"] if path.suffix == ".deb"]
    gnome_files = [path for path in grouped["debian-gnome"] if path.name == "apexshot-gnome-integration.zip"]
    if not _has_primary_deb(debian_files, version) or not gnome_files:
        raise ReleaseGateError("Release manifest is missing the matching amd64 Debian package or GNOME extension zip")
    for directory in ("opensuse-rpms", "fedora-rpms"):
        paths = grouped[directory]
        if not _has_source_rpm(paths, version):
            raise ReleaseGateError(f"Release manifest is missing the matching {directory} source RPM")
        if not _has_primary_rpm(paths, version):
            raise ReleaseGateError(f"Release manifest is missing the matching x86_64 {directory} ApexShot RPM")
    if not _has_primary_arch_package(grouped["arch-package"], version):
        raise ReleaseGateError("Release manifest is missing the matching x86_64 ApexShot Arch package")

    try:
        checksum_lines = checksums_path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise ReleaseGateError("Missing SHA256SUMS") from error
    listed = set()
    for line in checksum_lines:
        match = re.fullmatch(r"([0-9a-f]{64})  ([^/\\\n]+)", line)
        if match is None:
            raise ReleaseGateError("SHA256SUMS contains an invalid entry")
        digest, name = match.groups()
        if name in listed or name not in expected_names:
            raise ReleaseGateError("SHA256SUMS contains duplicate or unexpected assets")
        listed.add(name)
        if hashlib.sha256((asset_dir / name).read_bytes()).hexdigest() != digest:
            raise ReleaseGateError(f"Checksum verification failed for {name}")
    if listed != expected_names:
        raise ReleaseGateError("SHA256SUMS does not cover every release asset")
    return metadata


def validate_marker_advances(candidate: int, previous: int) -> None:
    if candidate <= previous:
        raise ReleaseGateError("The candidate build marker must be newer than the latest stable marker")


def validate_key_continuity(candidate: str, previous: str) -> None:
    if re.fullmatch(r"[0-9a-f]{64}", candidate) is None or re.fullmatch(
        r"[0-9a-f]{64}", previous
    ) is None:
        raise ReleaseGateError("Studio release metadata has an invalid verification-key fingerprint")
    if candidate != previous:
        raise ReleaseGateError(
            "The Studio verification key changed; a token migration must be designed before promotion"
        )


def inject_aur_config(pkgbuild: Path, verify_key: str, release_timestamp: str) -> None:
    validate_studio_config(verify_key, release_timestamp)
    contents = pkgbuild.read_text(encoding="utf-8")
    contents, key_count = re.subn(
        r"^_APEXSHOT_STUDIO_VERIFY_KEY=.*$",
        f"_APEXSHOT_STUDIO_VERIFY_KEY={shlex.quote(verify_key)}",
        contents,
        count=1,
        flags=re.MULTILINE,
    )
    contents, timestamp_count = re.subn(
        r"^_APEXSHOT_STUDIO_RELEASE_TIMESTAMP=.*$",
        f"_APEXSHOT_STUDIO_RELEASE_TIMESTAMP={shlex.quote(release_timestamp)}",
        contents,
        count=1,
        flags=re.MULTILINE,
    )
    if key_count != 1 or timestamp_count != 1:
        raise ReleaseGateError("PKGBUILD is missing Studio configuration placeholders")
    pkgbuild.write_text(contents, encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    config = commands.add_parser("validate-config")
    config.add_argument("verify_key")
    config.add_argument("release_timestamp")

    authorize = commands.add_parser("authorize-promotion")
    authorize.add_argument("actor")
    authorize.add_argument("ref")
    authorize.add_argument("tag")
    authorize.add_argument("confirmation")

    prepare = commands.add_parser("prepare-assets")
    prepare.add_argument("build_root", type=Path)
    prepare.add_argument("asset_dir", type=Path)
    prepare.add_argument("tag")
    prepare.add_argument("verify_key")
    prepare.add_argument("release_timestamp")
    prepare.add_argument("source_commit")

    validate = commands.add_parser("validate-assets")
    validate.add_argument("asset_dir", type=Path)
    validate.add_argument("--require-stable", action="store_true")
    validate.add_argument("--verify-key")
    validate.add_argument("--release-timestamp")
    validate.add_argument("--source-commit")

    marker = commands.add_parser("validate-marker-advances")
    marker.add_argument("candidate", type=int)
    marker.add_argument("previous", type=int)

    key_continuity = commands.add_parser("validate-key-continuity")
    key_continuity.add_argument("candidate")
    key_continuity.add_argument("previous")

    inject = commands.add_parser("inject-aur-config")
    inject.add_argument("pkgbuild", type=Path)
    inject.add_argument("verify_key")
    inject.add_argument("release_timestamp")

    args = parser.parse_args(argv)
    try:
        if args.command == "validate-config":
            validate_studio_config(args.verify_key, args.release_timestamp)
        elif args.command == "authorize-promotion":
            authorize_promotion(args.actor, args.ref, args.tag, args.confirmation)
        elif args.command == "prepare-assets":
            prepare_assets(
                args.build_root,
                args.asset_dir,
                args.tag,
                args.verify_key,
                args.release_timestamp,
                args.source_commit,
            )
        elif args.command == "validate-assets":
            validate_assets(
                args.asset_dir,
                args.require_stable,
                args.verify_key,
                args.release_timestamp,
                args.source_commit,
            )
        elif args.command == "validate-marker-advances":
            validate_marker_advances(args.candidate, args.previous)
        elif args.command == "validate-key-continuity":
            validate_key_continuity(args.candidate, args.previous)
        elif args.command == "inject-aur-config":
            inject_aur_config(args.pkgbuild, args.verify_key, args.release_timestamp)
        else:
            parser.error("unknown command")
    except (OSError, ReleaseGateError) as error:
        print(f"release gate: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
