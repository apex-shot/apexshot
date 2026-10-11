# ApexShot → Flathub Launch Plan

Written 2026-07-30. Based on an audit of the repo at `v0.2.33` (689 commits) against
current Flathub submission requirements.

---

## TL;DR

**Yes, prepare for Flathub — but do the quick wins first, because Flathub is the
most expensive item on the list and the others are nearly free.**

Flathub is the highest-value distribution channel for a Linux desktop app, and it
is realistically **2–4 weeks of focused work** for ApexShot, not a weekend. The
app currently assumes it owns the host system: it shells out to ~20 host binaries,
writes to `/usr/local/bin`, installs `/etc/xdg/autostart` entries, drops a Chrome
native-messaging manifest into `/etc/opt/chrome/`, and shells out to
`gnome-extensions` to install its own GNOME Shell extension. None of that works
inside a Flatpak sandbox.

So sequence it: **Phase 0 (one day, big relative payoff) → Phase 1 (metainfo,
useful everywhere) → Phase 2–5 (the real Flatpak port).**

---

## Phase 0 — Do these first (≈1 day total, no Flatpak work)

These are done and paid for already; they are just not published. Nothing about
Flathub should block them.

### 0.1 `.github/FUNDING.yml` — minutes

Does not exist. Right now a user who loves ApexShot has no button to press.

```yaml
github: [codegoddy]
ko_fi: apexshot          # if you create one
custom: ["https://apexshot.org/pricing"]
```

Then add a short "Support ApexShot" section near the top of `README.md`.

### 0.2 Publish to the AUR — hours

`packaging/arch/PKGBUILD` exists. `docs/AUR_PUBLISHING.md` exists. The AUR RPC
currently returns **0 results** for `apexshot` — it was never pushed. Arch users
are the single most likely audience to adopt a Wayland-native capture tool, and
AUR presence also gets you into `yay`/`paru` search results, which is organic
discovery you cannot buy.

### 0.3 Submit the GNOME extension to extensions.gnome.org — hours

`gnome-extension/SUBMISSION_GUIDE.md` exists; the extension is not on EGO
(verified: not in the extension query API). README currently says "may not yet be
published — use the release zip." That instruction loses most users at the door,
and it is a hard requirement for full functionality on GNOME. EGO listings also
drive traffic back to the app itself.

### 0.4 Remove the free-tier read gate — small change

`src/history/cloud_page.rs:277-302` tells a signed-in free user
"Upgrade to browse your cloud uploads." You are charging for read access to files
the user already uploaded. Gate on **capacity and capability** (storage, retention,
file size, private links) — never on seeing your own data. This gate produces
uninstalls, not conversions.

---

## Phase 1 — AppStream metainfo (≈1 day, benefits every distro)

**There is no AppStream metainfo file anywhere in `packaging/` or `data/`.** This is
a hard Flathub requirement, and it also improves the deb/rpm/AUR packages
immediately (GNOME Software and Discover will show ApexShot properly). Do this even
if Flathub slips.

Create `packaging/<app-id>.metainfo.xml` with:

- `<id>` matching the app ID and the `.desktop` filename exactly
- `<name>`, `<summary>` (max ~35 chars, no marketing), `<description>`
- `<metadata_license>CC0-1.0</metadata_license>` and `<project_license>GPL-3.0-or-later</project_license>`
- `<screenshots>` — **must be stable public URLs, not repo-relative paths.** Reuse
  `media/*.png`; raw.githubusercontent.com links are acceptable. Animated GIFs are
  not valid screenshots, so `capture-workflow.gif` and
  `image-editor-tutorial.gif` need static PNG equivalents.
- `<releases>` with real release notes per version (Flathub reviewers check this)
- `<content_rating type="oars-1.1"/>` — ApexShot uploads user content and has
  a social/share surface, so expect at minimum
  `social-info` / `social-chat` considerations; generate it at
  https://hughsie.github.io/oars/
- `<developer id="…"><name>codegoddy</name></developer>`
- `<url type="homepage|bugtracker|vcs-browser|donation">`
- `<launchable type="desktop-id">` pointing at the desktop file

Validate locally before submitting:

```bash
appstreamcli validate --explain packaging/<app-id>.metainfo.xml
desktop-file-validate packaging/<app-id>.desktop
```

Also note: **`Categories=Graphics;` alone is invalid** for a menu entry — it needs a
registered subcategory such as `Graphics;Photography;` or `Utility;`. And the
desktop file must be renamed to exactly `<app-id>.desktop`.

---

## The app ID decision

**Recommendation: rename to `org.apexshot.ApexShot`.**

