#!/usr/bin/env bash
# Cross-compile the gmsl-picam Rust examples for aarch64 (Raspberry Pi 5).
#
# Usage:
#   ./scripts/build-rust-arm64.sh [output-dir]
#
# Default output: out/rust-arm64/
# Binaries produced: libcamera_capture  gstreamer_capture
#
# Prerequisites:
#   The libcamera arm64 tarball must exist at out/libcamera-arm64/.
#   If it is missing, run ./scripts/build-libcamera-arm64.sh first.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
output_dir="${1:-${repo_root}/out/rust-arm64}"
libcamera_tarball="${repo_root}/out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz"

# ── Preflight check ───────────────────────────────────────────────────────────
if [[ ! -f "${libcamera_tarball}" ]]; then
    echo "ERROR: libcamera arm64 tarball not found at:"
    echo "  ${libcamera_tarball}"
    echo ""
    echo "Build it first with:"
    echo "  ./scripts/build-libcamera-arm64.sh"
    exit 1
fi

mkdir -p "${output_dir}"

# ── QEMU binfmt registration ──────────────────────────────────────────────────
# ARM64 apt packages run post-install scripts that are ARM64 binaries (e.g.
# python3.12 compiling .pyc files).  Without a binfmt handler those scripts
# fail with "Exec format error" on an x86_64 host.
#
# tonistiigi/binfmt registers handlers with the kernel's binfmt_misc and keeps
# the interpreter fd open (F flag), so ARM64 binaries inside any Docker
# container on this host will transparently execute via QEMU.
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

# ── Docker build ──────────────────────────────────────────────────────────────
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
echo "  scp ${output_dir}/libcamera_capture sbeskur@192.168.50.24:/tmp/"
echo "  scp ${output_dir}/gstreamer_capture sbeskur@192.168.50.24:/tmp/"
echo ""
echo "Run on Pi:"
echo "  sudo /tmp/libcamera_capture --frames 10 --exposure-us 8000"
echo "  GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0 /tmp/gstreamer_capture --frames 30"
