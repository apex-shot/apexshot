# AGENTS.md

Working agreement for LLM agents in this repository. `CONTRIBUTING.md` covers
setup, the subsystem map, and code style; this file covers how work arrives here
and what "done" means. Read this file every session; open `CONTRIBUTING.md`
only when you actually need setup, the subsystem map, or style rules.

## The loop

Most work starts as a *suspected* issue: the maintainer describes something that
looks wrong. Move it through these states in order, and say which state you are
in. Do not skip to step 2.

**Fast path — direct requests.** When the maintainer already says what to
change ("make it 48px", "match Motion", "drop the orange bar"), that
instruction *is* the confirmation: skip step 1 and edit. Also skip subagent
research, plan documents, and option-polling whenever one reading of the
request is clearly the right one — pick it, say what you picked in one
sentence, and keep moving. Step 1's diagnosis is for suspected bugs where the
cause is not already named. The guardrails that actually matter are the
branch, the narrow verify, and the no-CI-watching rules below; everything
else is ceremony for a direct request.

### 1. Investigate and confirm, with no code changes

- Restate the suspected issue in one or two sentences.
- Reproduce it, or trace it to the exact code that would cause it. Quote
  evidence: `path/file.rs:123`, the command you ran, the output or log line.
- Where to look: daemon stderr (`cargo run -- daemon`),
  `journalctl /usr/bin/gnome-shell -f | grep apexshot` for the extension,
  `docs/ARCHITECTURE.md` and `docs/MODULES.md` for the code path, `git log` for
  the last change in that area.
- Do not edit while confirming. Diagnosis and fix are separate steps, and the
  maintainer decides whether to proceed.
- Finish with a verdict, each backed by its evidence: **confirmed**,
  **partly confirmed**, **not reproducible**, or **already fixed / by design**.
- Only a confirmed issue earns a fix, and only a fix that changes behaviour
  earns a branch (step 2). **Not reproducible** and **by design** verdicts stop
  here and go back to the maintainer with the evidence.
- Say what else a fix would touch (callers, tests, docs, packaging) before
  anyone edits anything.

### 2. Pick the lightest path that protects `main`

Not every change needs a branch. A *confirmed* issue or a direct instruction
from the maintainer is what earns one; a typo does not.

| Change | Path |
| --- | --- |
| Docs, comments, typos, metadata, message strings, other small self-contained edits with no behaviour risk | Commit and push straight to `main` |
| A confirmed issue whose fix changes behaviour, or that touches several files or subsystems | Branch + PR |
| Features, refactors, packaging, CI changes | Branch + PR |

When you cannot tell which side of that line a change is on, ask. Rules that
apply to either path:

- Base new branches on `origin/main`, never on a stale local copy:
  `git fetch origin && git switch -c fix/<slug> --no-track origin/main`
  (`feat/`, `docs/`, `chore/` for other kinds of work).
- One logical change per branch and commit. No drive-by refactors or "while I
  was here" fixes; report those separately and let the maintainer decide.
- Asked to continue on an existing long-lived branch? Merge `main` into it first
  (`git merge origin/main`) so it does not drift.
- Fix the cause, not the symptom, and keep the diff as small as it can be.
- Never merge your own PR. Merges are the maintainer's call.

### 3. Verify, then say exactly what you verified

- Re-run step 1's reproduction and show the before and after behaviour.
- Anything user-visible (windows, overlays, hotkeys, portals, recording,
  clipboard) needs a manual check on the running app — and the maintainer is
  the one who runs it. Never launch, drive, or script the app yourself: this
  is a live Linux desktop, and app runs here are disruptive and unreliable.
  List what needs checking under "Not verified" as "manual check: deferred to
  the maintainer" and leave it for the maintainer to confirm before merge.
