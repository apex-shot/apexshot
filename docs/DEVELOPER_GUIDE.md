# Developer Guide

This guide provides information for developers who want to contribute to ApexShot.

## Prerequisites

### System Requirements
- Linux (GNOME Ubuntu Wayland recommended)
- Rust 1.70 or later
- CMake 3.10 or later
- Qt5 development libraries
- GStreamer development libraries
- Tesseract OCR

### Install Dependencies

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install -y \
    build-essential \
    cmake \
    libqt5-dev \
    libqt5x11extras5-dev \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    tesseract-ocr \
    libtesseract-dev \
    libleptonica-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    pkg-config
```

**Fedora:**
```bash
sudo dnf install -y \
    gcc-c++ \
    cmake \
    qt5-qtbase-devel \
    qt5-qtx11extras-devel \
    gstreamer1-devel \
    gstreamer1-plugins-base-devel \
    gstreamer1-plugins-base-tools \
    gstreamer1-plugins-good \
    gstreamer1-plugins-bad-free \
    gstreamer1-plugins-bad-nonfree \
    gstreamer1-plugins-ugly \
    tesseract \
    tesseract-devel \
    leptonica-devel \
    libxcb-devel \
    pkg-config
```

## Building

### Clone Repository
```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
```

### Build Rust Application
```bash
cargo build --release
```

### Build C++ Capture Overlay
```bash
cd capture-overlay
mkdir build && cd build
cmake ..
make
cd ../..
```

### Build Everything
```bash
# Build Rust application
cargo build --release

# Build C++ overlay
cd capture-overlay
mkdir build && cd build
cmake ..
make
cd ../..
```

### Build Debian Package
```bash
cargo deb
```

The package will be created in `target/debian/`.

## Running

### Run in Daemon Mode
```bash
cargo run --release -- daemon
```

### Run GUI Mode
```bash
cargo run --release
```

### Run CLI Commands
```bash
cargo run --release -- capture screen
cargo run --release -- capture area
cargo run --release -- record screen
```

## Development Workflow

### Code Style
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Follow Rust best practices and idioms

### Running Tests
```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture
```

### Running Specific Module Tests
```bash
# Recording module tests
cargo test --package apexshot --lib recording

# Config module tests
cargo test --package apexshot --lib config
```

## Project Structure

```
apexshot/
├── src/                    # Rust source code
│   ├── annotations/        # Annotation editor
│   ├── backend/           # Backend API
│   ├── capture/           # Screen capture
│   ├── config.rs          # Configuration
│   ├── daemon/            # Background daemon
│   ├── gnome_integration/ # GNOME integration
│   ├── gnome_shell.rs     # GNOME Shell D-Bus
│   ├── hotkeys/           # Global hotkeys
│   ├── icons/             # Icon resources
│   ├── main.rs            # Entry point
│   ├── ocr/               # OCR functionality
│   ├── onboarding/        # First-time setup
│   ├── overlay.rs         # GTK4 overlay
│   ├── qr/                # QR code detection
│   ├── recording/         # Screen recording
│   ├── settings/          # Settings UI
│   ├── tray/              # System tray
│   └── utils/             # Utilities
├── capture-overlay/        # C++ Qt5 overlay
│   ├── src/               # C++ source
│   └── CMakeLists.txt     # CMake build
├── gnome-extension/       # GNOME Shell extension
│   ├── extension.js       # Extension logic
│   ├── metadata.json      # Extension metadata
│   └── keystroke-display.js
├── packaging/             # Packaging scripts
│   └── debian/           # Debian packaging
├── tests/                 # Integration tests
├── Cargo.toml             # Rust dependencies
└── build.rs               # Build script
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

**Step 4:** Add UI in appropriate settings file (e.g., `src/settings/general.rs`):
```rust
let your_option_entry = Entry::new();
your_option_entry.set_text(&config.your_new_option);
```