Current ID is `io.github.codegoddy.apexshot`. That is a problem for Flathub, which
requires the app ID to reflect a domain or repository **you demonstrably control**.
The repo lives under the GitHub org `apex-shot`, not under the user `codegoddy`, so
the current ID matches neither your website nor your repo owner. Expect a reviewer
to flag it.

| Option | Verdict |
|---|---|
| `org.apexshot.ApexShot` | **Recommended.** You own `apexshot.org`, so ownership is provable. Clean, permanent, independent of GitHub. |
| `io.github.apex_shot.apexshot` | Valid (hyphens become underscores in AppStream IDs), but ties your identity to GitHub forever. |
| Keep `io.github.codegoddy.apexshot` | Rejection risk. Do not ship this to Flathub. |

### Rename cost — it is not just a string

Grep found the ID in these places, and all must move together or the app breaks in
subtle ways:

- `Cargo.toml` deb assets (desktop file name, icon name, native-host manifest)
- `src/app_identity.rs:3` — `OFFICIAL_APP_ID`, `OFFICIAL_DESKTOP_FILE`
- `gnome-extension/window-list.js:37` — window-class matching for the always-on-top
  preview logic. **If this is missed, preview windows silently stop staying on top.**
- `packaging/<app-id>.desktop` — `StartupWMClass`
- `native-host/io.github.codegoddy.apexshot.json` + both Chrome/Chromium install paths
- `packaging/debian/postinst`, `scripts/ubuntu-update.sh`
- `docs/ARCHITECTURE.md`, `docs/MODULES.md`, `docs/DEVELOPER_GUIDE.md`

Migration for existing installs: keep the **old** desktop file and icon installed as
aliases for one or two releases, and have `postinst` clean up stale
`~/.config/autostart/apexshot.desktop` entries (the script already does some of
this). The portal's `restore_token` for ScreenCast is keyed to app ID, so users will
get **one** re-prompt after the rename — mention it in the release notes so it does
not read as a bug.

Also worth deciding: `org.apexshot.ApexShot` for the Flatpak while the deb/rpm keep
the old ID is *not* recommended — you would be maintaining two identities and the
GNOME extension can only match one cleanly. Rename everywhere, once.

---

## Phase 2 — Sandbox blockers (the real work, ≈1–2 weeks)

This is what actually makes the port non-trivial. Each item below is a thing the app
does today that a Flatpak cannot do.

### 2.1 Host binaries shelled out to

Verified via grep across `src/`. Inside a sandbox these must be bundled, replaced
with a portal/library call, or feature-gated off.

| Called | Where | Flatpak resolution |
|---|---|---|
| `xdg-open` | `history/actions.rs:79,116`, `daemon/mod.rs:2791`, `settings/cloud.rs:408`, `onboarding/extensions.rs:16` | Use `gtk::UriLauncher`/`FileLauncher` (OpenURI portal). Straightforward. |
| `wl-copy` / `xclip` | `utils/clipboard.rs:19` | Use the GTK4 clipboard API. Needed anyway; the current path breaks in-sandbox. |
| `notify-send` | `utils/notify.rs:157` | Use the Notification portal via `zbus` (already a dependency). |
| `ffmpeg` | `history/thumbnails.rs`, recording editor | **Bundle** as a manifest module, or use the `org.freedesktop.Platform.ffmpeg-full` extension. |
| `tesseract` | OCR path | **Bundle** leptonica + tesseract + `eng.traineddata`. Non-trivial module work. |
| `grim`, `wf-recorder` | wlroots capture/record paths | Not available in sandbox. Drop these tiers in the Flatpak build; rely on portals. |
| `pactl` | `daemon/mod.rs:1666` | Query PipeWire directly, or accept degraded audio-source listing. |
| `gsettings` | `hotkeys/mod.rs:1317`, `settings/windowing.rs:63-99` | Host schemas are not visible. Theme detection → use the Settings portal. GNOME keybinding install → cannot work; must use GlobalShortcuts portal. |
| `gnome-extensions` | `onboarding/extensions.rs:28-45` | **Impossible in sandbox.** Detect Flatpak and link to extensions.gnome.org instead of attempting install. |
| `dbus-send` / `gdbus` | `gnome_shell.rs`, `gnome_integration/mod.rs`, `backend/portal_permissions.rs` | Replace with `zbus` calls and add matching `--talk-name=` permissions. Talking to a Shell extension from a sandbox needs an explicit hole and reviewers will ask about it. |
| `gtk-launch` | `daemon/mod.rs:1812`, `hotkeys/mod.rs:318` | Self-relaunch differs in Flatpak; rework or gate off. |
| `pacman`/`dpkg-query`/`rpm` | `main.rs:631-651` | Package detection is meaningless in Flatpak. Gate off. |
| `hyprctl`, `swaymsg` | `hotkeys/mod.rs:1065,1103` | Config reload for compositor keybinds; not reachable. Gate off. |
| `wget`/`curl`/`unzip` | in-app extension installer | Gate off with the extension installer. |

