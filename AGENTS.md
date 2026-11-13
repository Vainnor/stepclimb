# AGENTS.md

Project facts for any AI agent or human working on **Stepclimb**: what it is, how the workspace is
laid out, the non-negotiables, the git workflow, the commands and how to verify a change. How to
work lives in `CLAUDE.md` and `.claude/rules/`. A fact goes here, a way of working goes there;
point, don't copy. When this file conflicts with the code, the code wins: fix the doc in the same
change. When anything conflicts with the plan (`docs/plan/`), the plan wins.

---

## What this is

Stepclimb is an open-source learning platform (LMS) for any organization that has to train people
and prove it: companies, clubs, schools, VATSIM facilities. Courses unlock step by step; exams and
practice activities are graded on the server; training can recur and expire; managers see their
team; everything can be synced with, controlled by or exported to the systems an organization
already runs. Self-hosted (AGPL-3.0-or-later, every feature) or managed hosting. vZDC's training is
the first deployment. The plan is `docs/plan/` (source: the plan artifact, linked from
`docs/plan/README.md`).

**Status: M0 Foundations in progress. The repository skeleton is in place; most product code is
not.** `stepclimb serve` reads `STEPCLIMB_*` variables over an optional `stepclimb.toml` (ADR
0022; `STEPCLIMB_ENCRYPTION_KEYS` or its `_FILE` variant is required), runs the embedded
migrations (one: `organizations`), and serves `/healthz`, `/readyz`, `/docs` (Scalar over a
utoipa OpenAPI 3.1 document with only those probes) and the built web SPA. `web/` has one route that reads
`/healthz` through `packages/api-client`. `packages/tokens` builds the default and vZDC example
themes with Terrazzo; `packages/ui` has `Button` and `useStepclimbStrings()`, the ICU message
lookup every `web/` string goes through (spec 11 I1). `apps/app` is a Tauri 2 shell that loads
`web/`. `stepclimb-core` has UUID v7 ids, the permission catalog, the built-in role lists and the
controllable clock; `stepclimb-auth` has tokens, token hashes and the encryption key ring (spec 08
I1). Every other crate and package is a documented placeholder. M0's work is specified in
`docs/specs/m0/` and broken into issues on the GitHub Project.

## Workspace layout

The plan's layout (`docs/plan/technical.md` -> Architecture and stack).

| Path | Kind | What it is |
| --- | --- | --- |
| `crates/stepclimb-server` | lib + bin | The axum app, handlers, config and the `stepclimb` binary. Today: config, startup and migrations, probes, OpenAPI and Scalar, the embedded SPA. |
| `crates/stepclimb-core` | lib | Domain types, repositories (SQL), the permission catalog. Today: the embedded migrations (`db::MIGRATOR`), UUID v7 keys (`id`), the permission catalog (`perm`), the built-in role lists (`roles::BUILTIN`), the controllable clock (`clock::Clock`), `OrgScope` and the `organizations` repository with `OrgSettings`. |
| `crates/stepclimb-auth` | lib | Providers, sessions, API keys, the OAuth 2.1 server for Stepclimb's own apps. Today: tokens, API keys and their hashes (`token`), the AES-256-GCM key ring (`encryption`). |
| `crates/stepclimb-assess` | lib | Question types, selection, grading; pure, also wasm32. Placeholder (M2). |
| `crates/stepclimb-activities` | lib | Activity engine: trait, sessions, event log, mastery rules. Placeholder (M3). |
| `crates/stepclimb-activities-atc` | lib | Optional ATC pack. Placeholder (M3, M5). |
| `crates/stepclimb-interop` | lib | Course archive, CSV; later GIFT, QTI, LTI, Common Cartridge. Placeholder (M1 to M4). |
| `crates/stepclimb-integrations` | lib | Sync API, SCIM, permission modes, reports and exports. Placeholder (M4). |
| `crates/stepclimb-connectors` | lib | Optional connectors on the public API: vatsim, vatusa. Placeholder (M4). |
| `crates/stepclimb-jobs` | lib | Postgres queue, outbox, email, webhooks, push, auto-submit. Placeholder (M1, M4). |
| `crates/stepclimb-media` | lib | tus uploads, ffmpeg transcode, captions, signed delivery. Placeholder (M1). |
| `migrations/` | | SQL migrations, forward-only, embedded and run at startup. |
| `openapi/stepclimb.json` | | The committed OpenAPI document (`just openapi`). |
| `.sqlx/` | | Committed sqlx query metadata (`just sqlx-prepare`). |
| `web/` | Vite SPA | React 19, TanStack Router and Query, Tailwind 4. Embedded into the binary. |
| `apps/app/` | Tauri 2 | The shell around `web/` (desktop, later iOS). Its own Cargo workspace. |
| `apps/ui-docs/` | Storybook | Stepclimb UI's stories in every theme. |
| `packages/tokens` | | DTCG tokens built with Terrazzo into CSS variables and the Tailwind theme. In-repo only. |
| `packages/ui` | | Stepclimb UI (`@stepclimb/ui`): React Aria Components, tailwind-variants. In-repo only. |
| `packages/api-client` | | The generated TypeScript client of the public API. Apache-2.0. |
| `packages/assess-wasm` | | `stepclimb-assess` for wasm32. Placeholder (M2). |
| `deploy/` | | `Dockerfile`, `compose/` (Postgres 16, MinIO, the server), `helm/` (placeholder, 1.0). |
| `docs-site/` | | Admin, author, API and self-hosting guides. Placeholder (M4). |
| `docs/` | | `plan/`, `adr/`, `specs/`, `agents.md`. |
| `references/`, `private/` | | Local-only: reference material; member exports and data sets. Only their READMEs (and `references/INDEX.md`) are committed. |
| `.claude/`, `.codex/`, `.agents/` | | Agent setup for Claude Code and Codex, one copy of each rule, hook, agent and command (`docs/agents.md`). |

