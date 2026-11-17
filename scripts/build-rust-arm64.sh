#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
output_dir="${1:-${repo_root}/target/aarch64-unknown-linux-gnu/release}"
libcamera_tarball="${repo_root}/out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz"

if [[ ! -f "${libcamera_tarball}" ]]; then
    echo "ERROR: libcamera arm64 tarball not found at:"
    echo "  ${libcamera_tarball}"
    echo ""
    echo "Place the tarball there before running the Rust build."
    exit 1
fi

mkdir -p "${output_dir}"

if [[ ! -e /proc/sys/fs/binfmt_misc/qemu-aarch64 ]]; then
    echo "Registering QEMU ARM64 binfmt handler (one-time host setup)..."
    docker run --privileged --rm tonistiigi/binfmt --install arm64
    echo "Done."
else
    echo "QEMU ARM64 binfmt already registered."
fi

echo "Building Rust arm64 examples..."
echo "  Output: ${output_dir}"
echo ""

docker buildx build \
    --build-context "libcamera-tarball=${repo_root}/out/libcamera-arm64" \
    --file "${repo_root}/docker/rust-arm64.Dockerfile" \
    --target artifact \
    --output "type=local,dest=${output_dir}" \
    "${repo_root}"

echo ""
echo "Done. Install bundle and binaries in ${output_dir}:"
ls -lh "${output_dir}"
echo ""
echo "Copy the install bundle to the Pi and run:"
echo "  sudo bash /path/to/bundle/install-irisd.sh"
echo "Camera control:"
echo "  iris status"
echo "  iris set --exposure-us 59000 --gain 1.5"
echo "  iris stop"
echo "  iris start"
