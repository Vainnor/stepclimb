#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Print a Markdown fact sheet of Stepclimb's status (milestones, issues, PRs, ADRs, code size,
# worktrees) for /project-status to judge. Changes nothing but
# origin's remote-tracking refs; the logic is in .claude/hooks/lib/project_status.py.
here="$(cd "$(dirname "$0")" && pwd)"
exec python3 -X utf8 -B "$here/../hooks/lib/project_status.py" "$@"
