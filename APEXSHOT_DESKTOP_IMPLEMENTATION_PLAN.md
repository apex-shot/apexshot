# ApexShot Desktop — Distribution-First and Team Business Implementation Plan

Status: Proposed  
Prepared: 2026-08-02  
Scope: `apexshot` (desktop) only  
Sibling plan: `../apexshot-cloud/APEXSHOT_CLOUD_IMPLEMENTATION_PLAN.md`  
Source plan: `../apexshot-cloud/DISTRIBUTION_AND_TEAMS_IMPLEMENTATION_PLAN.md`  
Excluded: `t3code/` (unrelated product)

This document is the desktop half of the distribution-first plan. Cloud funnel,
Team leads, measurement scorecard, workspace API/web UI, billing, domains, and
SSO live in the sibling cloud plan. Shared product decisions and gates are
repeated here so this file stands alone.

Detailed original Flatpak audit: `docs/FLATHUB_LAUNCH_PLAN.md`. Some quick wins
in it are now complete, but its sandbox blockers remain valid.

---

## 1. Executive Decision

ApexShot will not try to reach meaningful revenue by adding more features to the
$5/month individual storage plan. The near-term sequence is:

1. Fix the current acquisition and measurement gaps (**cloud** + desktop channel
   markers / truthful packaging claims).
2. Publish ApexShot in the Linux channels that already exist or are nearly ready.
3. Complete a deliberately reduced, portal-only Flatpak and submit it to Flathub.
4. Validate demand for a `$10-20/seat/month` team product before building it
   (**cloud discovery**).
5. Build one small team workspace product only after named pilot teams commit
   (**cloud API/UI first; desktop selector after**).
6. Add retention controls, paid seats, custom domains, and SSO only when each has
   direct customer evidence (**cloud**).

The working team price hypothesis is **$15 per seat per month**, tested during
discovery rather than hard-coded before validation. Existing Free and Pro Cloud
customers remain supported, but individual storage receives maintenance and
reliability work only.

### Desktop-owned outcomes

- Release hardening and next native tagged release with AppStream metadata.
- AUR publication of the existing package.
- GNOME extension (EGO) submission and evidence.
- Portal-only Flatpak and Flathub submission.
- Installer/package channel markers and attribution forwarding.
- Desktop Personal/workspace selector once cloud workspace APIs exist.
- Native package install/update/uninstall verification.

---

## 2. Why This Sequence

- Approximately 958 historical release downloads cannot support 200 individual
  subscribers at a realistic sub-1% FOSS conversion rate.
- The desktop application already has a large surface area. Another editor or
  capture feature does not solve discoverability.
- AUR, extensions.gnome.org, package repositories, and especially Flathub put the
  application where Linux users already look.
- ApexShot competes with free storage and its own XBackBone integration. Storage
  alone is not a strong reason for a Linux power user to pay.
- Teams can pay for coordination and administration: a shared library, centralized
  retention, controlled membership, branded sharing, and SSO (**cloud product**).
- The existing cloud schema already contains `organizations`,
  `organization_members`, and `uploads.organization_id`. Desktop only needs to
  send an optional workspace destination once that product exists.

---

## 3. Validated Current State (Desktop)

- GitHub releases currently ship Debian, Fedora, and Arch artifacts for x86_64.
- Package metadata for AUR exists, but no live AUR package is published.
- A GNOME extension release ZIP exists, but there is no approved EGO listing.
- AppStream metadata exists in source but was added after the latest published
  release and has not shipped in a release artifact.
- There is no Flatpak manifest or Flathub submission.
- The application currently depends on host commands, direct portal permission
  manipulation, system installation paths, a Qt/X11 helper, autostart files, and
  browser native messaging. A Flatpak must not preserve those assumptions.
- `docs/FLATHUB_LAUNCH_PLAN.md` contains the detailed original audit. Some quick
  wins in it are now complete, but its sandbox blockers remain valid.

### Baseline caution

The historical 958-download number and first-party telemetry count different
things. Before evaluating a channel, record one baseline table containing
(cloud scorecard consumes these; desktop must emit clean channel/install signals):

- GitHub release asset downloads by artifact and version;
- completed first-party package downloads, excluding extension ZIPs;
- unique install IDs where available;
- active installs over 1, 7, and 30 days;
- first heartbeat with non-zero capture activity;
- active Pro subscriptions and normalized MRR;
- qualified team leads and committed pilot seats.

