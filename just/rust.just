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
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Rust tests: nextest for unit and integration tests, cargo test for doctests. The server's SPA
# tests read the web build, and the database tests need `just up`.
test-rust: web-build
    cargo nextest run --workspace --locked --no-tests=pass --profile {{ env('NEXTEST_PROFILE', 'default') }}
    cargo test --workspace --doc --locked

# Fail if a crate reaches one it must not (bin/check-layering.sh, .claude/rules/architecture.md)
layering:
    bash bin/check-layering.sh

# Dependency advisories, licenses, bans and sources
deny:
    cargo deny check

# Apply the migrations to the local database
migrate:
    cargo sqlx migrate run --source migrations

# Regenerate .sqlx/ from the local database after changing a query; commit the result
sqlx-prepare: migrate
    cargo sqlx prepare --workspace -- --all-targets

# Fail if .sqlx/ does not match the queries in the code (needs the database: `just up`)
sqlx-check: migrate
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
# STEPCLIMB_ENCRYPTION_KEYS(_FILE) it uses the local development key from compose.yaml, whose
# bytes spell "stepclimb-local-development-key!"; never use it for real data.
serve: web-build
    if [[ -z "${STEPCLIMB_ENCRYPTION_KEYS:-}${STEPCLIMB_ENCRYPTION_KEYS_FILE:-}" ]]; then export STEPCLIMB_ENCRYPTION_KEYS="local:c3RlcGNsaW1iLWxvY2FsLWRldmVsb3BtZW50LWtleSE="; fi; STEPCLIMB_DATABASE_URL="${STEPCLIMB_DATABASE_URL:-$DATABASE_URL}" cargo run --locked -p stepclimb-server --bin stepclimb -- serve
