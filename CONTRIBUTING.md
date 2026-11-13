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

## What we will not merge

- Code copied or translated from Canvas, Moodle or another LMS. They are UX references only.
- Copyrighted course material, vNAS assets, or real member data in any form.
- VATSIM- or vZDC-specific behavior in the core crates; it belongs in a connector, a pack or
  configuration.
- Changes that loosen or skip a CI gate.

## Conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
