# SPDX-License-Identifier: AGPL-3.0-or-later
# The Stepclimb server image: the web app built with Node, embedded into a release build of the
# `stepclimb` binary, on a distroless non-root base (plan, Technical -> Hosting and operations).
# Built from the repository root: `docker compose -f deploy/compose/compose.yaml build`.
# Bases are pinned by digest; bump them with the toolchain pins (rust-toolchain.toml,
# .node-version).

FROM node:26.13.0-bookworm-slim@sha256:add5c17c14752c54d2f3d519ec46a3815684d41c33f198bcb09fb998f7dcbf68 AS web
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

FROM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS server
WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY .sqlx .sqlx
COPY crates crates
COPY migrations migrations
COPY openapi openapi
COPY --from=web /src/web/dist web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p stepclimb-server --bin stepclimb \
    && cp target/release/stepclimb /stepclimb

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=server /stepclimb /usr/local/bin/stepclimb
USER nonroot
EXPOSE 8080
ENV STEPCLIMB_BIND=0.0.0.0:8080
ENTRYPOINT ["/usr/local/bin/stepclimb"]
CMD ["serve"]
