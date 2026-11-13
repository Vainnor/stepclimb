#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Runs apps/ui-docs' Playwright story suite (screenshots and axe over storybook-static/) inside
# Playwright's pinned image, so baselines render with the same Chromium and system fonts on every
# machine (docs/specs/m0/13-base-ui-components.md Q4). The image's Playwright and Node match
# package.json's @playwright/test and .node-version. The repository is mounted at its own path so
# pnpm's node_modules links resolve. Arguments go to `playwright test`, for example
# `--update-snapshots` to rewrite the baselines after an intended change.
set -euo pipefail

image="mcr.microsoft.com/playwright:v1.64.1-noble@sha256:2f05f8a734509a7b3f26a912639488ee08f58a7b292d14d62271bcff9038768f"
root="$(git rev-parse --show-toplevel)"

[[ -f "$root/apps/ui-docs/storybook-static/index.json" ]] || {
    echo "ui-visual: no Storybook build; run \`pnpm --filter @stepclimb/ui-docs build\` first" >&2
    exit 2
}

# Rootless Docker maps the container's root to the calling user, so files it writes (results,
# updated baselines) are already ours. Rootful Docker runs as the calling user instead.
user=()
if ! docker info --format '{{.SecurityOptions}}' | grep -q rootless; then
    user=(--user "$(id -u):$(id -g)" --env HOME=/tmp)
fi

exec docker run --rm --init --ipc=host "${user[@]}" \
    --volume "$root:$root" --workdir "$root/apps/ui-docs" \
    "$image" node node_modules/@playwright/test/cli.js test "$@"
