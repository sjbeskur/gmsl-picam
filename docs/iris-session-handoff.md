# Iris session handoff

Date: 2026-11-16 (America/Denver)
Session tag: `iris-reliability-2026-11-16`

## Daemon/client follow-up

The working tree now splits the previous service executable into `irisd` and an
`iris` control client. See [the current service guide](irisd-service.md) for build,
installation, API, and validation commands. The tagged session described below
predates this split: its `/tmp/iris` command is the old daemon, not the new client.

The new install bundle is exported under `out/iris-daemon`. `just deploy-iris`
copies both binaries and the installer/unit/defaults to `/tmp/iris-install`.
Persistent defaults live in `/etc/iris/irisd.env`. Runtime gain/exposure settings
can change without a capture restart. Camera start/stop is separate from systemd
service start/stop. The installed control API uses authenticated HTTP/JSON over TCP on port 8081;
preview stays on port 8080. The installer generates `/etc/iris/control.token`;
remote clients use `--address <pi>:8081 --token-file <private-file>`. Manual daemon
startup defaults to loopback and requires a token file for non-loopback binds. Local tests cover serialized ownership, live updates, failed-worker
recovery, validation, and the control API's required mutation header.

`iris-api` now holds shared serializable settings/status types and validation;
`iris-client` is the async HTTP foundation used by the CLI, independent of camera
and server dependencies. It builds for native and wasm32 browser targets. Browser
usage requires an application proxy; no browser UI was built. See the client
library README for usage.

The daemon/client pair passed Pi hardware checks: live exposure changed from
59000 us to measured 44999 us and gain from 1.5 to 2.0, without restarting capture.
Camera stop/start passed in the same daemon, and full SIGTERM daemon restart
checks also passed. The install bundle is on the Pi at `/tmp/iris-install`.
System-wide installation remains pending an interactive sudo command because
passwordless sudo is unavailable: `sudo bash /tmp/iris-install/install-irisd.sh`.
Tests leave capture stopped so the install does not conflict with a manual daemon.
Direct authenticated TCP control from the development host to `192.168.50.150:8081`
also passed status/settings/start/stop and rejected a missing credential. The
remote test daemon was stopped and its test token removed; the real installer
creates a fresh persistent credential. Validation now includes 34 native unit
checks, a client documentation example, native/ARM builds and a wasm32 client
build. The systemd unit was verified against its staged executable; activation
under the dedicated service account still needs the sudo installation step.

## Connectivity utility follow-up

`iris ping` checks the default connection; `iris ping <name>` and
`iris camera <name> ping` use a saved camera profile. The library exposes
`IrisClient::ping()`. A new authenticated `GET /ping` route responds with an empty
HTTP 204 without touching camera state. The CLI reports round-trip milliseconds
and uses a three-second timeout. Older daemon builds return 404 until updated.
The native installed CLI and `/tmp/iris-install` bundle have been refreshed.
An isolated idle Pi daemon returned pong (0.71 ms on loopback); subsequent status
remained Stopped with zero frames. The probe daemon was terminated, and no active
service was restarted. Workspace validation passed 42 tests and a client doc
example; ARM and wasm32 builds passed.

## Named client profiles follow-up

The workstation's native CLI is installed at `~/.local/bin/iris` using
`cargo install --locked --path crates/iris --bin iris --no-default-features
--features iris-client --root ~/.local`. The directory is already on PATH;
invocation from outside the repository was verified. Reinstall/update from the
repository with `just install-iris`. This does not install or start the Pi daemon.

`iris config [name] --endpoint <url> --token-file <path>` now saves native client
TOML under `$XDG_CONFIG_HOME/iris/config.toml` or `~/.config/iris/config.toml`.
`iris camera <name> start|stop|status|set` resolves the saved profile. Names are
endpoint aliases for each daemon's selected camera; there is still one capture
worker per daemon. No-argument `iris config` creates/updates the default profile.
Use `iris show` (or `iris config --show`) for offline inspection, and `--config`/`IRIS_CONFIG` for another
file. The reusable native implementation is `iris-client`'s optional `config`
module; its transport still builds for WebAssembly without filesystem access.

Config persistence is atomic, mode 0600 on Unix, stores credential paths rather
than secrets, preserves other profiles, and rejects malformed TOML rather than
overwriting it. Named CLI routing and credentials are exercised through an actual
local HTTP server. Legacy top-level commands still work using the default profile
or localhost, so older regression scripts remain compatible.
Validation for this follow-up: 39 native unit/integration tests and the client
library documentation example pass; the ARM bundle and wasm32 client build pass.
On the Pi, saving/showing a profile with an isolated temporary config passed and
its file mode was 0600. The refreshed bundle remains at `/tmp/iris-install`;
no real user config or active systemd service was changed by the profile check.

## Tagged session baseline (historical)

- Raspberry Pi 5: `192.168.50.150`, SSH user `sbeskur`.
- One IMX477 enumerated, camera index `0`.
- Iris is running from `/tmp/iris` at `0.0.0.0:8080`, 1280×720 NV12,
  requested exposure `59000 us`, analogue gain `1.5`.
- Actual metadata: exposure `58998 us`, gain approximately `1.4992679`, frame
  duration `60985 us`; valid advancing sensor timestamps and no reported errors.
