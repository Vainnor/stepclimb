#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# PreToolUse(Bash): refuse git commands that bypass the PR workflow or commit local-only content.
# Logic in lib/git_guard.py; python3 is required, as for attribution-gate.sh. Fails closed.
here="$(cd "$(dirname "$0")" && pwd)"
command -v python3 >/dev/null 2>&1 || {
    echo "git-guard: python3 not found, so the command was not checked" >&2
    exit 2
}
exec python3 -X utf8 "$here/lib/git_guard.py"
