compose := "docker compose -f deploy/compose/compose.yaml"

# Start PostgreSQL and MinIO for local development: one stack for every worktree, started under a
# lock and never recreated while it runs
up:
    python3 -X utf8 -B bin/ci_gate.py dev-up

# Stop the Compose services (data volumes are kept). Other worktrees may be using them.
down:
    {{ compose }} --profile stack down

# Build the server image and start the whole stack on :58080
up-stack:
    {{ compose }} --profile stack up -d --wait --build

# Playwright end-to-end tests against this run's own stack (bin/ci_gate.py), built from this
# checkout. The stack gets one organization and its admin from `stepclimb admin create`, whose
# printed setup link the enrollment test opens (SC_E2E_ENROLL_URL; it is a test credential of a
# stack that exists only for this run).
e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${SC_CI_PROJECT:-}" ]]; then
      exec python3 -X utf8 -B bin/ci_gate.py run -- "{{ just_executable() }}" e2e
    fi
    gate=(python3 -X utf8 -B bin/ci_gate.py compose --)
    "${gate[@]}" up --detach --wait --build stepclimb
    report="$("${gate[@]}" exec -T stepclimb stepclimb admin create --org-slug example \
      --org-name "Example Training" --email avery@example.test --name "Avery Example")"
    address="$("${gate[@]}" port stepclimb 8080)"
    SC_E2E_URL="http://localhost:${address##*:}" SC_E2E_ENROLL_URL="$(tail -n 1 <<<"$report")" \
      pnpm --filter web e2e

# Boot the production Compose example with generated secrets and check it, then remove it; inside
# a gate run its project, subnet and image are the run's own
compose-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${SC_CI_PROJECT:-}" ]]; then
      exec python3 -X utf8 -B bin/ci_gate.py run -- "{{ just_executable() }}" compose-check
    fi
    bash bin/check-production-compose.sh

# Boot the production example with Caddy (internal CA) and check TLS, forwarded headers, nightly
# dumps and a restore, then remove it; inside a gate run its projects and subnet are the run's own
compose-ops-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${SC_CI_PROJECT:-}" ]]; then
      exec python3 -X utf8 -B bin/ci_gate.py run -- "{{ just_executable() }}" compose-ops-check
    fi
    bash bin/check-compose-ops.sh

# Build the server image for linux/amd64 and linux/arm64 (arm64 only on an arm64 machine) and
# check each: non-root, no shell, the right architecture, check-config runs (bin/check-image.sh)
image-check:
    bash bin/check-image.sh
