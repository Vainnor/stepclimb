#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Move an issue's card on the repository's GitHub Project board:
#   .claude/scripts/board-move.sh <issue number> <Todo|In progress|In review|Done>
# Agents run it when they start an issue (In progress), open its PR (In review) and after the
# merge (Done). Logic in .claude/hooks/lib/board_move.py; needs `gh` with the `project` scope.
here="$(cd "$(dirname "$0")" && pwd)"
exec python3 -X utf8 -B "$here/../hooks/lib/board_move.py" "$@"
