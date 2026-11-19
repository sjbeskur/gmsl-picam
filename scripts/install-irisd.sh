#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'HELP'
Usage: install-irisd.sh [artifact-directory] [--no-start] [--destdir directory]

Install irisd, iris, iris-view, and their systemd services. Run with sudo on the Pi.
Existing /etc/iris/irisd.env is preserved. By default, enable and restart irisd.
--no-start installs without enabling or starting the service.
--destdir stages files only; skips accounts, binary execution, and systemd.
HELP
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
artifact_dir=""
destdir=""
start_service=true
while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-start) start_service=false; shift ;;
        --destdir)
            [[ $# -ge 2 && -n "$2" ]] || { usage >&2; exit 1; }
            destdir="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
        *) [[ -z "$artifact_dir" ]] || { usage >&2; exit 1; }; artifact_dir="$1"; shift ;;
    esac
done
if [[ -z "$artifact_dir" ]]; then
    if [[ -x "$script_dir/irisd" && -x "$script_dir/iris" ]]; then
        artifact_dir="$script_dir"
    else
        artifact_dir="${IRIS_ARTIFACT_DIR:-$script_dir/../target/aarch64-unknown-linux-gnu/release}"
    fi
fi
artifact_dir="$(cd "$artifact_dir" && pwd)"
if [[ -f "$script_dir/irisd.service" ]]; then
    unit_dir="$script_dir"
else
    unit_dir="$script_dir/../deploy"
fi
for file in "$artifact_dir/irisd" "$artifact_dir/iris" "$artifact_dir/iris-view" "$unit_dir/irisd.service" "$unit_dir/irisd.env" "$unit_dir/iris-view.service" "$unit_dir/iris-view.env"; do
    [[ -f "$file" ]] || { echo "Missing install input: $file" >&2; exit 1; }
done
if [[ -n "$destdir" ]]; then
    mkdir -p "$destdir"
    destdir="$(cd "$destdir" && pwd)"
    [[ "$destdir" != / ]] || { echo "Use sudo without --destdir for a real install" >&2; exit 1; }
else
    [[ "$EUID" -eq 0 ]] || { echo "Run with sudo on the Pi, or use --destdir for staging" >&2; exit 1; }
    command -v systemctl >/dev/null
    # Fail before changing files if these binaries cannot run with this Pi's libraries.
    "$artifact_dir/irisd" --help >/dev/null
    "$artifact_dir/iris" --help >/dev/null
    "$artifact_dir/iris-view" --help >/dev/null
    for group in video render; do
        getent group "$group" >/dev/null || groupadd --system "$group"
    done
    getent group iris >/dev/null || groupadd --system iris
    if ! getent passwd iris >/dev/null; then
        useradd --system --gid iris --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin iris
    fi
fi
install -d -m 0755 "$destdir/usr/local/bin" "$destdir/etc/systemd/system" "$destdir/etc/iris"
for binary in irisd iris iris-view; do
    # Atomic replacement works even while the previous executable is running.
    temporary="$(mktemp "$destdir/usr/local/bin/.${binary}.XXXXXX")"
    install -m 0755 "$artifact_dir/$binary" "$temporary"
    mv -f "$temporary" "$destdir/usr/local/bin/$binary"
done
for unit in irisd iris-view; do
    install -m 0644 "$unit_dir/$unit.service" "$destdir/etc/systemd/system/$unit.service"
done
if [[ ! -e "$destdir/etc/iris/iris-view.env" ]]; then
    install -m 0644 "$unit_dir/iris-view.env" "$destdir/etc/iris/iris-view.env"
fi
if [[ ! -e "$destdir/etc/iris/irisd.env" ]]; then
    install -m 0644 "$unit_dir/irisd.env" "$destdir/etc/iris/irisd.env"
fi
# Preserve camera defaults while giving upgraded installations a viewer output.
if ! grep -q '^IRIS_TRANSMIT=' "$destdir/etc/iris/irisd.env"; then
    printf '\nIRIS_TRANSMIT=tcp://127.0.0.1:9001\n' >> "$destdir/etc/iris/irisd.env"
fi
if [[ -z "$destdir" ]]; then
    systemctl daemon-reload
    if "$start_service"; then
        systemctl enable irisd.service iris-view.service
        systemctl restart irisd.service iris-view.service
        systemctl --no-pager --full status irisd.service iris-view.service
    fi
else
    echo "Staged service files under $destdir; no accounts or services changed."
fi
echo "Installed. Camera: iris status | iris start | iris stop | iris set --gain 1.5"
echo "Services: sudo systemctl stop irisd iris-view; logs: journalctl -u irisd -u iris-view -f"
