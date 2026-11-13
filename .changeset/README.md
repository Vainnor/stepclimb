# Changesets

Changesets versions the two UI packages inside this repository: `@stepclimb/ui` and
`@stepclimb/tokens`. Both stay `"private": true` and are never published (ADR 0015); the version
and the changelog are for the people who build on them here. Every other workspace package is in
`ignore` in `config.json`.

A change to `packages/ui` or `packages/tokens` adds a changeset in the same PR:

```bash
pnpm changeset          # pick the package and the bump, write one line on what changed
pnpm changeset --empty  # a change with nothing to note (a test, a story, a comment)
```

`just changeset-check`, part of `just web-lint` and so of `just ci`, fails a branch that changed
either package since `origin/next` without one. Commit the changeset before running the gate: an
untracked one is not counted.

At release, `pnpm changeset version` bumps the versions and writes each package's `CHANGELOG.md`.
A breaking change to a `stable` component ships with a codemod (`docs/plan/ui.md` -> Definition of
done); no component is stable yet.
