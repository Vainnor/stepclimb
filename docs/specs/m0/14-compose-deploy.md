# Compose deploy

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Hosting and operations (Docker Compose, Images,
  Configuration, CLI, Backups, Operations), Architecture and stack (Database: migrations run at
  startup), Security and privacy (Secrets, Supply chain), Testing (Web: Playwright against the
  Compose stack), Milestones (M0 "Compose deploy", M4, 1.0); `docs/plan/product.md` -> Ways to
  run it
- **Depends on:** 01-repo-and-ci (the CI gates, the release workflow); the configuration keys of
  02, 05, 06, 07 and 08, which this spec loads but does not define
- **Review:** standard

## Summary

A self-hoster can run Stepclimb with Docker Compose: the server and PostgreSQL, with optional
Caddy for automatic TLS and optional MinIO, configured by `STEPCLIMB_*` variables over an
optional `stepclimb.toml`, secrets from files, migrations applied at startup and probes for the
orchestrator. M0 delivers the production example beside today's development stack, the
configuration loader the other M0 specs add keys to, the image requirements, backups and a
self-hosting guide in the repository.

## What exists today

Read on 2026-11-12.

- **`deploy/compose/compose.yaml`** (project `stepclimb`): `postgres` on
  `postgres:16.16-alpine` pinned by digest, user and database `stepclimb`, password
  `stepclimb-local`, published on `127.0.0.1:55432`, named volume `postgres`, a `pg_isready`
  healthcheck; `minio` on `cgr.dev/chainguard/minio:latest` pinned by digest (ADR 0019,
  Proposed), root user `stepclimb` with the same local password, API and console on
  `127.0.0.1:59000` and `:59001`, named volume `minio`; `stepclimb` in the `stack` profile, built
  from `deploy/Dockerfile` as `stepclimb:local`, `STEPCLIMB_DATABASE_URL` and `STEPCLIMB_BIND` set
  inline, `STEPCLIMB_ENCRYPTION_KEYS_FILE` pointing at the Compose secret
  `encryption_keys` (the published development key in
  `deploy/compose/development-encryption-keys`), published on `127.0.0.1:58080`, starting after `postgres` is healthy. The header says
  the credentials are for a local machine only.
- **`just` recipes** (`just/stack.just`): `up` (postgres and minio, `--wait`), `up-stack` (the
  whole stack, rebuilt), `down` (keeps volumes), `e2e` (Playwright against `up-stack`).
- **`deploy/Dockerfile`**: a Node 26.13 stage builds the tokens and `web/dist`; a Rust 1.98 stage
  builds `stepclimb` in release mode with the SPA embedded; the final stage is
  `gcr.io/distroless/cc-debian12:nonroot`, all three pinned by digest, `USER nonroot`,
  `EXPOSE 8080`, `STEPCLIMB_BIND=0.0.0.0:8080`, entry point `stepclimb serve`. It builds for the
  host's architecture only; nothing builds multi-arch, signs or attaches an SBOM, and
  nothing pushes an image. There is no hosted CI (ADR 0021).
- **Configuration** (`crates/stepclimb-server/src/config.rs`): `STEPCLIMB_BIND` (default
  `127.0.0.1:8080`, must parse as a socket address), `STEPCLIMB_DATABASE_URL` or
  `STEPCLIMB_DATABASE_URL_FILE` (exactly one; the file is read and trimmed; the URL must start
  with `postgres://` or `postgresql://`), `STEPCLIMB_ENCRYPTION_KEYS` or
  `STEPCLIMB_ENCRYPTION_KEYS_FILE` (exactly one, required; the AES-256-GCM key ring of
  `08-sessions-and-csrf.md` B3). Each error names the variable and never repeats a secret. `main.rs` prints
  `stepclimb: configuration error: <message>` and exits with code 2; `STEPCLIMB_LOG` sets the
  JSON log filter (default `info`). No TOML file, no other keys. The server logs a warning at
  startup naming any key in the ring that is the published development key.
