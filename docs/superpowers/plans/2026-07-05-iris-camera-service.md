# Iris Camera Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `iris`, a direct-libcamera Rust camera service that serves `/status`, `/frame.jpg`, and `/stream.mjpg`, and cross-compiles to arm64 through `just build-rust`.

**Architecture:** Add focused Rust modules for timing, frame storage, JPEG encoding, HTTP serving, and libcamera capture. The capture loop is the only owner of libcamera request state; HTTP handlers read the latest encoded frame and metadata from a shared frame store. Add a root `justfile` as the canonical workflow while keeping existing shell scripts working.

**Tech Stack:** Rust 2021, `libcamera` crate, `tokio`, `axum`, `clap`, `serde`, `image`, `tracing`, Docker BuildKit, `just`.

---

## File Structure

- Create `justfile`: canonical build/deploy/check commands.
- Modify `scripts/build-rust-arm64.sh`: keep compatibility, but update output messaging for `iris`.
- Modify `docker/rust-arm64.Dockerfile`: build and export `iris` alongside examples.
- Modify `rust/Cargo.toml`: add `iris` binary, `iris-service` feature, and dependencies.
- Modify `rust/src/lib.rs`: expose focused modules and keep public timing helpers.
- Create `rust/src/timing.rs`: center-of-integration helpers with validation and tests.
- Create `rust/src/frame_store.rs`: latest-frame store, metadata/status types, tests.
- Create `rust/src/jpeg.rs`: NV12-to-JPEG encoding and tests.
- Create `rust/src/http.rs`: HTTP router and endpoint handlers with tests that do not require camera hardware.
- Create `rust/src/camera.rs`: direct libcamera capture loop and config types.
- Create `rust/src/bin/iris.rs`: CLI, tracing, startup orchestration.
- Update `docs/superpowers/specs/2026-07-05-iris-camera-service-design.md`: only if implementation reveals a necessary spec correction.

Before editing, check the existing dirty Cargo files. If `rust/Cargo.toml` and `rust/Cargo.lock` contain unrelated dependency churn, either get user approval to keep it or restore those specific dependency-version changes before starting.

---

### Task 1: Add Root Just Workflow

**Files:**
- Create: `justfile`
- Modify: `scripts/build-rust-arm64.sh`

- [ ] **Step 1: Write the failing workflow check**

Run:

```bash
just --list
```

Expected: FAIL with an error like `No justfile found`.

- [ ] **Step 2: Add the root `justfile`**

Create `justfile`:

```make
set shell := ["bash", "-uc"]

default:
    @just --list

build-libcamera output_dir="out/libcamera-arm64":
    ./scripts/build-libcamera-arm64.sh "{{output_dir}}"

build-rust output_dir="out/rust-arm64":
    ./scripts/build-rust-arm64.sh "{{output_dir}}"

build-iris: build-rust

deploy-iris host="192.168.50.24" user="sbeskur" path="/tmp/iris":
    test -x out/rust-arm64/iris
    scp out/rust-arm64/iris "{{user}}@{{host}}:{{path}}"

pi-camera-check host="192.168.50.24" user="sbeskur":
    ssh "{{user}}@{{host}}" '/usr/local/bin/cam --list && media-ctl -d /dev/media0 -p 2>/dev/null | sed -n "1,120p"'

package-kernel kernel_release kernel_tree_tar output_dir="":
    if [[ -n "{{output_dir}}" ]]; then \
      ./scripts/build-kernel-pack.sh --kernel-release "{{kernel_release}}" --kernel-tree-tar "{{kernel_tree_tar}}" --output-dir "{{output_dir}}"; \
    else \
      ./scripts/build-kernel-pack.sh --kernel-release "{{kernel_release}}" --kernel-tree-tar "{{kernel_tree_tar}}"; \
    fi
```

- [ ] **Step 3: Update Rust build script artifact messaging**

In `scripts/build-rust-arm64.sh`, change:

```bash
# Binaries produced: libcamera_capture  gstreamer_capture
```

to:

```bash
# Binaries produced: iris  libcamera_capture  gstreamer_capture
```

and change the deployment section to include:

```bash
echo "  scp ${output_dir}/iris sbeskur@192.168.50.24:/tmp/"
```

and the run section to include:

```bash
echo "  sudo /tmp/iris --bind 0.0.0.0:8080 --width 1280 --height 720"
```

- [ ] **Step 4: Verify Just lists recipes**

Run:

```bash
just --list
```

Expected: PASS and lists `build-libcamera`, `build-rust`, `build-iris`, `deploy-iris`, `pi-camera-check`, and `package-kernel`.

- [ ] **Step 5: Commit**

```bash
git add justfile scripts/build-rust-arm64.sh
git commit -m "build: add just workflow"
```

---

### Task 2: Add Iris Crate Structure And Timing Tests

**Files:**
- Modify: `rust/Cargo.toml`
- Modify: `rust/src/lib.rs`
- Create: `rust/src/timing.rs`

- [ ] **Step 1: Add dependency and binary declarations**

In `rust/Cargo.toml`, add these dependencies:

