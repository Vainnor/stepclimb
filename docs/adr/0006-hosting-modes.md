# 0006. Hosting modes: self-hosted single or multi-organization, managed

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Hosting and operations, Tenancy and structure

## Context

The same software runs on a homelab, on vZDC's servers and as the maintainer's managed
service.

## Decision

Three modes run the same binary: self-hosted with one organization (the default, the switcher
hidden), self-hosted with several organizations, and managed hosting (multi-organization, a
subdomain or custom domain each, on-demand TLS, an operator console, quotas). Managed hosting uses
one shared database with `org_id` on every row, plus per-organization restore from the
organization export, and launches after 1.0 (M6). Development and the pilot run on the
maintainer's homelab; vZDC's instance moves to vZDC's servers after adoption.

## Consequences

Every query is org-scoped from M0, and a cross-organization test covers every route, so
multi-organization mode needs no retrofit. Deploy targets: Docker Compose, Helm, a static binary.