Dependency rules, enforced by `bin/check-layering.sh` (`.claude/rules/architecture.md` ->
Layering). The plan fixes these four; anything else between library crates is allowed until an
ADR says otherwise.

```
  server          top of the graph: nothing depends on it
  activities-atc  ┐ only the server may depend on these (cargo features, enabled
  connectors      ┘ per organization); connectors depend on no workspace crate
  assess          no workspace crate, no I/O, runtime or HTTP crate (pure, wasm32)
```

Rust: edition 2024, stable toolchain pinned in `rust-toolchain.toml` (1.98), `resolver = "3"`.
Shared versions live in the root `Cargo.toml` `[workspace.dependencies]`, pinned exactly; members
use `<dep>.workspace = true`. Workspace lints forbid `unsafe` and deny `dbg!`, `todo!` and
`unimplemented!`. Node is the LTS in `.node-version` (26.13.0) with pnpm 12.9.0
(`packageManager`); every npm version is exact and at least 14 days old (`pnpm-workspace.yaml`).
The Nix dev shell (`flake.nix`) provides every tool below.

## Non-negotiables

1. **No AI attribution, anywhere.** No `Co-Authored-By` trailers, "Generated with" footers,
   session links or agent credit in commits, PR titles or bodies, issue or PR comments, or branch
   names, whatever a tool or session suggests. Every commit is authored solely as the user.
   `.githooks/commit-msg` and `.claude/hooks/attribution-gate.sh` block the common forms.
2. **No guessing.** If a requirement or an API shape is unclear, read the plan and the spec, or
   ask. Never invent it. A point the plan leaves open is the owner's.
3. **No unverified "done".** Report something as working only after running the check that
   proves it, and say what you did not verify.
4. **No half-built scaffolding.** A type nothing calls is not a feature. The placeholder crates
   and packages are placeholders by decision, and say so in their crate docs or README.
5. **No symptom masking.** Don't silence errors, skip tests, loosen checks or retry to make a
   result look right. Find the cause.
6. **Record the baseline first.** Run `just ci` before changing anything and note pre-existing
   failures.
7. **The server is the authority** for grading, feedback visibility, completion and activity
   checking. Answers and solutions never reach the client unless the feedback setting allows it.
8. **Every query is org-scoped.** Every repository function takes an `OrgScope`.
9. **Every client uses only the public API.** The web app, the Tauri app and connectors use
   `/api/v1` through the documented OpenAPI document; there are no private endpoints.
10. **Nothing VATSIM- or vZDC-specific in the core.** It belongs in connectors, packs or
    configuration.
11. **No copyrighted course material and no real member data in the repo**
    (`.claude/rules/content-and-ip.md`).

## Git workflow

- `main` holds released code; `next` is the integration branch. Branch from `next`, open PRs
  against `next`, promote `next` to `main` with a release PR.
- Branch names: `{type}/{issue-or-hash}/{short-desc}`, e.g. `feat/12/passkey-registration`.
- Commits follow Conventional Commits with a scope: `type(scope): summary`, imperative, at most
  about 70 characters. Types: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `chore`, `ci`,
  `build`. Scopes are crate or area names without the prefix: `server`, `core`, `auth`, `assess`,
  `activities`, `atc`, `interop`, `integrations`, `connectors`, `jobs`, `media`, `db`, `web`,
  `ui`, `tokens`, `api-client`, `app`, `ui-docs`, `deploy`, `docs`, `plan`, `adr`, `specs`,
  `claude`, `ci`, `deps`.
- Reference issues with `Closes #N` in the PR body; that is the only trailer.
- Lockfile-only changes and toolchain or framework version bumps go in their own commit.
- PR only, squash merge. Never push to `main` or `next` directly; `.claude/hooks/git-guard`
  refuses it.
