FROM ubuntu:24.04 AS build

ARG DEBIAN_FRONTEND=noninteractive

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
    build-essential \
    ca-certificates \
    g++-aarch64-linux-gnu \
    gcc-aarch64-linux-gnu \
    git \
    libdrm-dev:arm64 \
    libdw-dev:arm64 \
    libevent-dev:arm64 \
    libexif-dev:arm64 \
    libglib2.0-dev-bin \
    libgnutls28-dev:arm64 \
    libgstreamer-plugins-base1.0-dev:arm64 \
    libgstreamer1.0-dev:arm64 \
    libjpeg-dev:arm64 \
    libtiff-dev:arm64 \
    libudev-dev:arm64 \
    libunwind-dev:arm64 \
    libyaml-dev:arm64 \
    meson \
    ninja-build \
    openssl \
    pkg-config \
    python3 \
    python3-jinja2 \
    python3-ply \
    python3-yaml \
    zlib1g-dev:arm64 && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src/libcamera

COPY libcamera/ /src/libcamera/
COPY toolchains/libcamera-aarch64-cross.ini /toolchains/libcamera-aarch64-cross.ini

RUN meson setup /build /src/libcamera \
    --cross-file /toolchains/libcamera-aarch64-cross.ini \
    --buildtype=release \
    --prefix=/usr/local \
    -Dpipelines=rpi/pisp,rpi/vc4,simple,uvcvideo \
    -Dipas=rpi/pisp,rpi/vc4,simple \
    -Dgstreamer=enabled \
    -Dcam=enabled \
    -Dcam-output-kms=disabled \
    -Dcam-output-sdl2=disabled \
    -Dqcam=disabled \
    -Ddocumentation=disabled \
    -Dpycamera=disabled \
    -Dv4l2=disabled \
    -Dtest=false && \
    ninja -C /build && \
    DESTDIR=/opt/stage ninja -C /build install && \
    mkdir -p /opt/artifacts && \
    tar -C /opt/stage -czf /opt/artifacts/libcamera-arm64-ubuntu.tar.gz .

FROM scratch AS artifact

COPY --from=build /opt/artifacts/ /
COPY --from=build /opt/stage/ /
