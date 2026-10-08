#!/usr/bin/env python3

"""Register the Flatpak native host with a host-installed Chrome browser."""

import argparse
import json
import os
import shlex
import shutil
import sys
import tempfile
from pathlib import Path


APP_ID = "org.apexshot.ApexShot"
HOST_NAME = "io.github.codegoddy.apexshot"
MANIFEST_NAME = f"{HOST_NAME}.json"
DEFAULT_EXTENSION_ID = "kaejmfabajnakpodjffipckmcpfpdenj"
LAUNCHER_NAME = "apexshot-flatpak-native-host"
FLATPAK_INFO_PATH = Path("/.flatpak-info")
SANDBOX_ERROR = (
    "Refusing to register a browser host from inside Flatpak. "
    "Run this script with the host Python interpreter."
)
BROWSER_CONFIG_DIRS = {
    "chrome": Path("google-chrome/NativeMessagingHosts"),
    "chromium": Path("chromium/NativeMessagingHosts"),
}


class InstallError(Exception):
    """A safe-to-show error while preparing the host registration."""


def running_in_flatpak(environ=None):
    env = os.environ if environ is None else environ
    return bool(env.get("FLATPAK_ID")) or FLATPAK_INFO_PATH.exists()


def validate_extension_id(extension_id):
    if len(extension_id) != 32 or any(char < "a" or char > "p" for char in extension_id):
        raise InstallError("Extension ID must be exactly 32 lowercase letters from a to p.")


def config_dir(environ=None):
    env = os.environ if environ is None else environ
    xdg_config_home = env.get("XDG_CONFIG_HOME")
    if xdg_config_home:
        path = Path(xdg_config_home).expanduser()
        if not path.is_absolute():
            raise InstallError("XDG_CONFIG_HOME must be an absolute path.")
        return path

    home = env.get("HOME")
    if not home:
        raise InstallError("HOME is not set; cannot locate browser configuration.")
    path = Path(home).expanduser()
    if not path.is_absolute():
        raise InstallError("HOME must be an absolute path.")
    return path / ".config"


def browser_manifest_paths(configuration_dir, browsers):
    return [configuration_dir / BROWSER_CONFIG_DIRS[browser] / MANIFEST_NAME for browser in browsers]


def resolve_flatpak(path_lookup=shutil.which):
    found = path_lookup("flatpak")
    if not found:
        raise InstallError("Could not find the host Flatpak executable in PATH.")
    resolved = Path(found).expanduser().resolve()
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise InstallError(f"Host Flatpak executable is not executable: {resolved}")
    return resolved


def launcher_contents(flatpak_path):
    return "#!/bin/sh\nexec " + shlex.quote(str(flatpak_path)) + f" run {APP_ID} native-host\n"


def host_manifest(extension_id, launcher_path):
    return {
        "name": HOST_NAME,
        "description": "ApexShot Flatpak native host",
        "path": str(launcher_path),
        "type": "stdio",
        "allowed_origins": [f"chrome-extension://{extension_id}/"],
    }


def existing_content_matches(path, expected, json_file=False):
    if not path.exists() or path.is_symlink():
        return False
    if json_file:
        try:
            return json.loads(path.read_text(encoding="utf-8")) == expected
        except (OSError, json.JSONDecodeError):
            return False
    try:
        return path.read_text(encoding="utf-8") == expected
    except (OSError, UnicodeDecodeError):
        return False


def check_destination(path, expected, replace, json_file=False):
    if not path.exists() and not path.is_symlink():
        return
    if path.is_dir() and not path.is_symlink():
        raise InstallError(f"Refusing to replace a directory: {path}")
    if existing_content_matches(path, expected, json_file):
        return
    if not replace:
        raise InstallError(
            f"Refusing to overwrite existing file: {path}\n"
            "Use --replace only if you intend to switch this browser to the Flatpak app."
        )


def stage_file(path, contents, mode):
    path.parent.mkdir(parents=True, exist_ok=True)
    file_descriptor, temporary_path = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(file_descriptor, "w", encoding="utf-8") as stream:
            stream.write(contents)
        os.chmod(temporary_path, mode)
        return Path(temporary_path)
    except BaseException:
        try:
            os.unlink(temporary_path)
        except FileNotFoundError:
            pass
        raise


def install_bridge(browsers, extension_id, replace, environ=None, flatpak_path=None):
    env = os.environ if environ is None else environ
    if running_in_flatpak(env):
        raise InstallError(SANDBOX_ERROR)

    validate_extension_id(extension_id)
    configuration_dir = config_dir(env)
    home = env.get("HOME")
    if not home:
        raise InstallError("HOME is not set; cannot locate the native-host launcher directory.")
    home_path = Path(home).expanduser()
    if not home_path.is_absolute():
        raise InstallError("HOME must be an absolute path.")

    flatpak = resolve_flatpak() if flatpak_path is None else Path(flatpak_path).resolve()
    launcher_path = home_path / ".local" / "bin" / LAUNCHER_NAME
    launcher = launcher_contents(flatpak)
    manifest = host_manifest(extension_id, launcher_path)
    manifest_text = json.dumps(manifest, indent=2) + "\n"
    manifests = browser_manifest_paths(configuration_dir, browsers)

    check_destination(launcher_path, launcher, replace)
    for path in manifests:
        check_destination(path, manifest, replace, json_file=True)

    staged = []
    try:
        staged.append((stage_file(launcher_path, launcher, 0o755), launcher_path))
        staged.extend(
            (stage_file(path, manifest_text, 0o644), path)
            for path in manifests
        )
        for temporary_path, path in staged:
            os.replace(temporary_path, path)
    finally:
        for temporary_path, _ in staged:
            try:
                temporary_path.unlink()
            except FileNotFoundError:
                pass

    return manifests


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description="Register a host Chrome/Chromium browser to launch the ApexShot Flatpak native host."
    )
    parser.add_argument(
        "--browser",
        choices=("chrome", "chromium", "both"),
        default="both",
        help="which host browser(s) to register (default: both)",
    )
    parser.add_argument(
        "--extension-id",
        default=DEFAULT_EXTENSION_ID,
        help="Chrome extension ID allowed to connect (default: the published extension)",
    )
    parser.add_argument(
        "--replace",
        action="store_true",
        help="replace existing per-user browser registration files",
    )
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    if running_in_flatpak():
        print(SANDBOX_ERROR, file=sys.stderr)
        return 1

    browsers = tuple(BROWSER_CONFIG_DIRS) if args.browser == "both" else (args.browser,)
    try:
        manifests = install_bridge(browsers, args.extension_id, args.replace)
    except (InstallError, OSError) as error:
        print(f"Browser host registration failed: {error}", file=sys.stderr)
        return 1

    for path in manifests:
        print(f"Registered {path}")
    print("The native package remains installed; this changes only browser registration.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
