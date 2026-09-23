# AGENTS.md

`CONTRIBUTING.md` has the setup, subsystem map, and style rules.

## Branches

Small, self-contained change (docs, strings, a one-off fix): commit to `main`.

Anything big — a feature, a refactor, a behavioural change across files or
subsystems — goes on its own branch off `origin/main`, and a PR. Never merge
your own PR; that call is the maintainer's.

Never `reset --hard`, `git clean`, force-push, or delete branches unless asked.
Uncommitted work belongs to the maintainer.
