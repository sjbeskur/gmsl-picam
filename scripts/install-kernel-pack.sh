#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  install-kernel-pack.sh <kernel-pack.tar.gz>
EOF
}

pack_path="${1:-}"
if [[ -z "${pack_path}" ]]; then
  usage >&2
  exit 1
fi

if [[ ! -f "${pack_path}" ]]; then
  echo "Kernel pack not found: ${pack_path}" >&2
  exit 1
fi

manifest_path="usr/share/gmsl-picam/kernel-pack-manifest.txt"
manifest_contents="$(
  tar -xOf "${pack_path}" "${manifest_path}" 2>/dev/null || \
  tar -xOf "${pack_path}" "./${manifest_path}" 2>/dev/null || \
  true
)"
kernel_release="$(awk -F': ' '/^Kernel release:/ { print $2; exit }' <<< "${manifest_contents}")"

sudo tar -C / -xzf "${pack_path}"

if [[ -n "${kernel_release}" && -d "/lib/modules/${kernel_release}" ]]; then
  sudo depmod -a "${kernel_release}"
  if [[ "${kernel_release}" != "$(uname -r)" ]]; then
    echo "Installed kernel pack for ${kernel_release}, but the running kernel is $(uname -r)."
    echo "Reboot into the matching kernel before testing camera modules."
  fi
fi

echo "Kernel pack installed."
echo "If you added overlays, confirm /boot/firmware/config.txt references the right dtoverlay entries."
