#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Codex PreToolUse/PostToolUse(apply_patch): run a Claude file hook once per file the patch
# touches. Usage: codex-adapter.sh <hook script name in this directory>. Logic in
# lib/codex_adapter.py; python3 is required. Fails closed, so a guard hook never silently passes.
here="$(cd "$(dirname "$0")" && pwd)"
command -v python3 >/dev/null 2>&1 || {
    echo "codex-adapter: python3 not found, so the patch was not checked" >&2
    exit 2
}
exec python3 -X utf8 "$here/lib/codex_adapter.py" "$here/${1:?usage: codex-adapter.sh <hook script>}"