### 2.2 The Qt5 overlay

`capture-overlay/CMakeLists.txt` requires `Qt5 Widgets DBus X11Extras Network` plus
`X11 Xtst`. Bundling Qt5 into the Flatpak means pulling the KDE BaseApp and adds
hundreds of MB, for a component that is largely the **GNOME X11-era** path.

You already have a Rust GTK4 + layer-shell overlay (`src/overlay/`, 15 files) used on
wlroots. **Recommendation: build the Flatpak GTK4-only and drop `apexshot-capture`
from it.** This is the single biggest scope decision in the port. If the GTK4 overlay
is not yet at parity with the Qt5 one on GNOME, closing that gap is the critical
path — do it before anything else in Phase 2.

Note also `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` in the desktop
file: the KDE-native screenshot backend will not be reachable from the sandbox
either. Portals only.

### 2.3 Install/autostart model

`main.rs:497-1213` and `settings/windowing.rs:108-193` install binaries to
`/usr/local/bin`, write `/etc/xdg/autostart`, and `chown` files. In a Flatpak:

- The `install` / `uninstall` CLI subcommands must be hidden or hard-error.
- Autostart must go through the **Background portal**
  (`org.freedesktop.portal.Background` + `RequestBackground` with autostart), which
  prompts the user. This changes the daemon's first-run UX and needs designing.
- The daemon-plus-tray model itself is fine, but the tray needs
  `--talk-name=org.kde.StatusNotifierWatcher` for `ksni`.

### 2.4 Browser native messaging — will break

The deb installs `native-host/io.github.codegoddy.apexshot.json` to
`/etc/opt/chrome/NativeMessagingHosts/` and `/etc/chromium/NativeMessagingHosts/`.
A Flatpak cannot write there, and a sandboxed binary is not directly executable by
the host browser.

Full-page scroll capture therefore does not work in the Flatpak build without extra
plumbing (a host-side manifest pointing at `flatpak run --command=…`, which the user
must install manually). **Plan to document this as a known Flatpak limitation** and
keep the feature for deb/rpm/AUR. Do not try to solve it in v1.

### 2.5 Telemetry default — likely rejection risk

`src/config.rs:282` sets `telemetry_enabled: true` by default, and
`src/usage_telemetry.rs:23` posts a daily heartbeat to `apexshot.org`. The privacy
design is genuinely careful (random install ID, no content, rate-limited,
fail-open) — but **Flathub reviewers consistently require telemetry to be opt-in**,
not opt-out, for apps on the store.

Recommendation: default `telemetry_enabled` to `false` when running inside a
Flatpak (detect `/.flatpak-info`), or add a first-run consent step in onboarding.
Do not fight this in review; it will cost you weeks.

### 2.6 Repo hygiene — flag before a reviewer does

The repo contains **full vendored source trees** of other projects:
`flameshot/` (238 src files), `obs-studio/` (thousands of files), `spectacle/`
(143 src files), plus committed `build/` and `target/` artifacts, an `.mp4`
recording, `replit.md`/`replit.nix`, and `install-fixed-apexshot.txt`.

These are presumably reference material, but a GPL-3.0 project shipping copies of
three other projects' source raises immediate licensing questions for a reviewer,
and it bloats the clone enormously. **Move them out of the repo (or into a separate
`reference/` repo) and add `build/`, `target/` to `.gitignore` before submitting.**

---

## Phase 3 — Build the Flatpak (≈3–5 days once Phase 2 is done)

Good news on dependencies: `Cargo.lock` has **453 crates and zero git
dependencies**, so the standard generator works cleanly.

```bash
# Generate vendored cargo sources
python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
```

Manifest sketch — `org.apexshot.ApexShot.yml`:

```yaml
id: org.apexshot.ApexShot
runtime: org.gnome.Platform
runtime-version: '48'
sdk: org.gnome.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: apexshot
finish-args:
  - --socket=wayland
  - --socket=fallback-x11
  - --share=ipc
  - --device=dri
  - --share=network                      # cloud upload, OCR model download
  - --socket=pulseaudio                  # recording audio
  - --filesystem=xdg-pictures
  - --filesystem=xdg-videos
  - --talk-name=org.kde.StatusNotifierWatcher    # ksni tray
  - --talk-name=org.gnome.Shell                  # extension integration (justify in PR)
modules:
  - leptonica
  - tesseract (+ eng.traineddata)
  - ffmpeg (or use the ffmpeg-full extension)
  - gtk4-layer-shell
  - apexshot (cargo build --release, with cargo-sources.json)
```

