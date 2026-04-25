FROM ubuntu:24.04 AS build

ARG DEBIAN_FRONTEND=noninteractive
ARG TARGET_KERNEL_RELEASE

RUN test -n "${TARGET_KERNEL_RELEASE}"

RUN apt-get update && apt-get install -y --no-install-recommends \
    bc \
    build-essential \
    ca-certificates \
    cpp-aarch64-linux-gnu \
    device-tree-compiler \
    file \
    g++-aarch64-linux-gnu \
    gcc-aarch64-linux-gnu \
    kmod \
    make \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY kernel/ /workspace/kernel/
COPY scripts/internal/build-kernel-pack-inside.sh /workspace/scripts/internal/build-kernel-pack-inside.sh
COPY scripts/install-kernel-pack.sh /workspace/scripts/install-kernel-pack.sh
COPY out/kernel-input/pi-kernel-tree.tar.gz /workspace/input/pi-kernel-tree.tar.gz

RUN chmod +x /workspace/scripts/internal/build-kernel-pack-inside.sh /workspace/scripts/install-kernel-pack.sh && \
    TARGET_KERNEL_RELEASE="${TARGET_KERNEL_RELEASE}" /workspace/scripts/internal/build-kernel-pack-inside.sh

FROM scratch AS artifact

COPY --from=build /opt/artifacts/ /
COPY --from=build /opt/stage/ /
