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
io='sqlx|sqlx-.*|tokio|tokio-.*|mio|axum|axum-.*|hyper|hyper-.*|tower|tower-.*|reqwest|object_store'
ws='stepclimb-.*'
pack='stepclimb-activities-atc|stepclimb-connectors'

declare -A banned=(
    [stepclimb-core]="stepclimb-server|$pack"
    [stepclimb-auth]="stepclimb-server|$pack"
    [stepclimb-assess]="$ws|$io"
    [stepclimb-activities]="stepclimb-server|$pack"
    [stepclimb-activities-atc]="stepclimb-server|stepclimb-connectors"
    [stepclimb-interop]="stepclimb-server|$pack"
    [stepclimb-integrations]="stepclimb-server|$pack"
    [stepclimb-connectors]="$ws"
    [stepclimb-media]="stepclimb-server|$pack"
    [stepclimb-jobs]="stepclimb-server|$pack"
)

members="$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -X utf8 -c 'import json,sys; print("\n".join(sorted(p["name"] for p in json.load(sys.stdin)["packages"])))')"
failed=0
checked=0
for crate in $members; do
    [[ "$crate" == stepclimb-server ]] && continue
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
