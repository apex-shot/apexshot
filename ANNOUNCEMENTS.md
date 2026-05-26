# ApexShot Launch Announcements

> **Note (May 2026):** The runtime click-overlay and keystroke-overlay features mentioned in
> some of these historical announcements have since been removed. The webcam PiP, mic/speaker
> audio monitoring, and recording controls remain available.

## Reddit - r/linux

Title: ApexShot — open-source screen capture, recording and OCR for Linux (alpha, looking for testers)

> r/linux is one of the stricter subs (account age + karma minimums,
> heavy on AutoMod). Keep the post factual, technical, no
> CleanShot-X marketing language — the audience here pushes back on
> hype hard. If you can't post directly, try r/LinuxApps or
> r/coolgithubprojects first to build karma.

GitHub: https://github.com/apex-shot/apexshot

ApexShot is an open-source screen capture and recording tool for Linux that I've been working on. It tries to bundle the things people usually wire together from Flameshot + OBS + a stray OCR script into one workflow.

What works today
- Captures: full screen, area, window, crosshair
- Editor: pen, highlighter, shapes, text, blur, crop, color picker, gradient backgrounds
- Recording: VP9 / H.264 / GIF, optional webcam PiP, click overlay during recording
- OCR: Tesseract with multi-PSM voting (English by default; tessdata for other languages works)
- QR detection on captured regions
- Browser extension + native messaging host for full-page scroll capture
- Daemon mode with system tray and global hotkeys

How it captures (likely the part this sub cares about)
Tiered backend: wlr-screencopy → grim → XDG Screenshot portal → ScreenCast portal. The ScreenCast path persists a `restore_token` to `~/.cache/apexshot/`, so you only get prompted once per source. X11 path uses standard tools.

Tested today
- GNOME Shell 47–49 on Ubuntu 24.04 / 25.10, Wayland
- Best-effort on KDE Plasma 6, Sway, X11
- Honest matrix in CONTRIBUTING.md, gaps included

Known rough edges
- Keystroke overlay during recording is gated behind a feature flag until the recorder side lands. UI shows a "Soon" badge instead of pretending to work.
- Recent .deb hard-depended on `xclip` + `pulseaudio-utils` + `gstreamer1.0-pulseaudio`; that's been switched to apt alternatives (`wl-clipboard | xclip`, `pipewire-pulse | pulseaudio-utils`) so the next release installs cleanly on PipeWire/Wayland-only systems.

Stack: Rust core, C++17/Qt5 overlay, GTK4 + gtk4-layer-shell, GStreamer, PipeWire. GPL-3.0.

Feedback and testers welcome — especially anyone on KDE / Sway / Hyprland who can tell me where the portal flow falls over.

Demo: [attach example.gif]

---

## Reddit - r/opensource

Title: ApexShot v0.2.25 — open-source screen capture, recording and OCR for Linux

> r/opensource is permissive about account age but the audience values
> licensing clarity, contribution-friendliness, and honest scope.
> Lead with those, not with feature lists.

GitHub: https://github.com/apex-shot/apexshot · License: GPL-3.0

I released ApexShot v0.2.25, an open-source screen capture and screen-recording tool for Linux. It's an alpha, but the core workflow is stable enough that I'd like more eyes on it.

What it does
- Screenshots: full screen, area, window, crosshair, with an annotation editor (pen, highlighter, shapes, text, blur, crop, color picker, gradient backgrounds)
- Recording: VP9 / H.264 / GIF, optional webcam PiP, click overlay during recording
- OCR via Tesseract (multi-PSM voting), QR detection on captured regions
- GNOME Shell extension for runtime overlays and area-selection mask
- Chrome/Chromium extension + native messaging host for full-page scroll capture
- Daemon mode with system tray (`ksni`) and global hotkeys

