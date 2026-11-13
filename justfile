# Stepclimb task runner. `just ci` is the merge gate: it must pass on the committed HEAD before a
# merge, on the machine the change was made on. There is no hosted CI (docs/adr/0021).
set shell := ["bash", "-cu"]

import 'just/secrets.just'
import 'just/rust.just'
import 'just/web.just'
import 'just/stack.just'

# The local database (`just up`); `just ci` sets its own run's database. sqlx's test and query macros read DATABASE_URL.
export DATABASE_URL := env('DATABASE_URL', 'postgres://stepclimb:stepclimb-local@127.0.0.1:55432/stepclimb')

# List recipes
default:
    @just --list

# Every merge check. Must pass on the committed HEAD before a merge. Safe to run from several
# worktrees at once: bin/ci_gate.py gives the run its own database and e2e stack, removes them
# after, and lets at most SC_CI_SLOTS (default 3) gates run on the machine.
# The stamp names the commit checked out when the run started, and only if it still is and the
# working tree was clean at the start and the end.
ci:
    #!/usr/bin/env bash
    set -euo pipefail
    tested="$(git rev-parse HEAD)"
    tree=clean
    [[ -z "$(git status --porcelain --untracked-files=normal)" ]] || tree=dirty
    python3 -X utf8 -B bin/ci_gate.py run -- "{{ just_executable() }}" _ci-checks
    bash bin/ci-stamp.sh "$tested" "$tree"

_ci-checks: secrets-scan fmt-check lint layering lint-escapes spdx-check gate-check test-rust deny sqlx-check openapi-diff web-lint typecheck test-web ui-visual e2e compose-check compose-ops-check agents-check
    @echo "just ci: all gates passed"

# Tests of bin/ci_gate.py, the gate's locks and per-run stack
gate-check:
    python3 -X utf8 -B -m unittest discover -s bin -p 'test_*.py'

# Guard hook tests, Codex agent and skill pointers to .claude/, rule-file limits (docs/agents.md).
# Python runs in UTF-8 mode: the files and hook output it reads are UTF-8.
agents-check:
    python3 -X utf8 -B -m unittest discover -s .claude/hooks/lib -p 'test_*.py'

# Every source file carries an SPDX license header (AGPL-3.0-or-later; Apache-2.0 in SDK packages)
spdx-check:
    bash bin/check-spdx.sh

# Rules in .claude/hooks/lib/clean_stale.py. `--days N` (default 3, at least 2); `--dry-run`
# reports only. Delete build output unused for days from every worktree's target/
[positional-arguments]
clean-stale *args:
    python3 -X utf8 -B .claude/hooks/lib/clean_stale.py "$@"

# Install the repo's git hooks (pre-commit secret scan and formatting, commit-msg attribution check). Once per clone.
install-hooks:
    git config core.hooksPath .githooks
    @echo "hooks installed: core.hooksPath=$(git config core.hooksPath)"
