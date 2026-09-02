# GNOME Extension Submission Guide

## Prerequisites

1. **GNOME account**: create one at https://extensions.gnome.org
2. **Screenshots**: 3–5 images showing the extension in action (see `screenshots/README.md`)
3. **Extension package**: build it from a clean checkout, see below

## Building the package

Only the six shipped files belong in the zip — no tests, no docs:

```bash
cd gnome-extension
zip apexshot-gnome-integration.zip \
  extension.js metadata.json \
  cursor-classifier.js shell-overlay.js window-list.js preview-stacking.js
```

Before uploading, run the review checks locally:

```bash
# Static analysis used by the EGO reviewers
uv tool install --force --with "tree-sitter==0.25.2" shexli
~/.local/bin/shexli "$(pwd)/apexshot-gnome-integration.zip"

# Syntax check every source file
for f in *.js tests/*.js; do node --check "$f"; done

# Unit tests (need the Mutter/Shell typelibs)
GI_TYPELIB_PATH=/usr/lib/x86_64-linux-gnu/mutter-18:/usr/lib/gnome-shell \
  gjs -m tests/window-list.test.js
```

`shexli` must report `clean (0 findings, 0 errors, 0 warnings)`.

## Submission steps

1. Go to https://extensions.gnome.org and log in.
2. Click "Upload Extension".
3. Fill in the metadata, which must match `metadata.json`:
   - **UUID**: `apexshot-gnome-integration@apexshot.github.io`
   - **Name**: ApexShot
   - **Description**: Support for the ApexShot screenshot and screen recording app —
     dims the area outside a recording, lets ApexShot list and focus windows for its
     window picker, and keeps ApexShot's preview windows above other windows.
   - **Version**: 4
   - **Supported GNOME versions**: 48, 49, 50
   - **Website**: https://github.com/apex-shot/apexshot
4. Upload the zip and the screenshots.
5. Submit for review.

Reviewers check code quality, timer and signal cleanup, and that no private
GNOME Shell internals are touched. Review usually takes one to two weeks.

## Alternative: GitHub Releases

To skip the official review, host the zip on GitHub Releases:

```bash
gh release create gnome-extension-v4 gnome-extension/apexshot-gnome-integration.zip
```

Users then install with:

```bash
wget https://github.com/apex-shot/apexshot/releases/download/gnome-extension-v4/apexshot-gnome-integration.zip
gnome-extensions install apexshot-gnome-integration.zip
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

## Testing before submission

```bash
mkdir -p ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io
cp extension.js metadata.json cursor-classifier.js shell-overlay.js window-list.js preview-stacking.js \
  ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io/
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
journalctl /usr/bin/gnome-shell -f | grep apexshot
```

On Wayland, log out and back in to reload the shell.

## Notes

- The extension does nothing on its own; ApexShot must be installed and running.
- Requires GNOME Shell 48 or newer.