Do not combine these into one "downloads" number.

---

## 4. Product Principles

1. **Distribution before feature volume.** Acquisition work outranks new desktop
   capability until the active-install base materially changes.
2. **Pro remains supported, not expanded.** Fix security, billing, data-loss, and
   reliability issues; skip speculative storage features.
3. **Personal and team data stay separate.** Existing uploads remain personal.
   Users explicitly choose a workspace for future uploads.
4. **Portals, not sandbox escapes.** The Flatpak will accept limited functionality
   instead of using `flatpak-spawn --host` or broad filesystem permissions.
5. **Measure decisions, not vanity.** The key funnel is distribution channel to
   activated install to retained use to qualified team demand to paid seats.

(Cloud tenancy principles — one org = one workspace, pilot before platform,
server-side enforcement — live in the sibling plan; desktop must respect them by
never inventing client-side authorization.)

---

## 5. Explicit Non-Goals (Desktop-Relevant)

- No Windows or macOS build in this plan.
- No AppImage before AUR, EGO, and Flathub work. Reconsider only if users on an
  unsupported distribution create measurable demand.
- No Snap package unless a maintainer or a measured channel justifies it.
- No browser native messaging in Flatpak v1.
- No broad third-party product analytics SDK.
- No modification of `t3code/`.
- No cloud Team lead form, workspace API, billing, domains, or SSO implementation
  in this repo (cloud plan).

---

## 6. Decision Gates

These thresholds are initial operating rules. Change them only in a written weekly
decision record, not because a feature is interesting to build.

| Decision | Go threshold | If the threshold is missed |
| --- | --- | --- |
| Phase 0 complete | Public claims are truthful, Team leads persist, internal metrics are protected, the next native release passes package checks, AUR is live, and EGO is submitted | Do not run a coordinated launch |
| Continue Flatpak-only feature work | 250 Flathub installs or first-run devices within 30 days of listing | Maintain the listing, but stop Flatpak-only enhancements |
| Build workspace MVP | 10 qualified discovery calls, 3 committed pilot organizations, at least 15 expected seats, and 2 teams explicitly willing to pay at least `$10/seat/month` | Continue interviews; do not build tenancy UI |
| Resume individual Pro roadmap | At least 5,000 monthly active desktop installs and 25 active Pro subscriptions | Keep Pro in maintenance mode |

Desktop owns: next native release package checks, AUR live, EGO submitted,
Flatpak/Flathub work, channel markers. Cloud owns Team lead persistence, metrics
protection, truthful marketing copy, and Team product gates. Phase 0 complete is
a joint gate.

### Definitions

- **Activated install:** an install that reports at least one screenshot,
  recording, or OCR action within 7 days of its first known install/download event.
- **D30 retained install:** an activated install with a heartbeat between days 22
  and 30 after activation.
- **Qualified lead / committed pilot / weekly active workspace / MRR:** defined in
  the cloud plan; desktop does not own Team pipeline data.

---

## 7. Delivery Overview (Desktop)

| Phase | Calendar target | Result | Dependency |
| --- | --- | --- | --- |
| 0B. Native quick wins | Days 1-5 | Next release, AUR publication, EGO submission | Release hardening |
| 0C. Channel/attribution emit | Days 2-5 | Installer + heartbeat channel markers | Cloud accepts new fields |
| 1. Flathub | Weeks 1-4 plus review latency | Portal-only Flatpak submitted | Stable native release and app identity decision |
| 3. Desktop workspace support | After cloud workspace API | Personal/workspace selector and scoped upload/list | Discovery gate + cloud MVP |

Flathub and Team discovery (cloud) run in parallel. Neither blocks Phase 0B native
distribution work.

---

## 8. Phase 0B: Native Distribution Quick Wins

### 8.1 Release hardening before promotion

Do not promote another package until one tagged release contains the current
AppStream metadata and is built from the same commit that passed checks.

In `.github/workflows/release.yml`:

- run format, Clippy, and tests for tags instead of skipping them;
- make package jobs depend on those checks;
- validate AppStream and desktop files;
- build Debian against the oldest claimed supported Ubuntu baseline;
- publish user-facing SHA-256 checksums;
- pin mutable build inputs such as `gtk4-layer-shell`, `cargo-deb`, and container
  images where practical;
