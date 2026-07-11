# Plan: Native XBackBone (self-hosted) upload support

**Status: Implemented.** Kept as historical design notes. Live code lives in:

- `src/cloud/{mod.rs, upload.rs, destination.rs, apexshot.rs, auth.rs, xbackbone.rs}`
- Settings UI: `src/settings/cloud.rs` (ApexShot Cloud + XBackBone destination panels)
- Onboarding: `src/onboarding/cloud.rs`
- CLI: `apexshot login` / `apexshot logout`
- Env: `.env.example` (`APEXSHOT_CLOUD_BACKEND_URL`, `APEXSHOT_XBACKBONE_*`)
- Tests: `tests/xbackbone_upload.rs`, `tests/xbackbone_e2e.rs`

Do not treat the “current state of the codebase” snapshot below as accurate —
it describes pre-implementation research for issue #36.

---

Resolves GitHub issue #36 — feature request to add native support for uploading
screenshots and recordings to a self-hosted XBackBone instance from ApexShot.

## Background research (pre-implementation snapshot)

### Codebase state *before* XBackBone landed

- Only **one upload destination** existed at research time: ApexShot Cloud (proprietary REST API).
- Upload code lived in `src/cloud/{mod.rs, upload.rs, auth.rs}`.
  - `upload.rs::upload_file()` is the public entry point (preview overlay + annotation editor).
  - ApexShot Cloud uses an OAuth 2.0 Device Authorization Grant (`auth.rs::login()`),
    accessible via the `apexshot login` CLI subcommand.
  - Upload is two-step: `POST /v1/uploads` (create session) → `PUT uploadUrl` (upload bytes).
  - Tokens auto-refresh on 401/403 via `refresh_access_token()`.
- Config (`src/config.rs`): `AppConfig` has `cloud_*` and `xbackbone_*` fields stored at
  `~/.config/apexshot/config.yml`. `cloud_backend_url` falls back to the
  `APEXSHOT_CLOUD_BACKEND_URL` env var.
- **Post-implementation:** Settings Cloud tab is a real destination picker (ApexShot Cloud + XBackBone), not a waitlist.
- Call sites still go through `is_configured()` + `upload_file()`, which dispatch via `destination.rs`.
- `ureq` is at version `2.10` with the `json` feature; multipart for XBackBone is built in `xbackbone.rs`.

### XBackBone (3.x stable AND 4.x upcoming)

Researched from `github.com/SergiX44/XBackBone` (3.8.2 tag + master/next branch),
`xbackbone.app` docs, and the 3.x source code (`app/routes.php`,
`app/Controllers/UploadController.php`, `app/Controllers/ClientController.php`).

XBackBone has **two incompatible API versions** that ApexShot must support:

| Aspect              | 3.x (3.8.2, current stable, Slim PHP)         | 4.x (upcoming, Laravel + Sanctum)              |
|---------------------|-----------------------------------------------|-------------------------------------------------|
| Endpoint            | `POST /upload`                                | `POST /api/v1/upload`                           |
| Auth                | `token` multipart form field                  | `Authorization: Bearer <token>` header         |
| File field name     | `upload` (first uploaded file, any name ok)  | `file`                                           |
| Success response    | `201 {message:"OK", url, raw_url}`           | `200 {data:{preview_ext_url, raw_url, ...}}`    |
| Share URL field     | `url` (preview page)                          | `preview_ext_url` (preview page)                |
| Error response      | `{message, version}`                          | Laravel error format                             |
| Error codes         | 400/401/404/503/507                           | 401/403/413/422                                  |

- **3.x** (the version most users run today): the token is NOT a Bearer header —
  it's a plain multipart form field named `token`. The file is the first uploaded
  file in the request (field name doesn't matter server-side, but the generated
  ShareX config uses `upload`). Response is flat JSON with `url` (the share page)
  and `raw_url` (direct file). Errors carry a `message` field.
- **4.x** (unreleased, on the `next` branch): Laravel + Sanctum rewrite. Bearer
  token auth, `/api/v1/upload`, `file` multipart field, nested `{data:{...}}`
  response.