```toml
axum = { version = "0.7", optional = true }
clap = { version = "4", features = ["derive"], optional = true }
image = { version = "0.25", default-features = false, features = ["jpeg"], optional = true }
serde = { version = "1", features = ["derive"], optional = true }
serde_json = { version = "1", optional = true }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "time"], optional = true }
tokio-stream = { version = "0.1", features = ["sync"], optional = true }
```

Add the feature and binary:

```toml
iris-service = [
    "dep:axum",
    "dep:clap",
    "dep:image",
    "dep:libcamera",
    "dep:serde",
    "dep:serde_json",
    "dep:tokio",
    "dep:tokio-stream",
]

[[bin]]
name = "iris"
path = "src/bin/iris.rs"
required-features = ["iris-service"]
```

- [ ] **Step 2: Write timing tests first**

Create `rust/src/timing.rs` with tests and stubs:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingError {
    InvalidFrameHeight,
    InvalidRow,
    ExposureLongerThanFrame,
}

pub fn center_of_integration_ns(sensor_ts_ns: i64, exposure_us: i32) -> i64 {
    sensor_ts_ns + (exposure_us as i64 * 1_000 / 2)
}

pub fn center_of_integration_frame_center_ns(
    _sensor_ts_ns: i64,
    _exposure_us: i32,
    _frame_duration_us: i64,
) -> Result<i64, TimingError> {
    unimplemented!("implemented after failing tests")
}

pub fn center_of_integration_row_ns(
    _sensor_ts_ns: i64,
    _exposure_us: i32,
    _frame_duration_us: i64,
    _row: u32,
    _frame_height: u32,
) -> Result<i64, TimingError> {
    unimplemented!("implemented after failing tests")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_center_adds_half_exposure_and_half_readout() {
        let coi = center_of_integration_frame_center_ns(1_000_000_000, 8_000, 33_333)
            .expect("valid timing");
        assert_eq!(coi, 1_016_666_500);
    }

    #[test]
    fn row_zero_is_first_row_center_of_integration() {
        let coi = center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 0, 720)
            .expect("valid timing");
        assert_eq!(coi, 1_004_000_000);
    }

    #[test]
    fn bottom_row_is_later_than_top_row() {
        let top = center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 0, 720)
            .expect("valid timing");
        let bottom = center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 719, 720)
            .expect("valid timing");
        assert!(bottom > top);
    }

    #[test]
    fn rejects_zero_frame_height() {
        assert_eq!(
            center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 0, 0),
            Err(TimingError::InvalidFrameHeight)
        );
    }

    #[test]
    fn rejects_row_at_frame_height() {
        assert_eq!(
            center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 720, 720),
            Err(TimingError::InvalidRow)
        );
    }

    #[test]
    fn rejects_exposure_longer_than_frame() {
        assert_eq!(
            center_of_integration_frame_center_ns(1_000_000_000, 40_000, 33_333),
            Err(TimingError::ExposureLongerThanFrame)
        );
    }

    #[test]
    fn compatibility_helper_returns_first_row_center_of_integration() {
        assert_eq!(
            center_of_integration_ns(1_000_000_000, 8_000),
            1_004_000_000
        );
    }
}
```

- [ ] **Step 3: Wire module exports**

Replace `rust/src/lib.rs` with:

```rust
pub mod timing;

pub use timing::{
    center_of_integration_frame_center_ns, center_of_integration_ns,
    center_of_integration_row_ns, TimingError,
};
```

- [ ] **Step 4: Run tests to verify failure**

Run:

```bash
cargo test --no-default-features timing
```

Expected: FAIL because the two `unimplemented!()` functions panic.

- [ ] **Step 5: Implement timing helpers**

Replace the two `unimplemented!()` functions in `rust/src/timing.rs`:

```rust
pub fn center_of_integration_frame_center_ns(
    sensor_ts_ns: i64,
    exposure_us: i32,
    frame_duration_us: i64,
) -> Result<i64, TimingError> {
    let exposure_ns = exposure_us as i64 * 1_000;
    let frame_duration_ns = frame_duration_us * 1_000;
    if exposure_ns > frame_duration_ns {
        return Err(TimingError::ExposureLongerThanFrame);
    }
    let readout_ns = frame_duration_ns - exposure_ns;
    Ok(sensor_ts_ns + exposure_ns / 2 + readout_ns / 2)
}

