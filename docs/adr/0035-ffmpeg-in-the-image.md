# 0035. ffmpeg in the image: a pinned static build, under prlimit, with its source offer

- **Status:** Proposed
- **Date:** 2026-11-19
- **Decider:** the owner (Vainnor)
- **Related:** `docs/plan/technical.md` -> Hosting and operations ("the image bundles ffmpeg");
  `docs/specs/m1/08-uploaded-video.md` Q7, D19, D20, D42 to D44, I6, T17; ADR 0002 (licenses),
  ADR 0021 (no hosted CI), ADR 0031 (release image and SBOM); issue #240

## Context

The transcoder runs ffmpeg and ffprobe as child processes (spec 08 B8 to B11). The release image
is `gcr.io/distroless/cc-debian12:nonroot`: glibc, no shell, no package manager, and until now
only the `stepclimb` binary. Spec 08 Q7 chose (a), copying a pinned static build into that image,
pending this ADR.

The owner decided on #240 that every ffmpeg and ffprobe run starts under util-linux `prlimit`,
so the kernel enforces an address-space, file-size, open-file and CPU limit from the first
instruction. An input that needs too much memory must then fail in ffmpeg, charged to the input,
instead of being killed by the kernel's out-of-memory killer, which the transcoder treats as a
host fault and refunds. Distroless has no `prlimit`, so the image needs one too.

The B11 encoders are libx264 and AAC. Every ffmpeg build with libx264 is GPL; Stepclimb is
AGPL-3.0-or-later (ADR 0002).

## Options

### Option A: copy `mwader/static-ffmpeg` binaries (recommended)

The static-ffmpeg project publishes `mwader/static-ffmpeg:<ffmpeg version>-<build>` on Docker
Hub. Each tag is a multi-architecture image (amd64 and arm64) holding two static-pie musl
binaries, `/ffmpeg` and `/ffprobe` (138 MB each), plus `/versions.json`, which names the version
of each of its 61 components. The pinned tag is `9.0.2-1`, pushed on 2026-10-09 and 41 days old
when this was written, digest
`sha256:f5d100184884eb3e82b0eb14bb5ebf98da99826ac824989720297fb9ef21c822`. It was built from
commit `a6e07cb2da57b29dc45a4b932dd6a5c07740efc9` of `github.com/wader/static-ffmpeg`. That
commit's Dockerfile downloads every component's source by URL and checks each against a SHA-256.
Checked by running the amd64 binary: `-buildconf` shows `--enable-gpl --enable-version3` and no
`--enable-nonfree`, `-L` prints GPL version 3 or later, and `-encoders` lists no `libfdk_aac`.
`versions.json` names fdk-aac only because the recipe compiles it; ffmpeg is not configured to
link it.

- For: one `COPY --from` of two files, pinned by digest, per architecture; no compiler stage and
  no hour-long cold build; the binaries are static, so they need nothing from the distroless
  base.
- Against: Stepclimb trusts one maintainer's build pipeline; the build enables far more than B11
  needs (network protocols, 61 components), so a bug in any linked decoder is reachable. The
  transcoder already limits that surface: the `file` protocol only, a pinned demuxer per
  container, `-max_pixels`, `-max_alloc`, an empty environment, and now `prlimit`.
- Cost: about 280 MB more per image architecture.

### Option B: build ffmpeg from source in a Dockerfile stage

Configure ffmpeg with only what B11 needs: the MP4, Matroska, WebM and QuickTime demuxers, the
common decoders, libx264, the native AAC encoder and the HLS muxer.

- For: a smaller binary, a smaller attack surface, and the source is the build itself.
- Against: a cold build of ffmpeg and x264 for two architectures is a long job (ADR 0021), and
  cross-compiling them from amd64 is its own project. Stepclimb would maintain the recipe and
  follow ffmpeg's security releases itself.

### Option C: a Debian-based transcoder image with Debian's ffmpeg

- Against: two images to build, sign and document; a shell and a package manager in one of them;
  the plan says one image.

