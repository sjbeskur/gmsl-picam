#!/usr/bin/env bash

set -euo pipefail

kernel_release="${1:-$(uname -r)}"
output_path="${2:-$(pwd)/pi-kernel-${kernel_release}.tar.gz}"

build_link="/lib/modules/${kernel_release}/build"
source_link="/lib/modules/${kernel_release}/source"

if [[ ! -e "${build_link}" ]]; then
  cat >&2 <<EOF
Kernel build tree not found at ${build_link}

On the Pi, install matching headers first, for example:
  sudo apt-get update
  sudo apt-get install -y linux-headers-${kernel_release}
EOF
  exit 1
fi

build_dir="$(readlink -f "${build_link}")"
source_dir=""
if [[ -e "${source_link}" ]]; then
  source_dir="$(readlink -f "${source_link}")"
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

mkdir -p "${tmpdir}/lib/modules/${kernel_release}"
cp -aL "${build_dir}" "${tmpdir}/lib/modules/${kernel_release}/build"

if [[ -n "${source_dir}" && "${source_dir}" != "${build_dir}" ]]; then
  cp -aL "${source_dir}" "${tmpdir}/lib/modules/${kernel_release}/source"
fi

mkdir -p "${tmpdir}/metadata"
cat > "${tmpdir}/metadata/kernel-tree-manifest.txt" <<EOF
Kernel release: ${kernel_release}
Captured from: $(hostname)
Captured at: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
Build tree: ${build_dir}
Source tree: ${source_dir:-<same as build or unavailable>}
EOF

tar -C "${tmpdir}" -czf "${output_path}" .
echo "Wrote ${output_path}"