- Preview: `http://192.168.50.150:8080/stream.mjpg`.
- Status: `http://192.168.50.150:8080/status`.
- Snapshot: `http://192.168.50.150:8080/frame.jpg`.
- Current test log: `/tmp/iris-quality.log`; last observed process ID was `2600`.
  Verify the command line before sending any signal; PIDs can be reused.
- `/tmp/iris` is temporary and disappears on reboot. No boot service was installed.

## Changes completed

1. Shutdown handles SIGTERM and Ctrl-C, requests capture cancellation, explicitly
   stops libcamera, and joins the worker before exit. Open MJPEG clients cannot
   block shutdown. Stop and wait for exit before starting another camera process.
2. Camera failures and worker panics update status to `Failed`. Cancelled requests,
   invalid timestamps, failed buffers, and invalid bytes-used lengths are rejected.
   Normal PiSP `Startup` buffers are recycled during a bounded warmup interval.
3. Frame freshness uses a monotonic clock with a two-second maximum age.
   Snapshots and new streams return 503 when unhealthy or stale. Existing MJPEG
   streams close on failure/expiry and never repeatedly publish the same frame.
   Image responses disable caching; status includes `frame_age_ms` and `frame_stale`.
4. Timing metadata no longer substitutes requested settings for missing actual
   exposure/gain/frame duration. These values can be null. `coi_ns` is null because
   active-array readout timing is unavailable. `first_row_coi_ns` reports the first
   row exposure midpoint when known. Timing helpers now validate inputs, return
   `Result`, and take active readout duration rather than frame period.
5. JPEG conversion handles NV12 plane stride/padding, validates negotiated pixel
   format, and selects BT.601/BT.709 and full/limited range from color metadata.
6. PiSP configuration propagates link-state errors and rejects missing required
   media links, with entity/pad diagnostics. This change lives in the libcamera
   submodule, committed as `a8589ac3` on `feature/fixes_for_pi_ubunt`; the root
   repository records that gitlink.
7. `just` build and deploy recipes share `artifact_dir`. Override with
   `IRIS_ARTIFACT_DIR` or `just artifact_dir=out/iris-quality ...`; the positional
   build-directory argument was replaced by this shared variable.

API changes: clients must accept nullable timing/exposure/gain fields. Consumers
of Rust timing helpers must handle their `Result` return type. Output-image rows
must not be treated as active sensor-array rows for rolling-shutter correction.

## Validation

- `cargo test --offline --features http-support,jpeg-support`: 24 tests pass.
- Formatting, shell syntax, root/submodule diff checks pass.
- Clippy completes with an existing unused `tracing-subscriber` dependency warning
  in the non-service feature combination.
- Iris and both examples cross-build successfully. Artifacts: `out/iris-quality/`.
- Modified libcamera cross-build succeeds. Artifacts: `out/libcamera-quality/`.
  **The new libcamera library was not installed on the Pi.** Hardware tests used
  its existing libcamera v0.7.0 installation, with the new Iris executable.
- `scripts/check-iris-restart.py` verifies advancing timestamps, healthy status,
  a JPEG response, and clean SIGTERM exit across two capture sessions. It passed
  at exposure 59000 us with gain 1.0 and again with gain 1.5.

Run the hardware regression only on an idle Pi:

```bash
python3 /tmp/check-iris-restart.py /tmp/iris --exposure-us 59000 --gain 1.5
```

## Exposure findings and next steps

- A clean-start test at exposure `75000 us`, requesting frame period `77000 us`,
  still causes a camera frontend timeout and cancelled requests on this setup.
  This is a known failing setting, not a measured maximum exposure.
- Returning to `59000 us` with the new build recovered without a power cycle;
  two subsequent sessions with gain 1.5 passed. Gain 1.5 is the validated brightness
  adjustment for now. Longer exposures need separate sensor/GMSL investigation.
- Earlier abrupt SIGTERM shutdowns with the old binary left capture failing until
  the camera was reconnected or power-cycled. Clean restarts now pass. Do not infer
  that every historical timeout was solely caused by shutdown behavior.
- Next: inspect supported exposure/frame-duration bounds and sensor mode timing,
  preserve useful kernel/libcamera diagnostics for the longer-exposure failure,
  and investigate driver/bridge behavior before another exposure sweep.
- The new PiSP link diagnostics still need hardware validation after installing
  the rebuilt libcamera. Installation requires sudo; passwordless sudo was not
  available during this session. Avoid replacing the working library casually.
- Exact frame-center CoI requires active sensor mode/line timing and crop mapping;
  do not restore the previous frame-period approximation for IMU synchronization.
- Dual-camera work remains separate. The older
  [dual-camera handoff](gmsl-dual-camera-debug-handoff.md) records vendor questions;
  no new vendor response or dual-camera validation occurred this session.

## Build and resume

```bash
export IRIS_ARTIFACT_DIR=out/iris-quality
just build
just deploy-iris
```

The libcamera artifact tarball required by the Rust cross-build is still
`out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz`. The newly built library in
`out/libcamera-quality/` is separate; choosing that library for future builds or
installation should be intentional.

On the Pi:

```bash
/tmp/iris --bind 0.0.0.0:8080 --camera-index 0 --width 1280 --height 720 \
  --exposure-us 59000 --gain 1.5
```

SSH/scp on the development host needed `-F /dev/null` because the system SSH
configuration reported bad ownership/permissions for
`/etc/ssh/ssh_config.d/20-systemd-ssh-proxy.conf`. `just` recipes still use normal
SSH configuration; fix that host configuration or use explicit SSH/scp commands.
