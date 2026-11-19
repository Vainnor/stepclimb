# SPDX-License-Identifier: AGPL-3.0-or-later
# The Stepclimb server image: the web app built with Node, embedded into a release build of the
# `stepclimb` binary, on a distroless non-root base (plan, Technical -> Hosting and operations).
# Built from the repository root: `docker compose -f deploy/compose/compose.yaml build`.
# Bases are pinned by digest; bump them with the toolchain pins (rust-toolchain.toml,
# .node-version).
#
# The image is built for linux/amd64 and linux/arm64 (`just image-check`, which also checks
# each one). The web and Rust stages run on the build machine's platform: the web build is the
# same for every target, and the Rust stage cross-compiles for the target, so no stage runs
# under emulation. Only the final stage, which runs nothing, uses the target's base image. An
# amd64 machine builds both; an arm64 machine builds arm64 only.
#
# The final stage also holds ffmpeg, ffprobe and util-linux `prlimit` for the transcoder
# (docs/specs/m1/08-uploaded-video.md I6, docs/adr/0035-ffmpeg-in-the-image.md), with their
# licenses and source offers in /LICENSES.

FROM --platform=$BUILDPLATFORM node:26.13.0-bookworm-slim@sha256:add5c17c14752c54d2f3d519ec46a3815684d41c33f198bcb09fb998f7dcbf68 AS web
WORKDIR /src
RUN npm install -g pnpm@12.9.0
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml tsconfig.base.json ./
COPY web/package.json web/
COPY packages/tokens/package.json packages/tokens/
COPY packages/ui/package.json packages/ui/
COPY packages/api-client/package.json packages/api-client/
COPY packages/assess-wasm/package.json packages/assess-wasm/
COPY apps/app/package.json apps/app/
COPY apps/ui-docs/package.json apps/ui-docs/
RUN pnpm install --frozen-lockfile --filter web...
COPY web web
COPY packages packages
RUN pnpm --filter @stepclimb/tokens build && pnpm --filter web build

