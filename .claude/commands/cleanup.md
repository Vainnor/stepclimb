---
description: Remove merged local branches and clean agent worktrees (the only path that removes one) under .claude/worktrees, and clear the target/ of every merged worktree, after checking nothing unpushed would be lost
argument-hint: "[--targets-only] (otherwise none: the script judges every worktree and branch)"
---

Clean up merged work with `.claude/scripts/post-merge-cleanup.sh`, the same script the
post-merge-cleanup hook runs in the background after each `gh pr merge`. The hook runs it with
`--keep-worktrees` and never removes a worktree; this command is the only thing that does. Both
clear merged worktrees' `target/` directories. Its rules are in the docstring of
`.claude/hooks/lib/post_merge_cleanup.py`; do not delete anything by hand.

1. **Preview.** `.claude/scripts/post-merge-cleanup.sh --dry-run` and show the report.
2. **Run.** `.claude/scripts/post-merge-cleanup.sh`. It removes only a worktree under
   `.claude/worktrees/` that is not on `main` or `next`, not locked by a live pid or by hand (a
   lock naming no pid), not used by any running process, has no changed, untracked or ignored
   file other than `target/`, has nothing outside `origin/next`, `origin/<branch>` or its PR head,
   and whose PR is merged or closed with none open (or whose detached HEAD is in `origin/next`),
   with its branches; and local branches that pass the same PR and pushed checks. It never
   touches `main`, `next`, the current branch or checkout, or remote branches other than a merged
   PR's. Of every other worktree in `git worktree list`, T3's under `~/.t3/worktrees/` included,
   it deletes only the top-level `target/` (cargo rebuilds it), under the same checks. "Used"
   means a process runs an executable from it, has its working directory there or holds a file
   there open (`ps` and `lsof`). When `ps` or `lsof` is missing or fails, nothing is removed or
   cleared. The report ends with free space on the repository's drive.
3. **Kept items.** For each worktree or branch kept as dirty or with unpushed commits, list the
   files (`git -C <wt> status --short`) or commit subjects (`git log origin/next..<branch>`)
   that removing it would lose, and ask the user before doing anything with it.
4. **Report.** The script's report, and that remote branches and PRs were left untouched.

With `--targets-only` as the argument, clear build output and nothing else: preview with
`.claude/scripts/post-merge-cleanup.sh --dry-run --targets-only`, run it without `--dry-run`,
skip step 3, and report.

This clears the `target/` of merged work only. For build output gone stale inside worktrees
still in use, `just clean-stale` (`.claude/hooks/lib/clean_stale.py`) deletes what no build has
used for days.
