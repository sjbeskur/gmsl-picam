# Iris daemon and camera control

`irisd` owns the camera and serves the preview. systemd manages its process.
`iris` is a small client that starts/stops capture and changes camera settings.

## Libraries and applications

- `crates/iris-api`: shared wire types, settings validation, and status models.
- `crates/iris-client`: async typed HTTP library, re-exporting the API types.
- `crates/iris`: the `iris` CLI and `irisd` daemon. The CLI calls `IrisClient`;
  the daemon uses `iris-api` without depending on the client transport.

The [client library guide](../crates/iris-client/README.md) includes a reusable
Rust example and browser WebAssembly/proxy guidance. Both native and wasm32 builds
are checked. No browser UI or application proxy is included yet.

## Build and install

On the development host:

```bash
export IRIS_ARTIFACT_DIR=out/iris-daemon
just build
just deploy-iris
```

The artifact directory includes `irisd`, `iris`, `irisd.service`, `irisd.env`, and
`install-irisd.sh`, plus the capture examples. The deploy recipe copies the
install bundle to `/tmp/iris-install` on the Pi (default `192.168.50.150`).

Stop any previous manually launched camera process before installation, so it
does not hold the camera or preview port. Use SIGTERM and wait for it to exit.
Then, on the Pi:

```bash
sudo bash /tmp/iris-install/install-irisd.sh
iris status
```

The installer checks that both binaries run with the Pi's libraries, installs
binaries atomically under `/usr/local/bin`, creates the `iris` system account,
and installs/enables/restarts `irisd.service`. The service has camera access via
`video` and `render` groups. Existing `/etc/iris/irisd.env` is preserved on upgrades.
The libcamera shared libraries are external prerequisites; this script does not
replace them.

To install without enabling or starting, use `--no-start`. To inspect a package
without root or system changes, use `--destdir /tmp/iris-package`.

## Client configuration and camera commands

Save the connection once on the client machine:

```bash
iris config front --endpoint http://192.168.50.150:8081 --token-file ~/.config/iris/token
iris camera front status
iris camera front start
iris camera front set --exposure-us 59000 --gain 1.5
iris camera front stop
```

`iris config` with no arguments creates or updates a `default` profile, using
localhost when there is no saved endpoint. `iris config --show` displays the saved
TOML without changing it. The default path is `$XDG_CONFIG_HOME/iris/config.toml`
or `~/.config/iris/config.toml`. Override it with `--config <path>` or `IRIS_CONFIG`.

Example file:

```toml
[cameras.front]
endpoint = "http://192.168.50.150:8081"
token_file = "/home/user/.config/iris/token"

[cameras.rear]
endpoint = "http://192.168.50.151:8081"
token_file = "/home/user/.config/iris/rear.token"
```

Names are connection aliases for the camera owned by each daemon. Each daemon
currently captures one selected sensor. Adding a profile does not create another
capture worker or switch sensors on a daemon. Multiple aliases can refer to the
same daemon. Use letters, digits, `-`, `_`, or `.` in names.

Config saves preserve other camera profiles and replace the file atomically with
Unix mode 0600. Only credential-file paths are saved; raw bearer tokens are never
written to TOML. Token paths supplied on the command line are saved as absolute
paths. Relative paths written manually in TOML resolve against its directory.
Malformed configuration is reported and preserved. A profile's endpoint/token
can be overridden for a command with the existing connection flags/environment.

For local use on the Pi, configure its own alias without credentials:

```bash
iris config local --endpoint http://127.0.0.1:8081
iris camera local status
iris camera local set --fps 15 --jpeg-quality 85
iris camera local set --auto-exposure --auto-gain
```

The original `iris status`, `iris start`, `iris stop`, and `iris set` remain
compatible: they use the saved `default` profile or localhost when it is absent.

`iris stop` stops and releases the camera; it leaves the daemon available for the
next `iris start`. Start/stop are serialized and idempotent. Starting a failed
capture reaps the old worker before reacquiring the camera. An unexpected worker
failure remains visible until explicit recovery; there is no automatic retry
loop changing sensor state.

`iris set` validates the whole patch before accepting it. Exposure, gain, FPS,
and JPEG quality apply during capture, without restarting the daemon or camera.
Changes reach the sensor through subsequent capture requests and may take several
frames to appear. `requested_settings` describes desired values; actual exposure,
gain and frame duration are in `camera.latest_frame`. A successful command is
acceptance of the requested settings, not confirmation that hardware applied them.
Sensor limits may clamp settings; check actual metadata and `camera_state`.

Width, height, and camera index require stopped capture:

```bash
iris camera front stop
iris camera front set --width 1280 --height 720 --camera-index 0
iris camera front start
```

The fixed frame duration is the larger of `1_000_000 / fps` and manual exposure
plus 2000 microseconds. FPS is therefore a requested maximum when exposure needs
more time, and sensor mode constraints can further limit it.

