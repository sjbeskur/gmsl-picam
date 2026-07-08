set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

build $output_dir="target/aarch64-unknown-linux-gnu/release":
    cd "{{justfile_directory()}}" && ./scripts/build-rust-arm64.sh "$output_dir"

deploy-iris $host="192.168.50.150" $user="sbeskur" $path="/tmp/iris":
    cd "{{justfile_directory()}}" && test -x target/aarch64-unknown-linux-gnu/release/iris
    cd "{{justfile_directory()}}" && scp target/aarch64-unknown-linux-gnu/release/iris "${user}@${host}:${path}"

deploy-libcam $host="192.168.50.150" $user="sbeskur" $path="/tmp/libcamera_capture":
    cd "{{justfile_directory()}}" && test -x target/aarch64-unknown-linux-gnu/release/libcamera_capture
    cd "{{justfile_directory()}}" && scp target/aarch64-unknown-linux-gnu/release/libcamera_capture "${user}@${host}:${path}"


pi-camera-check $host="192.168.50.150" $user="sbeskur":
    cd "{{justfile_directory()}}" && ssh "${user}@${host}" '/usr/local/bin/cam --list && media-ctl -d /dev/media0 -p 2>/dev/null' | sed -n "1,120p"

package-kernel $kernel_release $kernel_tree_tar $output_dir="":
    cd "{{justfile_directory()}}" && if [[ -n "${output_dir}" ]]; then \
      ./scripts/build-kernel-pack.sh --kernel-release "${kernel_release}" --kernel-tree-tar "${kernel_tree_tar}" --output-dir "${output_dir}"; \
    else \
      ./scripts/build-kernel-pack.sh --kernel-release "${kernel_release}" --kernel-tree-tar "${kernel_tree_tar}"; \
    fi
