# Issue 32/33 Handoff

This note summarizes the recent packaging work and remaining follow-ups.

## Completed

### Issue 33: Arch/EndeavourOS uninstall bug

- Confirmed the report was valid for package-managed Arch installs.
- Root cause: `apexshot uninstall` only removed local source-install files under `/usr/local/bin`, while Arch release/AUR installs place binaries under `/usr/bin` and are owned by `pacman`.
- Fixed `apexshot uninstall` to detect package-managed installs and delegate Arch removal to `pacman -R apexshot`.
- Also fixed local/source uninstall to remove `/usr/local/bin/apexshot-native-host`.
- Commit: `dce6fc2 Fix uninstall for package-managed installs`
- Pushed to `origin/main`.
- Replied on issue 33: https://github.com/apex-shot/apexshot/issues/33#issuecomment-4763496897

Verified with:

```bash
cargo fmt --check
cargo check
cargo test uninstall
```

### Issue 32: openSUSE packaging request

- Investigated the user suggestion: a tested `zypper` dependency list for `opensuse/tumbleweed`.
- Implemented initial openSUSE support as a source-install path, not full RPM packaging.
- Added `scripts/opensuse-install.sh`.
- Added `scripts/opensuse-update.sh`.
- Wired `zypper` detection into `scripts/install.sh` and `scripts/update.sh`.
- Updated openSUSE distro support metadata to point at `scripts/opensuse-install.sh`.
- Updated `README.md` with openSUSE install/update instructions.
- Added regression coverage in `tests/package_metadata.rs` to keep the openSUSE dependency list and dispatcher wiring from regressing.
- Commit: `b867f02 Add openSUSE source install support`
- Pushed to `origin/main`.
- Added an initial `packaging/opensuse/apexshot.spec` RPM recipe, a local
  `scripts/build-opensuse-rpm.sh` helper, README instructions for local RPM
  builds, and zypper-backed package uninstall support. This is not yet OBS
  published or runtime validated.

Verified with:

```bash
bash -n scripts/opensuse-install.sh
bash -n scripts/opensuse-update.sh
cargo fmt --check
cargo check
cargo test opensuse_installer_contains_reported_dependency_set
```

## Current openSUSE Behavior

- The generic installer now dispatches to openSUSE when `zypper` is available.
- The openSUSE installer installs the reported build/runtime dependencies with `zypper`.
- The source installer clones the repo, builds with `cargo build --release`, and installs binaries into `/usr/local/bin`.
- It installs shared assets under `/usr/local/share/apexshot`.
- It runs user-level ApexShot setup separately from privileged system file installation.
- The update script reuses the installer with `--force`.
- The RPM spec packages `/usr/bin/apexshot`, `/usr/bin/apexshot-capture`, the
  native-host helper, desktop files, icons, GNOME extension files, browser
  native-messaging manifests, editor backgrounds, and sound assets.
- `apexshot uninstall` now delegates RPM-owned installs to
  `zypper --non-interactive remove apexshot`.

## Remaining Work

- Test the new scripts in a real `opensuse/tumbleweed` VM or container with GUI-related dependencies available.
- Verify runtime behavior on openSUSE KDE Plasma Wayland:
  - screenshot capture
  - area capture
  - screen recording
  - OCR
  - tray behavior
  - autostart behavior
  - uninstall flow
- Confirm all package names exist on both Tumbleweed and Leap. Some names may differ by release or enabled repository.
- Decide whether to install or recommend Packman for `ffmpeg`/codec behavior on openSUSE.
- Validate the initial RPM recipe on real openSUSE Tumbleweed and Leap systems.
- Build an OBS recipe/publishing flow once the spec is runtime validated.
- Add openSUSE CI/container build coverage if practical.
- Consider posting a progress reply on issue 32 once openSUSE runtime testing is complete.

## Useful Commands

Install on openSUSE:

```bash
curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/opensuse-install.sh | bash
```

Update on openSUSE:

```bash
curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/opensuse-update.sh | bash
```

Remove source install:

```bash
apexshot uninstall --autostart-only
sudo apexshot uninstall
```

Build a local RPM from a checkout:

```bash
scripts/build-opensuse-rpm.sh
sudo zypper install target/opensuse-rpmbuild/RPMS/*/apexshot-*.rpm
```
