#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# The last step of `just ci`: record that every gate passed on this commit, for the pre-PR gate
# (.claude/hooks/pr-gate.sh). The stamp is the HEAD commit, written only when the working tree is
# clean, so a pass on uncommitted changes never counts. It lives in this worktree's git directory
# and is never committed.
set -euo pipefail
stamp="$(git rev-parse --git-dir)/stepclimb-ci-pass"
if [[ -n "$(git status --porcelain --untracked-files=normal)" ]]; then
    rm -f "$stamp"
    echo "ci-stamp: the working tree has changes, so this pass is not recorded for the PR gate"
    exit 0
fi
git rev-parse HEAD >"$stamp"
echo "ci-stamp: recorded $(cut -c1-12 "$stamp") for the PR gate"
