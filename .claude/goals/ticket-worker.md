# Goal: ticket worker

You take the next M0 issue from the Stepclimb Project board and deliver it as one pull request into
`next` that passes `just ci` and is ready for the maintainer's review. One issue per run.

## Pick
1. `.claude/scripts/project-status.sh`; take the earliest open issue in the current milestone
   that is in Todo, has no open PR, and whose "Depends on" issues are closed. None: stop and say so.
2. Read the issue and the spec it links in `docs/specs/` in full, then `docs/plan/` sections the
   spec cites. If the spec leaves a point open or contradicts the plan, comment the question on
   the issue, leave it in Todo, report, and stop. Never decide it.
3. `.claude/scripts/board-move.sh <n> "In progress"`.

## Build
Work as the `implementer` agent (`.claude/agents/implementer.md`): branch
`{type}/<n>/<desc>` from `origin/next`, baseline `just ci`, read the rules the change touches,
tests with the code, four-case and cross-organization tests for every route, mutation-check each
new guard, `just sqlx-prepare` and `just openapi` when queries or endpoints change. Scope is the
issue: anything else is a new issue you propose, not code you write.

## Ship
`/ship`: `just ci` green on the committed HEAD (the pre-PR gate refuses otherwise), `/review`
with fresh reviewers, CRITICAL and MAJOR findings fixed, PR against `next` with the template
filled and `Closes #<n>`, then `.claude/scripts/board-move.sh <n> "In review"`. There is no hosted
CI: the PR states the `just ci` summary lines and the OS they ran on.

## Never
Merge, push to `main` or `next`, add AI attribution anywhere, change `docs/plan/`, accept an ADR,
loosen a gate, or touch another repository. A PR touching auth, sessions, permissions, grading,
feedback visibility, billing or deletion says so in its first line.

## Report
Issue and PR links, the gate summary lines, the review findings and what you did with each,
anything not verified, and the questions waiting on the owner.
