# Iris session handoff

Date: 2026-11-16 (America/Denver)
Session tag: `iris-reliability-2026-11-16`

## Working state

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
