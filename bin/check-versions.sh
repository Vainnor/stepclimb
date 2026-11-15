#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# One version for the server, the web app and the Tauri app (docs/specs/m0/01-repo-and-ci.md B14):
# fails when a manifest, lockfile, tauri.conf.json or package.json outside Changesets differs from
# Cargo.toml's [workspace.package] version. The rules are in bin/versions.py.
# Usage: check-versions.sh [repo-root]
set -euo pipefail
exec python3 -X utf8 -B "$(dirname "$0")/versions.py" check "${1:-.}"
