# Format Rust code
fmt:
    cargo fmt --all

# Check Rust formatting
fmt-check:
    cargo fmt --all -- --check

# Type-check every crate and target
check:
    cargo check --workspace --all-targets --locked

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Rust tests: nextest for unit and integration tests, cargo test for doctests. The server's SPA
# tests read the web build, and the database tests need DATABASE_URL: `just up`, or the run's own
# database inside `just ci`.
test-rust: web-build
    cargo nextest run --workspace --locked --no-tests=pass --profile {{ env('NEXTEST_PROFILE', 'default') }}
    cargo test --workspace --doc --locked

# Fail if a crate reaches one it must not (bin/check-layering.sh, .claude/rules/architecture.md)
layering:
    bash bin/check-layering.sh

# Fail on the ways around the OrgScope `disallowed-methods` gate clippy does not refuse (inner
# allows, wide expects, lint settings in Cargo files); the self-test runs the script's fixtures first
lint-escapes:
    python3 -X utf8 -B bin/check-lint-escapes.py --self-test
    python3 -X utf8 -B bin/check-lint-escapes.py

# Dependency advisories, licenses, bans and sources
deny:
    cargo deny check

# Apply the migrations to the local database
migrate:
    cargo sqlx migrate run --source migrations

# Regenerate .sqlx/ after changing a query; commit the result. Like sqlx-check, it runs against a
# database of its own with this checkout's migrations (bin/ci_gate.py), never the shared one
sqlx-prepare:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${SC_CI_PROJECT:-}" ]]; then
      exec python3 -X utf8 -B bin/ci_gate.py run -- "{{ just_executable() }}" sqlx-prepare
    fi
    cargo sqlx migrate run --source migrations
    cargo sqlx prepare --workspace -- --all-targets

# Fail if .sqlx/ does not match the queries in the code
sqlx-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${SC_CI_PROJECT:-}" ]]; then
      exec python3 -X utf8 -B bin/ci_gate.py run -- "{{ just_executable() }}" sqlx-check
    fi
    cargo sqlx migrate run --source migrations
    cargo sqlx prepare --workspace --check -- --all-targets

# Regenerate openapi/stepclimb.json and the TypeScript client from the code; commit both
openapi:
    cargo run -q --locked -p stepclimb-server --example openapi > openapi/stepclimb.json
    pnpm --filter @stepclimb/api-client generate

# Fail if the committed OpenAPI document or the generated client differ from what the code makes
openapi-diff: web-install
    #!/usr/bin/env bash
    set -euo pipefail
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    cargo run -q --locked -p stepclimb-server --example openapi > "$tmp/stepclimb.json"
    diff -u openapi/stepclimb.json "$tmp/stepclimb.json" \
      || { echo "openapi-diff: openapi/stepclimb.json is stale; run just openapi" >&2; exit 1; }
    node packages/api-client/scripts/generate.mjs "$tmp/schema.ts"
    diff -u packages/api-client/src/schema.ts "$tmp/schema.ts" \
      || { echo "openapi-diff: packages/api-client/src/schema.ts is stale; run just openapi" >&2; exit 1; }
    echo "openapi-diff: ok (document and client match the code)"

# Run the server against the local database, with the web build embedded. Without
# STEPCLIMB_ENCRYPTION_KEYS(_FILE) it uses the published development key ring,
# deploy/compose/development-encryption-keys, and says so; never use it for real data. Without
# STEPCLIMB_PUBLIC_URL it uses http://localhost:8080, and without STEPCLIMB_ALLOWED_ORIGINS it
# allows the Vite dev server's origin, http://localhost:5173 (`just dev`). With STEPCLIMB_CONFIG
# set it supplies none of these defaults, since a variable would override the file.
serve: web-build
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "${STEPCLIMB_CONFIG:-}" ]]; then
      echo "just serve: STEPCLIMB_CONFIG is set; the database URL and encryption keys come from it or from your STEPCLIMB_* variables, with no development defaults" >&2
    else
      if [[ -z "${STEPCLIMB_ENCRYPTION_KEYS:-}${STEPCLIMB_ENCRYPTION_KEYS_FILE:-}" ]]; then
        echo "just serve: using the development encryption key (deploy/compose/development-encryption-keys); never use it for real data" >&2
        export STEPCLIMB_ENCRYPTION_KEYS_FILE="{{ justfile_directory() }}/deploy/compose/development-encryption-keys"
      fi
      if [[ -z "${STEPCLIMB_DATABASE_URL:-}${STEPCLIMB_DATABASE_URL_FILE:-}" ]]; then
        export STEPCLIMB_DATABASE_URL="$DATABASE_URL"
      fi
      if [[ -z "${STEPCLIMB_PUBLIC_URL:-}" ]]; then
        export STEPCLIMB_PUBLIC_URL="http://localhost:8080"
      fi
      # `just dev` serves the web app from Vite on :5173 and proxies to this server; the CSRF
      # check accepts that Origin only because development lists it here.
      if [[ -z "${STEPCLIMB_ALLOWED_ORIGINS:-}" ]]; then
        export STEPCLIMB_ALLOWED_ORIGINS="http://localhost:5173"
      fi
    fi
    exec cargo run --locked -p stepclimb-server --bin stepclimb -- serve