- **Startup** (`startup.rs`): connect with a 5 s pool acquire timeout, run the embedded
  migrations (`stepclimb_core::db::MIGRATOR`; one migration, `organizations`), then listen, with
  graceful shutdown on SIGINT and SIGTERM. A connect or migration failure exits non-zero with the
  cause.
- **Probes** (`health.rs`): `/healthz` answers `{"status":"ok"}` whenever the process runs;
  `/readyz` answers 200 when `SELECT 1` returns within 2 s, otherwise 503 `unavailable`. Both are
  in the OpenAPI document. The CLI has only `serve`.
- `deploy/helm/` is empty (the Helm chart is 1.0).

## Behavior

### Two Compose files, two purposes

- **B1.** The development stack stays as it is: local credentials, ports on `127.0.0.1`, MinIO
  for the M1 media work, the server in the `stack` profile, used by `just up` and `just up-stack`.
  `just e2e` starts the same file, with `deploy/compose/ci.yaml` over it, as a project of its own
  per worktree (`bin/ci_gate.py`).
- **B2.** A production example sits beside it for self-hosters: `stepclimb` and `postgres`, with
  Caddy and MinIO as options (`docs/plan/technical.md` -> Hosting: "stepclimb and postgres, with
  optional Caddy (automatic TLS) and MinIO"). It contains no default password: every secret is
  read from a file through Compose `secrets:` (`POSTGRES_PASSWORD_FILE` for Postgres,
  `STEPCLIMB_*_FILE` for the server), and starting it without those files fails with a message
  naming the missing file. Its images are pinned by digest like the development file. How Caddy
  is made optional is Open question Q1.
- **B3.** In the production example, only the reverse proxy (or, without Caddy, the server on a
  port the self-hoster chooses) is published to the network. Postgres is reachable only on the
  Compose network. With the `tls` profile the server keeps its `127.0.0.1:8080` port, reachable
  from the host only, because a Compose profile cannot remove another service's port (Q1 chose
  one file with profiles).
- **B4.** MinIO stays optional and out of the production example's default services in M0,
  because nothing reads object storage before M1 (ADR 0019). The image choice remains ADR 0019's
  open decision (Q6).
- **B5.** Every service restarts unless stopped, and the server starts after Postgres reports
  healthy, as the development file already does.

### Caddy

- **B6.** With Caddy enabled, Caddy terminates TLS with automatic certificates for the
  self-hoster's domain and reverse-proxies to the server on the Compose network. Its certificate
  and config state live on named volumes so a restart does not request new certificates. The
  self-hoster supplies the domain (and an ACME contact address if Caddy needs one) through the
  environment.
- **B7.** Behind Caddy, the server sees the original scheme and host, so cookies marked `Secure`
  (08-sessions-and-csrf: `__Host-sc_session`), the Origin allowlist (08) and provider redirect
  URLs (05, 06) use the public HTTPS origin. Caddy discards the forwarding headers a client
  sends and writes its own, and has a fixed address on the `proxy` network it shares with the
  server, so the server can trust exactly that address. Which forwarded headers the server
  trusts, and from whom, is configuration this spec's loader carries and 08 defines; the key
  and the code that reads it are 08's I5.

### Configuration

- **B8.** `STEPCLIMB_*` environment variables override an optional `stepclimb.toml`, key by key
  (`docs/plan/technical.md` -> Hosting: Configuration). With no file, the environment alone
  configures the server, as today. An empty value, in a variable or in the file, counts as unset:
  `STEPCLIMB_BIND=""` or `bind = ""` means the default, and an empty required secret is missing.
- **B9.** Every secret value has a `*_FILE` variant naming a file whose trimmed contents are the
  value (today only the database URL has one). Setting both the value and its `_FILE` variant is
  an error. A secret may also be in the TOML file, with a `_file` key beside it; a relative
  `_file` path resolves against the directory holding `stepclimb.toml`, while a variable's
  relative `_FILE` path is relative to the working directory. The self-hosting guide says a file
  holding a secret needs owner-only permissions. In the Compose production example, the secrets
  directory is owner-only (0700) and the files are 0644, because Compose outside Swarm
  bind-mounts secret files and ignores their mode and uid, and the server and MinIO run as uid
  65532.
