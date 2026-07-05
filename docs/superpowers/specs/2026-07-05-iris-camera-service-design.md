# Iris Camera Service Design

Date: 2026-07-05

## Goal

Build `iris`, a small Rust camera service for the GMSL IMX477 Raspberry Pi setup.
The first version should prove that Rust can directly own libcamera capture and
serve usable images over the network.

The service must:

- Capture from the working single-camera baseline through direct `libcamera`.
- Expose a latest-frame snapshot endpoint.
- Expose a simple live MJPEG stream.
- Expose camera/frame metadata through a status endpoint.
- Cross-compile to `aarch64-unknown-linux-gnu` with the existing Docker build.

GStreamer remains useful for comparison and future streaming work, but it is not
the first `iris` backend.

## Architecture

`iris` will be a Rust binary in the existing `rust/` crate, separate from the
current examples.

It will use three layers:

1. `camera`
   - Owns `libcamera` initialization.
   - Acquires the configured camera.
   - Configures an NV12 video stream.
   - Allocates and maps buffers.
   - Runs the request completion/requeue loop.
   - Extracts frame metadata from completed requests.

2. `frame_store`
   - Stores the latest JPEG frame and metadata in memory.
   - Provides thread-safe reads for HTTP handlers.
   - Keeps camera ownership out of the HTTP layer.

3. `http`
   - Serves `/status`, `/frame.jpg`, and `/stream.mjpg`.
   - Reads only from `frame_store`.
   - Does not touch `libcamera` objects directly.

The capture loop is the only component that owns and mutates libcamera request
state. This keeps libcamera lifecycle handling single-threaded and avoids request
reuse bugs.

## Endpoints

Initial HTTP API:

```text
GET /status
GET /frame.jpg
GET /stream.mjpg
```

`/status` returns JSON with camera state, dimensions, frame count, last metadata,
and the last error if present.

`/frame.jpg` returns the latest JPEG frame. It returns `503 Service Unavailable`
until the first frame is captured.

`/stream.mjpg` returns a multipart MJPEG stream. Each part is the latest newly
captured JPEG. Client disconnects are normal and should not produce noisy errors.

## Capture Settings

The PoC defaults to:

```text
bind: 0.0.0.0:8080
camera-index: 0
width: 1280
height: 720
fps: 30
exposure: auto unless provided
gain: auto/default unless provided
pixel format: NV12
```

CLI shape:

```bash
iris \
  --bind 0.0.0.0:8080 \
  --width 1280 \
  --height 720 \
  --fps 30 \
  --exposure-us 8000 \
  --gain 1.0
```

Manual exposure and gain are optional. If exposure is provided, the service will
disable AE and set exposure/gain controls similarly to the current
`libcamera_capture` example.

## Data Flow

Startup:

```text
parse config
initialize tracing
acquire camera
configure NV12 stream
allocate and map buffers
start HTTP server
start capture loop
```

Capture loop:

```text
receive completed request
read metadata
read mapped frame bytes
convert/encode latest frame to JPEG
update frame_store
reuse and requeue request
```

For the PoC, JPEG encoding can run on the capture path. If this limits frame
rate, a later iteration can move encoding to a worker thread or store raw NV12
and encode on demand.

## Frame Metadata

Each stored frame should include:

```text
frame_index
sensor_ts_ns
exposure_us
analogue_gain
coi_ns
width
height
pixel_format
captured_at timestamp
```

The timing helpers must use libcamera's `SensorTimestamp` definition: the time
when the first row of the image sensor active array is exposed. A first-row CoI
is `sensor_ts_ns + exposure_ns / 2`; a frame-center CoI also adds half the
rolling readout duration. This work should preserve the existing
`center_of_integration_ns` symbol as a first-row compatibility helper and expose
the corrected frame-center value from `iris`.

## Error Handling

Startup failures:

- No camera: fail with a clear message that suggests `/usr/local/bin/cam --list`.
- Camera acquire/configure/start failure: fail startup.

Runtime behavior:

- If libcamera adjusts the requested stream config, continue and expose actual
  dimensions/format in `/status`.
- If frame capture times out, mark status degraded and keep the service alive
  if the capture loop can continue.
- If JPEG encoding fails, keep capture running and expose the last error in
  `/status`.
- HTTP client disconnects from `/stream.mjpg` are normal.

## Cross-Compilation And Artifacts

Cross-compilation is part of the PoC definition of done.

The project should use a root `justfile` as the canonical developer command
surface. Existing scripts can remain as implementation details while the workflow
is migrated.

Primary commands:

```bash
just build-libcamera
just build-rust
```

The Rust build recipe should produce `iris` for arm64 using the existing Docker
cross-build path.

Expected Rust artifacts:

```text
out/rust-arm64/iris
out/rust-arm64/libcamera_capture
out/rust-arm64/gstreamer_capture
```

Legacy script commands should continue to work during the transition:

```bash
./scripts/build-libcamera-arm64.sh
./scripts/build-rust-arm64.sh
```

Useful `just` recipes for this work:

```text
just build-libcamera
just build-rust
just deploy-iris host=192.168.50.24
just pi-camera-check host=192.168.50.24
```

Pi deployment:

```bash
just deploy-iris host=192.168.50.24
```

Local host builds should remain useful where practical, but the real verification
target is the Raspberry Pi arm64 environment because libcamera depends on the
installed camera stack and hardware.

## Testing And Verification

Automated checks:

- Unit tests for timestamp helpers, including invalid inputs.
- Unit tests for frame store behavior:
  - empty state
  - update
  - latest frame retrieval
  - status metadata
- Build check for the `iris` binary with the `libcamera` feature.
- Docker cross-build check that exports `iris`.

Manual Pi verification:

```bash
/usr/local/bin/cam --list
sudo /tmp/iris --bind 0.0.0.0:8080 --width 1280 --height 720
curl http://192.168.50.24:8080/status
curl -o frame.jpg http://192.168.50.24:8080/frame.jpg
```

Open `http://192.168.50.24:8080/stream.mjpg` from a browser or compatible client
to verify live MJPEG streaming.

## Out Of Scope For The First PoC

- Dual-camera support.
- GStreamer backend inside `iris`.
- Authentication/TLS.
- On-device recording.
- Runtime camera reconfiguration through HTTP.
- Advanced encoding controls.
- Browser UI beyond raw HTTP endpoints.
