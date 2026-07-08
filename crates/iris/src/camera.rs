use crate::frame_store::{FrameMetadata, FrameStore};
use crate::timing::center_of_integration_frame_center_ns;
use anyhow::{bail, Result};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
            width: 1_280,
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
        let fps = self.fps.max(1) as i64;
        1_000_000 / fps
    }
}

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
    exposure_us: i32,
    analogue_gain: f32,
    width: u32,
    height: u32,
    frame_duration_us: i64,
) -> FrameMetadata {
    let coi_ns =
        center_of_integration_frame_center_ns(sensor_ts_ns, exposure_us, frame_duration_us)
            .unwrap_or_else(|_| sensor_ts_ns + (exposure_us as i64 * 1_000 / 2));

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

pub fn run_capture(config: CameraConfig, store: Arc<FrameStore>) -> Result<()> {
    #[cfg(feature = "iris-service")]
    {
        return run_capture_libcamera(config, store);
    }

    #[cfg(not(feature = "iris-service"))]
    {
        let _ = (config, store);
        bail!("iris-service feature is required for camera capture");
    }
}

#[cfg(feature = "iris-service")]
fn run_capture_libcamera(config: CameraConfig, store: Arc<FrameStore>) -> Result<()> {
    use crate::frame_store::CameraState;
    use anyhow::Context;
    use libcamera::{
        camera::CameraConfigurationStatus,
        camera_manager::CameraManager,
        controls,
        framebuffer_allocator::{FrameBuffer, FrameBufferAllocator},
        framebuffer_map::MemoryMappedFrameBuffer,
        geometry::Size,
        pixel_format::PixelFormat,
        request::ReuseFlag,
        stream::StreamRole,
    };
    use std::time::Duration;
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
                .expect("create_request failed");
            req.add_buffer(&stream, buf).expect("add_buffer failed");
            req
        })
        .collect::<Vec<_>>();

    if let Some(first) = requests.get_mut(0) {
        let ctrls = first.controls_mut();

        if let Some(exposure_us) = config.exposure_us {
            ctrls.set(controls::ExposureTimeMode::Manual).ok();
            ctrls.set(controls::AnalogueGainMode::Manual).ok();
            ctrls.set(controls::ExposureTime(exposure_us)).ok();
            let min_frame_us = exposure_us as i64 + 2_000;
            ctrls
                .set(controls::FrameDurationLimits([min_frame_us, min_frame_us]))
                .ok();
        }
        if let Some(gain) = config.gain {
            ctrls.set(controls::AnalogueGainMode::Manual).ok();
            ctrls.set(controls::AnalogueGain(gain)).ok();
        }
    }

    cam.start(None).context("camera start() failed")?;
    for req in requests.drain(..) {
        cam.queue_request(req)
            .map_err(|(_, e)| e)
            .context("queue_request failed")?;
    }

    store.set_camera_state(CameraState::Running);

    let mut frame_index = 0_u64;
    loop {
        let mut req = match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(req) => req,
            Err(err) => {
                store.set_camera_state(CameraState::Degraded);
                store.set_last_error(format!("Timed out waiting for frame: {err}"));
                continue;
            }
        };

        let meta = req.metadata();
        let sensor_ts_ns = meta
            .get::<controls::SensorTimestamp>()
            .map(|ts| *ts)
            .unwrap_or(0);
        let exposure_us = meta
            .get::<controls::ExposureTime>()
            .map(|value| *value)
            .ok()
            .or(config.exposure_us)
            .unwrap_or(0);
        let analogue_gain = meta
            .get::<controls::AnalogueGain>()
            .map(|value| *value)
            .ok()
            .or(config.gain)
            .unwrap_or(1.0);

        let framebuffer: &MemoryMappedFrameBuffer<FrameBuffer> = req
            .buffer(&stream)
            .ok_or_else(|| anyhow::anyhow!("completed request missing stream buffer"))?;
        let planes = framebuffer.data();
        let Some(y_plane) = planes.first() else {
            store.set_camera_state(CameraState::Degraded);
            store.set_last_error("completed frame had no plane data");
            req.reuse(ReuseFlag::REUSE_BUFFERS);
            cam.queue_request(req)
                .map_err(|(_, e)| e)
                .context("re-queue failed")?;
            continue;
        };
        let Some(uv_plane) = planes.get(1) else {
            store.set_camera_state(CameraState::Degraded);
            store.set_last_error("completed frame was missing NV12 chroma data");
            req.reuse(ReuseFlag::REUSE_BUFFERS);
            cam.queue_request(req)
                .map_err(|(_, e)| e)
                .context("re-queue failed")?;
            continue;
        };

        let mut nv12 = Vec::with_capacity(y_plane.len() + uv_plane.len());
        nv12.extend_from_slice(y_plane);
        nv12.extend_from_slice(uv_plane);

        match crate::jpeg::encode_nv12_to_jpeg(
            actual_width,
            actual_height,
            &nv12,
            config.jpeg_quality,
        ) {
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
        cam.queue_request(req)
            .map_err(|(_, e)| e)
            .context("re-queue failed")?;
    }
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
    fn metadata_computes_coi_from_frame_center() {
        let metadata = metadata_from_values(0, 1_000_000_000, 8_000, 1.0, 1_280, 720, 33_333);
        assert_eq!(metadata.coi_ns, 1_020_666_500);
    }

    #[test]
    fn camera_inventory_summary_formats_indices_and_ids() {
        let summary = camera_inventory_summary([(0, "cam-a"), (1, "cam-b")]);
        assert_eq!(summary, "0: cam-a, 1: cam-b");
    }
}
