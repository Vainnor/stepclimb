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
0022; `STEPCLIMB_ENCRYPTION_KEYS` or its `_FILE` variant is required), writes the file's
`[[providers]]` to their organizations at startup (spec 06 B13, secrets kept in memory), runs the embedded
migrations (fifteen: `organizations`, `users`, `accounts`, `sessions`, `roles`, `auth_flows`, `identities`, `plans`, `audit_events`, `local_provider`, `local_tokens`, `user_appearance`, `passkeys`, `passkey_counter_flag`, `auth_flow_provider`), writes the built-in roles and the `unlimited` plan, gives every organization without a plan the `unlimited` one (spec 10 B4, B5) and without one its local provider row (spec 04 B1), and serves `/healthz`, `/readyz`, the
permission catalog, roles and role assignment routes under `/api/v1/admin` (spec 09 I5, I6), the local sign-in switches at `/api/v1/admin/local-sign-in` (spec 04 I1),
`GET /api/v1/me`
(spec 03 I5), the caller's organization and its settings under `/api/v1/organization` (spec 02
I4), the account tree under `/api/v1/accounts` (spec 02 I5), the organization's branding
under `/api/v1/admin/branding` and its generated stylesheet at `/theme.css` (spec 12 I7, I8), the discovery document at
`/.well-known/stepclimb` (spec 08 I9), `/docs` (Scalar over a
utoipa OpenAPI 3.1 document of those routes) and the built web SPA. `stepclimb migrate`
applies the migrations and exits; `stepclimb admin create` creates the instance's organization and its
first Organization admin user in one transaction, and refuses once one exists (no sign-in link
yet: spec 04 I2); `stepclimb check-config` validates the configuration without
connecting and prints its non-secret values (exit 2 on bad configuration, 1 on a database
failure). `web/` routes with TanStack Router through a
signed-out layout (`/sign-in`, which cannot sign in yet: spec 11 I5) and a signed-in AppShell
(Home) that loads `GET /api/v1/me` through `packages/api-client`, sends a 401 to `/sign-in` with
its return target, and signs out (spec 11 I3, I4). `packages/tokens` builds the default and vZDC example
themes with Terrazzo; `packages/ui` has `Button` and `useStepclimbStrings()`, the ICU message
lookup every `web/` string goes through (spec 11 I1). `apps/app` is a Tauri 2 shell that loads
`web/`. `stepclimb-core` has UUID v7 ids, the permission catalog, the entitlement catalogs, the
built-in role lists and the controllable clock; `stepclimb-auth` has tokens, token hashes and the encryption key ring (spec 08
I1); the browser session (`stepclimb-core` `sessions`, `stepclimb-server` `session`) and the
request-organization resolver (with its `RequestOrg` extractor for unauthenticated routes) exist and authenticate the `/api/v1` routes, but no sign-in route creates a session yet (specs 04 to 07). `stepclimb-mock-idp` is the test-only mock identity provider. `stepclimb-jobs` has the job queue core (spec m1/01 I1: `jobs` table, `enqueue`, the claim, `Worker::run_once`, the `Registry`); `stepclimb serve --role all|web|worker` (`STEPCLIMB_ROLE`) runs the worker loop (`Worker::run`, I2) in `all` and `worker` processes, but no job kind is registered yet (I3). Every other crate and package is a documented placeholder. M0's work is specified in
`docs/specs/m0/` and broken into issues on the GitHub Project.

## Workspace layout

The plan's layout (`docs/plan/technical.md` -> Architecture and stack).