- make absent AUR credentials visibly skip/fail the publishing expectation rather
  than returning a misleading green publication result.

Before tagging:

- choose one exact GPL SPDX expression and align Cargo, AppStream, RPM specs, the
  GNOME extension, and documentation;
- state x86_64 support on the download page (**cloud website**) and fail installer
  scripts cleanly on unsupported architectures;
- either build a real openSUSE artifact with a distinct name or remove openSUSE
  from the generic installer until it exists;
- exclude prereleases consistently from the website's stable-version resolver
  (**cloud**);
- include useful release notes and known distro limitations.

### 8.2 AUR publication

Reuse the existing package. Do not create both `apexshot` and `apexshot-bin`.

1. Run `scripts/aur-prepare.sh` for the new stable tag.
2. Replace `sha256s=('SKIP')` with the release archive checksum.
3. Regenerate `.SRCINFO` from the final `PKGBUILD`.
4. Build and install in a clean Arch environment.
5. Configure AUR SSH credentials and publish only the package files.
6. Verify the public AUR RPC/package page.
7. Only then update website instructions to `yay -S apexshot` and
   `paru -S apexshot` (**cloud download page**).

Checks:

```bash
cd packaging/arch
makepkg --verifysource --nobuild
makepkg --printsrcinfo > .SRCINFO
makepkg -sf
```

### 8.3 GNOME extension submission

Use the reduced five-file extension already prepared in `gnome-extension/`.

1. Run shexli, JS syntax checks, GJS tests, Rust tests, and Clippy.
2. Test mask, window picker, preview stacking, disable, and re-enable behavior on
   supported GNOME versions 48, 49, and 50.
3. Capture fresh screenshots required by the submission guide.
4. Build the ZIP from a clean checkout.
5. Submit version 4 to extensions.gnome.org.
6. Keep the release ZIP instructions until approval; switch the website to EGO
   only when the listing is live (**cloud**).

### 8.4 Website download truth (cloud-owned, desktop supplies facts)

Cloud updates `frontend/app/download/page.tsx`. Desktop must supply accurate:

- direct package artifact URLs and architectures;
- checksums;
- release notes;
- GNOME version support matrix;
- Fedora recording and openSUSE status;
- verified AUR/EGO/Flathub URLs only after each is public.

### 8.5 Non-code distribution tasks

These do not wait for Flathub:

- move release-to-Discord automation into this repository where releases actually
  occur;
- contact Fedora COPR and openSUSE OBS maintainers after the package checks pass;
- coordinate with cloud ops on AlternativeTo, awesome lists, press page, and
  campaign links (those surfaces are cloud/ops-owned).

### 8.6 Done criteria

- The new release passes checks on the exact tagged commit.
- AppStream metadata and the desktop file exist inside every shipped package.
- AUR install, launch, update, and uninstall pass in a clean environment.
- The EGO submission is accepted for review with its required evidence.
- Public download instructions match the artifacts that actually exist (cloud
  page updated from desktop facts).

---

## 9. Phase 0C: Minimum Measurement (Desktop Emit Side)

Cloud owns product_events, team_leads, protected scorecard, and metric definitions.
Desktop must emit the signals those systems join on.

### 9.1 Reuse existing telemetry

Extend the current download and usage telemetry instead of replacing it.

Add to download/install telemetry payloads:

- `attribution_id` (from website env when present);
- `source`, `medium`, `campaign`;
- `distribution_channel`;
- explicit install/update intent.

Add to usage heartbeat:

- build/runtime distribution channel: `github-deb`, `github-rpm-fedora`,
  `github-rpm-opensuse`, `github-arch`, `aur`, `flathub`, or `source`.

Keep the same random install ID through installer telemetry, desktop heartbeat,
and device linking where the user later signs in. Do not send filenames, capture
content, window titles, or account email in telemetry.

### 9.2 Installer attribution

The website's copied install command can pass `APEXSHOT_ATTRIBUTION_ID`, source,
and campaign environment variables to the existing installer. Installer scripts
forward them in current telemetry requests. README/direct installs use `direct`.

Do not rely on AUR or Flathub download counts alone. Embed the distribution channel
at package build time and use opted-in first-run/heartbeat data as the comparable
cross-channel signal.

### 9.3 Metric contracts desktop must honor

