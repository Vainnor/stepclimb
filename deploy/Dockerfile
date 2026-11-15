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
ARG BUILDARCH
ARG TARGETARCH
WORKDIR /src
COPY rust-toolchain.toml ./
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
    && cargo build --release --locked --target "$target" -p stepclimb-server --bin stepclimb \
    && cp "target/$target/release/stepclimb" /stepclimb

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
USER nonroot
EXPOSE 8080
ENV STEPCLIMB_BIND=0.0.0.0:8080
ENTRYPOINT ["/usr/local/bin/stepclimb"]
CMD ["serve"]
