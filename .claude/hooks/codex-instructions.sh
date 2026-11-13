#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Codex SessionStart: print CLAUDE.md and the rules without `paths:` as developer context, the
# startup instructions Claude Code loads and Codex does not. Logic in lib/codex_instructions.py;
# python3 is required.
here="$(cd "$(dirname "$0")" && pwd)"
command -v python3 >/dev/null 2>&1 || {
    echo "codex-instructions: python3 not found, so CLAUDE.md and the rules were not loaded" >&2
    exit 1
}
exec python3 -X utf8 "$here/lib/codex_instructions.py"
