# Developer Guide

This guide provides information for developers who want to contribute to ApexShot.

## Prerequisites

### System Requirements
- Linux (Ubuntu or Arch GNOME Wayland recommended for the known-good path)
  - X11 backend implementations exist in the codebase but have not been thoroughly tested
- Rust toolchain (latest stable)
- CMake 3.10 or later
- Qt5 development libraries
- GStreamer development libraries
- GTK4 development libraries
- Tesseract OCR

### Install Dependencies

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install -y \
    build-essential \
    cmake \
    pkg-config \
    libx11-dev \
    libxext6 \
    libxtst-dev \
    qtbase5-dev \
    libqt5widgets5 \
    libqt5x11extras5-dev \
    libqt5network5-dev \
    libqt5dbus5-dev \
    libgstreamer1.0-dev \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    libpipewire-0.3-dev \
    tesseract-ocr \
    libtesseract-dev \
    libleptonica-dev \
    libgtk-4-dev \
    libadwaita-1-dev \
    libgtk4-layer-shell-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev
```

**Fedora:**
```bash
sudo dnf install -y \
    gcc-c++ \
    cmake \
    pkg-config \
    libX11-devel \
    libXext-devel \
    libXtst-devel \
    qt5-qtbase-devel \
    qt5-qtx11extras-devel \
    gstreamer1-devel \
    gstreamer1-plugins-base-devel \
    gstreamer1-plugins-base-tools \
    gstreamer1-plugins-good \
    gstreamer1-plugins-bad-free \
    gstreamer1-plugins-bad-nonfree \
    gstreamer1-plugins-ugly \
    gstreamer1-libav \
    pipewire-devel \
    tesseract \
    tesseract-devel \
    leptonica-devel \
    gtk4-devel \
    libadwaita-devel \
    gtk4-layer-shell-devel \
    libxcb-devel
```

> **Note:** Fedora package names for GTK4 layer shell may vary by release. If a package is unavailable, consult your distribution's repositories or build the missing dependency from source.

**Arch Linux:**
```bash
sudo pacman -S --needed \
    base-devel \
    rust \
    cargo \
    git \
    cmake \
    clang \
    pkgconf \
    gtk4 \
    libadwaita \
    gtk4-layer-shell \
    gstreamer \
    gst-plugins-base \
    gst-plugins-good \
    gst-plugins-bad \
    gst-libav \
    gst-plugin-pipewire \
    pipewire \
    pipewire-pulse \
    libpipewire \
    tesseract \
    tesseract-data-eng \
    qt5-base \
    qt5-x11extras \
    libxtst \
    wl-clipboard \
    xclip \
    libnotify \
    xdg-utils \
    ffmpeg \
    grim \
    xdg-desktop-portal \
    xdg-desktop-portal-gnome
```

## Building

### Clone Repository
```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
```

### Build the Entire Project
The C++ Qt5 capture overlay is compiled automatically by `build.rs` during the Cargo build. No manual CMake step is required for normal development.

```bash
cargo build --release
```

> **Note:** `build.rs` will invoke `cmake` and `make` for the C++ overlay automatically. Ensure `cmake` is installed and available in your `PATH`.

### Build Debian Package
```bash
cargo deb
```

The package will be created in `target/debian/`.

### Manual C++ Overlay Build (if needed)
If you need to build the C++ overlay independently (e.g., for debugging):
```bash
cd capture-overlay
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build . -- -j$(nproc)
cd ../..
```

## Running

### Run in Daemon Mode
```bash
cargo run --release -- daemon
```

### Run GUI Mode (Settings)
```bash
cargo run --release
```

### Run CLI Commands
```bash
# Screenshot capture modes
cargo run --release -- capture screen
cargo run --release -- capture area
cargo run --release -- capture window
cargo run --release -- capture crosshair

# Recording modes
cargo run --release -- record screen
cargo run --release -- record area
cargo run --release -- record ui

# OCR
cargo run --release -- ocr /path/to/image.png

# Annotation editor
cargo run --release -- edit /path/to/image.png

# Video editor
cargo run --release -- video-editor
cargo run --release -- video-editor /path/to/recording.mp4

# Hotkey management
cargo run --release -- hotkeys install
cargo run --release -- hotkeys uninstall
cargo run --release -- hotkeys reset
```

### Environment Variables for Development
- `RUST_LOG=debug` — Enable debug logging
- `GST_DEBUG=3` — Enable GStreamer debug output
- `APEXSHOT_HOTKEY_DEBUG=1` — Enable hotkey debug logging in daemon
- `APEXSHOT_HOTKEY_LOG=/path/to/log` — Redirect daemon logs to file
- `APEXSHOT_REDUCED_TRANSPARENCY=1` — Disable transparent effects in overlays
- `APEXSHOT_APP_ID=your.app.id` — Override default portal app ID

## Development Workflow

### Code Style
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting (try to resolve all warnings)
- Follow Rust best practices and idioms
- Add doc comments (`///`) to all public functions and types
- Keep functions focused and small