| Path | Kind | What it is |
| --- | --- | --- |
| `crates/stepclimb-server` | lib + bin | The axum app, handlers, config and the `stepclimb` binary. Today: config, startup and migrations, probes, OpenAPI and Scalar, the embedded SPA with its per-request `index.html` (theme fingerprint and appearance, `spa`), the session cookie and `Authenticated` extractor (`session`), bearer API-key authentication (`api_key`), the service account routes (`service_accounts`), the request-organization resolver and the `RequestOrg` extractor (`request_org`), the sign-in and token rate limits (`rate_limit`), the trusted-proxy client address (`client_ip`) and the `ClientAddress` extractor audited routes record it with (`audit`), `Require<P, T>` and `Require<F>` (`require`), the per-request entitlements and the limit check (`entitlements`), the permission catalog and roles routes (`roles`) with RFC 3339 timestamps (`timestamp`), the role assignment routes (`role_assignments`), the signed-in user with their grants and features (`me`), their appearance choice (`appearance`), the organization, settings and entitlements routes (`organization`), the account routes (`accounts`), the branding routes and `/theme.css` (`branding`), the web app manifest (`manifest`), `/.well-known/stepclimb` (`discovery`), passkey sign-in (`passkey_sign_in`), the start of sign-in with an OIDC or OAuth 2 provider (`provider_sign_in`), the provider callback (`provider_callback`), the generic OAuth 2 sign-in through admission (`oauth2_sign_in`), provider presets read at every config read (`presets`), over the provider clients picked by organization count (`providers`), the OAuth 2.1 authorization endpoint for Stepclimb's own apps with its confirmation and error pages (`oauth_authorize`, `oauth_page`), its token and revocation endpoints (`oauth_token`), bearer app access token authentication (`app_token`), course file storage with its startup probe and the `storage.not_configured` answer (`storage`, M1 spec 06 I1), and `stepclimb admin create` (`admin`). |
| `crates/stepclimb-core` | lib | Domain types, repositories (SQL), the permission catalog. Today: the embedded migrations (`db::MIGRATOR`), UUID v7 keys (`id`), the permission catalog (`perm`), the built-in role lists (`roles::BUILTIN`), the controllable clock (`clock::Clock`), `OrgScope`, the `organizations` repository with `OrgSettings`, the `accounts` repository (the tree inside an organization), the `courses` repository (M1 spec 02 I1), the stored `files` repository, once per SHA-256 per organization (M1 spec 06 I1), the `modules` and `items` repositories (a course's outline, M1 spec 03 I1), the `users` repository and `Source`, the `sessions` repository with the browser-session credential resolver, the `service_accounts` repository with the API-key credential resolver, the `identity_providers` (read side) and `identities` repositories, `local_sign_in` (the local provider row and its method switches), the `passkeys` repository, the entitlement catalogs (`feature`: features and `Limit`), the `plans` repository with `PlanEntitlements`, the `org_plans` repository with the limit check (`check_room`), `requirement`, the trait both catalogs implement for `Require`, and `audit`, the entries the repository functions that change roles, assignments, accounts, organizations and user status write with the change (courses too), the theme generator's Rust port (`theme`), the `branding` repository (`organizations.branding`), a user's theme and density (`appearance`), the OAuth 2.1 authorization codes (`oauth_codes`), and its refresh token families with the app access token resolver (`oauth_refresh_tokens`). |
| `crates/stepclimb-auth` | lib | Providers, sessions, API keys, the OAuth 2.1 server for Stepclimb's own apps. Today: tokens, API keys and their hashes (`token`), the AES-256-GCM key ring (`encryption`), the outbound HTTP client and URL policy (`outbound`), the `oidc` and `oauth2` provider configs (`oidc_config`, `oauth2_config`) over what they share, problems and the sealed client secret (`provider_config`), OIDC discovery with its cache and the authorization request (`oidc`), the generic OAuth 2 authorization request, code exchange on the `oauth2` crate and userinfo call (`oauth2_flow`), the claim mapping of both kinds (`claims`), JSON Pointers (`json_pointer`), the WebAuthn relying party for passkeys (`passkey`, on webauthn-rs), and the OAuth 2.1 client registry, authorization-request and token-request checks and the HS256 app access tokens (`oauth_server`). |
| `crates/stepclimb-assess` | lib | Question types, selection, grading; pure, also wasm32. Placeholder (M2). |
| `crates/stepclimb-activities` | lib | Activity engine: trait, sessions, event log, mastery rules. Placeholder (M3). |
| `crates/stepclimb-activities-atc` | lib | Optional ATC pack. Placeholder (M3, M5). |
| `crates/stepclimb-interop` | lib | Course archive, CSV; later GIFT, QTI, LTI, Common Cartridge. Placeholder (M1 to M4). |
| `crates/stepclimb-integrations` | lib | Sync API, SCIM, permission modes, reports and exports. Placeholder (M4). |
| `crates/stepclimb-connectors` | lib | Optional connectors on the public API: vatsim, vatusa. Today: the VATSIM Connect sign-in preset's data (`vatsim::PRESET`, feature `vatsim`), which the server reads in `presets`; the connectors themselves are M4. |
| `crates/stepclimb-jobs` | lib | Postgres queue, outbox, email, webhooks, push, auto-submit. Today: the job queue core (`enqueue` with dedupe, `queue::claim` with `SKIP LOCKED` and leases, `Worker::run_once`, the worker loop `Worker::run` with concurrency, polling and graceful stop, the `Registry`, retry backoff). |
| `crates/stepclimb-mock-idp` | lib + bin | The mock OIDC and OAuth 2 identity provider every sign-in test runs against, on 127.0.0.1 (spec 06 I1, ADR 0027). Dev-dependency only. The `stepclimb-mock-idp` binary runs it as the `mock-idp` service of the `just e2e` stack only (`deploy/compose/ci.yaml`, Dockerfile target `mock-idp`; spec 06 B3). |
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
| `deploy/` | | `Dockerfile`, `compose/` (development: Postgres 16, MinIO, the server; the production example with optional Caddy and nightly dumps), `helm/` (placeholder, 1.0). |
| `docs-site/` | | Admin, author, API and self-hosting guides. Placeholder (M4). |
| `docs/` | | `plan/`, `adr/`, `specs/`, `agents.md`. |
| `references/`, `private/` | | Local-only: reference material; member exports and data sets. Only their READMEs (and `references/INDEX.md`) are committed. |
| `.claude/`, `.codex/`, `.agents/` | | Agent setup for Claude Code and Codex, one copy of each rule, hook, agent and command (`docs/agents.md`). |

Dependency rules, enforced by `bin/check-layering.sh` (`.claude/rules/architecture.md` ->
Layering). The plan fixes the first four; ADR 0027 adds the mock IdP's. Anything else between
library crates is allowed until an ADR says otherwise.

```
  server          top of the graph: nothing depends on it
  activities-atc  ┐ only the server may depend on these (cargo features, enabled
  connectors      ┘ per organization); connectors depend on no workspace crate
  assess          no workspace crate, no I/O, runtime or HTTP crate (pure, wasm32)
  mock-idp        no normal or build dependency on a workspace crate; a dev-dependency
                  only, never a normal or build one
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
  against `next`, promote `next` to `main` with a release PR (`CONTRIBUTING.md` -> Releases),
  the one PR merged with a merge commit (ADR 0030).
- Branch names: `{type}/{issue-or-hash}/{short-desc}`, e.g. `feat/12/passkey-registration`.
- Commits follow Conventional Commits with a scope: `type(scope): summary`, imperative, at most
  about 70 characters. Types: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `chore`, `ci`,
  `build`. Scopes are crate or area names without the prefix: `server`, `core`, `auth`, `assess`,
  `activities`, `atc`, `interop`, `integrations`, `connectors`, `jobs`, `media`, `db`, `web`,
  `ui`, `tokens`, `api-client`, `app`, `ui-docs`, `deploy`, `docs`, `plan`, `adr`, `specs`,
  `claude`, `ci`, `deps`.
- Reference issues with `Closes #N` in the PR body; that is the only trailer.
- Lockfile-only changes and toolchain or framework version bumps go in their own commit.
- PR only; PRs into `next` squash-merge, release PRs into `main` take a merge commit (ADR 0030). Never push to `main` or `next` directly; `.claude/hooks/git-guard`
  refuses it.
- Dependencies are updated by hand once a month and before each release, as in Mainline:
  `just update-deps` (`cargo update` in both Cargo workspaces, `pnpm update`, image digests, each
  new version at least 14 days old), then `just ci`, `just deny` and `just app-build`, opened as a
  lockfile-only PR (`CONTRIBUTING.md` -> Monthly dependency update). A framework or major-version bump goes through
  `dependency-researcher` first. Before bumping `react-aria-components`, read its release notes for
  the `UNSTABLE_` toast parts Stepclimb UI builds Toast on (ADR 0024). There is no Dependabot (owner, 13 Nov 2026).
- The remote is the private GitHub repository `Vainnor/stepclimb`; `next` is the default branch.
  A GitHub ruleset on `main` and `next` enforces PR-only, squash-only, and no force push or
  deletion (ADR 0018). It requires no status checks: there is no hosted CI (ADR 0021). `.claude/hooks/git-guard` refuses the
  same pushes locally.
- Issues come from the specs' Issues sections, one per PR, on the GitHub Project "Stepclimb"
  (Todo, In progress, In review, Done); `.claude/scripts/board-move.sh` moves a card.

## Commands (via `just`)

```bash
just ci            # the merge gate, run locally: secrets, fmt, clippy, layering, lint escapes, spdx, versions, tests, deny, sqlx, openapi, biome, npm audit, tsc, vitest, story screenshots, playwright, compose, image, agents; several worktrees may run it at once
just app-build     # the Tauri shell for this OS, unbundled and unsigned, after cargo-deny on its workspace
just up / down     # PostgreSQL 16 and MinIO in Compose (deploy/compose/compose.yaml), one stack shared by every worktree
just serve         # the server on :8080 against the Compose database, web build embedded
just dev           # Vite on :5173, proxying the API to `just serve`
just storybook     # Storybook on :6006
just check         # cargo check --workspace --all-targets --locked
just fmt / fmt-check   # cargo fmt
just lint          # cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
just test-rust     # web build, then cargo nextest run --workspace and doctests
just layering      # bin/check-layering.sh
just lint-escapes  # bin/check-lint-escapes.py: no way around the OrgScope lint gate (inner allows, wide expects, lint config)
just spdx-check    # bin/check-spdx.sh: SPDX headers on every source file
just deny          # cargo deny check
just version-check # bin/check-versions.sh: one version in Cargo.toml, the app's manifests, tauri.conf.json and the lockfiles
just release-version <x.y.z>  # write that version everywhere (OpenAPI info.version included), refresh both Cargo lockfiles, add its CHANGELOG.md notes (git-cliff)
just release-prepare <x.y.z>  # branch from origin/next, run release-version, commit the version and the notes, print the release steps; pushes nothing
just release <vX.Y.Z>  # in a worktree of the annotated merge tag on main, on amd64: check it and its notes, just ci, build the amd64 and arm64 image archive and SBOMs; publishes nothing
just release-publish <vX.Y.Z> [--dry-run]  # the owner's step: push the image to GHCR, cosign-sign it, attach the SBOMs, create the GitHub Release; --dry-run prints the commands
just update-deps   # the monthly update: both Cargo lockfiles, pnpm-lock.yaml and image digests, 14 days old or more (bin/update_deps.py)
just migrate       # apply migrations to the local database
just sqlx-prepare  # regenerate .sqlx/ after changing a query (against a database of its own)
just sqlx-check    # fail if .sqlx/ is stale (against a database of its own)
just openapi       # regenerate openapi/stepclimb.json and the TypeScript client
just openapi-diff  # fail if either is stale
just web-build     # tokens, then web/dist
just web-lint / web-fmt  # Biome, its UI rules and bin/check-ui-lint.mjs (raw colors, emoji)
just changeset-check  # a changeset for any change to packages/ui or packages/tokens (in web-lint)
just web-audit     # pnpm audit (prod, then all) fails on high or critical outside pnpm-audit-allowlist.json; SKIPPED offline
just typecheck     # tsc in every package
just test-web      # Vitest in every package
just e2e           # Playwright tests against this worktree's own Compose stack, with the mock identity provider as a service (builds both images)
just ui-visual     # Storybook story screenshots and axe in Playwright's pinned image (--update-snapshots)
just compose-check # the production Compose example booted with generated secrets on the slot's port, then removed
just compose-ops-check  # the same with Caddy (internal CA) on the slot's ports: TLS, forwarded headers, dumps, a restore
just image-check   # the server image for linux/amd64 and linux/arm64 (arm64 only on arm64): non-root, no shell, architecture, check-config runs
just agents-check  # agent hook tests and Codex mirrors of .claude/ (docs/agents.md)
just secrets-scan  # gitleaks over the tree (staged | history also accepted)
just clean-stale [--dry-run] [--days N]  # stale build output in every worktree's target/
just install-hooks # once per clone: .githooks (secret scan, formatting, no-attribution commit-msg)
```

Needs: the Nix dev shell (`nix develop`, or `use flake` in a local `.envrc`), or the same tools
installed by hand: Rust via rustup, `just`, `cargo-nextest`, `cargo-deny`, `sqlx-cli`,
`gitleaks` (or Docker), `git-cliff`, `skopeo` and `cosign` (releases), Node 26.13 with pnpm 12.9, Playwright's Chromium, `python3`, Docker with
Compose (with buildx), and on Linux WebKitGTK 4.1 for the Tauri app and QEMU's user-mode
emulators (`qemu-aarch64`) for `just image-check`.

There is no hosted CI and no GitHub Actions (ADR 0021): Actions minutes on a private repository
run out too fast. `just ci` on the committed HEAD, on the machine the change was made on, is the
merge gate, and every "done" names that machine's OS. `bin/ci-stamp.sh` records the pass and the
`pr-gate` hook refuses `gh pr create` without it.

Run the gate as plain `just ci` (`nix develop -c just ci`), from as many worktrees as you like:
no `flock` wrapper. `bin/ci_gate.py` holds one of `SC_CI_SLOTS` (default 3) slots in `/tmp`, so
the rest wait their turn, and one run per worktree. Each run gets its own PostgreSQL, a MinIO
with the bucket `stepclimb` for the storage tests (its URL in `SC_CI_MINIO_URL`), Compose
projects (`stepclimb-ci-<id>`, `stepclimb-prodcheck-<id>`, `stepclimb-opscheck-<id>`, images
`stepclimb:ci-<id>` and `stepclimb:ci-<id>-mock-idp`, `<id>` from the worktree path) and its slot's ports (`20000 + 10 * slot`
and up) and proxy subnets (`10.201.(100 + slot)`, `10.201.(150 + slot)`), so checkouts with
different migrations never share a database and runs never share a port or subnet.

