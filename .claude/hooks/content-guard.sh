#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# PreToolUse(Write|Edit|MultiEdit|NotebookEdit): refuse writes of local-only references and
# early art.
# Logic in lib/content_guard.py; python3 is required, as for attribution-gate.sh. Fails closed.
here="$(cd "$(dirname "$0")" && pwd)"
command -v python3 >/dev/null 2>&1 || {
    echo "content-guard: python3 not found, so the path was not checked" >&2
    exit 2
}
exec python3 -X utf8 "$here/lib/content_guard.py"