### Running Tests
```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Run a specific module's inline tests
cargo test --package apexshot --lib recording
cargo test --package apexshot --lib backend
cargo test --package apexshot --lib config

# Run integration tests only
cargo test --test wayland_backend_test
cargo test --test x11_backend_test
```

## Project Structure

```
apexshot/
├── src/                           # Rust source code
│   ├── annotations/               # Annotation persistence (mod.rs, schema.rs, storage.rs)
│   ├── backend/                   # Display backend abstraction
│   │   ├── mod.rs                 # DisplayBackend trait, CaptureData, PixelFormat
│   │   ├── x11.rs                 # X11 backend (x11rb + MIT-SHM)
│   │   ├── wayland.rs             # Wayland backend (ashpd ScreenCast portal + PipeWire)
│   │   ├── screencopy.rs          # wlr-screencopy protocol
│   │   └── portal_permissions.rs  # XDG portal permission persistence
│   ├── capture/                   # Screen capture + annotation editor
│   │   ├── mod.rs                 # Image saving, format conversion, cursor compositing
│   │   ├── editor.rs              # Annotation editor module root
│   │   ├── preview_overlay.rs     # Post-capture preview overlay
│   │   └── editor/                # Annotation editor submodules
│   │       ├── window/            # GTK4 editor window, toolbars, canvas
│   │       ├── types.rs           # Tool, AnnotationAction, DrawColor, etc.
│   │       ├── state.rs           # EditorState, undo/redo
│   │       ├── render.rs          # Cairo rendering for all tools
│   │       ├── selection.rs       # Hit-testing, resize handles
│   │       ├── color.rs           # Color palette, conversions
│   │       ├── pen_weight.rs      # Brush stroke weights
│   │       ├── numbering_style.rs # Numbered callout styles
│   │       ├── text_detect.rs     # ML text detection (ocrs/rten)
│   │       ├── preprocess.rs      # Image preprocessing
│   │       ├── io_ops.rs          # Clipboard URI operations
│   │       └── ui_support.rs     # Shared GTK4 UI helpers
│   ├── config.rs                  # YAML configuration, AppConfig
│   ├── daemon/                    # Background daemon (single file: mod.rs)
│   ├── gnome_integration/         # GNOME extension installation helpers
│   ├── gnome_shell.rs             # D-Bus proxy for GNOME Shell extension
│   ├── hotkeys/                   # Global hotkey management (single file: mod.rs)
│   ├── icons/                     # Icon resources
│   ├── lib.rs                     # Library exports for tests/downstream
│   ├── main.rs                    # CLI entry point, argument parsing
│   ├── ocr/                       # OCR functionality (Tesseract + QR)
│   ├── onboarding/                # First-time setup wizard
│   │   ├── mod.rs                 # Wizard flow controller
│   │   ├── welcome.rs             # Welcome screen
│   │   ├── extensions.rs          # Extension installation
│   │   ├── cloud.rs               # Cloud sync waitlist
│   │   └── complete.rs            # Completion screen
│   ├── overlay.rs                 # X11 area selector (GTK4 + gtk4-layer-shell)
│   ├── qr/                        # QR code detection (rqrr)
│   ├── recording/                 # Screen recording with GStreamer
│   │   ├── mod.rs                 # GStreamer pipeline, codec detection
│   │   ├── control_session.rs     # Active session D-Bus commands
│   │   ├── stop_overlay.rs        # Floating recording control bar
│   │   ├── countdown_overlay.rs   # Fullscreen 3-2-1 countdown
│   │   ├── dim_overlay.rs         # Fullscreen dim mask
│   │   ├── runtime_keystrokes.rs  # EI portal keystroke capture
│   │   └── dnd.rs                 # Do Not Disturb inhibition
│   ├── settings/                  # GTK4 settings window
│   │   ├── mod.rs                 # Main window builder
│   │   ├── general.rs             # General settings tab
│   │   ├── screenshots.rs         # Screenshot settings
│   │   ├── recording.rs           # Recording settings
│   │   ├── annotate.rs            # Annotation defaults
│   │   ├── quick_access.rs        # Quick-access overlay settings
│   │   ├── advanced.rs            # Advanced settings
│   │   ├── shortcuts.rs           # Hotkey binding editor
│   │   ├── after_capture.rs       # After-capture action matrix
│   │   ├── storage.rs             # Export location settings
│   │   ├── cloud.rs               # Cloud sync waitlist
│   │   ├── about.rs               # About tab (logo, version, links)
│   │   ├── actions.rs             # SaveInputs, save logic
│   │   ├── ui_support.rs          # Shared CSS, form helpers
│   │   └── windowing.rs           # Edge-drag resize, theme detection
│   ├── tray/                      # System tray (ksni) (single file: mod.rs)
│   ├── utils/                     # Utilities
│   │   ├── clipboard.rs
│   │   └── desktop_env.rs
│   └── capture_overlay.rs         # C++ Qt5 overlay launcher wrapper
├── capture-overlay/               # C++ Qt5 overlay (CMake)
│   ├── src/                       # C++ source files
│   │   ├── main.cpp
│   │   ├── CaptureOverlay.cpp/h
│   │   ├── CaptureOverlay_Drawing.cpp
│   │   ├── CaptureOverlay_Events.cpp
│   │   ├── CaptureOverlay_HitTest.cpp
│   │   ├── WindowPickerOverlay.cpp/h
│   │   ├── ScreenCapture.cpp/h
│   │   └── request.cpp/h
│   └── CMakeLists.txt
├── gnome-extension/               # GNOME Shell extension (JavaScript/GJS)
│   ├── extension.js
│   ├── controls-ui.js
│   ├── controls-ui-layout.js
│   ├── runtime-overlays.js
│   ├── runtime-overlays-visibility.js
│   ├── click-display.js
│   ├── keystroke-display.js
│   ├── mask-ui.js
│   ├── session-state.js
│   ├── window-list.js
│   ├── screenshot-lock.js
│   └── metadata.json
├── web-scroll-extension/          # Chrome/Chromium extension
│   ├── manifest.json
│   ├── background.js
│   ├── popup.html
│   └── popup.js
├── native-host/                   # Native messaging host manifest
│   ├── io.github.codegoddy.apexshot.json
│   └── apexshot-native-host
├── packaging/                     # Package assets
│   ├── apexshot.desktop
│   ├── apexshot-daemon.desktop
│   ├── apexshot.svg
│   ├── deb/                       # Deb helper scripts
│   └── debian/                    # Debian packaging control files
├── tests/                         # Integration tests
│   ├── desktop_identity.rs
│   ├── package_metadata.rs
│   ├── wayland_backend_test.rs
│   ├── x11_backend_test.rs
│   ├── window_picker_ui_contract.rs
│   └── wayland_backend_mock_test.rs
├── docs/                          # Documentation
│   ├── ARCHITECTURE.md
│   ├── DEVELOPER_GUIDE.md
│   └── MODULES.md
├── Cargo.toml                     # Rust dependencies
├── build.rs                       # Build script (CMake + relm4-icons)
├── README.md
├── CONTRIBUTING.md
└── LICENSE
```

