# bin/check-versions.sh (rules in bin/versions.py); its tests run in gate-check.
# Fail when a manifest, lockfile or tauri.conf.json differs from Cargo.toml's workspace version
version-check:
    bash bin/check-versions.sh

# Writes every manifest that carries Stepclimb's version, refreshes the workspace crates' entries
# in both Cargo lockfiles, then checks. Commit the result on its own.
# Set the one version (x.y.z) everywhere
release-version version:
    python3 -X utf8 -B bin/versions.py set "{{ version }}"
    cargo update --workspace --offline
    cargo update --workspace --offline --manifest-path apps/app/src-tauri/Cargo.toml
    bash bin/check-versions.sh