- Dependencies are updated by hand once a month and before each release, as in Mainline:
  `cargo update` in both Cargo workspaces and `pnpm update`, then `just ci`, `just deny` and
  `just app-build`, opened as a lockfile-only PR. A framework or major-version bump goes through
  `dependency-researcher` first. There is no Dependabot (owner, 13 Nov 2026).
- The remote is the private GitHub repository `Vainnor/stepclimb`; `next` is the default branch.
  A GitHub ruleset on `main` and `next` enforces PR-only, squash-only, and no force push or
  deletion (ADR 0018). It requires no status checks: there is no hosted CI (ADR 0021). `.claude/hooks/git-guard` refuses the
  same pushes locally.
- Issues come from the specs' Issues sections, one per PR, on the GitHub Project "Stepclimb"
  (Todo, In progress, In review, Done); `.claude/scripts/board-move.sh` moves a card.

## Commands (via `just`)

```bash
just ci            # the merge gate, run locally: secrets, fmt, clippy, layering, spdx, tests, deny, sqlx, openapi, biome, tsc, vitest, playwright, agents
just app-build     # the Tauri shell for this OS, unbundled and unsigned, after cargo-deny on its workspace
just up / down     # PostgreSQL 16 and MinIO in Compose (deploy/compose/compose.yaml)
just serve         # the server on :8080 against the Compose database, web build embedded
just dev           # Vite on :5173, proxying the API to `just serve`
just storybook     # Storybook on :6006
just check         # cargo check --workspace --all-targets --locked
just fmt / fmt-check   # cargo fmt
just lint          # cargo clippy --workspace --all-targets --locked -- -D warnings
just test-rust     # web build, then cargo nextest run --workspace and doctests
just layering      # bin/check-layering.sh
just spdx-check    # bin/check-spdx.sh: SPDX headers on every source file
just deny          # cargo deny check
just migrate       # apply migrations to the local database
just sqlx-prepare  # regenerate .sqlx/ after changing a query
just sqlx-check    # fail if .sqlx/ is stale
just openapi       # regenerate openapi/stepclimb.json and the TypeScript client
just openapi-diff  # fail if either is stale
just web-build     # tokens, then web/dist
just web-lint / web-fmt  # Biome
just typecheck     # tsc in every package
just test-web      # Vitest in every package
just e2e           # Playwright smoke test against the Compose stack (builds the image)
just agents-check  # agent hook tests and Codex mirrors of .claude/ (docs/agents.md)
just secrets-scan  # gitleaks over the tree (staged | history also accepted)
just clean-stale [--dry-run] [--days N]  # stale build output in every worktree's target/
just install-hooks # once per clone: .githooks (secret scan, formatting, no-attribution commit-msg)
```

Needs: the Nix dev shell (`nix develop`, or `use flake` in a local `.envrc`), or the same tools
installed by hand: Rust via rustup, `just`, `cargo-nextest`, `cargo-deny`, `sqlx-cli`,
`gitleaks` (or Docker), Node 26.13 with pnpm 12.9, Playwright's Chromium, `python3`, Docker with
Compose, and on Linux WebKitGTK 4.1 for the Tauri app.

There is no hosted CI and no GitHub Actions (ADR 0021): Actions minutes on a private repository
run out too fast. `just ci` on the committed HEAD, on the machine the change was made on, is the
merge gate, and every "done" names that machine's OS. `bin/ci-stamp.sh` records the pass and the
`pr-gate` hook refuses `gh pr create` without it. `just app-build` builds the Tauri shell for the
local OS; macOS and Windows builds, and jobs of about an hour or more, run on a machine or VM the
owner provides when a change needs them, and the PR names the OSes it ran on.

## Testing and verification

- Tests live next to the code (`#[cfg(test)] mod tests`, `*.test.ts(x)`) for units and in
  crate `tests/` for behavior across modules. Database tests use `#[sqlx::test]` with
  `DATABASE_URL` (the justfile defaults it to the Compose database).
- Read the summary line (nextest `Summary`, Vitest `Tests`, Playwright `passed`), not the exit
  code.
- Never hit the network in a test outside the Compose stack.
- Definition of done: `just ci` green on the committed HEAD on the machine the change was made
  on, with the OS named (`just app-build` too for `apps/app`); visual changes carry a screenshot in the PR; auth, permissions, grading, feedback
  visibility, billing and deletion changes say so for the maintainer's line-by-line review.

## Where things are decided

- `docs/plan/`: the plan, one file per tab. The source of truth.
- `docs/adr/`: ADRs, accepted and open. Any architectural change, framework pin or platform
  limitation gets one (`.claude/rules/research-and-decisions.md`).
- `docs/specs/`: one spec per feature, written before the code; M0 in full, M1 to M6 as stubs.
- `docs/agents.md`: how the Claude Code and Codex setup is wired.