## Adding Features

### 1. Add Configuration Option

**Step 1:** Add field to `AppConfig` in `src/config.rs`:
```rust
pub struct AppConfig {
    // ... existing fields
    pub your_new_option: String,
}
```

**Step 2:** Add default value:
```rust
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // ... existing defaults
            your_new_option: "default_value".to_string(),
        }
    }
}
```

**Step 3:** Add sanitization (if needed):
```rust
impl AppConfig {
    pub fn sanitized(mut self) -> Self {
        // ... existing sanitization
        self.your_new_option = sanitize_your_option(self.your_new_option);
        self
    }
}
```

**Step 4:** Add UI in the appropriate settings file (e.g., `src/settings/general.rs`).

**Step 5:** Add the widget to `SaveInputs` in `src/settings/actions.rs`.

**Step 6:** Add save logic in `src/settings/actions.rs`.

### 2. Add New Recording Feature

**Step 1:** Add config fields in `src/config.rs`.

**Step 2:** Add UI in `src/settings/recording.rs`.

**Step 3:** Update recording logic in `src/recording/mod.rs` (GStreamer pipeline).

**Step 4:** If the feature needs runtime UI, add it to `src/recording/stop_overlay.rs` or `src/recording/countdown_overlay.rs`.

**Step 5:** Add tests in the relevant recording submodule.

### 3. Add New Annotation Tool

**Step 1:** Add tool variant to `Tool` enum in `src/capture/editor/types.rs`.

**Step 2:** Add rendering logic in `src/capture/editor/render.rs`.

**Step 3:** Add hit-testing/selection logic in `src/capture/editor/selection.rs`.

