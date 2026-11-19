# gmsl-picam-rs

Rust examples for capturing and processing frames from the GMSL IMX477 camera
on a Raspberry Pi 5 running Ubuntu 24.04 with the custom libcamera build from
this repo.

---

## Examples

### `libcamera_capture` — direct libcamera, manual exposure, center-of-integration

Uses the `libcamera` Rust crate to talk to the camera without GStreamer.
Gives access to raw `SensorTimestamp` metadata, enabling precise
center-of-integration (CoI) timestamps for IMU synchronization.

```bash
cargo build --manifest-path crates/iris/Cargo.toml --example libcamera_capture --features libcamera-example

sudo ./target/debug/examples/libcamera_capture \
    --frames 10 \
    --exposure-us 8000 \
    --gain 1.5 \
    --width 1280 \
    --height 720
```

**Timing:** `SensorTimestamp` is the start of exposure of the first active
sensor row, in nanoseconds on `CLOCK_BOOTTIME`. The first-row midpoint is
`sensor_timestamp_ns + exposure_us * 500`.

A frame-center or row-specific midpoint additionally needs the active sensor
readout duration from the sensor mode and line timing. The frame period includes
blanking and is not a substitute. The checked timing helpers accept this active
readout duration; row indices refer to the active sensor array, before resizing.

---

### `gstreamer_capture` — GStreamer appsink, RGB24 frame processing

Uses `libcamerasrc` via GStreamer and pulls frames into Rust through `appsink`.
Frames are converted to RGB24 before delivery. Simpler setup, but GStreamer
pipeline-clock timestamps are less precise than `SensorTimestamp` — use
`libcamera_capture` when CoI matters.

```bash
cargo build --manifest-path crates/iris/Cargo.toml --example gstreamer_capture --features gstreamer-example

GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0 \
    ./target/debug/examples/gstreamer_capture \
        --frames 30 \
        --width 1280 \
        --height 720 \
        --exposure-us 8000   # 0 = leave auto-exposure enabled
```

### `irisd` daemon, `iris` client, and `iris-view` gateway

Install the native CLI into `~/.local/bin` with `just install-iris`.
This requires neither sudo nor camera libraries on the workstation.

`irisd` captures the selected IMX477, accepts UDP commands, and can transmit image
postcards over TCP or a Unix socket through the `Transmitter` trait. The optional
`iris-view` gateway serves `/status`, `/frame.jpg`, and `/stream.mjpg` over HTTP.
`iris` communicates directly with the daemon over UDP:

```bash
iris config front --endpoint udp://192.168.50.150:8081
iris camera front status
iris camera front start
iris camera front set --exposure-us 59000 --gain 1.5
iris camera front stop
```

Build and install the systemd bundle using the
[daemon installation guide](../../docs/irisd-service.md). Exposure and gain can
change while capture is running. Camera selection and dimensions require stopped
capture. The validated manual baseline is 59000 us exposure and gain 1.5.

For a manual development session:

```bash
cargo build --bin irisd --features iris-service
cargo build --bin iris --bin iris-view --features iris-client,iris-view
# Start these in separate terminals:
./target/debug/iris-view
./target/debug/irisd --transmit tcp://127.0.0.1:9001 --exposure-us 59000 --gain 1.5
# In another terminal:
./target/debug/iris status
```

---

## Dependencies

### JPEG encoding and performance benchmark

