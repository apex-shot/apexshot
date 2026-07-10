<!--
Thanks for contributing to ApexShot! Please fill the sections below so
reviewers can understand what changed and verify it quickly. Delete the
parts that don't apply.
-->

## Summary

<!-- One paragraph: what does this PR do and why? -->

## Type of change

- [ ] Bug fix (non-breaking change that resolves an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would change existing behaviour)
- [ ] Documentation / chore

## Related issues

<!-- Use "Fixes #123" / "Refs #456" so GitHub auto-links. -->

## How was this tested?

<!--
List the verification you ran. Paste copy-pastable commands when possible.
Examples:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets`
  - `cargo test`
  - `cmake --build capture-overlay/build`
  - `pnpm check:gnome`
  - Manual: triggered "Crosshair capture" on GNOME Wayland, confirmed the
    portal dialog only appeared on first run.
-->

## Subsystems touched

<!-- Tick the ones that apply so reviewers know what to focus on. -->

- [ ] Rust core (`src/`)
- [ ] C++ overlay (`capture-overlay/`)
- [ ] GNOME extension (`gnome-extension/`)
- [ ] Native messaging host / browser extension (`native-host/`, `web-scroll-extension/`)
- [ ] Packaging (`Cargo.toml [package.metadata.deb]`, `packaging/`)
- [ ] Documentation (`README.md`, `docs/`, `CONTRIBUTING.md`)
- [ ] CI / tooling (`.github/workflows/`, `rustfmt.toml`, `.editorconfig`,
      `.clang-format`)

## Screenshots / recordings

<!-- For any visual change, attach a before/after image or short clip. -->

## Checklist

- [ ] I read [`CONTRIBUTING.md`](../CONTRIBUTING.md) and followed the code
      style for the language I touched.
- [ ] I ran `cargo fmt --all -- --check` and `cargo clippy --workspace
      --all-targets` locally (Rust changes).
- [ ] I ran `cargo test` locally and all tests pass.
- [ ] I rebuilt the C++ overlay if I changed it (`cmake --build
      capture-overlay/build`).
- [ ] I ran `pnpm check:gnome` or `node --check` on modified GNOME extension
      files (`.js`).
- [ ] I updated documentation / `README.md` / inline comments where
      behaviour changed.
- [ ] My commits follow [Conventional Commits](https://www.conventionalcommits.org/)
      (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).
- [ ] I have not added any commented-out code, debug `eprintln!`s, or
      personal paths.