- **Strategy:** try 4.x first (`/api/v1/upload` with Bearer); on 404, fall back
  to 3.x (`/upload` with token form field). This auto-detects the version without
  user intervention. `test_connection` uses the same fallback logic.
- Existing CLI/KDE plugin stores config in `~/.config/xbackbone/config` as
  `KEY=value` lines (`XBB_URL`, `XBB_TOKEN`). Auto-detecting this file lets existing
  XBackBone users go zero-config.

### Decisions locked with the maintainer

- **Share URL format**: copy `preview_ext_url` (the preview page) to the clipboard.
- **ApexShot Cloud login UX**: "Connect account" spawns a terminal running
  `apexshot login` (the device-code flow prints a code and waits on stdin).
- **XBackBone token config**: auto-detect `~/.config/xbackbone/config`
  (`XBB_URL`/`XBB_TOKEN`) as defaults on load, overridable in Settings.

## Implementation phases

### Phase 1 — Backend abstraction (`src/cloud/`)

Introduce a destination dispatch layer so the public API
(`is_configured()` / `upload_file()`) routes to the right backend. Call sites stay
unchanged.

1. **New `src/cloud/destination.rs`** — enum + dispatch:
   ```rust
   pub enum Destination { ApexShot, XBackbone }
   impl Destination {
       pub fn from_config(c: &AppConfig) -> Self;
       pub fn is_configured(&self, c: &AppConfig) -> bool;
       pub fn upload(&self, c: &AppConfig, p: &Path) -> Result<UploadResult, UploadError>;
   }
   ```
   `from_config` reads `cloud_destination` (`"apexshot"` | `"xbackbone"`, default
   `"apexshot"`).

2. **Refactor `src/cloud/upload.rs`** — keep `upload_file()` + `is_configured()` as the
   public API (so the existing call sites keep working unchanged), but make them thin
   wrappers that delegate to `Destination::from_config(config)`. Move ApexShot-specific
   logic (`upload_file_with_token`, `refresh_access_token`, the `CreateUploadResponse`
   struct, `normalize_share_url`) into a new **`src/cloud/apexshot.rs`** so `upload.rs`
   becomes a dispatcher and the existing tests move with the moved code.

3. **New `src/cloud/xbackbone.rs`** — dual-version support (3.x + 4.x):
   - `upload(config, path)` tries 4.x first (`POST {url}/api/v1/upload`, Bearer
     auth, `file` multipart field), and on 404 falls back to 3.x (`POST {url}/upload`,
     `token` form field, `upload` file field). This auto-detects the instance version.
   - **4.x path:** parses `{data:{preview_ext_url, raw_url}}`; returns
     `preview_ext_url` (fallback `raw_url`).
   - **3.x path:** parses `{message, url, raw_url}`; returns `url` (the share page).
   - Error mapping differs per version:
     - 4.x: 401→`AuthExpired`, 413→`Server("Quota exceeded")`, 422→`Server("Validation")`.
     - 3.x: 404→`AuthExpired` ("token rejected"), 401→`Server` ("account disabled"),
       503→`Server` ("maintenance"), 507→`Server` ("quota exceeded").
   - `test_connection(config)` uses the same fallback: probe 4.x first; on 404
     probe 3.x. A successful probe means the URL + token are valid.
   - Multipart bodies built manually (ureq 2.x has no multipart feature). Two
     builders: `build_multipart` (file-only, 4.x) and `build_multipart_v3`
     (file + `token` field, 3.x).

4. Register modules in `src/cloud/mod.rs`:
   ```rust
   pub mod apexshot;
   pub mod auth;
   pub mod destination;
   pub mod upload;
   pub mod xbackbone;
   ```

### Phase 2 — Config schema (`src/config.rs`)

Add fields to `AppConfig` after the existing `cloud_*` block (after line 126):

