# 0023. Container health checks through `stepclimb healthcheck`

- **Status:** Accepted
- **Date:** 2026-11-13
- **Decider:** the owner, accepting the recommendation of `docs/specs/m0/14-compose-deploy.md` Q3
  on 2026-11-13
- **Related:** `docs/plan/technical.md` -> Hosting and operations (CLI, Images);
  `docs/specs/m0/14-compose-deploy.md` B17, Q3; `deploy/compose/compose.production.yaml`

## Context

The production Compose example needs a container health check so that a service can wait for the
server with `depends_on: condition: service_healthy`, and so that `docker compose up --wait`
returns only once the server answers. The server image is distroless
(`gcr.io/distroless/cc-debian12:nonroot`): it has no shell, `curl` or `wget` to run a probe
with. The plan's CLI list (`serve | migrate | admin create | export-course | import-course |
check-config | backup-verify`) has no command for it.

## Options

### Option A: a `stepclimb` subcommand that asks `/readyz`
The binary connects to the port it is configured to listen on, requests `/readyz` and exits 0 on
200, otherwise 1. A few dozen lines; adds a command to the plan's list.

### Option B: no container health check
Compose's `restart` covers crashes, and Caddy's active health check covers readiness. Nothing can
wait on the server's health, and `up --wait` returns once the container has started.

### Option C: a debug base image with a shell
Allows `curl` in the health check, and drops distroless.

## Recommendation

Option A: `depends_on: service_healthy` and `up --wait` then work, and the image stays
distroless.

## Decision

Option A. `stepclimb healthcheck` reads the same configuration as `serve`, connects to
`STEPCLIMB_BIND` (the loopback address of the same family when it is `0.0.0.0` or `[::]`),
requests `/readyz` over HTTP/1.1 with a 5 s timeout, and exits 0 when it answers 200 and 1
otherwise, printing the reason to stderr. A configuration error exits 2, as for every command.

## Consequences

The plan's CLI list gains `healthcheck`. The production Compose example uses it as the server's
health check. The check speaks HTTP over a plain TCP connection, so it adds no HTTP client to the
binary; if the server ever serves only TLS on its own port, the check needs revisiting.
