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
cargo build --example libcamera_capture --features libcamera-example

sudo ./target/debug/examples/libcamera_capture \
    --frames 10 \
    --exposure-us 8000 \
    --gain 1.5 \
    --width 1280 \
    --height 720
```

**Center-of-integration math:**

```
coi_ns = sensor_timestamp_ns - (exposure_us * 1000 / 2)
```

`sensor_timestamp_ns` is the V4L2 timestamp stamped at end of frame readout.
Subtracting half the exposure converts it to the photon-collection midpoint.

For rolling-shutter row-level precision add a per-row offset:

```
row_offset_ns = row * (frame_duration_ns - exposure_ns) / frame_height
coi_for_row   = coi_ns - row_offset_ns
```

---

### `gstreamer_capture` — GStreamer appsink, RGB24 frame processing

Uses `libcamerasrc` via GStreamer and pulls frames into Rust through `appsink`.
Frames are converted to RGB24 before delivery. Simpler setup, but GStreamer
pipeline-clock timestamps are less precise than `SensorTimestamp` — use
`libcamera_capture` when CoI matters.

```bash
cargo build --example gstreamer_capture --features gstreamer-example

GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0 \
    ./target/debug/examples/gstreamer_capture \
        --frames 30 \
        --width 1280 \
        --height 720 \
        --exposure-us 8000   # 0 = leave auto-exposure enabled
```

---

## Dependencies

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

## Rolling-shutter center-of-integration correction

The IMX477 is a rolling-shutter sensor. Each row starts and ends its exposure at
a slightly different absolute time. The per-row correction is:

```
frame_duration_ns  = 1_000_000_000 / fps           (e.g. 33_333_333 ns at 30 fps)
exposure_ns        = exposure_us * 1_000
readout_ns         = frame_duration_ns - exposure_ns

row_start_offset_ns = row * readout_ns / frame_height
coi_for_row_ns      = sensor_timestamp_ns
                      - exposure_ns / 2             # mid-exposure
                      - readout_ns                  # sensor_ts is end-of-last-row
                      + row_start_offset_ns         # advance to this row
```

At 1280×720/30fps with 8 ms exposure:
- `readout_ns` ≈ 25.3 ms  
- Row offset range ≈ 0 → 25.3 ms across the full frame height  
- Top-row CoI differs from bottom-row CoI by ~25 ms

For most machine-vision tasks (detecting object position, not absolute time),
a single frame-centre timestamp is sufficient:

```
coi_frame_centre_ns = sensor_timestamp_ns - exposure_ns / 2 - readout_ns / 2
```

Only implement the per-row form if you are fusing with a sensor whose rate
(e.g. 1 kHz IMU) is fast enough to resolve the ~25 ms row-spread.
