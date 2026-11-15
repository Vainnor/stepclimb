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
`just release-version <x.y.z>` writes it everywhere; commit that change on its own.
`packages/ui` and `packages/tokens` keep their own versions, set by Changesets
([`.changeset/README.md`](.changeset/README.md)).

## What we will not merge

- Code copied or translated from Canvas, Moodle or another LMS. They are UX references only.
- Copyrighted course material, vNAS assets, or real member data in any form.
- VATSIM- or vZDC-specific behavior in the core crates; it belongs in a connector, a pack or
  configuration.
- Changes that loosen or skip a CI gate.

## Conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
