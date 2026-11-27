# Screenshot organizer

Every capture gets a descriptive filename a moment after it is taken, its
on-screen text becomes searchable in History, and secrets never reach a
filename or the index. Windows and Linux, local only.

## Phase 1: shared core (`organizer/` crate)

GUI-free, platform-neutral, no new dependencies beyond what the root and
`windows-ui` lockfiles already carry (keeps the Flatpak's vendored sources
valid).

- `secrets`: emails, card numbers (Luhn), API keys, JWTs, password values;
  `scrub` removes them, `secret_lines` locates them for redaction.
- `naming`: window-title split (subject + app), site from the address bar,
  best on-screen heading, name styles (clean, dated, by app), filesystem-safe
  stems, collision-free paths.
- `index`: one JSON index file, typo-tolerant AND search over text, title,
  app, site, and filename; `app:`, `site:`, and date filters ("yesterday",
  "last week", `2026-11-27`).

## Phase 2: wiring (free)

- Capture context at capture time: foreground window title and process on
  Windows (capture shell) and X11; Wayland has no title, so names lean on OCR.
- Name before the path is handed out. The saved path goes to the clipboard
  (file-link mode), the editor, auto-upload, and the result card, so a rename
  after the fact would break them. Run OCR while the file is written, then
  rename; if OCR takes longer than ~1.5 s, keep the default name and offer the
  suggestion in History instead.
- Automatic rename on by default; History shows the original name and an undo.
- History search box over the index; matches highlighted.
- Copy with secrets redacted, using `secret_lines` boxes.

## Phase 3: Studio

New on/off Studio capability (beside `image_upscale` in
`StudioCapabilities`). Studio gates the feature, not the download.

- **AI names.** Opt-in, one-time download of a small open model
  (~0.5–1 GB quantized, e.g. a 0.6B–1.7B instruct model), run on the CPU in
  pure Rust (candle) at low priority. Input is only the scrubbed window title
  and OCR text, never the image. Output is a polished stem; any failure falls
  back to the heuristic name. The model is never bundled in installers or
  packages; inference code sits behind a cargo feature so distro builds can
  drop it.
- **Speed check before the trial or download (required).** Before the trial
  starts or the model is downloaded, time one short benchmark (a fixed prompt
  through the same runtime, or a CPU capability probe when the model is not
  yet present). If the machine cannot produce a name within the naming budget,
  say so up front ("This PC is too slow for AI names; heuristic names stay
  on") and do not start the trial or offer the purchase for this feature.
  Re-check after the first real runs: if names keep timing out, turn AI names
  off and tell the user once, rather than silently slowing captures.
- **Trial.** A local counter (like the Studio output counter) gives the first
  25–50 captures AI names, then falls back to heuristic names and shows the
  upgrade card once.
- **Bulk rename** of the existing library (suggestions reviewed before apply).
- **Learned renames**: store (suggested, chosen, context) pairs and reuse a
  pattern after two matching corrections.
- **Auto-tidy and clean-up**: move old captures off the Desktop, find
  duplicates and never-opened captures.
