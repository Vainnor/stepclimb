# bin/check-versions.sh (rules in bin/versions.py); its tests run in gate-check.
# Fail when a manifest, lockfile or tauri.conf.json differs from Cargo.toml's workspace version
version-check:
    bash bin/check-versions.sh

# Writes every file that carries Stepclimb's version (bin/versions.py, the OpenAPI document's
# info.version included), refreshes the workspace crates' entries in both Cargo lockfiles, adds
# the version's release notes to CHANGELOG.md with git-cliff (the commits since the previous
# release tag up to HEAD; bin/release.py notes), then checks. Commit the version on its own.
# Set the one version (x.y.z) everywhere and write its release notes
release-version version:
    python3 -X utf8 -B bin/versions.py set "{{ version }}"
    cargo update --workspace --offline
    cargo update --workspace --offline --manifest-path apps/app/src-tauri/Cargo.toml
    python3 -X utf8 -B bin/release.py notes "{{ version }}"
    bash bin/check-versions.sh

# bin/release.py; the steps are in CONTRIBUTING.md -> Releases. Pushes, tags and publishes nothing:
# branches from origin/next, runs release-version, commits the version and CHANGELOG.md, writes
# the release PR body to target/release/ and prints the commands to run next.
# Prepare a release (x.y.z): version bump, notes, release PR body
release-prepare version:
    python3 -X utf8 -B bin/release.py prepare "{{ version }}" "{{ just_executable() }}"

# Run in a clean worktree of the annotated tag on main, on an amd64 machine. Checks the tag against
# the workspace version and main's first-parent history, regenerates the notes with git-cliff and
# compares them with CHANGELOG.md, runs `just ci` on the tag, then builds the linux/amd64 and
# linux/arm64 image into an OCI archive with an SPDX SBOM per platform (bin/release_artifacts.py,
# docs/adr/0031). Publishes nothing; prints the release-publish commands.
# Check, gate and build a tagged release (vX.Y.Z) locally
release tag:
    python3 -X utf8 -B bin/release.py verify "{{ tag }}"
    "{{ just_executable() }}" ci
    python3 -X utf8 -B bin/release.py finish "{{ tag }}"
    python3 -X utf8 -B bin/release_artifacts.py build "{{ tag }}"

# The only recipe that publishes, run by a person after `just release`: pushes the image to GHCR
# with skopeo, signs it with cosign keyless, attaches the SBOMs as cosign attestations and creates
# the GitHub Release. `--dry-run` checks the same and prints the commands instead.
# Publish a built release (vX.Y.Z): GHCR image, cosign signature, SBOMs, GitHub Release
[positional-arguments]
release-publish tag *flags:
    python3 -X utf8 -B bin/release_artifacts.py publish "$@"
