#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# PreToolUse(Bash): refuse `gh pr create` unless `just ci` passed on the committed HEAD with a
# clean tree (the stamp bin/ci-stamp.sh writes). Logic in lib/pr_gate.py; python3 is required.
# Fails closed.
here="$(cd "$(dirname "$0")" && pwd)"
command -v python3 >/dev/null 2>&1 || {
    echo "pr-gate: python3 not found, so the command was not checked" >&2
    exit 2
}
exec python3 -X utf8 "$here/lib/pr_gate.py"