pub fn center_of_integration_row_ns(
    sensor_ts_ns: i64,
    exposure_us: i32,
    frame_duration_us: i64,
    row: u32,
    frame_height: u32,
) -> Result<i64, TimingError> {
    if frame_height == 0 {
        return Err(TimingError::InvalidFrameHeight);
    }
    if row >= frame_height {
        return Err(TimingError::InvalidRow);
    }
    let exposure_ns = exposure_us as i64 * 1_000;
    let frame_duration_ns = frame_duration_us * 1_000;
    if exposure_ns > frame_duration_ns {
        return Err(TimingError::ExposureLongerThanFrame);
    }
    let readout_ns = frame_duration_ns - exposure_ns;
    let row_offset_ns = row as i64 * readout_ns / frame_height as i64;
    Ok(sensor_ts_ns + exposure_ns / 2 + row_offset_ns)
}
```

- [ ] **Step 6: Run tests to verify pass**

Run:

```bash
cargo test --no-default-features timing
```

Expected: PASS, all timing tests pass.

- [ ] **Step 7: Commit**

```bash
git add rust/Cargo.toml rust/Cargo.lock rust/src/lib.rs rust/src/timing.rs
git commit -m "feat: add iris timing helpers"
```

---

### Task 3: Add Frame Store

**Files:**
- Modify: `rust/src/lib.rs`
- Create: `rust/src/frame_store.rs`

- [ ] **Step 1: Write frame store tests and stub**

Create `rust/src/frame_store.rs`:

```rust
use serde::Serialize;
use std::sync::{Arc, RwLock};
use tokio::sync::watch;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum CameraState {
    Starting,
    Capturing,
    Degraded,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FrameMetadata {
    pub frame_index: u64,
    pub sensor_ts_ns: Option<i64>,
    pub exposure_us: Option<i32>,
    pub analogue_gain: Option<f32>,
    pub coi_ns: Option<i64>,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub captured_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EncodedFrame {
    pub jpeg: Arc<Vec<u8>>,
    pub metadata: FrameMetadata,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CameraStatus {
    pub state: CameraState,
    pub frame_count: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub pixel_format: Option<String>,
    pub last_frame: Option<FrameMetadata>,
    pub last_error: Option<String>,
}

#[derive(Debug)]
struct FrameStoreInner {
    status: CameraStatus,
    latest: Option<EncodedFrame>,
}

#[derive(Clone, Debug)]
pub struct FrameStore {
    inner: Arc<RwLock<FrameStoreInner>>,
    notifier: watch::Sender<u64>,
}

impl FrameStore {
    pub fn new() -> Self {
        unimplemented!("implemented after failing tests")
    }

    pub fn status(&self) -> CameraStatus {
        unimplemented!("implemented after failing tests")
    }

    pub fn latest(&self) -> Option<EncodedFrame> {
        unimplemented!("implemented after failing tests")
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.notifier.subscribe()
    }

    pub fn update_frame(&self, jpeg: Vec<u8>, metadata: FrameMetadata) {
        let _ = (jpeg, metadata);
        unimplemented!("implemented after failing tests")
    }

    pub fn set_error(&self, message: impl Into<String>) {
        let _ = message;
        unimplemented!("implemented after failing tests")
    }
}

impl Default for FrameStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(index: u64) -> FrameMetadata {
        FrameMetadata {
            frame_index: index,
            sensor_ts_ns: Some(1_000),
            exposure_us: Some(8_000),
            analogue_gain: Some(1.0),
            coi_ns: Some(900),
            width: 1280,
            height: 720,
            pixel_format: "NV12".to_string(),
            captured_at_unix_ms: 42,
        }
    }

    #[test]
    fn new_store_has_starting_status_and_no_frame() {
        let store = FrameStore::new();
        assert_eq!(store.latest(), None);
        assert_eq!(store.status().state, CameraState::Starting);
        assert_eq!(store.status().frame_count, 0);
    }

    #[test]
    fn update_frame_sets_latest_and_status() {
        let store = FrameStore::new();
        store.update_frame(vec![1, 2, 3], metadata(7));
        let latest = store.latest().expect("latest frame");
        assert_eq!(&**latest.jpeg, &[1, 2, 3]);
        assert_eq!(latest.metadata.frame_index, 7);
        let status = store.status();
        assert_eq!(status.state, CameraState::Capturing);
        assert_eq!(status.frame_count, 8);
        assert_eq!(status.width, Some(1280));
        assert_eq!(status.height, Some(720));
    }

    #[test]
    fn set_error_marks_error_but_keeps_latest_frame() {
        let store = FrameStore::new();
        store.update_frame(vec![1], metadata(0));
        store.set_error("encode failed");
        assert!(store.latest().is_some());
        let status = store.status();
        assert_eq!(status.state, CameraState::Error);
        assert_eq!(status.last_error, Some("encode failed".to_string()));
    }
}
```

- [ ] **Step 2: Export frame store module**

Add to `rust/src/lib.rs`:

```rust
#[cfg(feature = "iris-service")]
pub mod frame_store;
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```bash
cargo test --features iris-service frame_store
```

Expected: FAIL because `FrameStore` methods are unimplemented.

- [ ] **Step 4: Implement `FrameStore`**

Replace `FrameStore` methods in `rust/src/frame_store.rs`:

```rust
impl FrameStore {
    pub fn new() -> Self {
        let status = CameraStatus {
            state: CameraState::Starting,
            frame_count: 0,
            width: None,
            height: None,
            pixel_format: None,
            last_frame: None,
            last_error: None,
        };
        let (notifier, _) = watch::channel(0);
        Self {
            inner: Arc::new(RwLock::new(FrameStoreInner {
                status,
                latest: None,
            })),
            notifier,
        }
    }

    pub fn status(&self) -> CameraStatus {
        self.inner.read().expect("frame store poisoned").status.clone()
    }

    pub fn latest(&self) -> Option<EncodedFrame> {
        self.inner.read().expect("frame store poisoned").latest.clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.notifier.subscribe()
    }

    pub fn update_frame(&self, jpeg: Vec<u8>, metadata: FrameMetadata) {
        let frame_index = metadata.frame_index;
        let mut inner = self.inner.write().expect("frame store poisoned");
        inner.status = CameraStatus {
            state: CameraState::Capturing,
            frame_count: frame_index + 1,
            width: Some(metadata.width),
            height: Some(metadata.height),
            pixel_format: Some(metadata.pixel_format.clone()),
            last_frame: Some(metadata.clone()),
            last_error: None,
        };
        inner.latest = Some(EncodedFrame {
            jpeg: Arc::new(jpeg),
            metadata,
        });
        drop(inner);
        let _ = self.notifier.send(frame_index + 1);
    }

    pub fn set_error(&self, message: impl Into<String>) {
        let mut inner = self.inner.write().expect("frame store poisoned");
        inner.status.state = CameraState::Error;
        inner.status.last_error = Some(message.into());
    }
}
```

- [ ] **Step 5: Run tests to verify pass**

Run:

```bash
cargo test --features iris-service frame_store
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rust/src/lib.rs rust/src/frame_store.rs rust/Cargo.toml rust/Cargo.lock
git commit -m "feat: add iris frame store"
```

---

### Task 4: Add NV12 JPEG Encoding

**Files:**
- Modify: `rust/src/lib.rs`
- Create: `rust/src/jpeg.rs`

- [ ] **Step 1: Write JPEG tests and stub**

Create `rust/src/jpeg.rs`:

```rust
use image::codecs::jpeg::JpegEncoder;
use image::ColorType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JpegError {
    InvalidDimensions,
    BufferTooSmall { expected_at_least: usize, actual: usize },
    Encode(String),
}

pub fn encode_nv12_to_jpeg(
    _nv12: &[u8],
    _width: u32,
    _height: u32,
    _quality: u8,
) -> Result<Vec<u8>, JpegError> {
    unimplemented!("implemented after failing tests")
}

fn yuv_to_rgb(_y: u8, _u: u8, _v: u8) -> [u8; 3] {
    unimplemented!("implemented after failing tests")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_small_gray_nv12_image_as_jpeg() {
        let nv12 = vec![
            128, 128, 128, 128,
            128, 128,
        ];
        let jpeg = encode_nv12_to_jpeg(&nv12, 2, 2, 80).expect("jpeg");
        assert_eq!(&jpeg[0..2], &[0xff, 0xd8]);
        assert_eq!(&jpeg[jpeg.len() - 2..], &[0xff, 0xd9]);
    }

    #[test]
    fn rejects_odd_dimensions() {
        assert_eq!(
            encode_nv12_to_jpeg(&[], 3, 2, 80),
            Err(JpegError::InvalidDimensions)
        );
    }

    #[test]
    fn rejects_short_buffer() {
        assert_eq!(
            encode_nv12_to_jpeg(&[0; 5], 2, 2, 80),
            Err(JpegError::BufferTooSmall {
                expected_at_least: 6,
                actual: 5
            })
        );
    }
}
```

- [ ] **Step 2: Export JPEG module**

Add to `rust/src/lib.rs`:

```rust
#[cfg(feature = "iris-service")]
pub mod jpeg;
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```bash
cargo test --features iris-service jpeg
```

Expected: FAIL because JPEG functions are unimplemented.

- [ ] **Step 4: Implement NV12 conversion and JPEG encoding**

Replace functions in `rust/src/jpeg.rs`:

```rust
pub fn encode_nv12_to_jpeg(
    nv12: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> Result<Vec<u8>, JpegError> {
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
        return Err(JpegError::InvalidDimensions);
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let y_len = width_usize * height_usize;
    let expected = y_len + y_len / 2;
    if nv12.len() < expected {
        return Err(JpegError::BufferTooSmall {
            expected_at_least: expected,
            actual: nv12.len(),
        });
    }

    let mut rgb = vec![0_u8; width_usize * height_usize * 3];
    let uv = &nv12[y_len..expected];
    for y in 0..height_usize {
        for x in 0..width_usize {
            let y_value = nv12[y * width_usize + x];
            let uv_index = (y / 2) * width_usize + (x / 2) * 2;
            let u = uv[uv_index];
            let v = uv[uv_index + 1];
            let rgb_pixel = yuv_to_rgb(y_value, u, v);
            let out = (y * width_usize + x) * 3;
            rgb[out..out + 3].copy_from_slice(&rgb_pixel);
        }
    }

    let mut out = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder
        .encode(&rgb, width, height, ColorType::Rgb8.into())
        .map_err(|err| JpegError::Encode(err.to_string()))?;
    Ok(out)
}

fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = y as i32 - 16;
    let d = u as i32 - 128;
    let e = v as i32 - 128;
    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;
    [clamp_u8(r), clamp_u8(g), clamp_u8(b)]
}

fn clamp_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}
```

- [ ] **Step 5: Run tests to verify pass**

Run:

```bash
cargo test --features iris-service jpeg
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rust/src/lib.rs rust/src/jpeg.rs rust/Cargo.toml rust/Cargo.lock
git commit -m "feat: add iris jpeg encoder"
```

---

### Task 5: Add HTTP Endpoints With Hardware-Free Tests

**Files:**
- Modify: `rust/src/lib.rs`
- Create: `rust/src/http.rs`

- [ ] **Step 1: Write HTTP tests and router skeleton**

Create `rust/src/http.rs`:

```rust
use crate::frame_store::{FrameMetadata, FrameStore};
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

pub fn router(store: FrameStore) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/frame.jpg", get(frame_jpg))
        .route("/stream.mjpg", get(stream_mjpg))
        .with_state(store)
}