- **B10.** Bad configuration stops startup before the server connects to anything, with a message
  that names the variable or the TOML key (with its file and line), says what is wrong and, where
  there is one, gives an example of a valid value. Bad configuration includes: a missing required
  value, an unparsable value, a value and its `_FILE` both set, an unreadable `_FILE`, a TOML
  syntax error, and an unknown TOML key. The whole file is checked even where a variable overrides
  a key: a value of the wrong type, or a value and its `_file` both set, is an error either way.
  An error about the configuration file itself (unreadable, or not valid TOML) names that file's
  path; an error about a `_FILE` variable or `_file` key names the variable or key, never the
  path it holds, since an operator may have pasted the secret there. The exit code stays 2.
- **B11.** The loader is shared: each M0 spec that needs configuration (02's organization mode,
  05's and 06's `[[providers]]` in the plan's TOML shape, 07's admission webhook, 08's public
  origin and allowlist) adds its keys to the one typed `Config` and its validation, with tests in
  the style of `crates/stepclimb-server/src/config/tests.rs` (an `Env` map and fixture text, not
  the process environment). This spec owns the loader, the precedence and the error format; each
  spec owns its keys. Names and layout are Q2's answer, recorded in ADR 0022.
- **B12.** A configuration reference in the self-hosting guide lists every key with its
  variable name, TOML path, type, default and whether it is secret, and a test fails when a key
  exists in `Config` but not in the reference.
- **B13.** Secrets never reach the logs (`docs/plan/technical.md` -> Security and privacy):
  `Config`'s `Debug` output redacts every secret field, and the startup log lists the
  configuration's non-secret values only.

### Startup, migrations and probes

- **B14.** `stepclimb serve` applies every pending embedded migration before it listens
  (`docs/plan/technical.md` -> Architecture and stack). While migrations run, the server is not
  listening, so `/readyz` cannot answer 200 before the schema is current.
- **B15.** Two server processes starting at once against the same database apply each migration
  once; the second waits for the first and then finds nothing to do. (Several replicas behind one
  proxy, or a restart racing a manual start, must not corrupt the schema.)
- **B16.** A migration failure stops the process with a non-zero exit, the migration's version
  and the database's error, and leaves the database at the last fully applied migration.
- **B17.** `/healthz` and `/readyz` keep their current meaning. The production example uses them
  for container health; how a distroless container without a shell or `curl` runs a health
  command is Open question Q3.

### Data, volumes and backups

- **B18.** Postgres data, Caddy state and (where enabled) MinIO data are named volumes, listed in
  the guide with what each holds and which must be backed up.
- **B19.** Self-hosters get nightly dumps by default (`docs/plan/technical.md` -> Hosting:
  "nightly dumps by default for self-hosters"; ADR 0012). The plan gives no milestone for this:
  M4 has "point-in-time backups and restore drill". Whether the nightly dump is M0, and how it
  runs, is Open question Q4. Whatever ships documents a tested restore.
- **B20.** Upgrades are `pull` then `up -d`: the new server applies forward-only migrations at
  startup (`docs/plan/technical.md` -> Releases: forward-only expand and contract migrations). The
  guide says to take a dump first.

### Image

- **B21.** The server image is distroless and runs as a non-root user (done), contains only the
  `stepclimb` binary with the SPA embedded, and is multi-arch (amd64 and arm64), signed with
  cosign and published to GHCR with an SBOM attached (`docs/plan/technical.md` -> Hosting:
  Images). The M0 row lists "releases" and "Compose deploy" but the plan does not say which spec
  publishes the image; that split is Open question Q5. The image bundles ffmpeg for transcoding
  (`docs/plan/technical.md` -> Scaling), which arrives with M1's media work, not here.

### Self-hosting guide

- **B22.** `deploy/compose/README.md` is the self-hosting guide until the docs site (M4): the
  requirements (Docker with Compose, a domain for TLS, 1 to 2 vCPU and 2 GB as the plan sizes a
  single deployment; coordinator decision (2026-11-14): the guide names no particular
  deployment), quick start with the production example, creating the secret files,
  enabling Caddy and MinIO, the configuration reference (B12), volumes and backups, restore,
  upgrades, and what each probe means. It names the development file as not for production.