**Step 4:** Add toolbar UI in `src/capture/editor/window/` and `src/capture/editor/ui_support.rs`.

**Step 5:** Add serialization support in `src/annotations/schema.rs`.

**Step 6:** Add keyboard shortcuts in `src/capture/editor.rs`.

## Debugging

### Rust Debug Logging
```bash
RUST_LOG=debug cargo run --release -- daemon
```

### GStreamer Pipeline Debug
```bash
GST_DEBUG=3 cargo run --release -- record screen
```

### D-Bus Communication
```bash
# Monitor all session bus messages
dbus-monitor --session

# Monitor ApexShot-specific traffic
busctl monitor --session org.apexshot.Daemon
```

### GNOME Extension Debug
```bash
# Watch GNOME Shell logs in real-time
journalctl /usr/bin/gnome-shell -f | grep apexshot

# Reload the extension after making changes
gnome-extensions disable apexshot-gnome-integration@apexshot.github.io
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

### Hotkey Debug
```bash
APEXSHOT_HOTKEY_DEBUG=1 cargo run --release -- daemon
# Or redirect to file:
APEXSHOT_HOTKEY_LOG=/tmp/apexshot-hotkeys.log cargo run --release -- daemon
```

### C++ Overlay Debug
The overlay binary is built in `target/release/build/apexshot-*/out/capture-overlay-build/`.
Run it directly with `--help` to see available options:
```bash
./target/release/build/apexshot-*/out/capture-overlay-build/apexshot-capture --help
```

## Testing

### Unit Tests
Write unit tests inline in the same file:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function() {
        assert_eq!(result, expected);
    }
}
```

Good examples of inline tests exist in:
- `src/backend/mod.rs` — Pixel format and capture data validation
- `src/capture/editor.rs` — Tool shortcut mapping, constrained drag logic
- `src/settings/after_capture.rs` — UI contract assertions

### Integration Tests
Add integration tests in `tests/` directory:
```rust
use apexshot::config::load_config;

#[test]
fn test_config_loading() {
    let config = load_config();
    assert!(!config.video_export_location.is_empty());
}
```

### Manual Testing Checklist
Before submitting a PR, manually verify on at least one known-good GNOME
Wayland target. Currently confirmed: Ubuntu GNOME Wayland and Arch Linux GNOME
Wayland.
- [ ] `cargo run --release -- capture area` works
- [ ] `cargo run --release -- capture screen` works
- [ ] `cargo run --release -- record area` works (start + stop)
- [ ] Annotation editor opens and all tools render correctly
- [ ] Settings window opens and saves changes persist after restart
- [ ] Daemon mode starts and tray icon appears
- [ ] Global hotkeys trigger captures
- [ ] GNOME extension keeps preview windows on top

For distro/compositor expansion work, also verify the shared ScreenCast portal
path on at least one non-GNOME portal backend:
- [ ] Fedora GNOME Wayland for Fedora/RPM family coverage
- [ ] Fedora or openSUSE KDE Plasma Wayland for `xdg-desktop-portal-kde`
- [ ] Arch Hyprland or Sway Wayland for `xdg-desktop-portal-hyprland`/`wlr`
- [ ] NixOS GNOME or KDE Wayland for non-FHS/runtime dependency coverage

See `CONTRIBUTING.md` for more detailed manual testing guidelines.

## GNOME Extension Development

### Build Extension
```bash
cd gnome-extension
zip -r apexshot-gnome-integration.zip . -x "*.git*" "*screenshots*" "*tests*"
```

### Install Extension Locally
```bash
# Unzip into GNOME extensions directory
mkdir -p ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io
unzip apexshot-gnome-integration.zip -d ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io/

# Enable extension
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

### Test Extension
```bash
# Check extension status
gnome-extensions list
gnome-extensions info apexshot-gnome-integration@apexshot.github.io

# View logs
journalctl /usr/bin/gnome-shell -f | grep apexshot

# Monitor D-Bus
busctl monitor --session org.apexshot.ShellOverlay
```

### Extension File Guide
- `extension.js` — Entry point; registers D-Bus services, connects signals
- `controls-ui.js` — Recording control buttons rendered on the shell stage
- `runtime-overlays.js` — Click/keystroke display rendered on the shell stage
- `mask-ui.js` — Recording mask (dimmed region) rendered on the shell stage
- `session-state.js` — Tracks active sessions, window lists

## Browser Extension Development

### Load Unpacked Extension
1. Open `chrome://extensions` or `chromium://extensions`
2. Enable **Developer mode**
3. Click **Load unpacked**
4. Select `web-scroll-extension/`