### `prlimit`

Debian 12's own `util-linux` and `libsmartcols1` packages, version `2.38.1-5+deb12u3`, match the
final stage's release and glibc. The build fetches them from snapshot.debian.org at
`20241209T144408Z`, checks each `.deb` against its SHA-256 (`ADD --checksum`), and unpacks
`/usr/bin/prlimit` and `libsmartcols.so.1` on the build machine's platform. Nothing in that stage
runs the target's code. `prlimit` is 2.38.1 from 2024, but its job is one `prlimit64` call and
an `exec`. A static `prlimit` would need a build stage of its own.

## Recommendation

Option A for ffmpeg and ffprobe, and Debian's packages for `prlimit`, as built in #240.

**Licenses.** ffmpeg and ffprobe in this build are GPL-3.0-or-later; `prlimit` is
GPL-2.0-or-later and libsmartcols LGPL-2.1-or-later. The server does not link any of them. It
starts them as separate programs with an argument list and reads their output, which the GPL FAQ
treats as communication between separate programs ("mere aggregation"), not one combined work. The
licenses still apply to the binaries as distributed in the image, which is a GPL distribution by
Stepclimb's release, so the image must carry each license and the source offer. GPL-3.0 and
AGPL-3.0 are compatible in any case (each license's section 13), so even an argument that the image
forms one work would not create a conflict for the GPLv3 parts.

**Source offer.** GPLv3 section 6(d) lets object code distributed over a network point to its
corresponding source on a server run by a third party, provided the distributor keeps clear
directions and ensures that the source stays available. GPLv2 section 3(b), for `prlimit`, takes a
written offer valid for three years. The image carries:

- `/LICENSES/README`: what each program is, its license and where its files are;
- `/LICENSES/common-licenses/`: the GPL-2, GPL-3 and LGPL-2.1 texts from Debian;
- `/LICENSES/ffmpeg/SOURCE` and `versions.json`: the pinned image digest, the static-ffmpeg commit
  whose Dockerfile names every component's source URL and checksum, ffmpeg's own tarball, and an
  offer to provide the complete source on request through the repository's issues;
- `/LICENSES/util-linux/`: Debian's copyright files and `SOURCE`, the Debian source package on
  snapshot.debian.org with a three-year written offer.

The release body (`bin/release_artifacts.py`) carries a "Third-party programs in the image"
section saying the same in short.

The weak point is that the offer relies on upstream servers: GitHub, ffmpeg.org, every library's
download site and snapshot.debian.org. If one goes away, the maintainers must still produce the
source, so they should keep a copy of it. The cheapest durable fix: the release step fetches the
static-ffmpeg recipe's sources, which it can do from the pinned Dockerfile's URLs and checksums,
and attaches them to the GitHub Release as one archive. That would make the image's own release
the designated place for 6(d). It is not built here, because it is the owner's call whether the
on-request offer is enough.

## Decision

Pending the owner.

## Consequences

- `bin/check-image.sh` checks that `/usr/local/bin` holds exactly `ffmpeg`, `ffprobe`, `prlimit`
  and `stepclimb`. It also runs each program's version, transcodes a 1 s `testsrc` clip to one
  360p HLS rendition in each architecture (under `prlimit` in the native image), and checks that
  `/LICENSES` exists (T17).
- Moving ffmpeg means a new static-ffmpeg tag at least 14 days old, a new digest, the version in
  `/LICENSES/ffmpeg/SOURCE`, the release body and this ADR's commit hash, and a run of
  `just image-check`. `just update-deps` does not move it.
- The SBOM (ADR 0031) lists Debian packages and crates. syft cannot see the components of a static
  musl binary, so `/LICENSES/ffmpeg/versions.json` is the only list of them.
- The image grows by about 280 MB per architecture.
- The thread default and the address-space formula (spec 08 D42) assume this build's memory use;
  a new ffmpeg version should be measured again with `-threads 0` on a large host.