The validated manual baseline is exposure 59000 us and gain 1.5. Longer exposure
can increase motion blur and reduce frame rate. Gain increases brightness with
additional noise. The previous clean-start 75000 us test failed; live updates do
not establish that this exposure is now supported.

## Persistent defaults and service management

Runtime `iris set` changes persist through camera stop/start, but not daemon
restart. Edit `/etc/iris/irisd.env` to change boot defaults:

```bash
sudoedit /etc/iris/irisd.env
sudo systemctl restart irisd
sudo systemctl stop irisd
sudo systemctl start irisd
systemctl status irisd
journalctl -u irisd -f
```

Defaults are 1280×720, exposure 59000 us, gain 1.5, requested FPS 30, JPEG quality
85. Capture starts automatically unless `IRIS_IDLE=true` is set. The daemon runs
in the foreground, handles SIGTERM/Ctrl-C, and waits for capture cleanup on exit.
The unit restarts a failed daemon process, with rate limits; stopping just the
camera intentionally does not trigger a service restart.

## Preview and local control API

Preview remains available at:

- `http://192.168.50.150:8080/stream.mjpg`
- `http://192.168.50.150:8080/frame.jpg`
- `http://192.168.50.150:8080/status` (camera metadata)

The installed service listens for control on TCP port 8081. The installer
creates a random bearer token in `/etc/iris/control.token` (root/iris, mode 0640),
preserves it across upgrades, and sets `IRIS_CONTROL_TOKEN_FILE` in the environment
file. Remote peers must authenticate for status and commands. Local loopback
connections trust local OS users, so `iris status` still works on the Pi.
A manually started daemon defaults to loopback; non-loopback binds require a
valid token file before the listener is opened.

To provision a remote client, first copy the token to a private file accessible
to your Pi login. On the Pi:

```bash
sudo install -m 0600 -o "$USER" /etc/iris/control.token "$HOME/iris-control.token"
```

On the client machine:

```bash
mkdir -p ~/.config/iris
scp sbeskur@192.168.50.150:iris-control.token ~/.config/iris/token
chmod 600 ~/.config/iris/token
iris --address 192.168.50.150:8081 --token-file ~/.config/iris/token status
iris --address 192.168.50.150:8081 --token-file ~/.config/iris/token set --gain 1.5
```

For subsequent commands:

```bash
export IRIS_CONTROL_ADDRESS=192.168.50.150:8081
export IRIS_TOKEN_FILE="$HOME/.config/iris/token"
iris start
iris stop
```

The library exposes `IrisClient::new("http://192.168.50.150:8081")?.with_token(&token)?`.
The client also accepts `IRIS_TOKEN`; a private token file avoids command-line
secrets. Debug output redacts its authorization header.

This is HTTP/JSON over TCP; UDP is not implemented. Direct service HTTP is for
trusted LAN connections. For an untrusted network, carry it over SSH forwarding
or an authenticated HTTPS proxy. A browser application still needs its proxy/CORS
integration; the daemon does not enable general cross-origin browser access.

Existing environment files are preserved on upgrade. To enable remote commands
on an older loopback-only installation, set `IRIS_CONTROL_BIND=0.0.0.0:8081` and
`IRIS_CONTROL_TOKEN_FILE=/etc/iris/control.token`, then restart the service.

Control routes:

| Method | Path | Result |
| --- | --- | --- |
| GET | `/status` | Camera metadata, requested settings, capture worker state |
| POST | `/camera/start` | Start capture, return current state |
| POST | `/camera/stop` | Wait for camera cleanup, return stopped state |
| PATCH | `/settings` | Validate and queue a settings patch |

Mutations require `X-Iris-Control: 1` to prevent browser cross-origin simple
requests from controlling a local daemon. The client supplies it automatically.
For example:

```bash
curl -H 'X-Iris-Control: 1' -H 'Content-Type: application/json' \
  -X PATCH -d '{"gain":1.5}' http://127.0.0.1:8081/settings
```

Missing/wrong remote credentials return 401. Invalid values return 400; geometry changes during capture return 409. Missing
or malformed JSON is rejected by the API. Capture errors are reported as `Failed`;
preview responses reject stale/unhealthy frames with 503. Camera status includes
`Stopped`, `Starting`, `Running`, `Stopping`, `Degraded`, and `Failed`.

## Validation

Non-hardware tests:

```bash
cargo test --features http-support,jpeg-support,iris-client
```

On an idle Pi with both binaries in the provided directory:

```bash
python3 scripts/check-irisd-control.py /tmp/iris-install
python3 scripts/check-iris-restart.py /tmp/iris-install/irisd
# On a separate host, against an idle authenticated daemon:
python3 scripts/check-iris-remote.py target/debug/iris \
  --address 192.168.50.150:8081 --token-file ~/.config/iris/token
```

The control check starts an idle daemon, starts capture, changes exposure/gain
live, checks actual metadata and advancing frame count, rejects invalid/conflicting
settings, stops capture and checks 503, and starts capture again in the same
daemon process. The restart check tests full daemon shutdown/restart separately.
