set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

build-libcamera $output_dir="out/libcamera-arm64":
    ./scripts/build-libcamera-arm64.sh "$output_dir"

build-rust $output_dir="out/rust-arm64":
    ./scripts/build-rust-arm64.sh "$output_dir"

build-iris: build-rust

deploy-iris $host="192.168.50.24" $user="sbeskur" $path="/tmp/iris":
    test -x out/rust-arm64/iris
    scp out/rust-arm64/iris "${user}@${host}:${path}"

pi-camera-check $host="192.168.50.24" $user="sbeskur":
    ssh "${user}@${host}" '/usr/local/bin/cam --list && media-ctl -d /dev/media0 -p 2>/dev/null' | sed -n "1,120p"

package-kernel $kernel_release $kernel_tree_tar $output_dir="":
    if [[ -n "${output_dir}" ]]; then \
      ./scripts/build-kernel-pack.sh --kernel-release "${kernel_release}" --kernel-tree-tar "${kernel_tree_tar}" --output-dir "${output_dir}"; \
    else \
      ./scripts/build-kernel-pack.sh --kernel-release "${kernel_release}" --kernel-tree-tar "${kernel_tree_tar}"; \
    fi
