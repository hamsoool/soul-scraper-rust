# ── Stage 1: Build ─────────────────────────────────────────────────────────
FROM rust:slim AS builder

# Install build dependencies (OpenSSL for reqwest rustls, pkg-config, curl for utoipa-swagger-ui build script)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Layer caching: copy manifests first so dep compilation is cached
COPY Cargo.toml ./
# Create dummy files so `cargo build` can compile deps without our source
RUN mkdir -p src && \
    touch src/lib.rs && \
    echo 'fn main() {}' > src/main.rs && \
    echo 'fn main() {}' > src/sync_bin.rs
RUN cargo build --release 2>/dev/null; true

# Now copy real source + build for real
COPY src/ src/
COPY migrations/ migrations/
RUN touch src/lib.rs src/main.rs src/sync_bin.rs && \
    cargo build --release

# ── Stage 2: Runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Runtime deps: libssl for TLS, ca-certificates for HTTPS, libstdc++ for pdfium, and curl/tar to fetch PDFium
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    libstdc++6 \
    curl \
    tar \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the compiled binaries
COPY --from=builder /app/target/release/soul-scrape-rust /usr/local/bin/soul-scrape-rust
COPY --from=builder /app/target/release/sync /usr/local/bin/sync

# Copy migrations (SQLx runs them at startup) and sources config
COPY migrations/ migrations/
COPY sources.json ./

# Download and extract PDFium library directly into /usr/local/lib/
RUN mkdir -p /tmp/pdfium && \
    curl -L https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz | tar -xz -C /tmp/pdfium && \
    cp /tmp/pdfium/lib/libpdfium.so /usr/local/lib/libpdfium.so && \
    rm -rf /tmp/pdfium

# Set LD_LIBRARY_PATH so dynamic linker knows where to find libpdfium.so
ENV LD_LIBRARY_PATH=/usr/local/lib

EXPOSE 8000

CMD ["soul-scrape-rust"]