FROM --platform=$BUILDPLATFORM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS server
WORKDIR /src
# The local file store's directory (docs/specs/m1/06-files.md: the production Compose example's
# STEPCLIMB_STORAGE_URL), copied into the runtime image owned by the server's user, so a named
# volume mounted on it starts with that owner. The runtime image has no shell to make it there.
RUN install -d -m 0750 /image-root/var/lib/stepclimb/storage
COPY rust-toolchain.toml ./
# cargo-auditable records the crates compiled into the binary in a `.dep-v0` section, which the
# release SBOM reads (docs/adr/0031-release-image-signing-and-sbom.md). It is installed before
# the target arguments, so one layer serves every target.
RUN cargo install cargo-auditable --version 0.7.7 --locked --root /usr/local/cargo
ARG BUILDARCH
ARG TARGETARCH
# The Rust target for TARGETARCH, in /target.env. A target other than the build machine's gets
# rustup's standard library for it and the cross C compiler and linker from Debian bookworm's
# archive (not pinned beyond the base image's release), named in /target.env; the only C code
# the server links is ring's, the TLS crypto of outbound HTTP, which the cross compiler builds.
RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) target=x86_64-unknown-linux-gnu ;; \
        arm64) target=aarch64-unknown-linux-gnu ;; \
        *) echo "deploy/Dockerfile: no Rust target for $TARGETARCH (amd64 and arm64 only)" >&2; exit 1 ;; \
    esac; \
    echo "target=$target" > /target.env; \
    if [ "$TARGETARCH" != "$BUILDARCH" ]; then \
        if [ "$TARGETARCH-$BUILDARCH" != arm64-amd64 ]; then \
            echo "deploy/Dockerfile: cross-compiling to $TARGETARCH works from amd64 only; build it on a $TARGETARCH machine" >&2; \
            exit 1; \
        fi; \
        rustup target add "$target"; \
        apt-get update; \
        apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross; \
        rm -rf /var/lib/apt/lists/*; \
        echo "export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc" >> /target.env; \
        echo "export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc" >> /target.env; \
    fi
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY .sqlx .sqlx
COPY crates crates
COPY migrations migrations
COPY openapi openapi
COPY --from=web /src/web/dist web/dist
# The target cache is shared by every build on the machine, whichever checkout it comes from, and
# cargo treats a workspace crate as fresh when its inputs are older than its last build. COPY keeps
# the checkout's mtimes, so the inputs are touched to now first: every workspace crate, and what it
# reads at compile time (migrations, .sqlx, web/dist), is compiled from these sources, while
# third-party crates stay cached. The touch runs in this RUN so that a cached layer cannot carry an
# older time, and `sharing=locked` holds the target cache for the whole RUN so that no concurrent
# build can write newer artifacts between the touch and cargo.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,sharing=locked \
    . /target.env \
    && find Cargo.toml Cargo.lock .sqlx crates migrations web/dist -exec touch {} + \
    && cargo auditable build --release --locked --target "$target" -p stepclimb-server --bin stepclimb \
    && cp "target/$target/release/stepclimb" /stepclimb

# ffmpeg and ffprobe 9.0.2: mwader/static-ffmpeg 9.0.2-1, static-pie musl binaries built with
# `--enable-gpl --enable-version3` and nothing nonfree, so GPL-3.0-or-later (ADR 0035). The digest
# is the manifest list's, which holds both architectures; the target's is pulled.
FROM mwader/static-ffmpeg:9.0.2-1@sha256:f5d100184884eb3e82b0eb14bb5ebf98da99826ac824989720297fb9ef21c822 AS ffmpeg

# util-linux `prlimit` 2.38.1, which every ffmpeg and ffprobe run starts under, and the
# libsmartcols it links, from Debian bookworm's own packages (the final stage's release), pinned
# by snapshot and checksum. Unpacked on the build machine's platform; nothing here runs the
# target's code.
FROM --platform=$BUILDPLATFORM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS prlimit
ARG SNAPSHOT=https://snapshot.debian.org/archive/debian/20241209T144408Z/pool/main/u/util-linux
ADD --checksum=sha256:8eb241ff24b2e1ee9c0373f4fc9bc728b85bf5966c1496154df7da01c0e42f34 \
    ${SNAPSHOT}/util-linux_2.38.1-5%2Bdeb12u3_amd64.deb /debs/util-linux_amd64.deb
ADD --checksum=sha256:c30366915d4dc21106d69c3d5a2bf6b010c4c2ff508a1231ede4620aed9874fc \
    ${SNAPSHOT}/util-linux_2.38.1-5%2Bdeb12u3_arm64.deb /debs/util-linux_arm64.deb
ADD --checksum=sha256:84088c6895fc07538fc51624899f3f71835188e0d38ebc3812097ac13da523b0 \
    ${SNAPSHOT}/libsmartcols1_2.38.1-5%2Bdeb12u3_amd64.deb /debs/libsmartcols1_amd64.deb
ADD --checksum=sha256:802c366408ebdf55ba8fe441975552925c65ce3f142f00e4b2045d3059e6dec7 \
    ${SNAPSHOT}/libsmartcols1_2.38.1-5%2Bdeb12u3_arm64.deb /debs/libsmartcols1_arm64.deb
ARG TARGETARCH
RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) triplet=x86_64-linux-gnu ;; \
        arm64) triplet=aarch64-linux-gnu ;; \
        *) echo "deploy/Dockerfile: no prlimit for $TARGETARCH (amd64 and arm64 only)" >&2; exit 1 ;; \
    esac; \
    dpkg-deb -x "/debs/util-linux_$TARGETARCH.deb" /x; \
    dpkg-deb -x "/debs/libsmartcols1_$TARGETARCH.deb" /x; \
    install -D -m 0755 /x/usr/bin/prlimit /out/usr/local/bin/prlimit; \
    install -d "/out/usr/lib/$triplet"; \
    cp -P /x/usr/lib/$triplet/libsmartcols.so.1* "/out/usr/lib/$triplet/"; \
    install -D -m 0644 /x/usr/share/doc/util-linux/copyright /out/LICENSES/util-linux/copyright; \
    install -D -m 0644 /x/usr/share/doc/libsmartcols1/copyright /out/LICENSES/util-linux/libsmartcols1.copyright; \
    for license in GPL-2 GPL-3 LGPL-2.1; do \
        install -D -m 0644 "/usr/share/common-licenses/$license" "/out/LICENSES/common-licenses/$license"; \
    done

# The mock identity provider for the end-to-end Compose stack only (deploy/compose/ci.yaml, `just
# e2e`; docs/specs/m0/06-oauth2-and-vatsim-preset.md B3), built with `--target mock-idp`. These two
# stages sit before the server's runtime stage, so the default target never builds or contains them.
FROM server AS mock-idp-build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,sharing=locked \
    . /target.env \
    && find Cargo.toml Cargo.lock crates -exec touch {} + \
    && cargo build --release --locked --target "$target" -p stepclimb-mock-idp --bin stepclimb-mock-idp \
    && cp "target/$target/release/stepclimb-mock-idp" /stepclimb-mock-idp

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f AS mock-idp
COPY --from=mock-idp-build /stepclimb-mock-idp /usr/local/bin/stepclimb-mock-idp
USER nonroot
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/stepclimb-mock-idp"]

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=server /stepclimb /usr/local/bin/stepclimb
COPY --from=server --chown=65532:65532 /image-root/var/lib/stepclimb /var/lib/stepclimb
COPY --from=ffmpeg /ffmpeg /ffprobe /usr/local/bin/
COPY --from=ffmpeg /versions.json /LICENSES/ffmpeg/versions.json
COPY --from=prlimit /out/ /
COPY deploy/licenses/ /LICENSES/
USER nonroot
EXPOSE 8080
ENV STEPCLIMB_BIND=0.0.0.0:8080
ENTRYPOINT ["/usr/local/bin/stepclimb"]
CMD ["serve"]