## API

No new endpoints. `/healthz` and `/readyz` exist and are in the OpenAPI document; this spec does
not change their responses.

## Data

No tables. The spec touches migrations only in B14 to B16 (how they run, not what they contain).
Volumes are in B18.

## Permissions

None. The probes are public and return no data beyond their status.

## Test cases

Configuration (unit tests in `crates/stepclimb-server/src/config/tests.rs`, through an `Env` map
and fixture text):

- **T1.** (B8) A key in the TOML file and the same key in the environment: the environment wins;
  a key only in the file is used.
- **T2.** (B9) For every secret key, the `_FILE` variant is read and trimmed; both set is refused
  by name; an unreadable `_FILE` or `_file` names the variable or key and not the path, while an
  unreadable or invalid configuration file names its own path. A test walks `Config`'s secret
  fields so a new secret without a `_FILE` variant fails.
- **T3.** (B10) A TOML syntax error and an unknown key each name the file and line; the binary
  exits with code 2 (an integration test runs the binary).
- **T4.** (B12) A key missing from the configuration reference fails the reference test.
- **T5.** (B13) `format!("{config:?}")` with every secret set contains none of the secret values.

Server (crate `tests/`, `#[sqlx::test]`):

- **T6.** (B15) Two migration runs started concurrently against one empty database both succeed
  and the migrations table lists each version once.
- **T7.** (B16) A migration that fails leaves the previous ones applied and returns an error
  naming its version (a fixture migrator, not the real migrations).

Compose and image (CI, Linux):

- **T8.** (B2) `docker compose config` validates the development file and the production example
  in each Caddy and MinIO combination Q1 allows.
- **T9.** (B2, B3, B14, B17) The production example starts with generated secret files, `/readyz`
  answers 200 through the published port, and Postgres's port is not reachable from the host,
  checked as no published port on the Postgres container and only internal networks attached to
  it.
- **T10.** (B2) The production example started without its secret files exits with an error
  naming a missing file.
- **T11.** (B21) The image's configured user is non-root and its final stage has no shell
  (`docker image inspect`, and `docker run --entrypoint sh` fails); the multi-arch build produces
  amd64 and arm64 images. Coordinator decision (2026-11-14): `docker run` cannot start an image
  of another architecture without an emulator, so for that image the check lists its file system
  for a shell instead, and runs its binary's `check-config` under QEMU user-mode emulation.
- **T12.** (B19) A dump taken from a stack with data restores into a fresh stack, and the server
  starts against it with `/readyz` 200.
- **T13.** (B1) `just e2e` keeps passing against the development Compose file.

Caddy's certificate issuance is not tested in CI (it needs a public domain); T9 runs with Caddy
using its internal CA, which proves the proxy path.

## Out of scope

- The Helm chart, the migration Job and CloudNativePG docs: 1.0 (`docs/specs/m5.md` notes the 1.0
  row).
