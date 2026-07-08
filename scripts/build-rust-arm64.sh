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
echo "Done. Binaries in ${output_dir}:"
ls -lh "${output_dir}"
echo ""
echo "Deploy to Pi:"
echo "  scp ${output_dir}/iris sbeskur@192.168.50.24:/tmp/"
echo "  scp ${output_dir}/libcamera_capture sbeskur@192.168.50.24:/tmp/"
echo "  scp ${output_dir}/gstreamer_capture sbeskur@192.168.50.24:/tmp/"
echo ""
echo "Run on Pi:"
echo "  sudo /tmp/iris --bind 0.0.0.0:8080 --camera-index 0 --width 1280 --height 720"
echo "  sudo /tmp/iris --bind 0.0.0.0:8081 --camera-index 1 --width 1280 --height 720"
echo "  sudo /tmp/libcamera_capture --frames 10 --exposure-us 8000"
echo "  GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0 /tmp/gstreamer_capture --frames 30"
