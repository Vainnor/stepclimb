#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# PostToolUse(Bash): after a `gh pr merge`, start lib/post_merge_cleanup.py in the background to
# delete merged branches and clear merged worktrees' target/ (it keeps every worktree; /cleanup
# removes them). Returns at once and
# never fails the call; the report goes to stepclimb-post-merge-cleanup.log in the git common dir.
# A command without the word "merge" cannot be a merge, so python3 starts only for those.
here="$(cd "$(dirname "$0")" && pwd)"
input="$(cat)"
grep -q 'merge' <<<"$input" || exit 0
command -v python3 >/dev/null 2>&1 || exit 0
python3 -X utf8 -B "$here/lib/post_merge_cleanup.py" --hook <<<"$input"
exit 0
