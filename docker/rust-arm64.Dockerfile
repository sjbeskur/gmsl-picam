# syntax=docker/dockerfile:1
#
# Cross-compile the gmsl-picam Rust examples for aarch64 (Raspberry Pi 5).
#
# Prerequisites:
#   Build libcamera first so the arm64 tarball is available:
#       ./scripts/build-libcamera-arm64.sh
#
# Build via the wrapper script (recommended):
#   ./scripts/build-rust-arm64.sh
#
# Or directly:
#   docker buildx build \
#     --file docker/rust-arm64.Dockerfile \
#     --target artifact \
#     --output type=local,dest=out/rust-arm64 \
#     .

FROM ubuntu:24.04 AS base

ARG DEBIAN_FRONTEND=noninteractive

# Same dual-arch apt sources used by libcamera-arm64.Dockerfile.
RUN rm -f /etc/apt/sources.list.d/ubuntu.sources && \
    cat > /etc/apt/sources.list <<'EOF'
deb [arch=amd64] http://archive.ubuntu.com/ubuntu noble main restricted universe multiverse
deb [arch=amd64] http://archive.ubuntu.com/ubuntu noble-updates main restricted universe multiverse
deb [arch=amd64] http://security.ubuntu.com/ubuntu noble-security main restricted universe multiverse
deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports noble main restricted universe multiverse
deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports noble-updates main restricted universe multiverse
deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports noble-security main restricted universe multiverse
EOF

RUN dpkg --add-architecture arm64 && \
    apt-get update && apt-get install -y --no-install-recommends \
    # Cross-compilation toolchain (matches libcamera-aarch64-cross.ini)
    build-essential \
    gcc-aarch64-linux-gnu \
    g++-aarch64-linux-gnu \
    # pkg-config (host binary, reads arm64 .pc files via PKG_CONFIG_LIBDIR)
    pkg-config \
    # clang + libclang required by bindgen (used by the libcamera Rust crate)
    clang \
    libclang-dev \
    # arm64 GStreamer dev libraries (headers + .so stubs for linking)
    libgstreamer1.0-dev:arm64 \
    libgstreamer-plugins-base1.0-dev:arm64 \
    # arm64 transitive dependencies of libcamera.so (needed at link time so the
    # linker can follow libcamera's NEEDED chain without "undefined reference" errors)
    libudev-dev:arm64 \
    libgnutls28-dev:arm64 \
    libyaml-dev:arm64 \
    # misc
    ca-certificates \
    curl && \
    rm -rf /var/lib/apt/lists/*

# Install Rust via rustup and add the aarch64 bare-metal target.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path && \
    /root/.cargo/bin/rustup target add aarch64-unknown-linux-gnu
ENV PATH="/root/.cargo/bin:${PATH}"

# ── libcamera arm64 artifacts ────────────────────────────────────────────────
# The tarball is produced by ./scripts/build-libcamera-arm64.sh.
# It is passed in as a named build context (--build-context libcamera-tarball=...)
# so that out/ does not need to be added to .dockerignore.
# It unpacks to ./usr/local/... so we extract straight to / to mirror
# the deployment path on the Pi.
COPY --from=libcamera-tarball libcamera-arm64-ubuntu.tar.gz /tmp/
RUN tar -C / -xzf /tmp/libcamera-arm64-ubuntu.tar.gz && \
    ldconfig && \
    rm /tmp/libcamera-arm64-ubuntu.tar.gz

# ── Cross-compilation environment ────────────────────────────────────────────

# Linker (also declared in rust/.cargo/config.toml, but set here as a fallback)
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# PKG_CONFIG_LIBDIR *replaces* the default search path entirely so no amd64
# .pc files leak in.  Mirrors the pkg_config_libdir from the meson cross-file.
ENV PKG_CONFIG_ALLOW_CROSS=1
ENV PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig:/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig

# Tell bindgen to emit aarch64 code.  The include path picks up libcamera
# headers from the extracted tarball and arm64 GStreamer headers from apt.
ENV BINDGEN_EXTRA_CLANG_ARGS="\
    --target=aarch64-unknown-linux-gnu \
    -I/usr/local/include \
    -I/usr/include"

# ── Build ─────────────────────────────────────────────────────────────────────
FROM base AS build

WORKDIR /src
COPY rust/ /src/

# BuildKit cache mounts keep the Cargo registry and the incremental build
# cache across Docker rebuilds so that only changed crates are recompiled.
# The compiled binaries are copied out of the cache mount before it is closed.
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build \
        --example libcamera_capture \
        --release \
        --target aarch64-unknown-linux-gnu \
        --features libcamera-example && \
    cargo build \
        --example gstreamer_capture \
        --release \
        --target aarch64-unknown-linux-gnu \
        --features gstreamer-example && \
    cp target/aarch64-unknown-linux-gnu/release/examples/libcamera_capture /tmp/ && \
    cp target/aarch64-unknown-linux-gnu/release/examples/gstreamer_capture /tmp/

# ── Artifact export ───────────────────────────────────────────────────────────
FROM scratch AS artifact
COPY --from=build /tmp/libcamera_capture  /
COPY --from=build /tmp/gstreamer_capture  /