- **Download completion:** completed package downloads divided by package download
  starts. Extension downloads are excluded.
- **Activation:** first non-zero capture/recording/OCR usage, not any heartbeat.
- **D7 activation / D30 retention:** as defined in the cloud plan.

Verify whether desktop usage counters are deltas or lifetime totals before cloud
sums them. Store/document that contract explicitly in desktop telemetry code and
cloud docs.

### 9.4 Privacy alignment

- Flatpak telemetry is off by default until the user explicitly opts in.
- Keep telemetry disclosure synchronized across privacy page (**cloud**),
  installer, desktop settings, and Flatpak onboarding.
- Never log access tokens, screenshot names, or capture content at info level.

### 9.5 Acceptance criteria (desktop slice)

- A website install can be joined from campaign to completed package download to
  first capture without storing personal content.
- Legacy events remain visible under `unknown` rather than disappearing.
- Channel is embedded at build/package time for comparable cross-channel reporting.

---

## 10. Phase 1: Flathub Portal-Only Build

Target 2-4 focused engineering weeks plus Flathub review latency. The shortest
credible build is not feature parity; it is a secure Flatpak with core capture,
annotation, recording, local save, and optional Cloud upload.

### 10.1 Pre-work decisions

1. Choose the permanent application ID. The audited recommendation is
   `org.apexshot.ApexShot`, backed by control of `apexshot.org`.
2. Apply the app ID consistently to desktop metadata, icons, D-Bus names, package
   files, GNOME matching, and tests.
3. Keep old desktop/icon aliases for two native releases and document the expected
   one-time portal permission re-prompt.
4. Resolve license SPDX inconsistencies before submission.
5. Mark a release stable rather than public beta when it is supportable as a
   Flathub stable application.

### 10.2 Compile-time Flatpak mode

Add a `flatpak` Cargo feature and a small runtime `portal_only()` check.

In portal-only mode:

- [x] do not compile the Qt5/X11 capture helper;
- [x] hide or reject host install/uninstall/package-detection commands;
- [x] do not call package managers, `grim`, `wf-recorder`, `hyprctl`, `swaymsg`,
  `gnome-extensions`, `dbus-send`, `gdbus`, `gtk-launch`, `wl-copy`, `xclip`, or
  `notify-send`;
- [x] do not write `/usr`, `/etc/xdg/autostart`, browser native-host directories, or
  host compositor configuration;
- [x] do not call the portal implementation PermissionStore directly.

Primary paths:

- `Cargo.toml`
- `build.rs`
- `src/app_identity.rs`
- `src/main.rs`
- `src/daemon/mod.rs`
- `src/backend/portal_permissions.rs`
- `src/recording/mod.rs`
- `src/hotkeys/mod.rs`

First gate: `cargo build --release --features flatpak` succeeds on a system without
Qt5, `grim`, `wf-recorder`, or distro package tools.

**Status (2026-08-03): COMPLETE.**
`flatpak` feature + `portal_only()` + `host_escape_blocked()`. Qt helper skipped;
PermissionStore no-op; install/uninstall/native-host rejected; hotkey host writes,
wf-recorder, dbus-send/gdbus/gtk-launch/hyprctl/swaymsg/gnome-extensions gated;
clipboard uses arboard in portal-only; notify uses D-Bus only; system Tesseract
optional (`tesseract-ocr` feature). Verified:
`cargo build --no-default-features --features flatpak`.

### 10.3 Portal/toolkit replacements

| Existing behavior | Flatpak behavior | Status |
| --- | --- | --- |
| Screenshot paths | XDG Screenshot portal | **done** — forced first under `portal_only`; native KDE/wlr skipped |
| Recording | XDG ScreenCast portal plus PipeWire | **done** — `wf-recorder` gated; existing portal path used |
| Hotkeys | GlobalShortcuts portal only | partial — host install gated; portal daemon already exists |
| Autostart/background | Background portal with explicit consent | **done** — ashpd Background portal under portal-only |
| Open URL/file | GTK `UriLauncher` / `FileLauncher` | **done** — `utils::open` via GIO (portal OpenURI; works off main thread) |
| Clipboard | GTK/GDK clipboard | **done** — arboard in portal-only (no wl-copy/xclip) |
| Notifications | Notification portal | **done** — D-Bus Notifications only under portal-only |
| Theme/settings | GTK settings or Settings portal | todo |
| GNOME extension install | Link to approved EGO page | **done** — opens URL, no host install |
| GNOME extension IPC | Typed `zbus` calls with narrowly scoped D-Bus permissions | todo — currently disabled under portal-only |
| Browser full-page capture | Unsupported and documented in Flatpak v1 | todo (doc) |
| Telemetry | Off by default until the user explicitly opts in | **done** — `telemetry_enabled: false` under `flatpak` feature |

