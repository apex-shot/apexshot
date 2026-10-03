# AGENTS.md

`CONTRIBUTING.md` has the setup, subsystem map, and style rules.

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
