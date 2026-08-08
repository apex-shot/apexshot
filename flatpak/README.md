# ApexShot Flatpak

Portal-only package (`--features flatpak`). App ID: `org.apexshot.ApexShot`.

## Prerequisites

```bash
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y flathub \
  org.gnome.Platform//49 \
  org.gnome.Sdk//49 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08 \
  org.freedesktop.Sdk.Extension.llvm20//25.08
```

GNOME 49 is based on Freedesktop 25.08 — SDK extensions must use that branch.

## Local build / install

From the repo root:

```bash
flatpak-builder --user --install --force-clean build-dir \
  flatpak/org.apexshot.ApexShot.yml
flatpak run org.apexshot.ApexShot
```

First build downloads crates (needs network). Subsequent rebuilds are faster.

## Flathub submission checklist

1. Pin a release tag + archive sha256 instead of `type: dir`.
2. Generate offline Cargo sources:

   ```bash
   curl -LO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
   python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json
   ```

3. In the manifest `apexshot` module:
   - add `flatpak/cargo-sources.json` to `sources`
   - switch build commands to `cargo --offline fetch` / `cargo --offline build`
4. Do **not** add `--filesystem=host` or `flatpak-spawn --host`.
5. Domain verification file on `apexshot.org` (cloud/ops).
6. Maintainer-authored PR description (no AI-paste policy risk).

## What this build disables

- Qt `apexshot-capture` helper
- Host package install/uninstall, compositor config writes, `wf-recorder`, etc.
- System Tesseract (OCR uses bundled pure-Rust `ocrs` + downloaded models)
- Telemetry unless the user opts in
