# ── Script engine for the browser ────────────────────────────────────────────
# The chart runs script indicators with the same Rust engine as the server, compiled to
# WebAssembly. `core` is a named build context (compose `additional_contexts`, workflow
# `build-contexts`): the Rust workspace, read only. The cache mounts keep the cargo registry,
# the wasm-bindgen CLI and the target dir between local builds.
# Built without the `core` context (a plain `docker build frontend`), this stand-in stage is
# what `COPY --from=core` reads, and it stops the build with the fix. A named context of the
# same name replaces the stage.
FROM busybox:1.38 AS core
RUN echo "error: the frontend image needs the Rust workspace as build context 'core': docker build --build-context core=core frontend (docker compose passes it)" >&2 && exit 1

FROM rust:1-slim AS wasm

RUN rustup target add wasm32-unknown-unknown
COPY --from=core . /core
COPY scripts/build-wasm.sh /build-wasm.sh
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/cache/bin \
    --mount=type=cache,target=/cache/target \
    CARGO_INSTALL_ROOT=/cache/bin PATH=/cache/bin/bin:$PATH CARGO_TARGET_DIR=/cache/target \
    sh /build-wasm.sh /core /out

# ── Build stage ──────────────────────────────────────────────────────────────
FROM node:26-slim AS builder

WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci

COPY . .
COPY --from=wasm /out src/lib/script/wasm
RUN npm run build

# ── Publish stage ────────────────────────────────────────────────────────────
# Tiny image whose only job: copy the built static assets into the shared volume
# that Caddy serves. Exits 0 after copying (compose treats it as a one-shot).
FROM busybox:1.38 AS publish

WORKDIR /publish
COPY --from=builder /build/build /publish/build

# On start: clear stale assets from the mounted /www volume, then copy the fresh
# build in, then exit. Wiping first is essential — SvelteKit emits content-hashed
# filenames, so a plain `cp` would leave old hashed chunks/CSS behind and the app
# could keep serving stale assets across rebuilds.
CMD ["sh", "-c", "rm -rf /www/* /www/.[!.]* 2>/dev/null; cp -r /publish/build/. /www/ && echo 'frontend assets published to /www' && ls /www"]
