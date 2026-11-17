use crate::frame_store::{FrameMetadata, FrameStore};
use crate::timing::center_of_integration_ns;
use anyhow::{bail, Result};
use std::sync::atomic::AtomicBool;
#[cfg(feature = "iris-service")]
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub use iris_api::CameraSettings as CameraConfig;

pub fn captured_at_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn metadata_from_values(
    frame_index: u64,
    sensor_ts_ns: i64,
    exposure_us: Option<i32>,
    analogue_gain: Option<f32>,
    width: u32,
    height: u32,
    frame_duration_us: Option<i64>,
) -> FrameMetadata {
    FrameMetadata {
        frame_index,
        sensor_ts_ns,
        exposure_us,
        analogue_gain,
        // No active-array readout duration is available from this capture path.
        // Do not substitute a frame period or silently change timestamp semantics.
        coi_ns: None,
        first_row_coi_ns: exposure_us
            .and_then(|exposure| center_of_integration_ns(sensor_ts_ns, exposure).ok()),
        frame_duration_us,
        width,
        height,
        pixel_format: "NV12".into(),
        captured_at_unix_ms: captured_at_unix_ms(),
    }
}

/// Convert worker errors and unwinding panics into an observable terminal state.
pub fn supervise_capture(store: Arc<FrameStore>, capture: impl FnOnce() -> Result<()>) {
    let failure = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(capture)) {
        Ok(Ok(())) => None,
        Ok(Err(err)) => Some(format!("{err:#}")),
        Err(payload) => Some(format!(
            "camera worker panicked: {}",
            payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic")
        )),
    };
    if let Some(error) = failure {
        tracing::error!(%error, "camera capture stopped");
        store.set_camera_state(crate::frame_store::CameraState::Failed);
        store.set_last_error(error);
    }
}

pub fn run_capture(config: CameraConfig, store: Arc<FrameStore>) -> Result<()> {
    run_capture_until_stopped(config, store, Arc::new(AtomicBool::new(false)))
}

pub fn run_capture_until_stopped(
    config: CameraConfig,
    store: Arc<FrameStore>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let settings = Arc::new(RwLock::new(config.clone()));
    run_capture_with_settings(config, store, stop, settings)
}

pub fn run_capture_with_settings(
    config: CameraConfig,
    store: Arc<FrameStore>,
    stop: Arc<AtomicBool>,
    settings: Arc<RwLock<CameraConfig>>,
) -> Result<()> {
    config.validate()?;
    #[cfg(feature = "iris-service")]
    {
        return run_capture_libcamera(config, store, stop, settings);
    }

    #[cfg(not(feature = "iris-service"))]
    {
        let _ = (config, store, stop, settings);
        bail!("iris-service feature is required for camera capture");
    }
}

