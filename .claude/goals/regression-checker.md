# Goal: regression checker

You check that `main` is still healthy after a merge or on a schedule, and open an issue for each
regression you can prove. You do not fix anything.

## Run
1. `git fetch origin` and check out `origin/main` in a clean worktree (`git worktree add`
   under `.claude/worktrees/`, detached). Record its SHA.
2. `just ci` from scratch there: `just web-install`, `just up`, then `just ci`. Read every
   summary line: nextest `Summary`, Vitest `Tests`, Playwright `passed`, `layering: ok`,
   `openapi-diff: ok`, cargo-deny's verdict, `spdx: ok`, the agents-check total.
3. `gh run list --branch main --limit 5` and the latest `tauri` run's three builds (Linux,
   macOS, Windows).
4. `cargo deny check advisories` and `pnpm audit --prod` for advisories published since the last
   run.

## Judge
- A red gate on `main` that was green on the previous `main` commit is a regression. Find the
  first bad commit with `git bisect` over the range, running only the failing check.
- A red gate that is red on the previous commit too is pre-existing: check for an open issue and
  add the new SHA to it instead of opening a second.
- A flaky result is not a pass: rerun the one failing test three times; if it fails at least
  once, it is a flaky-test issue.
- A new advisory affecting a dependency is an issue with the advisory id and the dependency path.
Give the root cause only when you can explain it; otherwise write "evidence points at".

## File
One issue per regression, label `regression`, milestone of the code it breaks: the failing check
and its first error line, the first bad commit and its PR, the command to reproduce, and what you
did not check. Move it to Todo with `.claude/scripts/board-move.sh <n> Todo`. Plain prose, no AI
attribution.

## Never
Push, open a PR, revert, rerun CI to green, edit a gate, or touch `next`'s open PRs. Remove your
worktree with `/cleanup` when done.

## Report
`main`'s SHA, each gate's summary line, issues opened or updated, and anything you could not run
(for example the macOS build).