The [NV12 and buffer-copy explanation](<../../docs/journey of a photon.md#what-nv12-looks-like>)
includes the memory layout, implementation snippets, and measured Pi results.

The daemon statically builds libjpeg-turbo with SIMD support. Source builds need
CMake and a C compiler; x86 builds also need NASM. The cross-build Docker image
includes these tools, and the Pi does not need an extra runtime JPEG package.

```bash
sudo apt-get install -y build-essential cmake nasm
cargo run --release --example jpeg_benchmark --features jpeg-support -- 50
```

Run this benchmark on the Pi to compare the previous floating-point/image-rs
pipeline against the reusable fixed-point/TurboJPEG encoder at 1280×720 and
quality 85. Its synthetic image is deliberately textured, so timings differ from
live camera scenes. It does not open the camera. To measure a running daemon's
CPU, publication rate, and estimated sensor-to-publication age on the Pi:

```bash
python3 scripts/measure-iris-performance.py --pid "$(systemctl show -p MainPID --value irisd)"
```

### libcamera example

The `libcamera` crate links against the system `libcamera.so`. The build from
this repo installs it to `/usr/local/lib`. Make sure `PKG_CONFIG_PATH` includes
that prefix:

```bash
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH
```

### GStreamer example

Requires the GStreamer development headers and `gstreamer-app-1.0`:

```bash
sudo apt-get install -y \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-good
```

At runtime, `GST_PLUGIN_PATH` must point to the custom `libcamerasrc.so` plugin
that ships with the libcamera build from this repo:

```bash
export GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0
```

---

## Choosing between the two

| | `libcamera_capture` | `gstreamer_capture` |
|---|---|---|
| CoI / sensor timestamps | ✓ precise | ✗ pipeline clock only |
| Exposure control | `controls::ExposureTime` | `libcamerasrc` GObject property |
| Raw Bayer access | ✓ (request raw format) | needs extra element |
| Encoding / streaming | manual | plug in `h264enc`, `tcpserversink` |
| Complexity | higher | lower |

---

## GStreamer timestamp limitations

GStreamer buffer `PTS` values are relative to the **GStreamer pipeline clock**,
not the sensor hardware clock. This creates several problems for precision timing:

1. **Unknown epoch.** The pipeline clock starts at an arbitrary point when the
   pipeline moves to `PLAYING`. There is no fixed relationship to wall time or to
   the clock used by other sensors (e.g. an IMU).

2. **Queuing jitter.** By the time a buffer reaches `appsink` it has passed
   through at least one queue, a `videoconvert`, and the GStreamer scheduling
   loop. The jitter is typically 1–5 ms and is non-deterministic under load.

3. **No exposure midpoint.** GStreamer has no standard metadata field for
   `SensorTimestamp` or `ExposureTime`. The buffer timestamp does not encode
   *when* the photons were collected — only (approximately) when the frame
   arrived at that point in the pipeline.

For IMU fusion or any application where the camera timestamp must align with an
external clock to within a few hundred microseconds, use `libcamera_capture`
and compute CoI from `controls::SensorTimestamp` and `controls::ExposureTime`.

---

## Service reliability and metadata

The `iris-view` routes `/frame.jpg` and new `/stream.mjpg` connections return HTTP 503 when capture
is unhealthy or the newest frame is over two seconds old. Existing MJPEG streams
close on failure or expiry and send each frame at most once. Image responses use
`Cache-Control: no-store`. `/status` includes `frame_age_ms` and `frame_stale`;
freshness uses a monotonic clock independent of wall-clock corrections.

Exposure, analogue gain, and frame duration are reported from actual request
metadata. Missing values are JSON `null`. `first_row_coi_ns` is the first-row
exposure midpoint when valid exposure metadata is present. `coi_ns` is currently
`null` because this capture path does not provide active-array readout timing.
`captured_at_unix_ms` records host publication time, not sensor exposure time.

Stop the irisd daemon with Ctrl-C or SIGTERM and wait for it to exit before restarting.
Shutdown stops libcamera and joins the capture worker. Worker errors or panics
set the service status to `Failed`; restarting remains an explicit operation.

Build and deploy from the same artifact directory:

```bash
export IRIS_ARTIFACT_DIR=out/iris-arm64
just build
just deploy-iris
```

Alternatively, pass `just artifact_dir=out/iris-arm64 build` and the same override
to `deploy-iris`. The old positional `just build <directory>` is replaced by this
shared variable. Cross-builds export `iris`, `irisd`, `iris-view`, their service files, and the examples.

On an idle Pi with the irisd binary, run the hardware restart check:

```bash
python3 scripts/check-iris-restart.py /tmp/irisd --exposure-us 59000 --gain 1.0
```

The check starts two capture sessions in sequence, requires valid advancing
sensor timestamps in each, and checks SIGTERM exit before restarting.
