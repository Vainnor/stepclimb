# 0031. The release image: one archive pushed by digest, signed keyless, an SPDX SBOM from syft

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decider:** the coordinator on the owner's delegation (overnight, 2026-11-15); what a release
  publishes is the owner's Q1 option B (`docs/specs/m0/01-repo-and-ci.md`, 13 Nov 2026)
- **Related:** `docs/plan/technical.md` -> Hosting and operations (Images), Security and privacy
  (supply chain); `docs/specs/m0/01-repo-and-ci.md` B16, D9; ADR 0021, ADR 0030;
  `bin/release_artifacts.py`; `deploy/Dockerfile`

## Context

Q1 option B has each release push the `deploy/Dockerfile` image to GHCR for linux/amd64 and
linux/arm64, tagged with the version, with a cosign keyless signature and an SBOM. There is no
hosted CI (ADR 0021), so the owner builds and publishes from a worktree of the tag. Four choices
follow, and none is in the plan or the spec:

1. How the image that was scanned and checked becomes the image in the registry.
2. Which tool writes the SBOM, and in which format.
3. How the SBOM sees the crates in the server binary. A scanner that reads only the image's file
   system finds the distroless base's Debian packages; the binary carries no package metadata.
   syft 1.55.0 over a local server image built without cargo-auditable listed 11 Debian
   packages and no crate; over a build of `next` at 88ac8c6 with it, 11 Debian packages and 332
   crates on
   linux/amd64 (331 on linux/arm64), `stepclimb-server` among them.
4. Where the SBOM tool comes from, given the 14-day rule (`.claude/rules/architecture.md`). The
   Nix dev shell's nixpkgs (locked 11 Nov 2026) has syft 1.56.0, published 2 Nov 2026, 13 days
   before this decision.

## Options

### Option A: build once into an OCI archive, scan it, push it by digest

`docker buildx build --output type=oci` writes both platforms into one archive. syft scans each
platform of that archive, and `skopeo copy --all --preserve-digests` pushes the same bytes.
cosign then signs the digest the archive recorded, once the registry serves that digest. The
signed digest is the scanned image's. Needs skopeo, an extra tool, in the dev shell.

### Option B: rebuild with `docker buildx build --push`

No extra tool, but the push is a second build. Its layers carry new timestamps unless the build
is made reproducible, so the registry's digest can differ from the one scanned, and the SBOM
would describe another image.

### Option C: BuildKit's SBOM attestation (`--sbom=true`)

BuildKit runs its syft scanner inside the build and stores the SBOM as an attestation manifest in
the image index. `cosign verify-attestation` does not read that format, and the GitHub Release
would still need the file extracted from the index.

### SBOM format

SPDX 2.3 JSON (SPDX 2.2.1 is the ISO/IEC 5962:2021 standard) or CycloneDX JSON. syft writes both; cosign
attests both (`--type spdxjson`, `--type cyclonedx`).

## Recommendation

Option A, with syft writing SPDX 2.3 JSON, one SBOM per platform. The binary is built with
cargo-auditable so syft can list its crates.

## Decision

- **Push.** Option A: one OCI archive, pushed with skopeo (nixpkgs 1.24.1, published 16 Sep
  2026) keeping its digests. `just release-publish` refuses to sign when the registry serves
  another digest under the version's tag, and refuses to push over a tag that already holds
  another image.
- **Signing.** cosign 3.1.3 (nixpkgs, published 6 Aug 2026), keyless, by digest only:
  `cosign sign --recursive` on the image index signs the index and each platform's image.
- **SBOM.** syft, SPDX 2.3 JSON, one file per platform, attached as a signed cosign attestation
  (`cosign attest --type spdxjson`) to that platform's image and as a file to the GitHub Release.
  syft runs as its container image, `anchore/syft:v1.55.0` pinned by its index digest (built
  19 Oct 2026), with `--network none`, so the rule holds without a second nixpkgs. The pin moves
  by hand, to a build at least 14 days old; `just update-deps` does not move it.
- **Crates.** `deploy/Dockerfile` builds the server with `cargo auditable build`
  (cargo-auditable 0.7.7, published 2 Oct 2026), which stores the crates compiled into the binary
  in a `.dep-v0` section that syft reads. `bin/release_artifacts.py` refuses an SBOM that lists
  no Debian package, no crate, or not `stepclimb-server` at the release's version.

## Consequences

- Every image build, the gate's included, compiles cargo-auditable once into a cached layer and
  links a few kilobytes of dependency data into the binary.
- The npm packages bundled into the web app are not in the SBOM: the build leaves no package
  metadata in `web/dist`. `pnpm audit --prod` in `just ci` covers them. Revisit if a consumer
  needs them in the SBOM.
- The release builds on an amd64 Docker engine only, because `deploy/Dockerfile` cross-compiles
  arm64 from amd64.
- Keyless signing records the signer's email address and the image digests in Sigstore's public
  transparency log (Rekor), even while the repository is private.
- A version tag on GHCR is never replaced by the recipe. A broken release gets a new version.
