#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# PostToolUse(Write|Edit|MultiEdit): format an edited file with the repo's formatter: rustfmt for
# .rs, Biome for TypeScript, JavaScript, JSON and CSS. Never blocks.
# Logic in lib/format_on_edit.py.
here="$(cd "$(dirname "$0")" && pwd)"
command -v python3 >/dev/null 2>&1 || exit 0
python3 -X utf8 "$here/lib/format_on_edit.py"
exit 0