### 10.4 Manifest and dependencies

Create under `flatpak/`:

- [x] `org.apexshot.ApexShot.yml` (GNOME 49, portal finish-args, no host FS);
- [ ] generated `cargo-sources.json` from `Cargo.lock` (Flathub offline requirement);
- [x] pinned `gtk4-layer-shell` module;
- [x] Tesseract optional — Flatpak uses pure-Rust `ocrs` (`--no-default-features --features flatpak`);
- [ ] OCR models bundled (currently downloaded at runtime with network);
- [ ] FFmpeg/platform extension if editor encode needs it beyond runtime GStreamer.

**Status (2026-08-03): MANIFEST SKELETON LANDED.** Local dir-source build path
documented in `flatpak/README.md`. App ID under `flatpak` feature is
`org.apexshot.ApexShot`. Next: install SDKs and run `flatpak-builder`, then
cargo-sources for Flathub.

Request only the permissions proven necessary during testing: Wayland, fallback
X11/IPC if required, DRI, network for Cloud, audio for recording, scoped XDG
Pictures/Videos access, StatusNotifierWatcher if the tray remains, and specific
ApexShot extension D-Bus names. Never request `--filesystem=host`.

Use the latest supported non-EOL GNOME runtime at submission time rather than
copying a stale runtime version from an old plan.

### 10.5 Verification matrix

| Environment | Required flow |
| --- | --- |
| GNOME 48/49/50 Wayland | install, first run, screenshot, edit, clipboard, hotkey, background consent, recording, Cloud login/upload |
| KDE Plasma Wayland | screenshot selector, recording, tray, settings, local save |
| wlroots compositor | core portal capture/recording and explicit limitation handling |
| No GNOME extension | core capture and annotation remain usable |
| Offline | capture, edit, OCR if bundled, and local save work |
| Portal denial | clear recoverable error; no loop or permission bypass |
| Telemetry disabled | no telemetry request occurs |
| Upgrade from native config | settings survive; documented portal re-prompt only |

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --jobs 2 -- --test-threads=1
appstreamcli validate --explain packaging/org.apexshot.ApexShot.metainfo.xml
desktop-file-validate packaging/org.apexshot.ApexShot.desktop
flatpak-builder --user --install --force-clean build-dir flatpak/org.apexshot.ApexShot.yml
flatpak run org.apexshot.ApexShot
```

Also run Flathub manifest, AppStream, and repository lints, inspect effective
permissions, and log session-bus access.

### 10.6 Submission gate

- Core matrix passes.
- No Qt dependency in the Flatpak build.
- No host command is attempted in normal portal-only workflows.
- No direct PermissionStore mutation.
- No broad filesystem permission or host escape.
- Telemetry is off by default.
- Browser-extension limitations are explicit.
- Metadata screenshots meet Flathub quality guidance.
- The manifest pins a fresh stable upstream release and checksum.
- Domain verification is ready at
  `apexshot.org/.well-known/org.flathub.VerifiedApps.txt` or DNS
  (**cloud/ops hosts the verification file**).

Current Flathub policy may restrict AI-generated submission material. A maintainer
must independently author the PR description, manifest comments, and review
responses and confirm the current policy before submission. Do not paste this plan
into a Flathub PR.

---

## 11. Phase 3: Desktop Workspace Support

Start only after the workspace gate in the cloud plan passes and cloud APIs exist.

Cloud implements tenancy, invites, authorization, quotas, and web UI. Desktop only
consumes memberships and optional `workspace_id` on upload/list.

### 11.1 Desktop paths

- `src/config.rs` - optional selected workspace ID/name
- `src/cloud/auth.rs` - refresh available memberships
- `src/cloud/apexshot.rs` - send optional workspace ID
- `src/cloud/listing.rs` - list selected scope
- `src/settings/cloud.rs` - Personal/workspace selector
- `src/history/cloud_page.rs` - shared library scope

### 11.2 Desktop behavior

- Personal remains the default for old configs.
- Show a workspace selector only when memberships exist.
- Auto-upload uses the explicitly selected destination and workspace.
- Logout clears workspace selection.
- A removed member refreshes account state, sees an understandable error, and
  falls back to Personal.
- XBackBone remains separate and unchanged.
- Frontend visibility is never authorization; server rejects unauthorized scopes.
- When retention policy ships (cloud Phase 5A), display effective retention in the
  desktop workspace selector.

### 11.3 API contract desktop depends on (cloud-owned)

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/v1/workspaces` | Current user's memberships and role |
| POST | `/v1/uploads` | optional `workspace_id` |
| GET | `/v1/uploads` | optional `workspace_id`; omit = personal |