```rust
pub cloud_destination: String,        // "apexshot" | "xbackbone", default "apexshot"
pub xbackbone_url: String,             // e.g. "https://files.example.com"
pub xbackbone_api_token: String,        // Sanctum bearer token
```

- Defaults in `impl Default for AppConfig`:
  - `cloud_destination: "apexshot".to_string()`
  - `xbackbone_url: String::new()`
  - `xbackbone_api_token: String::new()`
- Add a `sanitized()` branch to clamp `cloud_destination` to the two known values
  (fallback `"apexshot"`), mirroring the existing `match` for `quick_access_position`
  (lines 286-289).
- Add env-var fallback in `load_config()` (after the existing `APEXSHOT_CLOUD_BACKEND_URL`
  block, lines 357-361):
  - `APEXSHOT_XBACKBONE_URL` → `xbackbone_url`
  - `APEXSHOT_XBACKBONE_TOKEN` → `xbackbone_api_token`
- Auto-detect `~/.config/xbackbone/config`: if `xbackbone_url` and
  `xbackbone_api_token` are both empty and the file exists, parse `XBB_URL=` and
  `XBB_TOKEN=` lines (simple `KEY=value`, ignore blank/comment lines) and use them as
  defaults. Document this clearly so users know they can override in Settings.

### Phase 3 — Settings UI (`src/settings/cloud.rs`)

Replace the waitlist placeholder with a real Cloud tab. New
`build_cloud_section(config: &AppConfig) -> CloudSettingsWidgets` returns a widgets
struct with clones of all editable widgets (so `save_settings()` can read them on
Save).

Layout:

1. **Destination dropdown** (top) — `ComboBoxText` with two rows:
   - id `"apexshot"`, text "ApexShot Cloud"
   - id `"xbackbone"`, text "XBackbone (self-hosted)"
   Bound to `cloud_destination`. Changing the selection swaps the panel below.

2. **Two conditional panels** (only one visible at a time, toggled by the dropdown's
   `connect_changed`):
   - **ApexShot Cloud panel:**
     - Account status row: if `cloud_user_email` is non-empty, show an avatar circle
       (initial of the email), the email in `.cloud-user-email`, and a **Logout**
       button (`apexshot logout` subprocess + clear local fields). Otherwise show
       "Not connected" + a **Connect account** button that spawns a terminal running
       `apexshot login` (try `x-terminal-emulator -e`, fall back to `gnome-terminal
       --`, `konsole -e`, `xterm -e`).
     - Reuse the `.cloud-avatar` / `.cloud-user-name` / `.cloud-user-email` CSS
       classes from `ui_support.rs:1160-1176`.
   - **XBackbone panel:**
     - **Instance URL** `Entry` (placeholder `https://files.example.com`). Tooltip
       explains to point at the XBackBone root, not the API path (we append
       `/api/v1/upload` ourselves).
     - **API token** `Entry` with visibility toggled off by default (use
       `gtk4::Entry` with `set_visibility(false)` plus a small reveal toggle button,
       or `PasswordEntry` if available in the gtk4 version pinned).
     - Helper text: "Generate a token in your XBackBone instance → Profile → Tokens,
       with the `resource:upload` ability."
     - **Test connection** button → spawns a thread calling
       `xbackbone::test_connection(&config_from_inputs)`, then shows success/fail
       inline (update a status `Label` from the main thread via
       `glib::idle_add_local_once`).
     - "Open API docs" link button → opens
       `https://sergix44.github.io/XBackBone/clients/api` via `xdg-open`.

3. Return `CloudSettingsWidgets` with at least:
   ```rust
   pub struct CloudSettingsWidgets {
       pub section: GtkBox,
       pub destination_combo: ComboBoxText,
       pub xb_url_entry: Entry,
       pub xb_token_entry: Entry,
   }
   ```
   (ApexShot login/logout buttons mutate config directly via `auth.rs` + `save_config()`
   — they are **not** read back through `SaveInputs`, mirroring the existing CLI
   behaviour where login/logout persist immediately.)

### Phase 4 — Wire-up

