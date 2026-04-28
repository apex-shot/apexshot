# Contributing to ApexShot

Thank you for your interest in contributing to ApexShot! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Getting Started](#getting-started)
- [Where Things Live](#where-things-live)
- [Development Setup](#development-setup)
- [Code Style](#code-style)
- [Testing](#testing)
- [Submitting Changes](#submitting-changes)
- [Reporting Issues](#reporting-issues)
- [Areas for Contribution](#areas-for-contribution)
- [Communication](#communication)

## Getting Started

### Prerequisites

- **Rust** (latest stable, install via [rustup](https://www.rust-lang.org/tools/install)).
  The workflow expects the `rustfmt` and `clippy` components — `rustup
  component add rustfmt clippy` if you don't already have them.
- **C++17 compiler** (`g++` or `clang`) and **CMake ≥ 3.16** for the
  capture-overlay binary in `capture-overlay/`.
- **GTK4** + **gtk4-layer-shell** development headers (the latter built
  from source — see the GitHub Actions workflow for the exact recipe).
- **GStreamer 1.0** with `plugins-base`, `plugins-good`, `plugins-bad`,
  `libav`, `pipewire`, `pulseaudio` runtime plugins.
- **Qt5** (`qtbase5-dev`, `libqt5x11extras5-dev`).
- **PipeWire**, **Tesseract**, **xkbcommon**, **libxtst**, **libwayland**,
  **libdbus-1**, **pkg-config**.
- **Node.js ≥ 20** is needed *only* if you want to run the JS syntax
  check locally (`node --check gnome-extension/*.js`); the GNOME-side
  tests themselves run inside GJS, not Node.

The full list of run-time package names that ship via the `.deb` is in
`Cargo.toml` under `[package.metadata.deb] depends = ...`. The full list
of build-time `apt` packages used by CI is in
[`.github/workflows/release.yml`](.github/workflows/release.yml).

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/apexshot.git
   cd apexshot
   ```
3. Add the upstream repository:
   ```bash
   git remote add upstream https://github.com/apex-shot/apexshot.git
   ```

## Where Things Live

ApexShot is a Rust application with two satellite codebases (a Qt5 C++
overlay and a GNOME Shell extension) plus a browser-side scroll capture
helper. If you're new to the repo, this map should help you find the
right starting point for a change.

| Subsystem                             | Lives in                                            | Notes |
|---------------------------------------|-----------------------------------------------------|-------|
| Rust binary entry point + CLI         | `src/main.rs`                                       | Dispatches `daemon`, `capture`, `record`, `edit`, `preview`, `settings` subcommands. |
| Background daemon, hotkeys, tray      | `src/daemon/`, `src/hotkeys/`, `src/tray/`          | D-Bus triggers, system tray (`ksni`), global shortcut registration. |
| Capture backends (X11 / Wayland)      | `src/backend/`                                      | Tier list: wlr-screencopy → grim → portal Screenshot → portal ScreenCast (incl. `restore_token` cache). |
| Image editor + annotations            | `src/capture/editor/`                               | GTK4 + Cairo. Pen/highlighter rendering, color palette, selection, crop. |
| Preview overlay                       | `src/capture/preview_overlay.rs`                    | Quick-access card after capture (drag, edit, copy, save). |
| Recording pipeline                    | `src/recording/`                                    | GStreamer, GIF / video encoding, audio source discovery (`pactl`). |
| OCR + QR                              | `src/ocr/`, `src/qr/`                               | Tesseract LSTM with multi-PSM voting, deskew, QR-first detection. |
| Settings UI                           | `src/settings/`                                     | GTK4 preferences window, shortcut editor, recording options. |
| Onboarding wizard                     | `src/onboarding/`                                   | First-run setup, GNOME extension installer (`wget` + `curl`). |
| Cross-cutting utilities               | `src/utils/`                                        | Clipboard, desktop env detection, scoped portal identity. |
| Native interactive overlay (Qt5)      | `capture-overlay/src/`                              | Selection, recording controls, click-options panel; built independently with CMake. Feature flags in `CaptureOverlay.h`. |
| GNOME Shell extension                 | `gnome-extension/`                                  | Runtime click overlay, mask UI, recording controls dock, screenshot lock. ES modules + GJS tests. |
| Native messaging host (browser)       | `native-host/`                                      | Bridge between Chrome/Chromium and the daemon. |
| Chrome/Chromium extension             | `web-scroll-extension/`                             | Full-page scroll capture orchestration. |
| Packaging (.deb)                      | `Cargo.toml [package.metadata.deb]`, `packaging/`   | Asset list, postinst/prerm, desktop file, icon, native-host manifest. |
| CI                                    | `.github/workflows/release.yml`                     | Lint + build/test + tagged release. |
| Architecture / module docs            | `docs/ARCHITECTURE.md`, `docs/MODULES.md`, `docs/DEVELOPER_GUIDE.md` | Deeper dives once you know the file you're touching. |

A handful of cross-cutting conventions worth knowing up front:

- **Feature flags** for in-flight UI go in
  `capture-overlay/src/CaptureOverlay.h` (look for
  `apexshot::kKeystrokesFeatureAvailable` as the canonical example).
  The flag gates rendering, click handling, *and* the public accessor
  the recorder reads, so flipping it can never leave one half stranded.
- **Restore-token caches** for the XDG ScreenCast portal live at
  `~/.cache/apexshot/`. The Rust path uses
  `wayland-screencast-monitor.token` / `-window.token`; the C++ overlay
  uses `cpp-screencast.token`. Distinct files so neither can clobber
  the other's grant.
- **Drawing-area redraw throttle** for the editor is a single constant
  (`DRAG_REDRAW_INTERVAL_US` in `src/capture/editor/color.rs`). Keep
  per-frame work cheap — `draft_action()` runs on every redraw.

## Development Setup

### Building the Project

```bash
# Build in debug mode (faster compilation)
cargo build

# Build in release mode (optimized binary)
cargo build --release
```

### Running the Application

```bash
# Run the daemon
cargo run -- daemon

# Run with specific command
cargo run -- capture screen
```

### Building the Debian Package

```bash
# Build the .deb package
cargo deb

# The package will be in target/debian/
```

### Building the C++ capture overlay

The interactive area / window / crosshair selector and the recording
controls live in `capture-overlay/` as a separate Qt5 binary
(`apexshot-capture`). It's built independently from Cargo:

```bash
cd capture-overlay
cmake -S . -B build
cmake --build build -j
# Resulting binary: capture-overlay/build/apexshot-capture
```

When packaging the `.deb`, this binary is copied into
`packaging/deb/apexshot-capture` (see the `release` job in
`.github/workflows/release.yml`). For local development you can either:

- Replace the system binary at `/usr/bin/apexshot-capture` with the
  freshly built one, **or**
- Set `APEXSHOT_CAPTURE_BIN=/path/to/build/apexshot-capture` so the Rust
  side picks up your build (see `src/capture_overlay.rs::run_capture_binary`).

### Building / iterating on the GNOME extension

The extension is plain ES modules in `gnome-extension/`. Two workflows:

```bash
# Quick syntax check (the same one CI runs):
node --check gnome-extension/*.js

# Live-install into your session (works on GNOME 45–49):
make -C gnome-extension install     # if a Makefile is present, otherwise:
gnome-extensions pack gnome-extension --force \
  --extra-source=controls-ui.js --extra-source=controls-ui-layout.js \
  --extra-source=keystroke-display.js --extra-source=click-display.js \
  --extra-source=mask-ui.js --extra-source=runtime-overlays.js \
  --extra-source=runtime-overlays-visibility.js \
  --extra-source=screenshot-lock.js --extra-source=session-state.js \
  --extra-source=window-list.js
gnome-extensions install --force apexshot-gnome-integration@apexshot.github.io.shell-extension.zip
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

Then either log out / log in (Wayland) or press `Alt+F2` → `r` (X11) to
restart the shell. Use `journalctl /usr/bin/gnome-shell -f | grep
apexshot` to follow extension logs.

### Packaging the GNOME extension for release

```bash
cd gnome-extension
zip apexshot-gnome-integration.zip \
  extension.js metadata.json \
  controls-ui.js controls-ui-layout.js \
  keystroke-display.js click-display.js \
  mask-ui.js runtime-overlays.js runtime-overlays-visibility.js \
  screenshot-lock.js session-state.js window-list.js
```

This is identical to what the release workflow does in
`.github/workflows/release.yml`, so keep the two in sync if you add a
new file.

## Code Style

The shared formatting baseline is captured by three files at the repo
root, all of which CI honours:

- [`rustfmt.toml`](rustfmt.toml) — stable rustfmt options.
- [`.clang-format`](.clang-format) — Qt/KDE-leaning C++17 style for
  `capture-overlay/`.
- [`.editorconfig`](.editorconfig) — picked up automatically by most
  editors; covers indent width, line endings, trailing whitespace.

### Rust

```bash
cargo fmt --all                 # format
cargo fmt --all -- --check      # CI gate (must pass before merge)
cargo clippy --workspace --all-targets    # surfaces warnings
```

The CI lint job currently runs `cargo clippy` *without* `-D warnings`
because the codebase still carries some pre-existing lints; new code
should not add new warnings, and we'll flip the gate to `-D warnings`
once the backlog is cleared.

Other expectations:

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).
- Add doc comments to anything `pub`.
- Prefer small, focused functions; avoid stacking `unwrap()`s in code
  paths that can be triggered by user input.

### C++ (capture-overlay)

```bash
clang-format -i capture-overlay/src/*.{cpp,h}
```

- Modern C++17, no exceptions in the hot path — return `bool` / out
  parameters like the existing portal helpers in `ScreenCapture.cpp`.
- Feature flags that gate WIP UI live as `inline constexpr bool` in
  [`capture-overlay/src/CaptureOverlay.h`](capture-overlay/src/CaptureOverlay.h)
  (see `apexshot::kKeystrokesFeatureAvailable`). When you add a new flag,
  document **what** it gates, **why** it's off, and **where** the
  matching draw / event branches live.

### JavaScript (GNOME extension)

```bash
node --check gnome-extension/*.js   # CI gate
```

- ES modules, modern (ES2022+) syntax — GNOME Shell ≥ 45 ships a recent
  GJS.
- Don't import Node-only APIs; the runtime is GJS and the imports are
  `gi://...`.
- Tests in `gnome-extension/tests/` use `printerr` and other GJS globals
  and are intended to be run from inside GJS, not Node.

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

### Test expectations

- The Rust workspace currently ships ~380 unit tests (`cargo test --lib`).
  Keep that suite green; add tests next to the module you're changing
  rather than in a separate location.
- New behaviour deserves at least one happy-path test and one
  failure-mode test. We use `pretty_assertions`, `test-case`, and
  `mockall` from `[dev-dependencies]` — reach for them when they fit
  rather than rolling your own helpers.
- For the C++ overlay, a successful `cmake --build capture-overlay/build`
  is the minimum smoke check; visual changes should be accompanied by
  a before/after screenshot in the PR description.
- For the GNOME extension, the GJS-targeted suite in
  `gnome-extension/tests/` runs inside `gjs`, not Node — see the
  *Building / iterating on the GNOME extension* section above. PRs that
  only touch JS still need to pass the `node --check` step that CI runs.

### Manual testing matrix

The project is most thoroughly exercised on the configurations the
maintainer runs day-to-day, but contributors are encouraged to verify
elsewhere when they touch a related code path. As of today:

| Surface                     | Tested                       | Best-effort                | Untested              |
|-----------------------------|------------------------------|----------------------------|-----------------------|
| Display server              | Wayland                      | X11                        | XWayland edge cases   |
| Compositor                  | GNOME Shell 47–49            | KDE Plasma 6, Sway 1.x     | Hyprland, Niri, river |
| Distro                      | Ubuntu 24.04 / 25.10         | Fedora, Debian             | Arch, openSUSE, NixOS |
| Recording codecs            | VP9, H.264, GIF              | VP8, H.265, Theora         | —                     |
| Capture portal flow         | xdg-desktop-portal-gnome     | xdg-desktop-portal-gtk     | KDE / wlroots backend |

If your PR exercises one of the *Untested* squares, please mention that
in the PR description so the maintainer knows extra eyes might be
useful before merging.

## Submitting Changes

### Workflow

1. Create a new branch for your changes:
   ```bash
   git checkout -b feature/your-feature-name
   ```
   or
   ```bash
   git checkout -b fix/your-bug-fix
   ```

2. Make your changes and commit them:
   ```bash
   git add .
   git commit -m "Your descriptive commit message"
   ```

3. Push to your fork:
   ```bash
   git push origin feature/your-feature-name
   ```

4. Create a pull request on GitHub

### Commit Messages

Follow conventional commits format:
- `feat:` for new features
- `fix:` for bug fixes
- `docs:` for documentation changes
- `style:` for code style changes (formatting, etc.)
- `refactor:` for code refactoring
- `test:` for adding or updating tests
- `chore:` for maintenance tasks

Examples:
```
feat: add webcam PiP support for screen recording
fix: correct memory leak in overlay cleanup
docs: update installation instructions for Ubuntu 24.04
```

### Pull Request Guidelines

- Provide a clear description of the changes
- Reference related issues (e.g., `Fixes #123`)
- Include screenshots for UI changes
- Ensure all tests pass
- Update documentation if needed
- Keep PRs focused and small if possible

## Reporting Issues

### Before Reporting

1. Check if the issue already exists
2. Search for similar closed issues
3. Verify you're using the latest version

### Issue templates

GitHub will offer two structured templates when you click **New issue**:

- **Bug report** (`.github/ISSUE_TEMPLATE/bug_report.yml`) — collects the
  install method, distro, display server, desktop environment, and a
  reproduction. Required fields are minimal but pointed.
- **Feature request** (`.github/ISSUE_TEMPLATE/feature_request.yml`) —
  asks you to describe the *problem*, not just the implementation, plus
  which subsystem(s) the change would touch.

For security issues, please follow [`SECURITY.md`](SECURITY.md) and
**do not** open a public GitHub issue.

## Areas for Contribution

These are the areas that would actually benefit from help right now,
ordered roughly by how much value a single PR can deliver.

### High value

- **Keystroke overlay recorder side.** The UI is gated off behind
  `apexshot::kKeystrokesFeatureAvailable` in
  `capture-overlay/src/CaptureOverlay.h`. The remaining work is on the
  Rust recording pipeline (consume `recordKeystrokesEnabled()` and feed
  events into the GNOME extension). Flipping the flag should *just
  work* once the recorder side is done.
- **Compositor coverage.** Help us promote KDE / Sway / Hyprland from
  *best-effort* to *tested* in the matrix above. Specifically: verify
  the wlr-screencopy fast path in `src/backend/screencopy.rs`, the
  `grim` fallback in `src/backend/wayland.rs`, and the ScreenCast
  restore-token persistence in `capture-overlay/src/ScreenCapture.cpp`.
- **Clippy backlog.** ~65 pre-existing warnings prevent us from flipping
  the CI lint gate to `-D warnings`. Small, focused PRs that clear a
  category at a time are very welcome.
- **Browser extension polish.** The native messaging host
  (`native-host/`) and Chrome/Chromium extension (`web-scroll-extension/`)
  cover full-page scroll capture; rough edges around long pages and
  zoom levels remain.

### Medium value

- **OCR accuracy on niche scripts.** The default tessdata set is
  English; the multi-PSM strategy in `src/ocr/mod.rs` should generalise
  to other languages but hasn't been validated.
- **Editor / annotation tools.** Most of `src/capture/editor/` is GTK4
  Cairo painting — a self-contained codebase that's a friendly first PR
  surface.
- **Settings UI / onboarding polish.** Lives in `src/settings/` and
  `src/onboarding/`.
- **Documentation & tutorials.** `docs/ARCHITECTURE.md`,
  `docs/MODULES.md`, and `docs/DEVELOPER_GUIDE.md` are good but always
  drift behind the code.

### Lower urgency

- Internationalisation of the GTK UI strings.
- Custom theming for the editor / preview overlay.
- Plugin / scripting hooks for third-party effects.
- Additional codecs beyond the default VP9 / H.264 / GIF set.

## Communication

### Channels

- **GitHub Issues**: For bug reports and feature requests
- **GitHub Discussions**: For general questions and ideas
- **Pull Requests**: For code contributions

### Code of Conduct

This project follows the [Contributor Covenant 2.1](CODE_OF_CONDUCT.md).
Reports go to **codegoddy@gmail.com**; the maintainer will acknowledge
within 3 working days.

## Getting Help

If you need help:

1. Check the [documentation](docs/)
2. Search existing [issues](https://github.com/apex-shot/apexshot/issues)
3. Start a [GitHub Discussion](https://github.com/apex-shot/apexshot/discussions)
4. Ask in an existing issue or PR if relevant

## License

By contributing to ApexShot, you agree that your contributions will be licensed under the GPL-3.0 license.

## Recognition

Contributors will be acknowledged in the project's CONTRIBUTORS file and in release notes.

---

Thank you for contributing to ApexShot! 🎉
