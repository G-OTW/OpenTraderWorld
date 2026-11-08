# ── Build stage ──────────────────────────────────────────────────────────────
FROM rust:1-slim AS builder

WORKDIR /build

# Copy the whole workspace and build the core binary in release mode.
COPY . .
RUN cargo build --release --bin otw-core

# Pre-create the upload dir owned by the runtime user (65532). When the named
# volume is first mounted here it inherits this ownership, so the non-root
# process can write to it. Distroless has no shell to chown at runtime.
RUN mkdir -p /data/uploads && chown -R 65532:65532 /data

# ── Runtime stage ────────────────────────────────────────────────────────────
# Distroless: tiny, no shell, smaller attack surface (M11 security-by-design).
FROM gcr.io/distroless/cc-debian12 AS runtime

WORKDIR /app
COPY --from=builder /build/target/release/otw-core /app/otw-core
# Carry over the upload dir with its 65532 ownership for the volume mountpoint.
COPY --from=builder --chown=65532:65532 /data /data

# Defaults; overridden by the compose environment / .env.
ENV OTW_CORE_HOST=0.0.0.0 \
    OTW_CORE_PORT=8080 \
    OTW_LOG=info

EXPOSE 8080
USER 65532:65532

ENTRYPOINT ["/app/otw-core"]