- Point-in-time recovery with WAL archiving and the restore drill in CI: M4 (`docs/specs/m4.md`).
- `/metrics`, OpenTelemetry and Sentry: M4 has "metrics"; not here.
- The static musl binary and systemd unit for bare metal: no milestone in the plan; not M0.
- The docs site: M4.
- On-demand ACME for managed custom domains: M6.
- What each configuration key of 02, 05, 06, 07 and 08 means.
- The Scalar CDN and CSP question: 11-app-shell Q11.

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Caddy in the production file or an override.** Options: (a) one production file with
  Caddy behind a Compose profile (`--profile tls`); (b) a base file plus `compose.caddy.yaml`
  merged with `-f`; (c) Caddy in the default services, with a documented way to remove it for
  self-hosters who have their own proxy. Recommendation: (a): one file to read, and the profile
  name says what it adds. The same mechanism applies to MinIO.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Variable names and the `stepclimb.toml` layout.** Today's names are flat
  (`STEPCLIMB_BIND`, `STEPCLIMB_DATABASE_URL`, `STEPCLIMB_LOG`). Options: (a) TOML sections
  (`[server] bind`, `[database] url`) mapped to variables with `__` between section
  and key (`STEPCLIMB_DATABASE__URL`); (b) flat TOML keys matching the flat variables
  (`database_url` <-> `STEPCLIMB_DATABASE_URL`), with arrays such as `[[providers]]` in TOML only;
  (c) sections with single `_` variables, resolved by a fixed table. Also open: where the
  file is looked for (a `--config` flag, `STEPCLIMB_CONFIG`, a default path such as
  `/etc/stepclimb/stepclimb.toml`) and whether an unknown `STEPCLIMB_*` variable is an error or a
  warning. Recommendation: (b), keeping every existing name; `STEPCLIMB_CONFIG` plus `--config`,
  no default path; an unknown `STEPCLIMB_*` variable is an error, since a typo in a secret's name
  otherwise starts the server without it. The same question is raised as
  03-users-and-identities Q5 and by 08-sessions-and-csrf's public-URL question
  (`STEPCLIMB_PUBLIC_URL`); one answer, recorded in one ADR, covers all three.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. Health checks in a distroless container.** The final image has no shell, `curl` or
  `wget`. Options: (a) a `stepclimb` subcommand that requests `/readyz` on the local port and
  exits 0 or 1, used as the Compose healthcheck (an addition to the plan's CLI list, so an ADR);
  (b) no container healthcheck: Compose's `restart` covers crashes, and Caddy's active health
  check covers readiness; (c) a debug base image with a shell, which drops distroless.
  Recommendation: (a): Compose `depends_on: service_healthy` then works for the proxy, and the
  subcommand is a few lines.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. Nightly dumps: milestone and mechanism.** The plan makes nightly dumps the self-hoster
  default without a milestone. Options for when: (a) M0, with the production example; (b) M4,
  with point-in-time backups. Options for how: (a) a small `postgres` image service in the
  production example running `pg_dump` on a schedule into a named volume, keeping N days;
  (b) a documented host cron line running `docker compose exec postgres pg_dump`; (c) a
  `stepclimb` subcommand (the plan's CLI lists `backup-verify`, not a dump command).
  Recommendation: M0, as a sidecar service (a) that keeps 7 days, because the vZDC pilot's data
  lives on the test server from M0, and `backup-verify` comes with M4's restore drill.
  **Decided:** the recommendation (owner, 13 Nov 2026). Retention is by age: a dump is deleted
  when it is more than 7 days older than the newest dump, so the newest is never deleted and a
  dump taken by hand never pushes a nightly one out.
- **Q5. Which spec publishes the image.** The M0 row has "releases" (01-repo-and-ci) and
  "Compose deploy" (this spec). Options: (a) 01's release workflow builds multi-arch, signs with
  cosign, attaches the SBOM and pushes to GHCR, and this spec owns the Dockerfile's contents and a
  CI check that both architectures build; (b) this spec owns the whole image pipeline.
  Recommendation: (a): signing and publishing belong with the release tags and the other release
  artifacts. The arm64 build of the Rust stage under emulation is slow; a cross-compiling stage
  or a native arm64 machine the owner provides is the owner's call (ADR 0021: long jobs go to a
  VM the owner provides).
  **Decided:** the recommendation (owner, 13 Nov 2026).
  **Coordinator decision (2026-11-14), for the owner's review.** The build method was the
  owner's call; with the owner away, the coordinator decided it on the owner's delegation and
  accepted cross-compiling, with the arm64 image check inside `just ci`. The owner can overrule
  it, which then updates this spec. The Rust stage cross-compiles: the web and Rust stages
  run on the build machine's platform (`FROM --platform=$BUILDPLATFORM`), the Rust stage adds
  rustup's `aarch64-unknown-linux-gnu` target and Debian's `gcc-aarch64-linux-gnu` linker, and
  only the final distroless stage, which runs nothing, uses the target's base. Emulation was not
  available: the development machine's rootless Docker has no binfmt_misc emulator registered,
  so an arm64 container fails with `exec format error`, and registering one changes the host.
  Cross-compiling needs no host change and took 1 min 16 s for a cold arm64 build (x86_64 Linux,
  16 cores), so both architectures are checked in `just ci` (`just image-check`,
  `bin/check-image.sh`) rather than in a separate recipe. The arm64 binary is run under QEMU
  user-mode emulation (`qemu-aarch64`, added to the Linux dev shell). Cross-compiling works from
  amd64 only; an arm64 machine builds and checks arm64 alone and says so, and building amd64
  there is left until someone needs it.
- **Q6. The MinIO image (ADR 0019, Accepted).** Decided by the owner on 13 Nov 2026: option (a),
  Chainguard's image pinned by digest; revisited in the M1 media spec. Options are ADR 0019's: (a)
  Chainguard's image pinned by digest; (b) MinIO built from source into our own image; (c) another
  S3-compatible server (Garage, SeaweedFS). Recommendation: as the ADR: (a) for development now,
  revisited in the M1 media spec. Until the owner decides, the production example documents MinIO as
  optional with the same pin and names the ADR.
- **Q7. Which CLI subcommands land in M0.** The plan lists `serve | migrate | admin create |
  export-course | import-course | check-config | backup-verify` without milestones. Options:
  (a) M0 adds `check-config` (runs the loader and B10's checks, prints the non-secret values,
  exits 0 or 2) and `migrate` (applies migrations and exits), which the production guide and the
  1.0 Helm migration Job use; (b) only `serve` until 1.0. Recommendation: (a); `admin create` is
  02's or 04's (the first admin), and the export, import and verify commands are M4.
  **Decided:** the recommendation (owner, 13 Nov 2026).

## Issues

In dependency order. Each is one PR.

- **I1. Configuration loader.** Deliverable: optional `stepclimb.toml` under `STEPCLIMB_*`
  variables, a `_FILE` variant for every secret, TOML errors with file and line, unknown-key
  errors, redacted `Debug`, the existing keys moved onto it unchanged. Covers B8 to B11, B13;
  T1 to T3, T5. Depends on: Q2 answered.
- **I2. `check-config` and `migrate` subcommands.** Deliverable: the two subcommands if Q7 (a)
  is chosen, with integration tests on their exit codes. Covers B10, B14; T3. Depends on: I1;
  Q7 answered.
- **I3. Migrations under concurrency and failure.** Deliverable: tests proving concurrent starts
  and a failing migration behave as B15 and B16 say, and the fix if they do not. Covers B14 to
  B16; T6, T7. Depends on: none.
- **I4. Production Compose example.** Deliverable: the production file beside the development
  one, secrets from files, Postgres on the internal network only, restart policies, volumes,
  the container healthcheck as Q3 decides, and a CI job booting it with generated secrets.
  Covers B2, B3, B5, B17, B18; T8 to T10. Depends on: I1; Q1 and Q3 answered.
- **I5. Optional Caddy.** Deliverable: the Caddy service and Caddyfile with automatic TLS, its
  volumes, Caddy's side of the forwarded headers (its own values only, from a fixed address),
  and the T9 run through Caddy's internal CA. The server's trusted-proxy key is 08's I5. Covers
  B6, B7; T8, T9. Depends on: I4.
- **I6. Nightly dumps and restore.** Deliverable: the nightly dump as Q4 decides, with retention,
  and the dump-and-restore CI check. Covers B19, B20; T12. Depends on: I4; Q4 answered.
- **I7. Image architectures.** Deliverable: the Dockerfile building for amd64 and arm64 (a
  recipe run locally or on the owner's VM), the non-root and no-shell checks; publishing, cosign
  and SBOM in 01's `just release` if Q5 (a) is chosen (01 issue 7, not this issue). Covers B21;
  T11. Depends on: Q5
  answered; 01-repo-and-ci (release recipe).
- **I8. Self-hosting guide.** Deliverable: `deploy/compose/README.md` with every section of B22
  and the generated or tested configuration reference. Covers B12, B22; T4. Depends on: I1, I4,
  I5, I6.
