# Contributing to ApexShot

Thank you for your interest in contributing to ApexShot! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Code Style](#code-style)
- [Testing](#testing)
- [Submitting Changes](#submitting-changes)
- [Reporting Issues](#reporting-issues)
- [Areas for Contribution](#areas-for-contribution)
- [Communication](#communication)

## Getting Started

### Prerequisites

- **Rust** (latest stable version) - [Install Rust](https://www.rust-lang.org/tools/install)
- **C++ compiler** (g++ or clang) with C++17 support
- **CMake** (for building the native overlay)
- **GTK4 development libraries**
- **GStreamer development libraries**
- **Qt5 development libraries** (for the native overlay)

See the [README](README.md) for detailed system dependencies.

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

### Building the GNOME Extension

```bash
cd gnome-extension
zip -r apexshot-gnome-integration.zip *.js *.json README.md -x "tests/*" "screenshots/*"
```

## Code Style

### Rust Code

- Use `cargo fmt` to format code:
  ```bash
  cargo fmt
  ```
- Use `cargo clippy` for linting:
  ```bash
  cargo clippy -- -D warnings
  ```
- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use meaningful variable and function names
- Add doc comments to public functions and modules
- Keep functions focused and small

### C++ Code (Native Overlay)

- Follow modern C++17 standards
- Use consistent naming conventions (camelCase for functions, snake_case for variables)
- Add comments for complex logic
- Follow Qt coding style where applicable

### JavaScript Code (GNOME Extension)

- Use modern JavaScript (ES6+)
- Follow GNOME Shell extension coding guidelines
- Add JSDoc comments for functions
- Use meaningful variable names

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

### Test Coverage

- Write tests for new features
- Maintain test coverage above 80%
- Test edge cases and error conditions
- Use property-based testing where appropriate (using `test-case` crate)

### Manual Testing

**Current Testing Scope:**
- Currently tested on GNOME Ubuntu (Wayland)
- Support for other distributions and desktop environments will be added as the project grows

**When expanding support, test on:**
- Different Linux distributions (Fedora, Arch, Debian, etc.)
- Both X11 and Wayland
- Different GNOME versions (45, 46, 47, 48, 49)
- Other desktop environments (KDE Plasma, XFCE, etc.)
- The GNOME extension functionality
- Screen recording with different codecs
- OCR functionality

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

### Issue Template

When reporting an issue, include:

- **Description**: Clear description of the problem
- **Steps to reproduce**: Detailed steps to reproduce the issue
- **Expected behavior**: What you expected to happen
- **Actual behavior**: What actually happened
- **Environment**:
  - OS and version
  - Desktop environment (GNOME, KDE, etc.)
  - Display server (X11 or Wayland)
  - ApexShot version
- **Logs**: Relevant error messages or logs
- **Screenshots**: If applicable

## Areas for Contribution

### High Priority

- **Wayland improvements**: Better support for various Wayland compositors
- **Performance optimization**: Reduce memory usage and improve capture speed
- **GNOME extension**: Enhance features and fix bugs
- **Browser extension**: Complete Chrome/Chromium extension
- **Testing**: Increase test coverage and add integration tests

### Medium Priority

- **Additional codecs**: Support more video codecs for recording
- **OCR improvements**: Better text recognition accuracy
- **UI polish**: Improve the user interface and UX
- **Documentation**: Improve docs and add tutorials
- **Internationalization**: Add support for multiple languages

### Low Priority

- **Additional features**: New capture modes, effects, etc.
- **Theme support**: Add custom themes
- **Plugin system**: Allow third-party plugins

## Communication

### Channels

- **GitHub Issues**: For bug reports and feature requests
- **GitHub Discussions**: For general questions and ideas
- **Pull Requests**: For code contributions

### Code of Conduct

- Be respectful and constructive
- Welcome newcomers and help them learn
- Focus on what is best for the community
- Show empathy towards other community members

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
