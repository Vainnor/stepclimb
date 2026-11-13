# Stepclimb task runner. `just ci` is the merge gate: it must pass on the committed HEAD before a
# merge, on the machine the change was made on. There is no hosted CI (docs/adr/0021).
set shell := ["bash", "-cu"]

import 'just/secrets.just'
import 'just/rust.just'
import 'just/web.just'
import 'just/stack.just'

# The local database (`just up`); CI sets its own. sqlx's test and query macros read DATABASE_URL.
export DATABASE_URL := env('DATABASE_URL', 'postgres://stepclimb:stepclimb-local@127.0.0.1:55432/stepclimb')

# List recipes
default:
    @just --list

# Every merge check. Must pass on the committed HEAD before a merge.
ci: secrets-scan fmt-check lint layering spdx-check up test-rust deny sqlx-check openapi-diff web-lint typecheck test-web e2e
    @echo "just ci: all gates passed"
    bash bin/ci-stamp.sh

# Every source file carries an SPDX license header (AGPL-3.0-or-later; Apache-2.0 in SDK packages)
spdx-check:
    bash bin/check-spdx.sh
