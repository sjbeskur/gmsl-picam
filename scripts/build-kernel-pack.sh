#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  build-kernel-pack.sh --kernel-release <release> --kernel-tree-tar <path> [--output-dir <path>]

Example:
  ./scripts/build-kernel-pack.sh \
    --kernel-release 6.8.0-1018-raspi \
    --kernel-tree-tar ~/pi-kernel-6.8.0-1018-raspi.tar.gz
EOF
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

kernel_release=""
kernel_tree_tar=""
output_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --kernel-release)
      kernel_release="${2:-}"
      shift 2
      ;;
    --kernel-tree-tar)
      kernel_tree_tar="${2:-}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "${kernel_release}" || -z "${kernel_tree_tar}" ]]; then
  usage >&2
  exit 1
fi

if [[ ! -f "${kernel_tree_tar}" ]]; then
  echo "Kernel tree archive not found: ${kernel_tree_tar}" >&2
  exit 1
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="${repo_root}/out/kernel-pack/${kernel_release}"
fi

input_dir="${repo_root}/out/kernel-input"
mkdir -p "${input_dir}" "${output_dir}"
cp "${kernel_tree_tar}" "${input_dir}/pi-kernel-tree.tar.gz"

docker buildx build \
  --file "${repo_root}/docker/kernel-pack.Dockerfile" \
  --build-arg "TARGET_KERNEL_RELEASE=${kernel_release}" \
  --target artifact \
  --output "type=local,dest=${output_dir}" \
  "${repo_root}"
