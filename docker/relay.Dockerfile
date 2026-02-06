# Multi-stage build for the Scriptum relay server.
# Stage 1: Build the Rust binary.
# Stage 2: Minimal runtime image.

FROM rust:1-bookworm AS builder

WORKDIR /app

# Copy workspace root files first for dependency caching.
COPY Cargo.toml Cargo.lock ./
COPY crates/common/Cargo.toml crates/common/Cargo.toml
COPY crates/relay/Cargo.toml crates/relay/Cargo.toml
COPY crates/daemon/Cargo.toml crates/daemon/Cargo.toml
COPY crates/cli/Cargo.toml crates/cli/Cargo.toml

# Create stub lib.rs files so cargo can resolve the workspace.
RUN mkdir -p crates/common/src && echo "pub fn _stub() {}" > crates/common/src/lib.rs && \
    mkdir -p crates/relay/src && echo "fn main() {}" > crates/relay/src/main.rs && \
    mkdir -p crates/daemon/src && echo "pub fn _stub() {}" > crates/daemon/src/lib.rs && \
    echo "fn main() {}" > crates/daemon/src/main.rs && \
    mkdir -p crates/cli/src && echo "fn main() {}" > crates/cli/src/main.rs

# Build dependencies only (cached layer).
RUN cargo build --release -p scriptum-relay 2>/dev/null || true

# Copy actual source code.
COPY crates/ crates/

# Touch source files to invalidate the stub builds.
RUN touch crates/common/src/lib.rs crates/relay/src/main.rs

# Build the relay binary.
RUN cargo build --release -p scriptum-relay

# ── Runtime stage ─────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1001 relay && \
    useradd --uid 1001 --gid relay --shell /bin/false relay

COPY --from=builder /app/target/release/scriptum-relay /usr/local/bin/scriptum-relay

USER relay

ENV RUST_LOG=info
ENV SCRIPTUM_RELAY_HOST=0.0.0.0
ENV SCRIPTUM_RELAY_PORT=8080
ENV SCRIPTUM_RELAY_LOG_FILTER=info
ENV SCRIPTUM_RELAY_WS_BASE_URL=ws://0.0.0.0:8080

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:${SCRIPTUM_RELAY_PORT}/healthz > /dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/scriptum-relay"]