Old desktop clients remain personal because they omit `workspace_id`.

### 11.4 MVP acceptance criteria (desktop slice)

- Personal Cloud and old desktop clients keep working unchanged.
- Workspace selection/upload/list works when memberships exist.
- Removed member falls back to Personal with a clear error.
- Pilot workspace activity remains measurable without recording content.
- No existing local or R2 object is moved by the desktop client.

---

## 12. Phase 4/5 Touchpoints (Desktop Support Only)

Desktop does not implement pilot ops, retention policy storage, seat billing,
custom domains, or SSO. When those cloud gates pass:

- **Retention (5A):** show effective retention in workspace selector; no local
  policy engine.
- **Billing (5B):** no desktop checkout; entitlement failures surface as normal
  Cloud API errors.
- **Custom domains (5C):** share URLs may change; desktop should display
  server-returned share URLs rather than hard-coding `apexshot.org/s/...` if it
  currently constructs them.
- **SSO (5D):** use existing WorkOS/device-link login paths; no custom SAML in
  desktop.

---

## 13. Security and Privacy Checklist (Desktop)

- Do not attempt host escapes or PermissionStore mutation in Flatpak.
- Never request `--filesystem=host`.
- Telemetry off by default in Flatpak; opt-in only.
- Do not send filenames, capture content, window titles, or account email in
  telemetry.
- Never log access tokens or screenshot content at info level.
- Keep telemetry disclosure synchronized with cloud privacy page, installer, and
  Flatpak onboarding.
- Workspace selection is convenience only; server authorizes every request.
- On membership loss, clear selection and fall back to Personal.
- Domain verification file for Flathub is hosted on `apexshot.org` (cloud/ops).

---

## 14. Test Plan (Desktop)