#[cfg(feature = "iris-service")]
fn run_capture_libcamera(
    config: CameraConfig,
    store: Arc<FrameStore>,
    stop: Arc<AtomicBool>,
    settings: Arc<RwLock<CameraConfig>>,
) -> Result<()> {
    use crate::frame_store::CameraState;
    use anyhow::Context;
    use libcamera::{
        camera::CameraConfigurationStatus,
        camera_manager::CameraManager,
        color_space::{Range, YcbcrEncoding},
        controls,
        framebuffer::{AsFrameBuffer, FrameMetadataStatus},
        framebuffer_allocator::{FrameBuffer, FrameBufferAllocator},
        framebuffer_map::MemoryMappedFrameBuffer,
        geometry::Size,
        pixel_format::PixelFormat,
        request::{RequestStatus, ReuseFlag},
        stream::StreamRole,
    };
    use std::time::{Duration, Instant};
    use tracing::{debug, info, warn};

    const PIXEL_FORMAT_NV12: PixelFormat = PixelFormat::new(u32::from_le_bytes(*b"NV12"), 0);

    let mgr = CameraManager::new().context("Failed to create CameraManager")?;
    let cameras = mgr.cameras();
    if cameras.is_empty() {
        store.set_camera_state(CameraState::Failed);
        store.set_last_error("No cameras found. Try: /usr/local/bin/cam --list");
        bail!("No cameras found. Try: /usr/local/bin/cam --list");
    }

    let camera_list = camera_inventory_summary(
        cameras
            .iter()
            .enumerate()
            .map(|(index, camera)| (index, camera.id().to_string())),
    );
    info!(camera_count = cameras.len(), cameras = %camera_list, "Enumerated cameras");

    let cam_ref = cameras.get(config.camera_index).with_context(|| {
        format!(
            "camera index {} missing; available cameras: [{}]",
            config.camera_index, camera_list
        )
    })?;
    info!(
        camera_index = config.camera_index,
        camera_id = %cam_ref.id(),
        "Acquiring camera"
    );
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
        CameraConfigurationStatus::Invalid => bail!("Configuration is invalid"),
    }

    cam.configure(&mut cfg).context("configure() failed")?;

    let actual = cfg.get(0).context("no stream config after configure")?;
    let actual_size = actual.get_size();
    let actual_width = actual_size.width;
    let actual_height = actual_size.height;
    if actual.get_pixel_format() != PIXEL_FORMAT_NV12 {
        bail!("camera did not negotiate NV12");
    }
    let stride = actual.get_stride() as usize;
    let color_space = actual
        .get_color_space()
        .context("camera did not report a color space")?;
    let encoding = match color_space.ycbcr_encoding {
        YcbcrEncoding::Rec601 => crate::jpeg::YuvEncoding::Bt601,
        YcbcrEncoding::Rec709 => crate::jpeg::YuvEncoding::Bt709,
        _ => bail!("unsupported NV12 color space: {color_space:?}"),
    };
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
            let mut req = cam
                .create_request(Some(i as u64))
                .context("create_request failed")?;
            req.add_buffer(&stream, buf).context("add_buffer failed")?;
            Ok(req)
        })
        .collect::<Result<Vec<_>>>()?;

    if let Some(first) = requests.get_mut(0) {
        apply_camera_controls(first.controls_mut(), &config)?;
    }
    let mut applied_settings = config.clone();

    cam.start(None).context("camera start() failed")?;
    // Always stop streaming before dropping request buffers, including on errors.
    let capture_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<()> {
            for req in requests.drain(..) {
                cam.queue_request(req)
                    .map_err(|(_, e)| e)
                    .context("queue_request failed")?;
            }

            let mut frame_index = 0_u64;
            let started_at = Instant::now();
            while !stop.load(Ordering::Acquire) {
                let mut req = match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(req) => req,
                    Err(_) if stop.load(Ordering::Acquire) => break,
                    Err(err) => {
                        store.set_camera_state(CameraState::Degraded);
                        store.set_last_error(format!("Timed out waiting for frame: {err}"));
                        bail!("capture stopped while waiting for a frame: {err}");
                    }
                };

                if stop.load(Ordering::Acquire) {
                    break;
                }
                if req.status() != RequestStatus::Complete {
                    bail!("capture request was not completed: {:?}", req.status());
                }
                let meta = req.metadata();
                let sensor_ts_ns = meta
                    .get::<controls::SensorTimestamp>()
                    .map(|ts| *ts)
                    .unwrap_or(0);
                let exposure_us = meta
                    .get::<controls::ExposureTime>()
                    .map(|value| *value)
                    .ok();
                let analogue_gain = meta
                    .get::<controls::AnalogueGain>()
                    .map(|value| *value)
                    .ok();

                let framebuffer: &MemoryMappedFrameBuffer<FrameBuffer> = req
                    .buffer(&stream)
                    .ok_or_else(|| anyhow::anyhow!("completed request missing stream buffer"))?;
                let buffer_meta = framebuffer
                    .metadata()
                    .context("frame buffer metadata missing")?;
                // PiSP deliberately marks warmup buffers Startup while algorithms settle.
                // Recycle them without publishing; bound startup to avoid hiding a stalled sensor.
                if buffer_meta.status() == FrameMetadataStatus::Startup
                    && frame_index == 0
                    && started_at.elapsed() < Duration::from_secs(5)
                {
                    req.reuse(ReuseFlag::REUSE_BUFFERS);
                    cam.queue_request(req)
                        .map_err(|(_, err)| err)
                        .context("re-queue startup buffer failed")?;
                    continue;
                }
                if buffer_meta.status() != FrameMetadataStatus::Success {
                    bail!("frame buffer failed: {:?}", buffer_meta.status());
                }
                if sensor_ts_ns <= 0 {
                    bail!("capture request has no valid sensor timestamp; refusing to publish an invalid frame");
                }
                let mapped_planes = framebuffer.data();
                let used_planes = buffer_meta.planes();
                if mapped_planes.len() != used_planes.len() {
                    bail!("frame buffer plane count does not match metadata");
                }
                let planes = mapped_planes
                    .iter()
                    .zip(used_planes.into_iter())
                    .map(|(data, meta)| {
                        data.get(..meta.bytes_used as usize)
                            .context("frame buffer bytes_used exceeds allocation")
                    })
                    .collect::<Result<Vec<_>>>()?;
                let y_plane = planes
                    .first()
                    .context("completed frame has no luma plane")?;
                let contiguous_uv_offset = stride
                    .checked_mul(actual_height as usize)
                    .context("plane size overflow")?;
                let (y_plane, uv_plane) = if let Some(uv) = planes.get(1) {
                    (*y_plane, *uv)
                } else {
                    (
                        y_plane
                            .get(..contiguous_uv_offset)
                            .context("short NV12 luma plane")?,
                        y_plane
                            .get(contiguous_uv_offset..)
                            .context("missing NV12 chroma plane")?,
                    )
                };
                match crate::jpeg::encode_nv12_planes_to_jpeg(
                    crate::jpeg::Nv12Image {
                        width: actual_width,
                        height: actual_height,
                        y: y_plane,
                        uv: uv_plane,
                        y_stride: stride,
                        uv_stride: stride,
                        encoding,
                        full_range: color_space.range == Range::Full,
                    },
                    settings
                        .read()
                        .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
                        .jpeg_quality,
                ) {
                    Ok(jpeg) => {
                        let metadata = metadata_from_values(
                            frame_index,
                            sensor_ts_ns,
                            exposure_us,
                            analogue_gain,
                            actual_width,
                            actual_height,
                            meta.get::<controls::FrameDuration>()
                                .map(|value| *value)
                                .ok(),
                        );
                        store.update_frame(jpeg, metadata);
                        store.set_camera_state(CameraState::Running);
                        debug!(frame_index, "captured frame");
                        frame_index += 1;
                    }
                    Err(err) => {
                        store.set_camera_state(CameraState::Degraded);
                        store.set_last_error(format!("JPEG encode failed: {err:?}"));
                    }
                }

                req.reuse(ReuseFlag::REUSE_BUFFERS);
                let desired = settings
                    .read()
                    .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
                    .clone();
                if desired != applied_settings {
                    apply_camera_controls(req.controls_mut(), &desired)?;
                }
                cam.queue_request(req)
                    .map_err(|(_, e)| e)
                    .context("re-queue failed")?;
                applied_settings = desired;
            }
            Ok(())
        },
    ));
    let stop_result = cam.stop().context("camera stop() failed");
    match capture_result {
        Ok(result) => result.and(stop_result),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[cfg(feature = "iris-service")]
fn apply_camera_controls(
    controls: &mut libcamera::control::ControlList,
    config: &CameraConfig,
) -> Result<()> {
    use anyhow::Context;
    use libcamera::controls as c;
    controls
        .set(if config.exposure_us.is_some() {
            c::ExposureTimeMode::Manual
        } else {
            c::ExposureTimeMode::Auto
        })
        .context("set exposure mode failed")?;
    controls
        .set(if config.gain.is_some() {
            c::AnalogueGainMode::Manual
        } else {
            c::AnalogueGainMode::Auto
        })
        .context("set gain mode failed")?;
    if let Some(exposure) = config.exposure_us {
        controls
            .set(c::ExposureTime(exposure))
            .context("set exposure failed")?;
    }
    if let Some(gain) = config.gain {
        controls
            .set(c::AnalogueGain(gain))
            .context("set gain failed")?;
    }
    let duration = config.effective_frame_duration_us();
    controls
        .set(c::FrameDurationLimits([duration, duration]))
        .context("set frame duration failed")?;
    Ok(())
}

#[cfg(any(feature = "iris-service", test))]
fn camera_inventory_summary<I, S>(cameras: I) -> String
where
    I: IntoIterator<Item = (usize, S)>,
    S: Into<String>,
{
    cameras
        .into_iter()
        .map(|(index, camera_id)| format!("{index}: {}", camera_id.into()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_poc_defaults() {
        let config = CameraConfig::default();
        assert_eq!(config.camera_index, 0);
        assert_eq!(config.width, 1_280);
        assert_eq!(config.height, 720);
        assert_eq!(config.fps, 30);
        assert_eq!(config.jpeg_quality, 85);
    }

    #[test]
    fn frame_duration_uses_fps() {
        assert_eq!(CameraConfig::default().frame_duration_us(), 33_333);
    }

    #[test]
    fn metadata_does_not_invent_frame_center_timing() {
        let metadata = metadata_from_values(
            0,
            1_000_000_000,
            Some(59_000),
            Some(1.0),
            1_280,
            720,
            Some(61_000),
        );
        assert_eq!(metadata.coi_ns, None);
        assert_eq!(metadata.first_row_coi_ns, Some(1_029_500_000));
        assert_eq!(metadata.frame_duration_us, Some(61_000));
        let absent = metadata_from_values(0, 1, None, None, 1280, 720, None);
        assert_eq!(absent.first_row_coi_ns, None);
    }

    #[test]
    fn camera_inventory_summary_formats_indices_and_ids() {
        let summary = camera_inventory_summary([(0, "cam-a"), (1, "cam-b")]);
        assert_eq!(summary, "0: cam-a, 1: cam-b");
    }
    #[test]
    fn worker_panics_and_errors_are_reported_immediately() {
        for panic in [false, true] {
            let store = Arc::new(FrameStore::new(
                crate::frame_store::CameraState::Starting,
                1280,
                720,
                "NV12",
            ));
            let worker_store = Arc::clone(&store);
            std::thread::spawn(move || {
                supervise_capture(worker_store, || {
                    if panic {
                        panic!("injected failure");
                    }
                    bail!("injected failure")
                })
            })
            .join()
            .unwrap();
            assert_eq!(
                store.status().camera_state,
                crate::frame_store::CameraState::Failed
            );
            assert!(store
                .status()
                .last_error
                .unwrap()
                .contains("injected failure"));
            assert!(store.fresh_frame().is_none());
        }
    }
}