1. **`src/settings/actions.rs`** — extend `SaveInputs` (line 38) with the new cloud
   widgets:
   ```rust
   pub cloud_destination: ComboBoxText,
   pub xbackbone_url: Entry,
   pub xbackbone_api_token: Entry,
   ```
   In `save_settings()` (line 171), write the new fields to config:
   ```rust
   config.cloud_destination = combo_value(&inputs.cloud_destination, "apexshot");
   config.xbackbone_url = inputs.xbackbone_url.text().to_string().trim().to_string();
   config.xbackbone_api_token = inputs.xbackbone_api_token.text().to_string();
   ```
   **Important:** do **not** touch `cloud_api_token`/`cloud_refresh_token`/`cloud_user_*`
   in `save_settings()` — those are owned by `auth.rs`. The XBackBone token is a simple
   paste-and-save field, so it goes through `save_settings` normally.

2. **`src/settings/mod.rs`** — the call `cloud::build_cloud_section(&config)` (line 295)
   already exists; thread the new widgets into the `SaveInputs` construction (around
   line 428). The dropdown selection and the conditional-panel visibility logic live in
   `cloud.rs`; `mod.rs` only needs to pass the widget clones through.

3. **Call sites (unchanged):** `src/capture/preview_overlay.rs:664` and
   `src/capture/editor/window/events.rs:746` already call `is_configured()` +
   `upload_file()` — once those dispatch internally by destination, zero UI changes
   are needed. The upload button tooltip could optionally become dynamic ("Upload to
   XBackBone" vs "Upload to ApexShot Cloud") — deferred as a minor enhancement.

4. **`.env.example`** — document the new env vars after the
   `APEXSHOT_CLOUD_BACKEND_URL` line:
   ```
   # XBackBone (self-hosted) upload destination — instance URL and API token.
   # Override the Settings UI fields if you prefer to configure via env.
   # APEXSHOT_XBACKBONE_URL=https://files.example.com
   # APEXSHOT_XBACKBONE_TOKEN=your-sanctum-token
   ```

### Phase 5 — Tests & verification

- **New unit tests** in `src/cloud/xbackbone.rs`:
  - Multipart body construction (boundary present, filename + content-type embedded,
    bytes present, closing `--boundary--`).
  - Response parsing: `preview_ext_url` preferred; fallback to `raw_url`; missing both
    → `Server` error.
  - Error mapping: 401 → `AuthExpired`; 413 → `Server("Quota exceeded")`; 422 →
    `Server("Validation: ...")`.
- **New unit tests** in `src/config.rs`:
  - `cloud_destination` sanitize: `"xbackbone"` and `"apexshot"` preserved, anything
    else → `"apexshot"`.
  - XBackbone fields round-trip through YAML.
  - Env-var fallback for `APEXSHOT_XBACKBONE_URL` / `APEXSHOT_XBACKBONE_TOKEN`.
  - (Optional) `~/.config/xbackbone/config` parsing — keep this in a small pure helper
    function so it's testable without touching the real home dir.
- **Run:** `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`.
  - If clippy/lint commands are not the ones the project uses, ask the maintainer and
    record them in `AGENTS.md`.
- **Manual end-to-end:**
  1. Build and open Settings → Cloud.
  2. Switch destination to XBackbone, enter URL + token, click Test connection →
     expect success.
  3. Save, capture a screenshot, click the upload button in the preview overlay →
     expect the XBackBone preview page URL on the clipboard + a notification.
  4. Switch destination back to ApexShot Cloud → confirm the existing flow still works
     unchanged (regression check).

## Out of scope (deferred)

- Deleting XBackBone uploads via the `deletion_url` (not requested by the issue).
- Auto-upload after capture (not requested; current upload is always manual).
- A full in-app OAuth UI for ApexShot Cloud (we spawn a terminal for `apexshot login`).
- Dynamic upload-button tooltip text (minor polish; can follow up).
- Additional self-hosted destinations (Chevereto, Lychee, imgbb, etc.) — the
  `Destination` enum is designed to make these easy to add later.
