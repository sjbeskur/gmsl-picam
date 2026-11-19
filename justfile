set shell := ["bash", "-euo", "pipefail", "-c"]

# Override once for every artifact recipe: just artifact_dir=out/test build
artifact_dir := env("IRIS_ARTIFACT_DIR", "target/aarch64-unknown-linux-gnu/release")

default:
    @just --list

build:
    cd "{{justfile_directory()}}" && ./scripts/build-rust-arm64.sh "{{artifact_dir}}"

# Install the native client as ~/.local/bin/iris; no camera libraries or sudo needed.
install-iris $install_root=(env("HOME") + "/.local"):
    cd "{{justfile_directory()}}" && cargo install --locked --path crates/iris --bin iris --no-default-features --features iris-client --root "$install_root" --force

# Copy the install bundle to the Pi; then run sudo bash /tmp/iris-install/install-irisd.sh.
deploy-iris $host="192.168.50.150" $user="sbeskur":
    cd "{{justfile_directory()}}" && test -x "{{artifact_dir}}/irisd" && test -x "{{artifact_dir}}/iris" && test -x "{{artifact_dir}}/iris-view"
    ssh "${user}@${host}" 'mkdir -p /tmp/iris-install'
    cd "{{justfile_directory()}}" && scp "{{artifact_dir}}/irisd" "{{artifact_dir}}/iris" "{{artifact_dir}}/iris-view" scripts/install-irisd.sh deploy/irisd.service deploy/irisd.env deploy/iris-view.service deploy/iris-view.env "${user}@${host}:/tmp/iris-install/"

deploy-libcam $host="192.168.50.150" $user="sbeskur" $path="/tmp/libcamera_capture":
    cd "{{justfile_directory()}}" && test -x "{{artifact_dir}}/libcamera_capture"
    cd "{{justfile_directory()}}" && scp "{{artifact_dir}}/libcamera_capture" "${user}@${host}:${path}"


pi-camera-check $host="192.168.50.150" $user="sbeskur":
    cd "{{justfile_directory()}}" && ssh "${user}@${host}" '/usr/local/bin/cam --list && media-ctl -d /dev/media0 -p 2>/dev/null' | sed -n "1,120p"

package-kernel $kernel_release $kernel_tree_tar $output_dir="":
    cd "{{justfile_directory()}}" && if [[ -n "${output_dir}" ]]; then \
      ./scripts/build-kernel-pack.sh --kernel-release "${kernel_release}" --kernel-tree-tar "${kernel_tree_tar}" --output-dir "${output_dir}"; \
    else \
      ./scripts/build-kernel-pack.sh --kernel-release "${kernel_release}" --kernel-tree-tar "${kernel_tree_tar}"; \
    fi