- Then CI's gates:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets      # must not add warnings
cargo test --jobs 2 -- --test-threads=1     # every target, as CI runs it
```

  Test narrow by default (`cargo test --lib <module>`, or
  `./scripts/test.sh --test NAME`); `./scripts/test.sh` is the OOM-safe local
  runner. Run the full `cargo test` command above only when the change touches
  shared/test infrastructure, or when marking a PR merge-ready. CI already runs
  the full suite on every PR, so do not duplicate it locally on every change.
- Only if you touched that subsystem:
  - UI strings: `python3 scripts/check-i18n-catalogs.py`
  - GNOME extension: `pnpm check:gnome` (or `node --check gnome-extension/*.js`)
  - Desktop / AppStream metadata: `scripts/validate-metadata.sh`
  - C++ overlay: `cd capture-overlay && cmake -S . -B build && cmake --build build -j`
- Report the checks you ran, the ones you skipped, and any failure as it is.
  "Tests pass" without the command and result is not a verification.

### 4. Commit, push, publish

```bash
git add <files>
git diff --staged                              # review before committing
git commit                                     # summary, why, how you verified
```

For a direct-to-`main` change, push it (`git push`, or
`git push origin HEAD:main` when `main` is checked out in another worktree) and
stop there.

For a branch:

```bash
git push -u origin HEAD
gh pr create --base main --fill                # then make the body follow the template
```

- Summary line: short and imperative. Body: why the change is needed and how it
  was verified. `CONTRIBUTING.md` lists Conventional Commits prefixes while
  recent history uses plain prose summaries, so match the commits around you
  (`git log --oneline -20`).
- Fill `.github/PULL_REQUEST_TEMPLATE.md`: summary, type of change, `Fixes #N`,
  **How was this tested** with the real commands, subsystem checklist, and
  screenshots for visual changes.
- Stage only what belongs to the change: no debug prints, personal paths,
  commented-out code, or secrets. Stage by explicit path — under a sandbox
  `git status` invents untracked root dotfiles, so `git add -A` / `git add .`
  records junk (see the phantom-files trap under Repository traps).
- If `gh pr edit --body` fails with a Projects (classic) GraphQL error, patch it
  with `gh api -X PATCH repos/:owner/:repo/pulls/<n> -F body=@body.md`.
- CI red on your branch? Fix it there. Never merge a red PR.
- Do not watch CI in-session. After pushing, report the PR URL and which CI
  jobs will run, then stop — never run `gh pr checks --watch` or poll CI in a
  loop. A red CI becomes a new follow-up task, not a wait: GitHub CI is slow
  and sessions have parked ~30 min watching it finish.

### 5. After the merge

Sync `main` and start a fresh branch for the next change. Do not keep adding
unrelated work to an already-merged branch or to an old long-lived one.

## Repository traps

- **Phantom untracked files (sandbox):** inside a sandboxed Bash session,
  `git status` lists root dotfiles — `.bashrc`, `.bash_profile`, `.zshrc`,
  `.zprofile`, `.profile`, `.gitconfig`, `.gitmodules`, `.mcp.json`,
  `.ripgreprc`, `.idea`, `.vscode`, and the entries under `.claude/` — as
  untracked. They are not real files here: `ls -la` shows them as character
  devices (`1,3` = `/dev/null`, owner `nobody:nogroup`) and they do not exist in
  the maintainer's terminal. What a tool reports for them is unstable — `stat`
  may call them "regular empty file" — which is also why `.idea/` and `.vscode/`
  slip past their directory-only rules in `.gitignore`. So "commit and push all
  the changes" means the tracked modified files only: stage by name
  (`git add path/to/file.rs`), never `git add -A` or `git add .`, which would
  record the phantom entries. `git commit -a` is safe (it only stages tracked
  files) but naming paths stays clearer. Trust `git diff` for content and the
  maintainer's terminal for what actually exists.
- **Network commands run in background Bash, not terminal tabs:** `git push`,
  `git fetch`, `gh pr …` and other GitHub calls go to the Bash tool with
  `run_in_background: true`, then read the task output file (or wait for the
  completion notification). Do NOT open a terminal tab for these: the tab cap
  is small, and a tab sitting on a pager or an unfinished command blocks
  later work. If a background call fails with a proxy denial
  (`github.com:22` / `github.com:443` blocked), the sandbox really is blocking
  egress that run — retry once in the background, and only then fall back to
  `run_in_terminal` for that one command.
- **GTK tests:** GTK may only be initialized once per process, on one thread.
  Tests that need it must call `crate::test_support::with_gtk`, not
  `gtk4::init()`; a second init panics with "Attempted to initialize GTK from
  two different threads". Tests that need a display skip themselves when there
  is none. CI runs with `--test-threads=1`, so order-dependent failures surface
  there first.
- **i18n:** every UI string registered in code (helpers like `t(...)`,
  `tfmt(...)`) must exist in all eight shipped catalogs
  (`po/{ar,de,es,fr,ja,pt_BR,ru,zh_CN}.po`) or `tests/i18n_catalog.rs` fails.
  `scripts/translate-i18n-catalogs.py` needs `DEEPL_AUTH_KEY`; without it, add
  entries by hand in the repo's PO format.
- **Docs drift:** this repo has advertised removed features before (GIF
  recording, area recording, webcam PiP, `record ui`). When a capability is
  added or retired, update `README.md` and the matching `docs/*.md` in the same
  PR.
- **Fedora recording is unsupported by design.** Recording entry points refuse
  there with a notification; that is not a bug to fix.
- **Clippy backlog:** pre-existing warnings keep CI from using `-D warnings`. Do
  not clear them in an unrelated branch, and do not add new ones.
- **Privacy:** never paste tokens, cookies, portal restore tokens, file paths,
  window titles, or captured screen/audio content into issues, PRs, logs, or
  commits. See "Privacy-safe reports" in `CONTRIBUTING.md`.
- **Worktrees:** do not create one unless two checkouts have to be live at the
  same time (a long build or test run in one while the other is used). Prefer
  the checkout you are in and `git switch -c` for a new branch: it reuses the
  warm `target/`. When a worktree is warranted, keep the build cache shared so
  dependencies are not recompiled per checkout, for example
  `CARGO_TARGET_DIR=$HOME/.cache/apexshot-cargo-target cargo test`, and prune
  stale entries when you are done (`git worktree prune`).
- **Destructive git:** no `reset --hard`, `clean`, force-push, or branch
  deletion unless the maintainer asks. Treat uncommitted work as theirs.

## Report format

- **Verdict:** confirmed / partly confirmed / not reproducible / already fixed
- **Evidence:** reproduction, `file:line`, command output
- **Cause:** what is actually wrong
- **Fix:** branch, files, one-line summary
- **Verified:** manual check and commands run, with results
- **Not verified:** what you could not exercise, and why