### Desktop native

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --jobs 2 -- --test-threads=1
```

Verify native package install/update/uninstall, Personal Cloud compatibility,
workspace selection/upload/removal fallback, and channel telemetry.

### Flatpak

Use the matrix in Section 10.5 plus Flathub lints, effective permission inspection,
and session-bus logs. Test with telemetry disabled and all relevant portal prompts
denied once.

### AUR / EGO

- Clean Arch: install, launch, update, uninstall for AUR package.
- GNOME 48/49/50: mask, window picker, preview stacking, disable, re-enable.
- EGO submission evidence package complete.

---

## 15. Deployment and Rollback (Desktop Slice)

### Order relative to cloud

1. Cloud applies migration 014 and lead/event routes (cloud plan).
2. Cloud deploys truthful public pages (cloud plan).
3. **Desktop:** release instrumented native installers/desktop channel marker.
4. Observe at least 7 days before treating activation percentages as stable.
5. **Desktop:** AUR live, EGO submitted, Flathub work continues in parallel with
   cloud discovery.
6. Cloud completes discovery gate and workspace API (cloud plan).
7. **Desktop:** ship workspace selector after cloud authorization matrix passes.
8. Onboard external pilots (cloud ops); desktop is one access path.

### Rollback

- Keep old desktop clients compatible throughout the rollout (omit `workspace_id`
  = personal).
- Hide or disable workspace selector via config/feature if cloud suspends pilots;
  do not delete local user config destructively.
- Flatpak listing can remain published while portal-only enhancements stop if the
  Flatpak gate is missed.

---

## 16. Operating Scorecard (Desktop Inputs)

Cloud hosts the weekly scorecard UI. Desktop must keep these inputs healthy:

### Distribution

- installs and activated installs by channel;
- D7 activation and D30 retention by channel;
- package failure rate;
- supported-distro/version mix;
- AUR/EGO/Flathub listing and review status;
- GitHub stars and community joins as context, not success criteria.

Team funnel and individual Cloud MRR are cloud-owned.

---

## 17. Risks and Mitigations (Desktop)

| Risk | Mitigation |
| --- | --- |
| Flathub port consumes the roadmap | Hold portal-only scope; no browser native host or perfect native parity |
| GTK overlay is not adequate on GNOME | Make it the first Flatpak technical spike before manifest polish |
| Telemetry claims damage trust/review | Correct copy first (cloud + desktop); Flatpak default off; publish exact payload |
| AUR/EGO claims get ahead of approval | Website links only after public verification |
| One person becomes the bottleneck | Keep manual pilot operations and a two-role model until revenue funds complexity |
| Desktop workspace ships before API auth is solid | Depend on cloud authorization matrix passing first |

(Cloud tenancy leak, quota race, billing isolation risks are in the sibling plan.)

---

## 18. Ownership and Review Cadence

| Role | Responsibility |
| --- | --- |
| Product/founder | interviews, pricing evidence, pilot commitments, weekly go/no-go decisions |
| Desktop | release hardening, AUR, EGO, Flatpak, channel marker, desktop workspace support |
| Cloud backend (sibling) | leads, metrics, tenancy, authorization, billing, reaper/data safety |
| Cloud frontend (sibling) | public funnel, Team form, protected scorecard, shared-library UI |
| Operations | Dodo/WorkOS/Cloudflare configuration, backups, submissions, customer onboarding |

Weekly 30-minute review:

1. Update the scorecard.
2. Review blockers and data quality.
3. Compare each pending feature with its gate.
4. Choose at most one distribution and one Team objective for the next week.
5. Record go, hold, or stop and the evidence used.

---

## 19. Prioritized Backlog (Desktop)

### Now

- Harden and publish the next native release with AppStream metadata.
- Publish the existing AUR package.
- Complete and submit the EGO package.
- Add minimum channel/activation attribution emit (installer + heartbeat + build
  channel marker).
- Begin the Flatpak GTK/portal-only technical spike.
- Supply accurate download-page facts to cloud (artifacts, checksums, support
  matrix, openSUSE/Fedora status).

### After Phase 0

- Finish portal replacements and Flatpak manifest.
- Run the environment matrix and submit to Flathub.
- Maintain listings; stop Flatpak-only enhancements if the 250-install gate is
  missed.

### After cloud workspace gate + API

- Desktop Personal/workspace selector, scoped upload/list, membership refresh,
  removed-member fallback.

### Explicitly parked

- AppImage, Snap, Windows, macOS;
- individual storage feature expansion (cloud);
- Flatpak browser native messaging;
- multi-workspace UI, advanced roles, guests, folders, comments, audit logs, SCIM;
- desktop-side billing, custom domain admin, SSO admin.

---

## 20. Overall Definition of Done (Desktop Contribution)

This plan has succeeded for desktop when:

1. A new Linux user can discover and install ApexShot from at least AUR and
   Flathub, with EGO available for supported GNOME users.
2. Public package and telemetry claims match actual native and Flatpak behavior.
3. ApexShot can identify which channels produce activated and retained installs
   (desktop emits; cloud reports).
4. If demand passes the gate, pilot teams can securely upload to and browse one
   shared workspace from desktop as well as web.
5. No personal data or existing upload is silently moved into a workspace by the
   client.
6. Flatpak remains portal-only, telemetry-off-by-default, without host escapes.

Team demand evidence, paid seat billing, custom domains, and SSO are cloud-owned
DoD items in the sibling plan.

---

## 21. Cross-Repo Dependencies

| Desktop needs from cloud | Cloud needs from desktop |
| --- | --- |
| Team lead form live before discovery cadence | Channel marker values in heartbeats/install telemetry |
| Stable `/v1` APIs for optional `workspace_id` | Verified AUR/EGO/Flathub URLs before website links |
| Auth/device-link flows remain stable | Installer forwarding of `APEXSHOT_ATTRIBUTION_*` |
| Upload list/create scoped by workspace | Desktop workspace selector for full pilot UX |
| Protected metrics endpoints for scorecard | Aligned telemetry disclosure copy |
| Flathub domain verification hosted on apexshot.org | Accurate package/support facts for download page |

Do not modify `t3code/`.
