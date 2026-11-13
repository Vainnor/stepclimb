# 0012. Technical choices: tokens, OAuth server, job queue, search, messages, backups

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Decided, Architecture and stack, Hosting and operations

## Context

Several infrastructure choices could each pull in a large dependency or an external service.

## Decision

- **Design tokens:** Terrazzo, from DTCG JSON.
- **OAuth:** Stepclimb's own minimal OAuth 2.1 server, built on `oauth2`/`jsonwebtoken`, PKCE only,
  public clients for Stepclimb's own apps.
- **Job queue:** our own Postgres queue (`FOR UPDATE SKIP LOCKED`) with a transactional outbox; no
  Redis.
- **Search:** PostgreSQL full-text search with `pg_trgm`.
- **Messages:** English only at 1.0, with every string going through ICU messages.
- **Backups:** continuous WAL archiving (pgBackRest or CloudNativePG) for point-in-time recovery;
  RPO at most 5 min and RTO at most 4 h on managed hosting; nightly dumps by default for
  self-hosters; per-organization restore built on the organization export, tested in CI.

## Consequences

The single binary plus PostgreSQL stays the whole stack (object storage aside). `packages/tokens`
already builds with Terrazzo (M0 skeleton).
