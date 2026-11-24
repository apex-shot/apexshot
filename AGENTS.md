# AGENTS.md

`CONTRIBUTING.md` has the setup, subsystem map, and style rules.

> **Maintainer-local agreement:** if `AGENTS.local.md` exists in the repository root,
> read it and follow it before starting work. It is untracked and holds the maintainer's
> environment, delegation, testing, and delivery preferences for this repository.

## Branches

Small, self-contained change (docs, strings, a one-off fix): commit to `main`.

Anything big — a feature, a refactor, a behavioural change across files or
subsystems — goes on its own branch off `origin/main`, and a PR. Never merge
your own PR; that call is the maintainer's.

Never `reset --hard`, `git clean`, force-push, or delete branches unless asked.
Uncommitted work belongs to the maintainer.

## PRs

A PR title is a plain, sentence-case description in the imperative, exactly the
shape of every merged subject on `main` ("Unify the video and image editors'
Background panels", "Expose area capture across the tray, CLI, and shortcuts").

Never prefix a PR title (or a commit subject) with `feat:`/`fix:`/`chore:` — the
type lives in the branch name (`feat/…`, `fix/…`). `CONTRIBUTING.md`'s
conventional-commit examples do not match this repo's history; the history wins.

## Product references

Never name the products whose behaviour we study or take inspiration from — in
PR titles, PR descriptions, commit subjects or bodies, code comments, doc
comments, docs, or tests. Refer to the behaviour as *studied* or *researched*
the way the existing code does ("the studied ripple", "the reference we
studied"). A name in a comment is still a name, and a platform feature named
only to point at another product counts too.

Why: we do not want readers to learn where an idea came from. This holds even
when the mention is flattering or only a comparison.

## Flatpak local builds

`flatpak/README.md` documents the local package. Its dependency modules are
expensive (Qt 5 alone is about twenty minutes) and their results are cached in
`.flatpak-builder/`, so a rebuild reuses them instead of recompiling:

- Never delete `.flatpak-builder/`, and never uninstall the runtime or SDK as
  routine cleanup.
- Do not pass `--force-clean` for a routine rebuild. Add `--keep-build-dirs` so
  the application module's Cargo `target/` stays warm and a source change
  rebuilds incrementally.
- Keep `-Dlibdir=lib` on the meson modules. The builder the packaging workflow
  uses passes no libdir, so without the flag meson installs into the SDK's
  `lib64` and the Rust build cannot find `gtk4-layer-shell` in
  `/app/lib/pkgconfig`. A meson error saying the option is set twice means that
  local builder is passing the libdir itself: drop the flag for that build
  only, never in the manifest.
- Add `--ccache` to a rebuild so compiled C/C++ objects are reused; the SDK
  already ships `ccache` and its objects live in `.flatpak-builder/ccache`.
- If `flatpak-builder` refuses with "App dir ... is not empty", pass
  `--force-clean` anyway: a finished build leaves only its own staging
  directories there, and the module caches in `.flatpak-builder/cache` are
  untouched.
- A harness that redirects `XDG_CONFIG_HOME` breaks `flatpak run` as well as
  `--user`: the `xdg-pictures` and `xdg-videos` permissions resolve through
  `xdg-user-dir` and then mount nothing, so the sandbox cannot see those
  folders and the app reports host files as missing. Unset `XDG_DATA_HOME`,
  `XDG_CACHE_HOME`, and `XDG_CONFIG_HOME` for launch commands too.

## Cross-platform release checks

The maintainer usually works on Linux, so Windows breakage is invisible
locally. CI covers the gap; use it before every release and after any change
to recording, export, ffmpeg arguments, packaging, or bundled dependencies.

- **What runs where.** `windows.yml` builds and tests the Windows UI and the
  capture shell (`capture-overlay`) on every push and PR that touches them;
  its export tests run against the same MSYS2 ffmpeg the installer bundles,
  with `APEXSHOT_REQUIRE_MEDIA_TESTS=1` so a missing tool fails instead of
  skipping. `release.yml` builds every package (Windows installer, `.deb`,
  Fedora, openSUSE, Arch), installs each one, and smoke-tests it: a real
  two-second recording and an editor start with the shipped ffmpeg. It runs on
  tags, weekly, and on demand; the Windows installer also runs on every push to
  `main`.
- **From Linux, before tagging a release**, run the whole package matrix on
  the commit you will tag and wait for it to pass:
  `gh workflow run release.yml --ref main`, then
  `gh run watch "$(gh run list -w release.yml -L1 --json databaseId -q '.[0].databaseId')"`.
  A tag must never be the first run of a packaging change.
- **Pinned Windows dependencies.** `packaging/windows/msys2-versions.txt` pins
  the MSYS2 ffmpeg and GTK4 the installer bundles. When MSYS2 moves, CI fails
  with the new version; bump the pin only after the installer smoke test passes
  with it, and say so in the commit.
- **Locally on Linux**, `bash scripts/smoke-linux-package.sh` smoke-tests an
  installed package the same way CI does (needs `xvfb-run` and `ffmpeg`).
- **Locally on Windows**, the installed recorder can be checked with
  `apexshot-capture.exe --self-test-record <out.mp4>` (result in
  `<out.mp4>.selftest.txt`) and the editor with
  `apexshot-video-editor.exe --self-check <report.txt>`.
