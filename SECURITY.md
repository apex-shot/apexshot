# Security Policy

## Supported Versions

Only the latest tagged release of ApexShot receives security fixes. The
project moves quickly; older `.deb` builds and unreleased `main` snapshots
are **not** considered supported.

| Version          | Supported          |
|------------------|--------------------|
| Latest release   | :white_check_mark: |
| Previous tag     | Best-effort, no SLA |
| Older releases   | :x:                |

## Reporting a Vulnerability

ApexShot captures the user's screen, microphone, speaker output, and
clipboard, and persists XDG ScreenCast portal grants on disk. Issues
that could expose any of these to other users or processes deserve
responsible disclosure.

**Please do not open a public GitHub issue for security problems.**

Email reports to **codegoddy@gmail.com** with:

- A clear description of the issue and the conditions under which it
  triggers.
- The ApexShot version (`apexshot --version` or the `.deb` version),
  desktop environment, display server (X11 / Wayland), and GNOME Shell
  version if applicable.
- A proof-of-concept (script, recording, or steps) if you have one.
- Whether you'd like to be credited in the release notes.

You can expect:

- An initial acknowledgement within **3 working days**.
- A triage update within **7 working days** describing whether we can
  reproduce the issue and the proposed fix window.
- A public advisory + patched release once a fix is in place. We will
  coordinate the disclosure date with you when feasible.

## Particularly sensitive areas

These subsystems are the highest-leverage parts of the codebase from a
security perspective and warrant extra scrutiny:

- `src/backend/wayland.rs`, `src/backend/portal_permissions.rs`, and
  `capture-overlay/src/ScreenCapture.cpp` — XDG portal lifecycles and the
  on-disk `restore_token` cache (`~/.cache/apexshot/`).
- `src/utils/clipboard.rs` and `src/utils/desktop_env.rs` — clipboard data
  and environment forwarding.
- `src/recording/` — PipeWire / ffmpeg pipelines that touch raw audio
  and video buffers.
- `gnome-extension/` — runs inside the GNOME Shell process; any escape
  from its sandboxing is high impact.
- `native-host/` and `web-scroll-extension/` — Chrome / Chromium native
  messaging entry points.

## Out of scope

The following are not treated as security issues:

- Reports requiring physical access to an unlocked, logged-in session.
- Crashes that are not exploitable for privilege escalation, data
  exfiltration, or persistent integrity loss.
- Findings against modified builds (forks, custom patches, AUR rebuilds
  with substituted dependencies).

Thanks for helping keep ApexShot users safe.
