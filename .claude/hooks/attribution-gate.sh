#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# PreToolUse(Bash) hook: blocks a git commit, gh pr/issue write or gh api call whose command text
# carries AI attribution (AGENTS.md, non-negotiable 1). It sees only the command text: a body held
# in a variable or a file the command does not write is invisible to it, so the commit-msg hook is
# the backstop for commits.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/attribution.sh
. "$here/lib/attribution.sh"

input="$(cat)"
cmd="$(printf '%s' "$input" | python3 -X utf8 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))' 2>/dev/null)" || {
    echo "attribution-gate: could not parse the hook input, so the command was not checked" >&2
    exit 2
}
if ! grep -qE '(git (commit|tag)|gh (pr|issue|release)|gh api)' <<<"$cmd"; then
    exit 0
fi
hits="$(attribution_hits <<<"$cmd")"
if [[ $? -ne 1 ]]; then
    echo "attribution-gate: blocked; the command carries AI attribution or could not be checked:" >&2
    printf '  %s\n' "$hits" >&2
    echo "Remove the trailer, footer or link. Commits, PRs and issues here carry no AI attribution." >&2
    exit 2
fi
exit 0
