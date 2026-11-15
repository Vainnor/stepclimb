# 0030. Release PRs into main use a merge commit

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decider:** the owner (`docs/specs/m0/01-repo-and-ci.md` Q3, 13 Nov 2026; confirmed with
  `main` taking merge commits only on 2026-11-15); the release preparation PR is a coordinator
  decision on the owner's delegation (spec 01 D7, 2026-11-15)
- **Related:** ADR 0018 (squash merges only), ADR 0021; `docs/specs/m0/01-repo-and-ci.md` B8, B15;
  `bin/release.py`; `CONTRIBUTING.md` -> Releases

## Context

ADR 0018 squash-merges every PR, and a release PR promotes `next` to `main`. A squash merge into
`main` makes a commit `next` does not have, so the next release PR conflicts or carries the same
changes twice (spec 01, Q3). The release also has to carry the version bump and the release notes.
If those were committed on a branch that only `main` receives, `next` would keep the old version,
and the next bump would edit the same lines on both sides and conflict.

## Options

### Option A: merge commits for release PRs only
`main` gets a merge commit whose second parent is `next`'s head, so `main`'s history contains
`next`'s. Needs GitHub to allow merge commits on `main` while `next` stays squash-only.

### Option B: fast-forward `main` to `next`
No merge commit. GitHub's UI cannot do it, so the owner pushes `next:main` under an exception to
`git-guard`, outside the PR flow.

### Option C: squash, then merge `main` back into `next`
Two PRs per release. Bringing `main` back into `next` needs a merge commit on `next`, which
`next`'s squash-only rule does not allow, so C would also loosen `next`.

## Recommendation

A (spec 01, Q3).

## Decision

Option A. A release PR (`chore(release): vX.Y.Z`, head `next`, base `main`) is merged with a
merge commit and `next` is kept. `main` takes merge commits only; `next` takes squash merges only.
The version bump and the release notes reach `next` first, in an ordinary squash-merged release
preparation PR that `just release-prepare` sets up, so the release PR's head is `next` itself.
`just release-prepare` refuses to start when a file `origin/main` changed since it last met
`origin/next` reads differently on `origin/next`, and `just release` refuses a tag that is not a
merge commit on `origin/main`'s first-parent history.

## Consequences

- The owner changes the GitHub settings before the first release, since no file in the
  repository can: turn on "Allow merge commits" for the repository, and split ruleset 27175641 so
  that `main` allows the merge method only and `next` keeps squash only. Until then GitHub
  refuses the release PR's merge commit.
- Release PRs must not delete their head branch: `next` is protected from deletion by the
  ruleset, and `gh pr merge` is run without `--delete-branch`.
- A fix made directly on `main` (none is planned before 1.0) has to reach `next` through a PR
  before the next release.
- The tag goes on the merge commit on `main`; `just release` checks that it is there.
