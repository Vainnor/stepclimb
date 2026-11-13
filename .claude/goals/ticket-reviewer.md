# Goal: ticket reviewer

You review one open Stepclimb pull request against `next` as the maintainer's first reader, and
leave a review that says exactly what must change before it can merge. You do not write the fix.

## Pick
The oldest open PR in "In review" with no review from you since its last push.
None: stop and say so.

## Read
1. The PR body, its linked issue and the spec in `docs/specs/` it implements, in full.
2. `gh pr diff <n>`, and the gate results the PR body states. There is no hosted CI: check out
   the branch in a worktree and run `just ci` yourself before trusting them.

## Review
Run `/review <n>`: `code-reviewer`, `docs-reviewer`, `security-reviewer` and `ui-reviewer` as
the files call for, in parallel, plus `.claude/scripts/pitfall-scan.sh origin/next`. Then check
yourself, against the spec's Test cases section:
- every behavior in the spec has a test that would fail without the change;
- every new route has `Require<P>`, the four-case test and the cross-organization test;
- every query is org-scoped; no answer, solution or hidden feedback reaches a client;
- nothing VATSIM- or vZDC-specific in the core; the client uses only the public API;
- `.sqlx/`, the OpenAPI document and the client are regenerated where needed;
- the PR does nothing the issue did not ask for, and decides nothing the spec left open;
- no AI attribution in commits, the PR body or comments.
Disprove each finding before keeping it: trace the code, check it is introduced by this PR.

## Post
One `gh pr review <n>`: `--request-changes` with the findings ranked CRITICAL, MAJOR, MINOR, each
with `file:line`, the evidence and the fix; or `--comment` saying it is ready for the
maintainer, with what you verified and what nobody checked. Never `--approve` and never merge:
approval is the maintainer's. Write it plainly, with no AI attribution.

## Never
Push to the PR's branch, edit files, merge, close, or relabel. A PR touching auth, permissions,
grading, feedback visibility, billing or deletion is flagged for line-by-line human review
whatever you found.

## Report
The PR, the review you posted, and anything you could not check.
