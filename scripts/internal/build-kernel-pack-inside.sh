#!/usr/bin/env bash

set -euo pipefail

target_kernel_release="${TARGET_KERNEL_RELEASE:?TARGET_KERNEL_RELEASE is required}"
input_archive="/workspace/input/pi-kernel-tree.tar.gz"
kernel_root="/opt/target-kernel"
stage_root="/opt/stage"
artifacts_root="/opt/artifacts"
overlay_stage="${stage_root}/boot/firmware/overlays"
manifest_stage_dir="${stage_root}/usr/share/gmsl-picam"

if [[ ! -f "${input_archive}" ]]; then
  echo "Missing kernel tree archive: ${input_archive}" >&2
  exit 1
fi

mkdir -p "${kernel_root}" "${overlay_stage}" "${manifest_stage_dir}" "${artifacts_root}"
tar -xzf "${input_archive}" -C "${kernel_root}"

kernel_build_dir="${kernel_root}/lib/modules/${target_kernel_release}/build"
kernel_source_dir="${kernel_root}/lib/modules/${target_kernel_release}/source"

if [[ ! -d "${kernel_build_dir}" ]]; then
  echo "Expected kernel build tree at ${kernel_build_dir}" >&2
  exit 1
fi

declare -a built_overlay_paths=()
declare -a installed_module_paths=()

compile_overlay() {
  local source_file="$1"
  local output_file="$2"
  local preprocessed
  local -a cpp_args

  preprocessed="$(mktemp)"
  cpp_args=(
    -nostdinc
    -undef
    -x
    assembler-with-cpp
    -D__DTS__
  )

  if [[ -d "${kernel_build_dir}/include" ]]; then
    cpp_args+=(-I"${kernel_build_dir}/include")
  fi
  if [[ -d "${kernel_build_dir}/arch/arm64/boot/dts" ]]; then
    cpp_args+=(-I"${kernel_build_dir}/arch/arm64/boot/dts")
  fi
  if [[ -d "${kernel_build_dir}/arch/arm64/boot/dts/include" ]]; then
    cpp_args+=(-I"${kernel_build_dir}/arch/arm64/boot/dts/include")
  fi
  if [[ -d "${kernel_build_dir}/scripts/dtc/include-prefixes" ]]; then
    cpp_args+=(-I"${kernel_build_dir}/scripts/dtc/include-prefixes")
  fi
  if [[ -d "${kernel_source_dir}" && "${kernel_source_dir}" != "${kernel_build_dir}" ]]; then
    if [[ -d "${kernel_source_dir}/include" ]]; then
      cpp_args+=(-I"${kernel_source_dir}/include")
    fi
    if [[ -d "${kernel_source_dir}/arch/arm64/boot/dts" ]]; then
      cpp_args+=(-I"${kernel_source_dir}/arch/arm64/boot/dts")
    fi
    if [[ -d "${kernel_source_dir}/arch/arm64/boot/dts/include" ]]; then
      cpp_args+=(-I"${kernel_source_dir}/arch/arm64/boot/dts/include")
    fi
    if [[ -d "${kernel_source_dir}/scripts/dtc/include-prefixes" ]]; then
      cpp_args+=(-I"${kernel_source_dir}/scripts/dtc/include-prefixes")
    fi
  fi

  aarch64-linux-gnu-cpp "${cpp_args[@]}" "${source_file}" > "${preprocessed}"
  dtc -@ -I dts -O dtb -o "${output_file}" "${preprocessed}"
  rm -f "${preprocessed}"
}

if [[ -d /workspace/kernel/overlays ]]; then
  while IFS= read -r -d '' overlay_file; do
    overlay_name="$(basename "${overlay_file}")"
    cp "${overlay_file}" "${overlay_stage}/${overlay_name}"
    built_overlay_paths+=("boot/firmware/overlays/${overlay_name}")
  done < <(find /workspace/kernel/overlays -maxdepth 1 -type f -name '*.dtbo' -print0 | sort -z)

  while IFS= read -r -d '' overlay_source; do
    overlay_base="$(basename "${overlay_source}" .dts)"
    output_path="${overlay_stage}/${overlay_base}.dtbo"
    compile_overlay "${overlay_source}" "${output_path}"
    built_overlay_paths+=("boot/firmware/overlays/${overlay_base}.dtbo")
  done < <(find /workspace/kernel/overlays -maxdepth 1 -type f -name '*.dts' -print0 | sort -z)
fi

if [[ -d /workspace/kernel/modules ]]; then
  while IFS= read -r -d '' module_dir; do
    if [[ ! -f "${module_dir}/Makefile" && ! -f "${module_dir}/Kbuild" ]]; then
      continue
    fi

    make -C "${kernel_build_dir}" \
      M="${module_dir}" \
      ARCH=arm64 \
      CROSS_COMPILE=aarch64-linux-gnu- \
      modules

    make -C "${kernel_build_dir}" \
      M="${module_dir}" \
      ARCH=arm64 \
      CROSS_COMPILE=aarch64-linux-gnu- \
      INSTALL_MOD_PATH="${stage_root}" \
      INSTALL_MOD_DIR=extra \
      modules_install
  done < <(find /workspace/kernel/modules -mindepth 1 -maxdepth 1 -type d -print0 | sort -z)
fi

if [[ -d "${stage_root}/lib/modules/${target_kernel_release}" ]]; then
  while IFS= read -r -d '' module_file; do
    installed_module_paths+=("${module_file#${stage_root}/}")
  done < <(find "${stage_root}/lib/modules/${target_kernel_release}" -type f \( -name '*.ko' -o -name '*.ko.*' \) -print0 | sort -z)

  depmod -b "${stage_root}" "${target_kernel_release}"
fi

manifest_path="${manifest_stage_dir}/kernel-pack-manifest.txt"
{
  echo "Kernel release: ${target_kernel_release}"
  echo "Generated at: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  echo "Overlay count: ${#built_overlay_paths[@]}"
  for overlay_path in "${built_overlay_paths[@]}"; do
    echo "Overlay: ${overlay_path}"
  done
  echo "Module count: ${#installed_module_paths[@]}"
  for module_path in "${installed_module_paths[@]}"; do
    echo "Module: ${module_path}"
  done
} > "${manifest_path}"

cp "${manifest_path}" "${artifacts_root}/kernel-pack-${target_kernel_release}.manifest.txt"
cp /workspace/scripts/install-kernel-pack.sh "${artifacts_root}/install-kernel-pack.sh"
tar -C "${stage_root}" -czf "${artifacts_root}/kernel-pack-${target_kernel_release}.tar.gz" .