`just e2e`, `just compose-check` and `just compose-ops-check` always run inside the gate, alone
or not, and use the slot's ports. The fixed ports in `bin/check-production-compose.sh` (28180)
and `bin/check-compose-ops.sh` (28280, 28281, 28443) are only their defaults when run directly
with `bash`. All of these sit below Linux's ephemeral range (32768 to 60999), where a host socket
such as an outgoing connection's source port in TIME-WAIT can hold a port, and rootless Docker
cannot see that when it picks one. The shared `just up` stack's fixed ports (55432, 58080,
59000, 59001) are inside that range, so `just up` can occasionally hit it; no gate binds them.

The run removes its projects and images when it ends, whether it passed, failed or was stopped by
SIGTERM, SIGHUP or SIGINT. At its start it removes what a killed run of the worktree left, any
such project whose worktree no longer exists, and any gate project labelled with its slot or on
its slot's subnets (a killed run's, since the slot is exclusive); a project that is not a gate's
on those subnets stops the run instead. It never uses or stops the shared `just up` stack.

A gate under the old `flock /tmp/stepclimb-ci.lock <command>` wrapper still works: the wrapper
waits for running gates and runs alone, and the gate inside it skips the lock its wrapper holds.
The descriptor form (`exec 9>/tmp/stepclimb-ci.lock; flock 9`) is not recognized, and a gate
inside it waits forever. Every gate takes the old lock first, then the worktree's, then a slot,
so no two can wait on each other. The stamp records the commit the run started on, and nothing
if HEAD moved during the run or the tree had changes when it started.

`just app-build` builds the Tauri shell for the local OS; macOS and Windows builds, and jobs of
about an hour or more, run on a machine or VM the owner provides when a change needs them, and the
PR names the OSes it ran on.

## Testing and verification

- Tests live next to the code (`#[cfg(test)] mod tests`, `*.test.ts(x)`) for units and in
  crate `tests/` for behavior across modules. Database tests use `#[sqlx::test]` with
  `DATABASE_URL`: the gate run's own database inside `just ci`, otherwise the justfile's default,
  the shared `just up` database. `#[sqlx::test]` names a test's database after the test's path,
  so the same test run from two worktrees against the shared database can drop the other's.
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
