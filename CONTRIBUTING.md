# Contributing to Stepclimb

Thank you for wanting to help. Stepclimb has one maintainer: outside contributions are welcome
through pull requests, and every one is reviewed by hand, so small, focused changes get in fastest.

## Before you start

- Read [`AGENTS.md`](AGENTS.md): the workspace layout, the non-negotiables, the git workflow and
  the commands. The rules in [`.claude/rules/`](.claude/rules/) apply to everyone, not only to AI
  agents.
- Work from an issue. Features are specified in [`docs/specs/`](docs/specs/README.md) before they
  are built; if what you want to change has no spec or issue, open an issue first.
- The plan in [`docs/plan/`](docs/plan/README.md) is final. Proposals that change it go to an issue
  for the maintainer.

## Making a change

1. Fork, then branch from `next`: `{type}/{issue}/{short-desc}`, for example
   `fix/42/quiz-timer-rounding`.
2. `just install-hooks`, then `just ci` to record the baseline.
3. Write the change with its tests. Every route needs its four-case permission test and its
   cross-organization test; see [`.claude/rules/testing.md`](.claude/rules/testing.md).
   A change to `packages/ui` or `packages/tokens` adds a changeset
   ([`.changeset/README.md`](.changeset/README.md)).
4. `just ci` must pass on your final commit.
5. Commit with [Conventional Commits](https://www.conventionalcommits.org/) and a scope:
   `feat(auth): add passkey registration`. Lockfile-only changes go in their own commit. `Closes #N`
   is the only trailer.
6. Open a pull request against `next` and fill in the template. Pull requests are squash-merged.

## Developer Certificate of Origin

Contributions are accepted under the [Developer Certificate of Origin](https://developercertificate.org/).
Opening a pull request certifies that you wrote the change or have the right to submit it under
the project's licenses (AGPL-3.0-or-later; Apache-2.0 for `packages/api-client`); the pull request
template has a box for it. No `Signed-off-by` line is needed: `Closes #N` stays the only commit
trailer.

## Dependencies

Every version is pinned exactly and is at least 14 days old when it is added
([`.claude/rules/architecture.md`](.claude/rules/architecture.md) -> Dependencies). `just ci`
checks Rust crates with `cargo deny` (`deny.toml`) and npm packages with `just web-audit`, which
fails on a high or critical advisory. An advisory that does not apply to Stepclimb goes in
`pnpm-audit-allowlist.json` with the package, the reason and an expiry date; an expired entry, or
one that no longer matches a high or critical advisory, fails the gate; an entry may run at most 90
days (UTC). Without the network, `just web-audit` prints `SKIPPED`, `just ci` repeats that on its
last line, and the gate passes; run it online before you push. Behind a proxy, export
`HTTPS_PROXY`: the probe runs in Node, which reads the proxy only from the environment.

### Monthly dependency update

The maintainer updates dependencies by hand once a month and before each release; there is no
Dependabot.

1. Branch from `next`, for example `chore/<short-hash>/deps-2026-12`, with a clean tree.
2. `just update-deps`. It runs `cargo update` in both Cargo workspaces (the root and
   `apps/app/src-tauri`) and `pnpm update -r`, which keeps every exact pin in `package.json`. It
   moves the digests of the images in `deploy/Dockerfile` and `deploy/compose/` to their tag's
   current digest once that build is at least 14 days old, and prints how old each pinned build
   is. It checks every crate version in both lockfiles on crates.io: one published less than 14
   days ago is set back to the newest version of its series that is old enough, and the run fails
   when Cargo refuses that. pnpm applies the same rule itself (`minimumReleaseAge`). It prints
   the crates and images that changed and the ones it kept, fails if anything other than the
   lockfiles and image digests changed, and on a failure prints the `git checkout` that puts the
   files back. It asks crates.io about each of the roughly 600 crates in the two lockfiles
   once, at most one request a second, so it runs for 10 minutes or more.
3. `just ci`, `just deny` and `just app-build`.
4. Commit the lockfiles and digests on their own (`chore(deps): update lockfiles and image
   digests`) and open a pull request against `next`.

A major-version bump is not part of this update: it changes a pin, goes through the
`dependency-researcher` agent first and gets its own pull request. Pinning or upgrading a framework
(axum, sqlx, utoipa, tokio, React, Vite, Tauri, Tailwind) also needs an ADR.

## One version

The server, the web app and the Tauri app share one version: `[workspace.package] version` in
`Cargo.toml`. `apps/app/src-tauri/Cargo.toml`, `tauri.conf.json` and the lockfiles carry the same
value, and `just version-check` (part of `just ci`) fails when one differs.
`just release-version <x.y.z>` writes it everywhere, including `openapi/stepclimb.json`'s
`info.version` (utoipa takes it from the crate version), and adds the version's release notes to
`CHANGELOG.md` (Releases, below); commit the version on its own.
`packages/ui` and `packages/tokens` keep their own versions, set by Changesets
([`.changeset/README.md`](.changeset/README.md)).

## Releases

The maintainer cuts releases. Versions follow Semantic Versioning; before 1.0 a release is
`0.<milestone>.<patch>`, so M0 ends at `0.1.0`. No recipe pushes, tags or publishes: each one
prints the commands to run, and a person runs them. Run the monthly dependency update first.

1. From a clean worktree with `git-cliff` on the PATH (the Nix dev shell has it), run
   `just release-prepare 0.1.0`. It fetches `origin`, then refuses when the version is not newer
   than the one on `origin/next`, the tag `v0.1.0` exists, `CHANGELOG.md` on `origin/next`
   already has a `v0.1.0` section, a file `origin/main` changed reads differently on
   `origin/next`, there is no Conventional Commit to release, or a subject carries AI
   attribution. Otherwise it creates `chore/<hash>/release-v0.1.0` from `origin/next` and runs
   `just release-version 0.1.0`, which sets the version everywhere and has git-cliff add the
   `v0.1.0` section to `CHANGELOG.md`. It commits the version, then the notes (that commit's body
   names the `origin/next` commit the notes end at), and writes the release PR's body to
   `target/release/v0.1.0-pr.md`. If a step fails after the branch exists, it prints the
   `git reset --hard && git switch - && git branch -D ...` that removes it.
2. Read the new `CHANGELOG.md` section. The notes are the commit subjects since the previous
   release tag (`cliff.toml`): breaking changes first, then features, fixes, the other types and
   reverts. Leave the section as git-cliff wrote it: `just release` regenerates it and refuses a
   tag whose section differs. A subject that reads badly is fixed in the PR title before it
   merges.
3. `just ci`, push the branch, and open the release preparation PR against `next`
   (`chore(release): prepare v0.1.0`). Squash-merge it as usual, keeping the commit messages in
   the squash body. Merge nothing else into `next` until the release PR is merged: `just release`
   refuses a release whose `next` gained a commit after the notes were written.
4. Open the release PR from `next` to `main`:
   `gh pr create --base main --head next --title "chore(release): v0.1.0" --body-file target/release/v0.1.0-pr.md`.
   Merge it with a **merge commit**, never a squash, and keep `next`
   ([ADR 0030](docs/adr/0030-release-merge-commits.md)). `main` takes merge commits only; every
   PR into `next` is squash-merged.
5. Tag the merge commit on `main` with an annotated tag and push the tag:
   `git fetch origin && git tag -a v0.1.0 -m "Stepclimb v0.1.0" origin/main && git push origin v0.1.0`.
6. `git worktree add ../stepclimb-v0.1.0 v0.1.0`, then in that worktree `just release v0.1.0`.
   It stops unless the tree is clean and the tag is annotated, checked out, equal to the
   workspace version and a merge commit on `origin/main`'s first-parent history. It also stops
   when `next` gained anything but `chore(release)` commits after the notes were written, or
   when git-cliff, run again over the same commits, writes a different section. Then it writes
   the notes to `target/release/v0.1.0-notes.md`, runs `just ci` on the tag, and prints the
   `gh release create` command (with `--prerelease` for a version such as `0.1.0-rc.1`). Run that
   command to publish the GitHub Release.

Before the first release, the owner changes the repository's settings so `main` takes merge
commits only and `next` squash merges only (ADR 0030 -> Consequences); until then GitHub refuses
step 4's merge.

## What we will not merge

- Code copied or translated from Canvas, Moodle or another LMS. They are UX references only.
- Copyrighted course material, vNAS assets, or real member data in any form.
- VATSIM- or vZDC-specific behavior in the core crates; it belongs in a connector, a pack or
  configuration.
- Changes that loosen or skip a CI gate.

## Conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
