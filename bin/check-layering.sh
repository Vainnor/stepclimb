#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Fails if a workspace crate reaches a crate it must not, through any normal or build dependency
# on any target. The rules come from the plan (docs/plan/technical.md -> Architecture and stack,
# Integration platform) and are explained in .claude/rules/architecture.md. Run by
# `just layering`, which `just ci` runs.
set -euo pipefail

# Rule 1. stepclimb-assess is pure and also builds for wasm32: no other workspace crate and no
# I/O, runtime or HTTP crate.
# Rule 2. stepclimb-connectors uses only the public API, as any outside system would: no other
# workspace crate.
# Rule 3. Nothing VATSIM- or ATC-specific reaches the core: only stepclimb-server may depend on
# the ATC pack or the connectors (behind features, per organization).
# Rule 4. Nothing depends on stepclimb-server: it is the top of the graph.
# Rule 5. stepclimb-mock-idp is for tests: no crate, the server included, takes it as a normal or
# build dependency, so it never reaches the release binary; it depends on no workspace crate
# (docs/adr/0027-mock-identity-provider.md).
io='sqlx|sqlx-.*|tokio|tokio-.*|mio|axum|axum-.*|hyper|hyper-.*|tower|tower-.*|reqwest|object_store'
ws='stepclimb-.*'
pack='stepclimb-activities-atc|stepclimb-connectors'
mock='stepclimb-mock-idp'

declare -A banned=(
    [stepclimb-core]="stepclimb-server|$pack|$mock"
    [stepclimb-auth]="stepclimb-server|$pack|$mock"
    [stepclimb-assess]="$ws|$io"
    [stepclimb-activities]="stepclimb-server|$pack|$mock"
    [stepclimb-activities-atc]="stepclimb-server|stepclimb-connectors|$mock"
    [stepclimb-interop]="stepclimb-server|$pack|$mock"
    [stepclimb-integrations]="stepclimb-server|$pack|$mock"
    [stepclimb-connectors]="$ws"
    [stepclimb-mock-idp]="$ws"
    [stepclimb-server]="$mock"
    [stepclimb-media]="stepclimb-server|$pack|$mock"
    [stepclimb-jobs]="stepclimb-server|$pack|$mock"
)

members="$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -X utf8 -c 'import json,sys; print("\n".join(sorted(p["name"] for p in json.load(sys.stdin)["packages"])))')"
failed=0
checked=0
for crate in $members; do
    rule="${banned[$crate]:-}"
    if [[ -z "$rule" ]]; then
        echo "layering: $crate has no rule in bin/check-layering.sh; add one" >&2
        failed=1
        continue
    fi
    deps="$(cargo tree --locked -p "$crate" -e normal,build --target all --prefix none --format '{p}' \
        | awk '{print $1}' | sort -u | grep -vx "$crate" || true)"
    hits="$(grep -E "^($rule)$" <<<"$deps" || true)"
    if [[ -n "$hits" ]]; then
        echo "layering: $crate depends on crates it must not reach:" >&2
        sed 's/^/  /' <<<"$hits" >&2
        failed=1
    fi
    checked=$((checked + 1))
done

if [[ $failed -ne 0 ]]; then
    echo "layering: FAILED (.claude/rules/architecture.md -> Layering)" >&2
    exit 1
fi
echo "layering: ok ($checked crates checked)"
