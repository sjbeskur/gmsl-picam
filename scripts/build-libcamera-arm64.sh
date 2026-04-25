#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
output_dir="${1:-${repo_root}/out/libcamera-arm64}"

mkdir -p "${output_dir}"

docker buildx build \
  --file "${repo_root}/docker/libcamera-arm64.Dockerfile" \
  --target artifact \
  --output "type=local,dest=${output_dir}" \
  "${repo_root}"