Notes:
- Target `org.gnome.Platform` 48 (GTK4 + libadwaita are already your GUI stack).
- `--filesystem=host` will get you rejected; scope to XDG dirs and use the
  FileChooser portal for anything else.
- OCR models download at runtime from S3 to `~/.cache/apexshot/ocr-models` — that is
  acceptable with `--share=network`, but reviewers prefer bundled models. Consider
  bundling `text-detection.rten` / `text-recognition.rten` if size allows.
- `gtk4-layer-shell` works in Flatpak but layer-shell is a wlroots protocol; on
  GNOME the layer-shell overlay paths will not apply. Confirm which overlay path
  actually runs on GNOME before assuming parity.

Test locally:

```bash
flatpak-builder --user --install --force-clean build-dir org.apexshot.ApexShot.yml
flatpak run org.apexshot.ApexShot
flatpak run --command=sh org.apexshot.ApexShot   # poke around the sandbox
```

---

## Phase 4 — Submit (days to weeks of review latency)

1. Fork `flathub/flathub`, branch `new-pr`, add only the manifest + metainfo +
   `cargo-sources.json`.
2. Open a PR against the `new-pr` branch. Read
   `CONTRIBUTING.md` in that repo first.
3. Expect reviewer questions on: the `org.gnome.Shell` talk-name hole, telemetry
   default, network usage, and any bundled binaries.
4. Turnaround is commonly **1–4 weeks** with back-and-forth. Budget for it.
5. After merge, set up Flathub build notifications and treat the manifest as a
   release step in your existing workflow.

---

## Phase 5 — After Flathub

- **Fedora COPR** and **openSUSE OBS** — you already have both `.spec` files
  (`packaging/fedora/apexshot.spec`, `packaging/opensuse/apexshot.spec`). Cheap wins
  once the metainfo exists.
- Announce the Flathub availability on r/linux, r/gnome, r/unixporn, and Hacker News.
  Your [`ANNOUNCEMENTS.md`](../ANNOUNCEMENTS.md) drafts are reusable, but update them: they still advertise
  the removed click-overlay/keystroke-overlay features and webcam PiP.
- **Fix the Fedora recording gap or stop shipping the Fedora path as primary.**
  "Video recording is not supported on Fedora" is a large asterisk on one of the
  biggest desktop distros, and Flatpak is exactly the mechanism that could solve it
  (bundled ffmpeg + portal ScreenCast, no distro codec politics). This is a strong
  argument for prioritizing the Flatpak.

---

## Realistic effort summary

| Phase | Work | Effort |
|---|---|---|
| 0 | FUNDING.yml, AUR, EGO, ungate cloud reads | ~1 day |
| 1 | AppStream metainfo + desktop file fixes | ~1 day |
| — | App ID rename across code/docs/extension | ~0.5 day |
| 2 | Sandbox port: portals, bundling, gating, GTK4-only decision | **1–2 weeks** |
| 3 | Manifest, cargo-sources, tesseract/ffmpeg modules, local testing | 3–5 days |
| 4 | Flathub PR + review iterations | 1–4 weeks latency |

**Biggest risks, in order:** (1) GTK4 overlay not at parity with the Qt5 overlay on
GNOME, (2) telemetry-default pushback in review, (3) the sheer number of
shell-outs to host binaries, (4) repo hygiene questions from the vendored
Flameshot/OBS/Spectacle trees.

---

## Why this matters (the numbers)

| Metric | Value |
|---|---|
| Project age | ~5 months (first commit 2026-02-21) |
| Commits | 689 |
| GitHub stars | 46 |
| Total downloads, all 27 releases | ~958 |
| Best single release | 271 (v0.2.28) |
| Flathub | absent |
| AUR | absent (PKGBUILD written, never pushed) |
| extensions.gnome.org | absent |
| `.github/FUNDING.yml` | absent |

At ~958 lifetime downloads, a $5/mo subscription with a realistic sub-1%
FOSS-audience conversion rate yields approximately zero revenue. Reaching even
~$1,000/mo needs roughly 200 subscribers, which implies tens of thousands of active
users. **Shipping more features cannot close that gap; only distribution can.**
Flathub is the largest single lever available, which is why it is worth the
2–4 weeks — and why Phase 0 should not wait for it.

Separately: reconsider selling *storage* to Linux users at all. You compete with
free Imgur/GitHub/Discord and, more sharply, with your own free self-hosted
XBackBone support, which is advertised on your own pricing page. The user technical
enough to run ApexShot on Hyprland is exactly the user who will self-host rather
than pay. **Team/org features (shared libraries, retention policy, SSO, seat
management, custom share domains) at $10–20/seat are a far more reachable
business** — ten five-seat teams beats two hundred hobbyists.
