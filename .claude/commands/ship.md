---
description: Ship the current branch as a pull request into next, gated by just ci locally and in GitHub Actions and a review of the committed HEAD
argument-hint: "[short PR title] (default: from the commits)"
---

Ship the current branch. The order is deliberate: **gate, commit, review the commit, then push
and open the PR**, so what was reviewed is exactly what ships.

1. **Branch.** `git branch --show-current` is `{type}/{issue-or-hash}/{desc}`, never `main` or
   `next` (the git-guard hook refuses a commit there). `git fetch origin` and rebase onto
   `origin/next` if it moved; resolve conflicts by keeping both sides' intent.
2. **Gate.** `just ci`. Quote nextest's `Summary` line, Vitest's and Playwright's totals,
   `layering: ok`, `openapi-diff: ok` and cargo-deny's verdict line, not the exit code. Fix and
   rerun until green.
3. **Coverage.** `git diff origin/next...HEAD --stat`. Each new behavior has a test that would
   fail without the change; each new guard or permission check had a mutation check; each new
   route has its four-case and cross-organization tests (`testing.md`).
4. **Commit.** Stage explicit paths, never `git add -A`. `git status --short` shows nothing under
   `references/` or `private/` except their READMEs and `references/INDEX.md`. Message
   `type(scope): summary` with a body saying what and why. **No AI attribution of any kind**
   (AGENTS.md non-negotiable 1). Lockfile and toolchain changes get their own commit.
5. **Review the committed HEAD.** Run `/review`. Fix CRITICAL and MAJOR findings in new commits
   and review again; list accepted minors with a reason.
6. **Push.** `just secrets-scan history` first, and stop on any finding. Then
   `git push -u origin HEAD`.
7. **PR.** `gh pr create --base next` with the PR template filled and a body per
   `engineering-prose.md` -> PR descriptions: what changes in plain words, a before/after
   diagram, a screenshot for visual changes, the gates run and not run, `Closes #N`. No
   attribution.
8. **CI.** Wait for the PR's `ci` and `tauri` checks. A red check goes to `gate-doctor`; never
   loosen a check or rerun it to green.
9. **Merge** only when the owner asked or granted merge permission: `gh pr merge <n> --squash
   --delete-branch`. Then `/cleanup`.
