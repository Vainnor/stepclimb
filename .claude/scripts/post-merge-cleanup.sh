#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Remove merged agent worktrees, their Docker target volumes and merged local branches, and clear
# merged worktrees' target/, after checking nothing unpushed or open would be lost. `--dry-run`
# reports and changes nothing; `--targets-only` clears target/ directories and nothing else.
# The post-merge-cleanup hook and /cleanup both run this; the logic and its rules are in
# .claude/hooks/lib/post_merge_cleanup.py.
here="$(cd "$(dirname "$0")" && pwd)"
exec python3 -X utf8 -B "$here/../hooks/lib/post_merge_cleanup.py" "$@"
