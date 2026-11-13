---
description: Review the current branch or a PR before it merges, with code-reviewer, docs-reviewer, security-reviewer and ui-reviewer in parallel as they apply, plus the pitfall scan
argument-hint: "[PR number] (default: current branch against origin/next)"
---

Review `$ARGUMENTS` (empty means the current branch against `origin/next`). You coordinate; the
reviewers did not write the change, so their read is not anchored on the author's.

1. **Scope.** For a PR: `gh pr view $ARGUMENTS` and `gh pr diff $ARGUMENTS --name-only`. For the
   branch: `git fetch origin`, then `git diff origin/next...HEAD --stat`. Note which files are
   Rust, migrations or manifests, which are TypeScript or CSS, which are docs, specs, ADRs or
   rules, and whether auth, permissions, grading, feedback visibility, billing or deletion
   changed.
2. **Reviewers, in parallel, in one message.** Launch `code-reviewer` when any `.rs`, `.ts`,
   `.tsx`, migration, manifest, workflow or script file changed; `docs-reviewer` when any `.md`,
   spec, ADR, or the PR body and commit messages need checking; `security-reviewer` when
   `stepclimb-auth`, `-server`, `-core`, a migration, or anything touching permissions,
   entitlements, sessions, feedback visibility or deletion changed; `ui-reviewer` when
   `packages/ui`, `packages/tokens`, `web/` or `apps/ui-docs` changed. Give each the PR number or
   commit range and nothing else: no conclusion of yours to confirm.
3. **Pitfall scan.** `.claude/scripts/pitfall-scan.sh origin/next` on the branch (for a PR, after
   `gh pr checkout $ARGUMENTS`). Each hit is a lead; confirm it against the diff.
4. **Disprove each finding.** Read the code and trace the path before accepting a finding: is it
   introduced by this change (`git diff origin/next...HEAD -- <file>`), is a test or guard already
   covering it? List pre-existing issues separately.
5. **Report.** Findings ranked most severe first with `file:line`, which reviewer raised each, and
   whether you confirmed it; what was verified; what nobody checked. Do not fix or merge here;
   the author or `implementer` fixes, and merging is a separate decision.
