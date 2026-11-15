# On a branch with a clean tree: cargo update in both Cargo workspaces and pnpm update within the
# exact pins, image digests refreshed, every new version at least 14 days old (bin/update_deps.py).
# Then just ci, just deny and just app-build, and a lockfile-only PR (CONTRIBUTING.md).
# The monthly dependency update: lockfiles and image digests only
update-deps: web-install
    python3 -X utf8 -B bin/update_deps.py