**Step 5:** Add to `SaveInputs` struct in `src/settings/actions.rs`:
```rust
pub struct SaveInputs {
    // ... existing fields
    pub your_option_entry: Entry,
}
```

**Step 6:** Add save logic in `src/settings/actions.rs`:
```rust
config.your_new_option = inputs.your_option_entry.text().to_string();
```

### 2. Add New Recording Feature

**Step 1:** Add config fields in `src/config.rs` (see above)

**Step 2:** Add UI in `src/settings/recording.rs`

**Step 3:** Update recording logic in `src/recording/mod.rs`

**Step 4:** Add tests in `src/recording/mod.rs`

### 3. Add New Annotation Tool

**Step 1:** Add tool type to annotation schema in `src/annotations/schema.rs`

**Step 2:** Implement tool logic in `src/annotations/editor/`

**Step 3:** Add UI controls in annotation editor

**Step 4:** Add serialization/deserialization logic

## Debugging

### Enable Debug Logging
Set environment variable:
```bash
RUST_LOG=debug cargo run
```

### Debug D-Bus Communication
```bash
# Monitor D-Bus messages
dbus-monitor --session
```

### Debug GNOME Extension
```bash
# Enable GNOME Shell debug
journalctl /usr/bin/gnome-shell -f

# Reload extension
gnome-extensions disable apexshot-gnome-integration@apexshot.github.io
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

### Debug Recording Pipeline
```bash
# Enable GStreamer debug
GST_DEBUG=3 cargo run
```

## Testing

### Unit Tests
Write unit tests in the same file as the code:
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

### Manual Testing
See `CONTRIBUTING.md` for manual testing guidelines.

## GNOME Extension Development

### Build Extension
```bash
cd gnome-extension
zip -r apexshot-gnome-integration.zip . -x "*.git*" "*screenshots*" "*tests*"
```

### Install Extension Locally
```bash
# Copy to extensions directory
cp apexshot-gnome-integration.zip ~/.local/share/gnome-shell/extensions/
unzip apexshot-gnome-integration.zip -d ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io

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
```

## Packaging

### Create Debian Package
```bash
cargo deb
```

### Install Package
```bash
sudo dpkg -i target/debian/apexshot_*.deb
```

### Remove Package
```bash
sudo apt remove apexshot
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Run `cargo fmt` and `cargo clippy`
6. Commit with descriptive message
7. Push to your fork
8. Create a pull request

See `CONTRIBUTING.md` for detailed guidelines.

## Common Issues

### Build Errors

**Missing Qt5 headers:**
```bash
sudo apt install libqt5-dev libqt5x11extras5-dev
```

**Missing GStreamer headers:**
```bash
sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

### Runtime Errors

**GStreamer pipeline error:**
```bash
sudo apt install gstreamer1.0-plugins-good gstreamer1.0-plugins-bad
```

**Tesseract not found:**
```bash
sudo apt install tesseract-ocr libtesseract-dev
```

### GNOME Extension Issues

**Extension not loading:**
- Check GNOME version compatibility (45-49)
- Check extension UUID matches metadata.json
- Check D-Bus communication

## Performance Tips

### Recording Performance
- Use appropriate FPS (24-30 for most use cases)
- Consider resolution limits for lower-end systems
- Disable runtime overlays if not needed

### Capture Performance
- Use hardware acceleration if available
- Optimize annotation rendering
- Cache frequently used resources

## Security Considerations

- Validate all user inputs
- Sanitize file paths
- Use secure temporary directories
- Handle D-Bus messages securely
- Validate configuration values

## Resources

- [Rust Documentation](https://doc.rust-lang.org/)
- [GTK4 Documentation](https://docs.gtk.org/gtk4/)
- [GStreamer Documentation](https://gstreamer.freedesktop.org/documentation/)
- [GNOME Shell Extension Documentation](https://gjs.guide/extensions/overview.html)
- [Tesseract Documentation](https://tesseract-ocr.github.io/)
