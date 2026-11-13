#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# SessionStart: print branch, working-tree state, open PRs, worktrees and project status.
# Logic in lib/session_context.py. Never fails the session.
here="$(cd "$(dirname "$0")" && pwd)"
command -v python3 >/dev/null 2>&1 || exit 0
python3 -X utf8 "$here/lib/session_context.py"
exit 0