### Extension Architecture
- `manifest.json` — Chrome Extension Manifest V3
- `background.js` — Service worker handling scroll-stitch capture, native messaging
- `popup.html/js` — Extension popup UI

### Native Messaging Flow
1. `background.js` opens a `chrome.runtime.connectNative()` port to `io.github.codegoddy.apexshot`
2. Messages are JSON-encoded and sent over the native host's stdin/stdout
3. The native host is the ApexShot binary itself (or a symlink to it)
4. ApexShot daemon receives the message via `ImportWebScrollCapture` D-Bus action

## Packaging

### Create Debian Package
```bash
cargo deb
```

The package will be created in `target/debian/`.

### Install Package
```bash
sudo dpkg -i target/debian/apexshot_*.deb
sudo apt install -f  # Install missing dependencies if any
```

### Remove Package
```bash
sudo apt remove apexshot
```

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/your-feature`)
3. Make your changes
4. Add tests (unit and/or integration)
5. Run `cargo fmt` and `cargo clippy -- -D warnings`
6. Commit with a descriptive message (see `CONTRIBUTING.md` for conventional commit format)
7. Push to your fork
8. Create a pull request

See `CONTRIBUTING.md` for detailed guidelines.

## Common Issues

### Build Errors

**CMake not found:**
```bash
sudo apt install cmake
```

**Missing Qt5 headers:**
```bash
sudo apt install qtbase5-dev libqt5x11extras5-dev libqt5widgets5
```

**Missing GStreamer headers:**
```bash
sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

**Missing GTK4 layer shell:**
```bash
sudo apt install libgtk4-layer-shell-dev
```

### Runtime Errors

**GStreamer pipeline error:**
```bash
sudo apt install gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav
```

**Tesseract not found:**
```bash
sudo apt install tesseract-ocr libtesseract-dev libleptonica-dev
```

**PipeWire not available (audio monitoring fails):**
```bash
sudo apt install libpipewire-0.3-dev pipewire
```

### GNOME Extension Issues

**Extension not loading:**
- Check GNOME version compatibility (45–49) against `metadata.json`
- Verify UUID matches directory name: `apexshot-gnome-integration@apexshot.github.io`
- Check D-Bus signals with `busctl monitor --session`
- Look for errors in `journalctl /usr/bin/gnome-shell -f`

**Preview windows not staying on top:**
- Ensure extension is enabled: `gnome-extensions list`
- Check that ApexShot emits `TrackedWindowOpened` signals: `dbus-monitor --session`
- Verify the window title matches what the extension expects

### Hotkey Issues

**Hotkeys not triggering:**
- Ensure daemon is running: `busctl list | grep apexshot`
- Run `cargo run --release -- hotkeys install` to reinstall bindings
- Check GNOME custom keybindings: `gsettings get org.gnome.settings-daemon.plugins.media-keys custom-keybindings`
- On non-GNOME desktops, verify `ashpd` portal GlobalShortcuts are available

## Performance Tips

### Recording Performance
- Use appropriate FPS (24–30 for most use cases)
- Consider resolution limits for lower-end systems (`max_resolution` config)
- Disable runtime overlays (clicks/keystrokes/webcam) if not needed
- Use hardware-accelerated codecs (H.264/VA-API if available)

### Capture Performance
- PNG is slowest; use JPEG or WebP for faster saves
- Disable cursor compositing if not needed
- The daemon keeps the GTK stack warm, avoiding cold-start delays

### Annotation Editor Performance
- Large images with many annotations may slow down Cairo rendering
- Consider zooming out for bulk operations
- The undo/redo stack is unbounded; very long sessions may grow memory usage

## Security Considerations

- Validate all user inputs (file paths, configuration values)
- Sanitize file paths before writing
- Use secure temporary directories (`std::env::temp_dir()`)
- Handle D-Bus messages defensively (check sender, validate arguments)
- Never execute shell commands with unsanitized user input
- The native messaging host validates message origin before processing

## Resources

- [Rust Documentation](https://doc.rust-lang.org/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [GTK4 Documentation](https://docs.gtk.org/gtk4/)
- [GTK4 Layer Shell](https://github.com/wmww/gtk4-layer-shell)
- [GStreamer Documentation](https://gstreamer.freedesktop.org/documentation/)
- [GNOME Shell Extension Documentation](https://gjs.guide/extensions/overview.html)
- [ashpd (XDG Desktop Portal)](https://docs.rs/ashpd/)
- [zbus (D-Bus)](https://docs.rs/zbus/)
- [Tesseract Documentation](https://tesseract-ocr.github.io/)
- [ocrs (Rust OCR)](https://github.com/robertknight/ocrs)