Contributor-friendliness (the reason I'm posting here specifically)
I just finished a contributor-readiness pass:
- Code of Conduct (Contributor Covenant 2.1)
- Security policy with disclosure process
- Structured issue / PR templates
- `rustfmt.toml`, `.clang-format`, `.editorconfig`
- CI lint gate (`cargo fmt --check` + clippy + `node --check` over the GNOME extension)
- CONTRIBUTING.md with a subsystem map and a "where things live" table covering the Rust core, the Qt5 overlay, the GNOME extension, and the browser-side helpers

The high-value contribution areas are listed in CONTRIBUTING.md — including the keystroke-overlay recorder side (UI is built and gated behind a feature flag, recorder side is the open work) and clearing the ~65-warning clippy backlog.

Stack: Rust core, C++17/Qt5 overlay, GTK4 + gtk4-layer-shell, GStreamer, PipeWire, Tesseract.

Tested today on GNOME Shell 47–49 / Ubuntu 24.04 / 25.10 / Wayland; best-effort elsewhere. Full matrix in CONTRIBUTING.md.

Demo: [attach example.gif]

---

## Reddit - r/gnome

Title: ApexShot — screen capture + recording with a GNOME Shell extension for runtime overlays (looking for testers on 45–49)

> r/gnome is small but lenient on account age. The audience cares
> about extension hygiene (manifest version coverage, no Clutter
> warnings spamming `journalctl`) more than feature lists. Lead with
> the GNOME-specific bits.

GitHub: https://github.com/apex-shot/apexshot

I've been building ApexShot, a screen capture + recording tool for Linux. Most of it is a regular GTK4 / Rust app, but the parts that matter on GNOME live in the bundled Shell extension:

GNOME-side bits
- Runtime click overlay during recording (halo + pulse ring + filled marker, all `St.Widget` actors with proper Clutter signal lifecycle)
- Area-selection mask managed by the shell so it survives focus/workspace changes
- Recording-controls dock that follows the active monitor
- Webcam PiP that's draggable on the stage (the recent fix for the "webcam sticks to cursor" bug uses stage-level pointer events)
- Screenshot lock to keep a captured region pinned while you annotate
- Coverage: GNOME Shell 45 / 46 / 47 / 48 / 49 (manifest declares all five)

App-side bits
- Captures: full screen, area, window, crosshair, with annotation editor
- Recording: VP9 / H.264 / GIF, optional webcam PiP
- OCR (Tesseract multi-PSM voting), QR detection
- Browser extension + native messaging host for full-page scroll capture on Chrome/Chromium
- Daemon mode with system tray + global hotkeys

Capture path on Wayland: wlr-screencopy → grim → XDG Screenshot portal → ScreenCast portal. The ScreenCast path persists a `restore_token` so you only get prompted once.

Tested on GNOME Shell 47–49 on Ubuntu 24.04 / 25.10 / Wayland. The extension installs from `gnome-extension/` (or via the in-app onboarding wizard). Source for everything is GPL-3.0.

Looking for GNOME testers — especially anyone on a non-Ubuntu distro or on GNOME 45/46/49 who can tell me where the extension falls over.

Demo: [attach example.gif]

---

## Reddit - r/Ubuntu

Title: ApexShot — open-source screen capture + recording for Ubuntu (.deb available, tested on 24.04 / 25.10)

> r/Ubuntu has a moderate karma threshold and removes posts it
> reads as low-effort. The audience cares about "will this install
> cleanly on my machine" more than anything else — answer that
> question first.

GitHub: https://github.com/apex-shot/apexshot · Releases page has the .deb

ApexShot is an open-source screen capture + recording tool for Linux. I'm posting here because Ubuntu is the platform I test on every day, so the .deb install path is the most polished.

Install
```
DEB_URL=$(curl -s https://api.github.com/repos/apex-shot/apexshot/releases/latest \
  | grep "browser_download_url.*amd64.deb" | cut -d '"' -f 4)
curl -LO "$DEB_URL"
sudo apt install ./apexshot_*.deb
```
(Use `apt install ./file.deb` instead of `dpkg -i` — apt resolves
dependencies; raw dpkg leaves you to chase them.)

What it does
- Captures: full screen, area, window, crosshair, with annotation editor (pen, highlighter, shapes, text, blur, crop, color picker, gradient backgrounds)
- Recording: VP9 / H.264 / GIF, optional webcam PiP, click overlay during recording
- OCR (Tesseract multi-PSM voting), QR detection
- GNOME Shell extension for runtime overlays
- Chrome/Chromium extension + native messaging host for full-page scroll capture
- Daemon mode with system tray + global hotkeys

Tested today on Ubuntu 24.04 and 25.10, GNOME Wayland. X11 path works but is less exercised. Best-effort on KDE / Sway.

Known rough edges (so you know going in)
- The 0.2.25 .deb hard-depended on `xclip` + `pulseaudio-utils` + `gstreamer1.0-pulseaudio`. That's fixed in main (next release switches them to apt alternatives so PipeWire/Wayland-only setups install cleanly). If you hit it on 0.2.25, run `sudo apt install -y gstreamer1.0-pulseaudio pulseaudio-utils xclip && sudo dpkg --configure -a`.
- Keystroke overlay during recording is feature-flagged off until the recorder side lands.

GPL-3.0. Bug reports on the issue tracker are appreciated — the templates ask for distro / DE / display server up front so triage is fast.

Demo: [attach example.gif]

---

## Reddit - r/rust

Title: ApexShot — a Rust + GTK4 + Qt5 screen capture / recording tool for Linux (~380 unit tests, GPL-3.0)

> r/rust prefers the **Showcase Saturday** weekly thread for project
> announcements — top-level showcase posts on weekdays often get
> removed regardless of karma. Save this for the Saturday thread, or
> reframe it as a write-up on a specific Rust technique (e.g. "how
> ApexShot persists XDG ScreenCast restore_tokens") for a top-level
> post on a weekday.

GitHub: https://github.com/apex-shot/apexshot

ApexShot is an open-source screen capture and recording tool for Linux. The core is Rust 2021, ~380 unit tests, ~50k LOC. The interactive overlay is a separate Qt5/C++17 binary (`apexshot-capture`) that the Rust side spawns; the GNOME-side runtime overlays are a Shell extension.

Architecture worth talking about
- **Capture backend tiering** (`src/backend/`): wlr-screencopy → grim → XDG Screenshot portal → ScreenCast portal. The Wayland path uses `ashpd` for portal calls and persists a `restore_token` to `~/.cache/apexshot/` so the user only sees the portal dialog once per source.
- **Editor** (`src/capture/editor/`): GTK4 + Cairo. Drawing perf was a real bottleneck — the Pen / Highlighter draft path now skips Douglas–Peucker simplification on every redraw and only simplifies on stroke finalisation. Redraw throttle lives in a single `DRAG_REDRAW_INTERVAL_US` constant.
- **Preview overlay** (`src/capture/preview_overlay.rs`): PNG decode runs on a background `std::thread` so the GTK main loop never blocks; the preview window appears immediately and the texture swaps in once decoded.
- **Recording** (`src/recording/`): GStreamer pipelines (VP9 / H.264 / GIF) with PipeWire for audio source discovery via `pactl`.
- **OCR** (`src/ocr/`): Tesseract LSTM run with multiple `--psm` candidates, highest-confidence result selected, with an early-exit threshold.
- **Tray + hotkeys**: `ksni` for the system tray, custom hotkey daemon for global shortcuts.

Dev quality of life
- `[dev-dependencies]`: `pretty_assertions`, `test-case`, `mockall`.
- CI lint gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `node --check` over the GNOME extension.
- ~65 pre-existing clippy warnings, surfaced but not yet `-D warnings` — clearing the backlog is one of the high-value contribution areas listed in CONTRIBUTING.md.

GPL-3.0. Tested today on GNOME Shell 47–49 / Ubuntu 24.04 / 25.10. Honest matrix and "where things live" table in CONTRIBUTING.md.

---

## Reddit - r/coolgithubprojects

Title: ApexShot: open-source screen capture, recording and OCR for Linux (Rust + GNOME Shell extension)

> The sub uses casual, first-person, descriptive titles — no `[LANGUAGE]`
> prefix is required (that's a different subreddit). Pick the matching
> flair (`OTHER` or whatever fits) after posting. Keep the body honest
> and concrete; the audience here likes seeing what's actually built and
> what's still rough.

Repo: https://github.com/apex-shot/apexshot

I've been building ApexShot, a screen capture and screen-recording tool for Linux that tries to be the kind of all-in-one tool macOS users get from CleanShot X. Core is Rust (~380 unit tests), with a Qt5 C++ overlay for the interactive selector and a GNOME Shell extension for the runtime click overlay.

What it does today
- Area / window / full-screen / crosshair captures
- Screen recording (VP9, H.264, GIF) with optional webcam PiP
- Built-in editor: pen, highlighter, shapes, blur, crop, color picker, background gradients
- OCR (multi-PSM Tesseract voting) and QR detection on captures
- Click overlay during recording (halo + pulse ring + marker)
- Chrome/Chromium extension + native messaging host for full-page scroll capture

How it captures
Tiered backend: wlr-screencopy → grim → XDG Screenshot portal → ScreenCast portal (with restore_token persistence so you only get prompted once). X11 path uses the standard tools.

Tested today
- GNOME Shell 47–49 on Ubuntu 24.04 / 25.10, Wayland (primary)
- Best-effort on KDE Plasma 6, Sway, X11
- The matrix and the gaps are documented in CONTRIBUTING.md

Honest about the rough edges
- Keystroke overlay during recording is gated behind a feature flag (`kKeystrokesFeatureAvailable`) until the recorder side lands. The UI shows a "Soon" badge instead of pretending to work.
- ~65 pre-existing clippy warnings — CI surfaces them but doesn't block on `-D warnings` yet.

I just finished a contributor-readiness pass: Code of Conduct, security policy, structured issue/PR templates, rustfmt + clang-format + editorconfig, a CI lint gate (`cargo fmt --check` + clippy + `node --check`), and a CONTRIBUTING.md with a subsystem map. If any of the rough edges sound interesting, the high-value areas are listed in CONTRIBUTING.md.

License: GPL-3.0. Feedback and PRs welcome.

Posting checklist
- [ ] Pick a fitting flair (`OTHER` works) after submitting.
- [ ] Drop a screenshot or short GIF as the first comment — posts with visuals do noticeably better here.
- [ ] Cross-posts (`r/linux`, `r/opensource`, `r/rust`) spaced 1–2 days apart, not simultaneously.

---

## Reddit - r/vibecoding

Title: Windows has ShareX, Mac has CleanShot X — so I vibe-coded the Linux one

> r/vibecoding rewards casual, first-person, "I built this" energy
> over polished marketing copy. The audience genuinely wants to hear
> "I vibe-coded ___" and they engage hard with screenshots and short
> clips. Keep the tone conversational, drop a demo GIF as the first
> comment, and reply quickly in the first 30 minutes.

Hey r/vibecoding 👋

Quick story. I've been daily-driving Linux for years and the one thing that always bugged me is that Windows people get [ShareX](https://getsharex.com/) and Mac people get CleanShot X — both of these absurdly polished, all-in-one screenshot + recording + OCR tools — and on Linux you're stuck duct-taping Flameshot + OBS + a random OCR script and praying nothing breaks on Wayland.

So I vibe-coded the thing I wished existed. It's called **ApexShot**.

What it actually does today
- Screenshots: full screen, area, window, crosshair — with a real annotation editor (pen, highlighter, shapes, text, blur, crop, color picker, gradient backgrounds)
- Screen recording: VP9 / H.264 / GIF, with optional webcam PiP and a click overlay during recording
- OCR baked in (Tesseract, multi-PSM voting) — screenshot any text, it's on your clipboard
- QR detection on whatever you capture
- A little Chrome extension + native messaging host so full-page scroll capture actually works
- Daemon mode with system tray + global hotkeys
- A GNOME Shell extension for the runtime overlays so the recording mask survives focus/workspace changes

The capture path was the hardest part. On Wayland you can't just grab the screen — you fall through a tier of backends: wlr-screencopy → grim → XDG Screenshot portal → ScreenCast portal. ApexShot persists the portal `restore_token` so you only get the "share screen?" dialog once per source instead of every single time.

Stack: Rust core (~380 unit tests, ~50k LOC), C++17/Qt5 for the interactive selector overlay, GTK4 + gtk4-layer-shell for the editor, GStreamer + PipeWire for recording. GPL-3.0.

Honest about where it's at
- It's alpha (v0.2.25). Tested mostly on GNOME 47–49 / Ubuntu 24.04 / 25.10 / Wayland.
- Best-effort on KDE / Sway / X11 — works for me but I haven't beaten it up.
- Keystroke overlay during recording is feature-flagged off until the recorder side lands. UI shows a "Soon" badge instead of pretending.

Repo: https://github.com/apex-shot/apexshot
Site: https://apexshot.org/

Was kind of a wild thing to build solo and "vibe-coded" is doing a lot of heavy lifting in this title — a *lot* of late nights with the AI yelling at me about Clutter signal lifecycles. But it works and I use it every day now.

If you're on Linux and have ever ragequit because Flameshot can't record, give it a shot 🙃 Issues / PRs / "this broke on my distro" reports all very welcome.

Posting checklist
- [ ] Drop a short demo GIF/clip as the first comment — this sub eats visuals for breakfast.
- [ ] Reply to every comment in the first hour.
- [ ] Don't cross-post to r/linux the same day; r/linux flags vibecoding-style framing.
- [ ] Mention the stack in replies if anyone asks "did you actually write any of this" — having ~380 tests + a real architecture answers that fast.

---

## Hacker News

> **Why the previous two attempts didn't catch.**
> Two prior Show HN attempts for ApexShot got little traction. The most
> common reasons HN ignores a Show HN, in order:
>
> 1. **Title doesn't follow `Show HN:` format.** Without that prefix the
>    post lands in the regular feed where project announcements are
>    filtered hard. Question titles ("Is this the X users want?") read as
>    clickbait and get flagged or downvoted on sight.
> 2. **Marketing language in the body.** Words like "polished",
>    "all-in-one", "modern", "powerful", "the X experience" trigger an
>    immediate "this is PR copy" reaction. HN voters skim for substance
>    and dismiss anything that sounds rehearsed.
> 3. **Bulleted feature lists.** HN renders bullets but the audience
>    treats them as a tell that the post is product-launch flavoured.
>    Successful Show HNs read like a personal note.
> 4. **No author self-comment within 60 seconds of posting.** The first
>    comment from the author with the technical interesting bit is what
>    gets a Show HN out of `/newest` and onto the front page. Without it,
>    the post dies in `/newest` regardless of quality.
> 5. **Wrong timing.** HN front page is decided in the first 1–2 hours
>    after posting. Outside the Tue–Thu 06:30–08:30 PT window
>    (13:30–15:30 UTC), the post competes with the bulk of the day's
>    submissions and rarely surfaces.
> 6. **Same-project re-submission.** HN's duplicate detector quietly
>    suppresses re-submissions of the same URL/title for ~30 days. Two
>    standard project-announcement Show HNs in close succession may have
>    already burned that path. **The realistic play now is Angle B
>    below — a technical write-up Show HN with the project as the
>    artifact** — not a third announcement-style post.

### Angle A — Show HN: project announcement (only if 30+ days have passed since the last attempt)

Title (≤ 80 chars, follow this format exactly):
```
Show HN: ApexShot – open-source ShareX/CleanShot X for Linux (Rust, GPL-3.0)
```

Body (no bullets, no marketing words, two short paragraphs and the link):

```
I daily-drive Linux and got tired of stitching Flameshot, OBS, and a
random OCR script together every time I needed to share a screenshot.
ShareX on Windows and CleanShot X on macOS exist and are great. The
Linux equivalent didn't, so I spent the last several months building one.

ApexShot does area / window / full-screen capture with an annotation
editor, screen recording (VP9 / H.264 / GIF) with webcam PiP, OCR via
Tesseract, QR detection, and a GNOME Shell extension for the runtime
overlays. The Wayland capture path is tiered (wlr-screencopy → grim →
XDG Screenshot portal → ScreenCast portal) and persists the portal
restore_token so users only see the share-screen prompt once per source.

Core is Rust (~50k LOC, ~380 unit tests). The interactive selector is
a separate Qt5/C++17 binary the Rust side spawns. Editor is GTK4 +
gtk4-layer-shell + Cairo. Recording uses GStreamer + PipeWire. It is
alpha (v0.2.25), GPL-3.0, primarily tested on GNOME 47–49 / Ubuntu
24.04–25.10 / Wayland. Best-effort elsewhere.

Repo: https://github.com/apex-shot/apexshot
Site: https://apexshot.org/
```

### Angle B — Show HN: technical write-up (recommended given prior attempts)

This sidesteps the duplicate detector and aligns with what HN's front
page actually rewards: a specific, technically interesting subsystem
with the project as the artifact rather than the headline.

Pick **one** of the following angles. The first one is the strongest
fit because the Wayland portal experience is a recognised pain point
across Linux desktop dev.

**B1 — Wayland portal `restore_token` persistence**

Title:
```
Show HN: Skipping the Wayland share-screen prompt with persisted restore_tokens
```

Opening line: *"Every Wayland screen-capture tool re-prompts the user on
every capture. The XDG Desktop Portal spec has a `restore_token` field
that almost no one persists. Here is what it took to actually make it
work across GNOME, KDE, and Sway."*

Then walk through:
- The lifecycle of a ScreenCast session and where the token is returned
- Why most apps drop it (it's an `ashpd` detail that's easy to miss)
- The cache layout under `~/.cache/apexshot/` and how stale tokens are
  detected and refreshed
- Compositor-specific quirks you hit (mutter vs kwin vs sway behaviour)
- Link to the actual implementation in the repo

End with one line about the project: *"This is part of ApexShot, an
open-source screen capture tool for Linux I've been building.
Repo: github.com/apex-shot/apexshot"*

**B2 — Tiered capture backend**

Title:
```
Show HN: A tiered Linux screen-capture backend (wlr-screencopy → grim → portals)
```

Same structure as B1, but the subject is the fallback tree itself: when
each backend works, why ordering matters, what `WAYLAND_DISPLAY` /
`XDG_CURRENT_DESKTOP` tells you reliably (and what it doesn't), and the
benchmarks for each path.

**B3 — Spawning a Qt5/C++17 region-selector from a Rust process**

Title:
```
Show HN: Why our Rust desktop app spawns a separate Qt5 binary for region selection
```

Discuss the IPC contract between `apexshot` (Rust) and
`apexshot-capture` (Qt5/C++17), why GTK4 layer-shell wasn't enough for
the selector, the tradeoffs of multi-binary desktop apps, and how
crashes are isolated.

### Mandatory self-comment (post within 60 seconds of submission)

This is the single biggest lever for a Show HN escaping `/newest`.
Pre-write it and paste it the moment the post is live:

```
Author here. Happy to answer anything technical.

The part that took the longest by far was the Wayland capture stack —
specifically getting the portal restore_token persisted correctly so
users don't get re-prompted on every single capture. That detail is
documented almost nowhere; I had to read the xdg-desktop-portal source
to figure out the exact lifecycle.

If you want to look at one file, [link to the most interesting file,
e.g. src/backend/wayland_portal.rs or similar] is probably the one.

Known rough edges:
- Alpha. Primary testing is GNOME 47–49 / Ubuntu Wayland.
- KDE / Sway / X11 paths exist but I have not beaten them up.
- Keystroke overlay during recording is feature-flagged off until the
  recorder side lands.
```

### HN posting hygiene checklist

- [ ] **Title format**: `Show HN: ` prefix, ≤ 80 chars, no questions, no marketing adjectives.
- [ ] **Submit Tue–Thu, 06:30–08:30 PT (13:30–15:30 UTC)**. Outside that window the post competes with the daily flood and rarely surfaces.
- [ ] **First comment from you within 60 seconds** of submission. Pre-write it. This is non-negotiable.
- [ ] **Reply to every comment within 5 minutes** for the first 2 hours. HN explicitly weights author engagement velocity.
- [ ] **No emojis in title or body.** None. Not one. HN strips them visually but the audience reads them as red flags.
- [ ] **No hype words** in the body: avoid "polished", "modern", "powerful", "blazing fast", "the future of", "all-in-one", "experience".
- [ ] **No bulleted feature list at the top.** Lead with prose. A short, factual list at the bottom is fine.
- [ ] **Submit the GitHub URL or the website URL — not both.** A single canonical link. Repo URL converts better for technical audiences.
- [ ] **Have the README polished before submission.** First-time HN visitors land on the README; first 3 seconds determine whether they star or close.
- [ ] **Do not cross-post to Reddit on the same day.** Concentrate engagement on HN for the first 6 hours; cross-post in the evening or the next day.
- [ ] **If the post is dead in `/newest` after 30 minutes**, do not delete and resubmit — that pattern gets accounts shadowbanned. Email `hn@ycombinator.com` once politely asking for a second-chance pool review; dang occasionally rescues good Show HNs that mistimed.

---

## OMG! Ubuntu! Submission

Subject: ApexShot – The CleanShot X alternative Linux has been waiting for

I'd like to submit ApexShot for coverage on OMG! Ubuntu!. It's a screen capture tool that brings the CleanShot X experience to Linux — something Ubuntu users have been asking for whenever that macOS tool gets mentioned.

Key Features:
- Multiple capture modes (full screen, area, window, crosshair)
- Built-in annotation editor with arrows, shapes, text, blur, pixelate, and highlighter
- Screen recording to MP4/GIF with audio monitoring and webcam PiP
- Dual-engine OCR (Tesseract + ocrs) for text extraction from screenshots
- QR code detection and auto-copy
- GNOME Shell extension for always-on-top previews and recording masks
- Browser extension for full-page web capture
- Daemon mode with system tray and global hotkeys
- Easy installation via deb package

Tech: Built with Rust, C++/Qt5, GTK4, and GStreamer. Currently in alpha (v0.2.25), tested on GNOME Ubuntu Wayland.

Website: https://apexshot.org/
GitHub: https://github.com/apex-shot/apexshot

---

## It's FOSS Submission

Subject: ApexShot – The open-source CleanShot X alternative for Linux

I'd like to introduce ApexShot, an open-source screen capture tool for Linux that aims to be what CleanShot X is for macOS — an all-in-one capture, annotate, record, and OCR solution.

Features:
- Screenshots with annotation editor (arrows, shapes, text, blur, pixelate)
- Screen recording to MP4/GIF with audio monitoring and webcam PiP
- Dual-engine OCR (Tesseract + ocrs) for text extraction
- QR code detection and auto-copy
- GNOME Shell extension for always-on-top previews and recording masks
- Browser extension for full-page scroll capture
- Daemon mode with system tray and global hotkeys
- Deb package for easy installation on Debian/Ubuntu

Tech Stack: Rust core, C++/Qt5 overlay, GTK4, GStreamer, PipeWire

Status: Alpha (v0.2.25), tested on GNOME Ubuntu Wayland

Website: https://apexshot.org/
GitHub: https://github.com/apex-shot/apexshot

---

## GNOME Discourse

Title: ApexShot – CleanShot X-style screen capture with deep GNOME Shell integration

macOS has CleanShot X. I think GNOME can do even better — by leveraging the shell for things macOS can't.

ApexShot is a screen capture tool with deep GNOME Shell integration that goes beyond what's possible on other platforms:

GNOME Extension Features:
- Always-on-top preview windows during annotation
- Shell-managed recording masks for area selection
- Runtime overlays for click animations during recording
- Window tracking via D-Bus signals

Application Features:
- Screenshots with full annotation editor (arrows, shapes, text, blur, pixelate)
- Screen recording to MP4/GIF with audio monitoring and webcam PiP
- Dual-engine OCR (Tesseract + ocrs) for text extraction
- QR code detection and auto-copy
- Browser extension for full-page scroll capture
- Daemon mode with system tray and global hotkeys

Built with Rust, GTK4, and GStreamer. Extension supports GNOME 45-49.

Looking for feedback from the GNOME community — especially on the extension integration model.

Website: https://apexshot.org/
GitHub: https://github.com/apex-shot/apexshot

---

## Twitter/X Thread

Tweet 1:
Linux users have been asking for a CleanShot X alternative for years. So I built one.

Introducing ApexShot — an open-source screen capture tool for Linux with annotation, recording, OCR, and QR detection.

Website: https://apexshot.org/
GitHub: https://github.com/apex-shot/apexshot

#Linux #CleanShotX #OpenSource

Tweet 2:
What ApexShot does that your current screenshot tool doesn't:

- Full annotation editor (arrows, shapes, text, blur, pixelate, highlighter)
- Screen recording to MP4/GIF with audio monitoring and webcam PiP
- Dual-engine OCR — screenshot text, copy it instantly
- QR code detection and auto-copy
- GNOME Shell extension for always-on-top previews and recording masks

#Linux #GNOME

Tweet 3:
Built with Rust, C++/Qt5, GTK4, and GStreamer. Deep GNOME Shell integration — recording masks, click animations, and window tracking via D-Bus.

Currently in alpha, tested on GNOME Ubuntu Wayland. Looking for testers!

Try it: https://apexshot.org/

#Rust #GTK4 #LinuxDev

---

## Mastodon/Fediverse

Post:
Linux users have been jealous of CleanShot X on macOS for years. So I built the open-source alternative.

ApexShot is an all-in-one screen capture tool for Linux:

- Screenshots with annotation editor (arrows, shapes, text, blur, pixelate)
- Screen recording to MP4/GIF with audio monitoring and webcam PiP
- Dual-engine OCR (Tesseract + ocrs) — screenshot text, copy it instantly
- QR code detection and auto-copy
- GNOME Shell extension for always-on-top previews and recording masks
- Browser extension for full-page scroll capture
- Daemon mode with system tray and global hotkeys

Built with Rust, C++/Qt5, GTK4, and GStreamer. Currently in alpha (v0.2.25), tested on GNOME Ubuntu Wayland.

Website: https://apexshot.org/
GitHub: https://github.com/apex-shot/apexshot

#Linux #GNOME #OpenSource #Rust #CleanShotX

---

## LinkedIn Post

Linux users have watched macOS enjoy CleanShot X for years while cobbling together Flameshot + OBS + OCR scripts on their end. I decided to fix that.

I've been building ApexShot, an open-source screen capture tool for Linux that brings the CleanShot X experience to the Linux desktop:

- Full annotation editor (arrows, shapes, text, blur, pixelate, highlighter)
- Screen recording to MP4/GIF with webcam PiP and audio monitoring
- Dual-engine OCR (Tesseract + ocrs) for text extraction
- QR code detection and auto-copy
- Deep GNOME Shell integration — always-on-top previews, recording masks, click animations
- Browser extension for full-page scroll capture
- Daemon mode with system tray and global hotkeys

Tech stack: Rust, C++/Qt5, GTK4, GStreamer, PipeWire

Currently in alpha (v0.2.25), tested on GNOME Ubuntu Wayland.

Website: https://apexshot.org/
GitHub: https://github.com/apex-shot/apexshot

#OpenSource #Linux #Rust #GNOME #CleanShotX

---

## Product Hunt

Headline: ApexShot – The CleanShot X alternative for Linux

Tagline: Open-source screen capture for Linux with annotation, recording, OCR, and QR detection — finally

Description:
macOS has CleanShot X. Linux has been stuck with fragmented tools that each do one thing. ApexShot brings it all together in one open-source app:

- Multiple capture modes (full screen, area, window, crosshair)
- Annotation editor with arrows, shapes, text, blur, pixelate, and highlighter
- Screen recording to MP4/GIF with audio monitoring and webcam PiP
- Dual-engine OCR (Tesseract + ocrs) for text extraction
- QR code detection and auto-copy
- GNOME Shell extension for always-on-top previews and recording masks
- Browser extension for full-page scroll capture
- Daemon mode with system tray and global hotkeys

Built with Rust, C++/Qt5, GTK4, and GStreamer. Currently in alpha, tested on GNOME Ubuntu Wayland.

Website: https://apexshot.org/
GitHub: https://github.com/apex-shot/apexshot

---

## Low-Friction Alternatives for New / Low-Karma Accounts

The big subs (`r/linux`, `r/programming`, `r/rust` for top-level posts,
`r/Ubuntu`) have account-age and karma minimums that are mostly
undocumented and enforced by AutoMod \u2014 your post can vanish with no
notification. The list below is the set of channels that are reliably
permissive about new accounts, ordered roughly by audience fit for
ApexShot.

### Reddit (low to no karma threshold)

| Subreddit | Why it fits | Notes |
|-----------|-------------|-------|
| **r/coolgithubprojects** | Built for exactly this kind of post. | First-person descriptive title. Flair after posting. |
| **r/SideProject** | Indie / hobby projects, very welcoming. | Lead with what you built and why. No karma gate in practice. |
| **r/LinuxApps** | Purpose-built for announcing Linux desktop apps. | Small but on-target audience. Almost no gating. |
| **r/FOSS** | Sister sub to r/opensource, friendlier to newer accounts. | License + repo link up front. |
| **r/SomebodyMakeThis** \u2192 cross-link from there | If you find a "someone make a CleanShot for Linux" thread, replying with the repo is allowed and high-signal. | Don't post top-level promo. |
| **r/Open_Source** | Mirrors r/opensource with looser rules. | Lower traffic but no karma checks. |
| **r/unixporn** | Only if you have a stunning screenshot/GIF. | Visual-first; describe distro/DE in body. |
| **r/swaywm**, **r/kde**, **r/hyprland**, **r/pop_os**, **r/Fedora**, **r/archlinux**, **r/EndeavourOS** | Niche, lenient. | Only post in the ones you've actually tested on; mention compositor-specific bits up front. |
| **r/linuxmasterrace** | Permissive, but tone-sensitive. | Don't lead with feature lists \u2014 they want personality. |
| **r/selfhosted** | Borderline fit (ApexShot isn't a server), but the audience overlaps with privacy-minded users. | Frame around \"local-first, no telemetry\" if posting here. |

For the strict subs (`r/linux`, `r/Ubuntu`, top-level r/rust), the
realistic path is: post to the lenient ones first, accumulate ~50\u2013100
sub-specific karma over a couple of weeks, then post to the strict ones
last. Posting to all of them on day one mostly results in silent
removals.

### Non-Reddit channels (no karma model at all)

- **Hacker News (Show HN)** \u2014 only requires an account; the front-page
  algorithm cares about timing and substance, not history. Title format
  is fixed: `Show HN: ApexShot \u2013 ...`. Keep the body short, link to
  the repo, expect technical scrutiny.
- **Lobsters** \u2014 invite-only, but if you have an invite the audience is
  high-quality and lenient on submission frequency.
- **dev.to** \u2014 write a build-log style article (e.g. "How I made the
  preview overlay appear instantly by moving PNG decode off the main
  thread"). Articles cross-link to the repo naturally.
- **Phoronix Forums** (`Linux Software` board) \u2014 long-form, slow-moving,
  but the readership genuinely tries Linux apps. No karma gate.
- **GNOME Discourse** (`Applications` category) \u2014 already covered
  below. Permissive registration, GNOME-focused audience.
- **Mastodon / Fediverse** \u2014 hashtags `#FOSS`, `#Linux`, `#Rust`,
  `#GNOME`, `#OpenSource`. Boost from accounts in those circles is
  often more impactful than a Reddit hit.
- **GitHub `awesome-rust` / `awesome-selfhosted` / `awesome-linux-apps`** \u2014
  open a PR adding ApexShot once it has a release tag and a demo GIF.
  Long-tail traffic, no gating.
- **dev / launch aggregators**: `dang.ai`, `producthunt.com`,
  `betalist.com`, `indiehackers.com`. Each accepts new accounts;
  Product Hunt is most impactful but timing-sensitive.
- **YouTube creator outreach**: short, factual email to channels that
  cover Linux desktop apps (TheLinuxExperiment, Brodie Robertson,
  DistroTube). They occasionally cover indie projects with no PR.

### Posting hygiene that helps you survive AutoMod

- **Post from a logged-in account that's at least a few days old, with
  a couple of innocuous comments** before you make a project post. New
  accounts hit shadow filters even on permissive subs.
- **Don't post the same link to >2 subs in a 24 h window** \u2014 Reddit's
  cross-post detection is aggressive and most subs auto-remove.
- **Reply to comments fast.** Engagement in the first 30 minutes is
  what gets you out of New into Hot.
- **Visuals.** Every post above benefits from `example.gif` (or a
  fresh GIF) attached as the post media, not as a link in the body.

---

## Recommended Launch Order

Start with these platforms in this order:

1. **Hacker News** (Day 1, morning UTC)
   - Largest tech audience, can drive significant traffic
   - Technical users appreciate the Rust architecture
   - Post early in the day for maximum visibility

2. **Reddit - r/linux** (Day 1, afternoon UTC)
   - General Linux users, broad audience
   - Good for initial feedback and bug reports
   - Post 4-6 hours after HN to avoid overlap

3. **Reddit - r/opensource** (Day 1, evening UTC)
   - Open-source community, values FOSS alternatives to proprietary tools
   - CleanShot X comparison resonates — proprietary vs open-source angle
   - Potential contributors and advocates

4. **Reddit - r/gnome** (Day 2, morning UTC)
   - Targeted GNOME users, your primary audience
   - GNOME community provides valuable technical feedback
   - Extension-specific discussions

5. **Reddit - r/Ubuntu** (Day 2, afternoon UTC)
   - Ubuntu/Debian users, your main installation target
   - Deb package users will test installation flow
   - Practical feedback on package management

6. **Reddit - r/rust** (Day 3, morning UTC)
   - Rust developers interested in the tech stack
   - Potential contributors and technical discussions
   - Good for architecture feedback

7. **GNOME Discourse** (Day 3, afternoon UTC)
   - Official GNOME community forum
   - Extension developers and power users
   - Deep technical discussions about integration

8. **Twitter/X** (Day 4, spread throughout day)
   - Post the thread over 2-3 hours
   - Tag relevant accounts (@GNOME, @ubuntude, @rustlang)
   - Engage with replies and retweets

9. **Mastodon/Fediverse** (Day 4, evening UTC)
   - Technical audience on linux.social and fosstodon.org
   - More in-depth discussions possible
   - Cross-reference with Twitter post

10. **LinkedIn** (Day 5, morning UTC)
    - Professional network, potential job opportunities
    - Showcase development skills
    - Different audience than other platforms

11. **Product Hunt** (Day 6 or later)
    - Requires preparation (screenshots, demo video)
    - Launch when you have time to engage all day
    - Can drive significant traffic if it trends

12. **OMG! Ubuntu!** (Submit after Reddit traction)
    - Email submission to editors
    - They may pick it up if there's community interest
    - Timing depends on their editorial calendar

13. **It's FOSS** (Submit after OMG! Ubuntu! or independently)
    - Email submission to editors
    - Similar to OMG! Ubuntu!, editorial discretion
    - Can submit anytime, but better with existing traction

## Launch Timing Strategy

- Weekday mornings (Tue-Thu, 9-11 AM UTC) for Reddit/HN
- Stagger posts across platforms (1-2 days apart to maximize reach)
- Engage quickly with comments/questions in first 24 hours

## Pre-Launch Checklist

- Ensure deb package installs smoothly
- Test GNOME extension installation flow
- Have demo GIF and screenshots ready
- Prepare installation troubleshooting FAQ
- Set up GitHub Discussions for support
- Create issue templates for bug reports