async fn status(State(_store): State<FrameStore>) -> impl IntoResponse {
    unimplemented!("implemented after failing tests")
}

async fn frame_jpg(State(_store): State<FrameStore>) -> Response {
    unimplemented!("implemented after failing tests")
}

async fn stream_mjpg(State(_store): State<FrameStore>) -> Response {
    unimplemented!("implemented after failing tests")
}

fn multipart_part(jpeg: &[u8], metadata: &FrameMetadata) -> Vec<u8> {
    let mut part = Vec::new();
    part.extend_from_slice(b"--iris\r\n");
    part.extend_from_slice(b"Content-Type: image/jpeg\r\n");
    part.extend_from_slice(format!("X-Frame-Index: {}\r\n", metadata.frame_index).as_bytes());
    part.extend_from_slice(format!("Content-Length: {}\r\n\r\n", jpeg.len()).as_bytes());
    part.extend_from_slice(jpeg);
    part.extend_from_slice(b"\r\n");
    part
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_store::FrameMetadata;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    fn metadata() -> FrameMetadata {
        FrameMetadata {
            frame_index: 0,
            sensor_ts_ns: Some(1),
            exposure_us: Some(8_000),
            analogue_gain: Some(1.0),
            coi_ns: Some(1),
            width: 2,
            height: 2,
            pixel_format: "NV12".to_string(),
            captured_at_unix_ms: 42,
        }
    }

    #[tokio::test]
    async fn status_returns_json() {
        let app = router(FrameStore::new());
        let response = app
            .oneshot(Request::builder().uri("/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn frame_returns_503_before_first_frame() {
        let app = router(FrameStore::new());
        let response = app
            .oneshot(Request::builder().uri("/frame.jpg").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn frame_returns_jpeg_after_update() {
        let store = FrameStore::new();
        store.update_frame(vec![0xff, 0xd8, 0xff, 0xd9], metadata());
        let app = router(store);
        let response = app
            .oneshot(Request::builder().uri("/frame.jpg").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("image/jpeg")
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], &[0xff, 0xd8, 0xff, 0xd9]);
    }

    #[test]
    fn multipart_part_contains_frame_headers() {
        let part = multipart_part(&[1, 2, 3], &metadata());
        let text = String::from_utf8_lossy(&part);
        assert!(text.contains("Content-Type: image/jpeg"));
        assert!(text.contains("X-Frame-Index: 0"));
        assert!(text.contains("Content-Length: 3"));
    }
}
```

Add test dependency to `rust/Cargo.toml`:

```toml
[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
```

- [ ] **Step 2: Export HTTP module**

Add to `rust/src/lib.rs`:

```rust
#[cfg(feature = "iris-service")]
pub mod http;
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```bash
cargo test --features iris-service http
```

Expected: FAIL because endpoint handlers are unimplemented.

- [ ] **Step 4: Implement handlers**

Replace handlers in `rust/src/http.rs`:

```rust
async fn status(State(store): State<FrameStore>) -> impl IntoResponse {
    Json(store.status())
}

async fn frame_jpg(State(store): State<FrameStore>) -> Response {
    let Some(frame) = store.latest() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"))],
        (*frame.jpeg).clone(),
    )
        .into_response()
}

async fn stream_mjpg(State(store): State<FrameStore>) -> Response {
    let receiver = store.subscribe();
    let stream_store = store.clone();
    let stream = tokio_stream::wrappers::WatchStream::new(receiver).filter_map(move |_| {
        let store = stream_store.clone();
        async move {
            let frame = store.latest()?;
            Some(Ok::<Vec<u8>, std::convert::Infallible>(multipart_part(
                &frame.jpeg,
                &frame.metadata,
            )))
        }
    });
    let body = Body::from_stream(stream);
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/x-mixed-replace; boundary=iris"),
        )],
        body,
    )
        .into_response()
}
```

Add imports:

```rust
use tokio_stream::StreamExt;
```

- [ ] **Step 5: Run tests to verify pass**

Run:

```bash
cargo test --features iris-service http
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rust/src/lib.rs rust/src/http.rs rust/Cargo.toml rust/Cargo.lock
git commit -m "feat: add iris http endpoints"
```

---

### Task 6: Add Libcamera Capture Module

**Files:**
- Modify: `rust/src/lib.rs`
- Create: `rust/src/camera.rs`

- [ ] **Step 1: Add camera module with testable config**

Create `rust/src/camera.rs`:

```rust
use crate::frame_store::{FrameMetadata, FrameStore};
use crate::jpeg::encode_nv12_to_jpeg;
use crate::timing::center_of_integration_frame_center_ns;
use anyhow::{Context, Result};
use libcamera::{
    camera::CameraConfigurationStatus,
    camera_manager::CameraManager,
    controls,
    framebuffer::AsFrameBuffer,
    framebuffer_allocator::{FrameBuffer, FrameBufferAllocator},
    framebuffer_map::MemoryMappedFrameBuffer,
    geometry::Size,
    pixel_format::PixelFormat,
    request::ReuseFlag,
    stream::StreamRole,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

const PIXEL_FORMAT_NV12: PixelFormat = PixelFormat::new(u32::from_le_bytes(*b"NV12"), 0);

#[derive(Debug, Clone, PartialEq)]
pub struct CameraConfig {
    pub camera_index: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub exposure_us: Option<i32>,
    pub gain: Option<f32>,
    pub jpeg_quality: u8,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            camera_index: 0,
            width: 1280,
            height: 720,
            fps: 30,
            exposure_us: None,
            gain: None,
            jpeg_quality: 85,
        }
    }
}

impl CameraConfig {
    pub fn frame_duration_us(&self) -> i64 {
        1_000_000_i64 / self.fps.max(1) as i64
    }
}

pub fn captured_at_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn metadata_from_values(
    frame_index: u64,
    sensor_ts_ns: Option<i64>,
    exposure_us: Option<i32>,
    analogue_gain: Option<f32>,
    width: u32,
    height: u32,
    frame_duration_us: i64,
) -> FrameMetadata {
    let coi_ns = sensor_ts_ns
        .zip(exposure_us)
        .and_then(|(ts, exposure)| center_of_integration_frame_center_ns(ts, exposure, frame_duration_us).ok());
    FrameMetadata {
        frame_index,
        sensor_ts_ns,
        exposure_us,
        analogue_gain,
        coi_ns,
        width,
        height,
        pixel_format: "NV12".to_string(),
        captured_at_unix_ms: captured_at_unix_ms(),
    }
}

pub fn run_capture(config: CameraConfig, store: FrameStore) -> Result<()> {
    unimplemented!("implemented after compile-only structure is in place")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_poc_defaults() {
        let config = CameraConfig::default();
        assert_eq!(config.camera_index, 0);
        assert_eq!(config.width, 1280);
        assert_eq!(config.height, 720);
        assert_eq!(config.fps, 30);
    }

    #[test]
    fn frame_duration_uses_fps() {
        assert_eq!(CameraConfig::default().frame_duration_us(), 33_333);
    }

    #[test]
    fn metadata_computes_coi_when_values_exist() {
        let metadata = metadata_from_values(0, Some(1_000_000_000), Some(8_000), Some(1.0), 1280, 720, 33_333);
        assert_eq!(metadata.coi_ns, Some(1_016_666_500));
    }
}
```

- [ ] **Step 2: Export camera module**

Add to `rust/src/lib.rs`:

```rust
#[cfg(feature = "iris-service")]
pub mod camera;
```

- [ ] **Step 3: Run tests**

Run:

```bash
cargo test --features iris-service camera
```

Expected: PASS because tests do not call `run_capture`.

- [ ] **Step 4: Implement `run_capture` from the existing example**

Replace `run_capture` in `rust/src/camera.rs`:

```rust
pub fn run_capture(config: CameraConfig, store: FrameStore) -> Result<()> {
    let mgr = CameraManager::new().context("Failed to create CameraManager")?;
    let cameras = mgr.cameras();
    if cameras.is_empty() {
        anyhow::bail!("No cameras found. Try: /usr/local/bin/cam --list");
    }
    let cam_ref = cameras
        .get(config.camera_index)
        .with_context(|| format!("camera index {} missing", config.camera_index))?;
    info!("Acquiring camera: {}", cam_ref.id());
    let mut cam = cam_ref.acquire().context("Failed to acquire camera")?;

    let mut cfg = cam
        .generate_configuration(&[StreamRole::VideoRecording])
        .ok_or_else(|| anyhow::anyhow!("generate_configuration failed"))?;
    let mut stream_cfg = cfg.get_mut(0).context("No stream in configuration")?;
    stream_cfg.set_pixel_format(PIXEL_FORMAT_NV12);
    stream_cfg.set_size(Size {
        width: config.width,
        height: config.height,
    });
    stream_cfg.set_buffer_count(4);

    match cfg.validate() {
        CameraConfigurationStatus::Valid => {}
        CameraConfigurationStatus::Adjusted => {
            warn!("Configuration was adjusted by libcamera");
        }
        CameraConfigurationStatus::Invalid => anyhow::bail!("Configuration is invalid"),
    }

    cam.configure(&mut cfg).context("configure() failed")?;
    let actual = cfg.get(0).context("no stream config after configure")?;
    let actual_size = actual.get_size();
    let actual_width = actual_size.width;
    let actual_height = actual_size.height;
    let stream = actual.stream().context("no stream handle")?;

    let mut alloc = FrameBufferAllocator::new(&cam);
    let buffers = alloc
        .alloc(&stream)
        .context("FrameBufferAllocator::alloc failed")?
        .into_iter()
        .map(|fb| MemoryMappedFrameBuffer::new(fb).context("mmap failed"))
        .collect::<Result<Vec<_>>>()?;

    let rx = cam.subscribe_request_completed();
    let mut requests = buffers
        .into_iter()
        .enumerate()
        .map(|(i, buf)| {
            let mut req = cam.create_request(Some(i as u64)).expect("create_request failed");
            req.add_buffer(&stream, buf).expect("add_buffer failed");
            req
        })
        .collect::<Vec<_>>();

    if let Some(first) = requests.get_mut(0) {
        let ctrls = first.controls_mut();
        if let Some(exposure_us) = config.exposure_us {
            ctrls.set(controls::AeEnable(false)).ok();
            ctrls.set(controls::ExposureTime(exposure_us)).ok();
            let min_frame_us = exposure_us as i64 + 2_000;
            ctrls
                .set(controls::FrameDurationLimits([min_frame_us, min_frame_us]))
                .ok();
        }
        if let Some(gain) = config.gain {
            ctrls.set(controls::AnalogueGain(gain)).ok();
        }
    }

    cam.start(None).context("camera start() failed")?;
    for req in requests.drain(..) {
        cam.queue_request(req).map_err(|(_, e)| e).context("queue_request failed")?;
    }

    let mut frame_index = 0_u64;
    loop {
        let mut req = match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(req) => req,
            Err(err) => {
                store.set_error(format!("Timed out waiting for frame: {err}"));
                continue;
            }
        };

        let meta = req.metadata();
        let sensor_ts_ns = meta.get::<controls::SensorTimestamp>().map(|ts| *ts);
        let exposure_us = meta
            .get::<controls::ExposureTime>()
            .map(|e| *e)
            .or(config.exposure_us);
        let analogue_gain = meta
            .get::<controls::AnalogueGain>()
            .map(|g| *g)
            .or(config.gain);

        let framebuffer: &MemoryMappedFrameBuffer<FrameBuffer> = req
            .buffer(&stream)
            .ok_or_else(|| anyhow::anyhow!("completed request missing stream buffer"))?;
        let planes = framebuffer.data();
        let Some(y_plane) = planes.first() else {
            store.set_error("completed frame had no plane data");
            req.reuse(ReuseFlag::REUSE_BUFFERS);
            cam.queue_request(req).map_err(|(_, e)| e).context("re-queue failed")?;
            continue;
        };

        match encode_nv12_to_jpeg(y_plane, actual_width, actual_height, config.jpeg_quality) {
            Ok(jpeg) => {
                let metadata = metadata_from_values(
                    frame_index,
                    sensor_ts_ns,
                    exposure_us,
                    analogue_gain,
                    actual_width,
                    actual_height,
                    config.frame_duration_us(),
                );
                store.update_frame(jpeg, metadata);
                debug!(frame_index, "captured frame");
                frame_index += 1;
            }
            Err(err) => store.set_error(format!("JPEG encode failed: {err:?}")),
        }

        req.reuse(ReuseFlag::REUSE_BUFFERS);
        cam.queue_request(req).map_err(|(_, e)| e).context("re-queue failed")?;
    }
}
```

- [ ] **Step 5: Run compile check**

Run:

```bash
cargo check --features iris-service
```

Expected: PASS on a system with `libcamera.pc` available. On the dev host without libcamera, this may fail with `Package 'libcamera' not found`; if so, record the gap and rely on Docker/Pi verification in Task 8.

- [ ] **Step 6: Commit**

```bash
git add rust/src/lib.rs rust/src/camera.rs rust/Cargo.toml rust/Cargo.lock
git commit -m "feat: add iris libcamera capture"
```

---

### Task 7: Add Iris Binary

**Files:**
- Create: `rust/src/bin/iris.rs`

- [ ] **Step 1: Add CLI and orchestration**

Create `rust/src/bin/iris.rs`:

```rust
use anyhow::{Context, Result};
use clap::Parser;
use gmsl_picam_rs::camera::{run_capture, CameraConfig};
use gmsl_picam_rs::frame_store::FrameStore;
use gmsl_picam_rs::http::router;
use std::net::SocketAddr;
use std::thread;
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(name = "iris", about = "GMSL IMX477 camera service")]
struct Args {
    #[arg(long, default_value = "0.0.0.0:8080")]
    bind: SocketAddr,

    #[arg(long, default_value_t = 0)]
    camera_index: usize,

    #[arg(long, default_value_t = 1280)]
    width: u32,

    #[arg(long, default_value_t = 720)]
    height: u32,

    #[arg(long, default_value_t = 30)]
    fps: u32,

    #[arg(long)]
    exposure_us: Option<i32>,

    #[arg(long)]
    gain: Option<f32>,

    #[arg(long, default_value_t = 85)]
    jpeg_quality: u8,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "iris=info,gmsl_picam_rs=info,libcamera=info".to_string()),
        )
        .init();

    let args = Args::parse();
    let store = FrameStore::new();
    let camera_store = store.clone();
    let camera_config = CameraConfig {
        camera_index: args.camera_index,
        width: args.width,
        height: args.height,
        fps: args.fps,
        exposure_us: args.exposure_us,
        gain: args.gain,
        jpeg_quality: args.jpeg_quality,
    };

    thread::spawn(move || {
        if let Err(err) = run_capture(camera_config, camera_store.clone()) {
            error!(error = ?err, "camera capture stopped");
            camera_store.set_error(format!("{err:#}"));
        }
    });

    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("failed to bind {}", args.bind))?;
    info!("iris listening on http://{}", args.bind);
    axum::serve(listener, router(store))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("http server failed")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
```

- [ ] **Step 2: Run help command**

Run:

```bash
cargo run --features iris-service --bin iris -- --help
```

Expected: prints CLI help. On a system without `libcamera.pc`, this may fail at build time; if so, defer this exact check to the Pi or Docker cross-build.

- [ ] **Step 3: Run non-hardware tests**

Run:

```bash
cargo test --features iris-service frame_store jpeg http timing camera
```

Expected: PASS where libcamera development files are available; otherwise fail only at libcamera dependency discovery.

- [ ] **Step 4: Commit**

```bash
git add rust/src/bin/iris.rs rust/Cargo.toml rust/Cargo.lock
git commit -m "feat: add iris service binary"
```

---

### Task 8: Cross-Build Iris And Export Artifact

**Files:**
- Modify: `docker/rust-arm64.Dockerfile`
- Modify: `scripts/build-rust-arm64.sh`

- [ ] **Step 1: Update Dockerfile to build `iris`**

In `docker/rust-arm64.Dockerfile`, add this build before the examples or after them:

```dockerfile
    cargo build \
        --bin iris \
        --release \
        --target aarch64-unknown-linux-gnu \
        --features iris-service && \
```

Then add this copy line before the existing example copies:

```dockerfile
    cp target/aarch64-unknown-linux-gnu/release/iris /tmp/ && \
```

Add export line in the artifact stage:

```dockerfile
COPY --from=build /tmp/iris  /
```

- [ ] **Step 2: Verify `build-rust-arm64.sh` mentions `iris`**

Confirm `scripts/build-rust-arm64.sh` includes:

```bash
echo "  scp ${output_dir}/iris sbeskur@192.168.50.24:/tmp/"
echo "  sudo /tmp/iris --bind 0.0.0.0:8080 --width 1280 --height 720"
```

- [ ] **Step 3: Run cross-build**

Run:

```bash
just build-rust
```

Expected: PASS and `out/rust-arm64/iris` exists. If `out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz` is missing, run:

```bash
just build-libcamera
just build-rust
```

- [ ] **Step 4: Verify exported binary**

Run:

```bash
file out/rust-arm64/iris
```

Expected: output contains `ELF 64-bit LSB pie executable, ARM aarch64`.

- [ ] **Step 5: Commit**

```bash
git add docker/rust-arm64.Dockerfile scripts/build-rust-arm64.sh
git commit -m "build: export iris arm64 binary"
```

---

### Task 9: Pi Verification And Docs

**Files:**
- Modify: `docs/gmsl-camera-operations.md`
- Modify: `rust/README.md`

- [ ] **Step 1: Add `iris` operations docs**

Append this section to `docs/gmsl-camera-operations.md`:

````markdown
## Iris Camera Service

Build and deploy:

```bash
just build-libcamera
just build-rust
just deploy-iris host=192.168.50.24
```

Run on the Pi:

```bash
sudo /tmp/iris --bind 0.0.0.0:8080 --width 1280 --height 720
```

Check endpoints:

```bash
curl http://192.168.50.24:8080/status
curl -o frame.jpg http://192.168.50.24:8080/frame.jpg
```

Open `http://192.168.50.24:8080/stream.mjpg` in a browser or compatible MJPEG
client for live preview.
````

- [ ] **Step 2: Add Rust README service note**

Add to `rust/README.md`:

````markdown
### `iris` - camera service

`iris` is the direct-libcamera service binary. It captures from the first
enumerated IMX477 camera and serves:

- `GET /status`
- `GET /frame.jpg`
- `GET /stream.mjpg`

Cross-build from the repository root:

```bash
just build-rust
```
````

- [ ] **Step 3: Deploy to Pi**

Run:

```bash
just deploy-iris host=192.168.50.24
```

Expected: `out/rust-arm64/iris` copied to `/tmp/iris`.

- [ ] **Step 4: Run Pi camera baseline check**

Run:

```bash
just pi-camera-check host=192.168.50.24
```

Expected: one `imx477` camera listed and media graph includes `imx477 10-001a`.

- [ ] **Step 5: Run `iris` on Pi**

Run on the Pi:

```bash
sudo /tmp/iris --bind 0.0.0.0:8080 --width 1280 --height 720
```

Expected: logs show `iris listening on http://0.0.0.0:8080` and captured frames.

- [ ] **Step 6: Check service from host**

Run from the dev machine:

```bash
curl http://192.168.50.24:8080/status
curl -f -o /tmp/iris-frame.jpg http://192.168.50.24:8080/frame.jpg
file /tmp/iris-frame.jpg
```

Expected:

- `/status` JSON includes `"state":"Capturing"` after first frame.
- `curl -f` exits 0.
- `file` reports JPEG image data.

- [ ] **Step 7: Commit docs**

```bash
git add docs/gmsl-camera-operations.md rust/README.md
git commit -m "docs: add iris service usage"
```

---

## Self-Review Notes

- Spec coverage:
  - Direct libcamera capture: Task 6.
  - `/status`, `/frame.jpg`, `/stream.mjpg`: Task 5.
  - Metadata and corrected `coi_ns`: Tasks 2, 3, and 6.
  - Cross-compile and `just` workflow: Tasks 1 and 8.
  - Manual Pi verification: Task 9.
- Marker scan: no incomplete markers or unspecified implementation steps remain.
- Type consistency: `FrameStore`, `FrameMetadata`, `CameraConfig`, and endpoint names are introduced before use and reused consistently.
