/// Example: direct libcamera capture with manual exposure and center-of-integration timing.
///
/// Build:  cargo build --example libcamera_capture --features libcamera-example
/// Run:    sudo ./target/debug/examples/libcamera_capture --frames 10 --exposure-us 8000
///
/// The binary must run as root (or a user in the `video` group with access to
/// /dev/media* and /dev/video*) so that libcamera can open the media controller.
use anyhow::{Context, Result};
use gmsl_picam_rs::center_of_integration_ns;
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
use std::time::Duration;
use tracing::{debug, info, warn};

// NV12 pixel format (4CC = 0x3231564E)
const PIXEL_FORMAT_NV12: PixelFormat = PixelFormat::new(u32::from_le_bytes(*b"NV12"), 0);

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "gmsl_picam_rs=debug,libcamera=info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let n_frames: usize = arg_value(&args, "--frames").unwrap_or(5);
    let exposure_us: i32 = arg_value(&args, "--exposure-us").unwrap_or(10_000);
    let gain: f32 = arg_value(&args, "--gain").unwrap_or(1.0);
    let width: u32 = arg_value(&args, "--width").unwrap_or(1280);
    let height: u32 = arg_value(&args, "--height").unwrap_or(720);

    info!(n_frames, exposure_us, gain, width, height, "Starting libcamera capture");

    let mgr = CameraManager::new().context("Failed to create CameraManager")?;
    let cameras = mgr.cameras();

    if cameras.is_empty() {
        anyhow::bail!(
            "No cameras found. Is the GMSL sensor enumerated? Try: /usr/local/bin/cam --list"
        );
    }

    info!("Found {} camera(s)", cameras.len());
    let cam = cameras.get(0).context("camera 0 missing")?;
    info!("Acquiring camera: {}", cam.id());
    let mut cam = cam.acquire().context("Failed to acquire camera")?;

    // --- Stream configuration ---
    let mut cfg = cam
        .generate_configuration(&[StreamRole::VideoRecording])
        .ok_or_else(|| anyhow::anyhow!("generate_configuration failed"))?;

    let mut stream_cfg = cfg.get_mut(0).context("No stream in configuration")?;
    stream_cfg.set_pixel_format(PIXEL_FORMAT_NV12);
    stream_cfg.set_size(Size { width, height });
    stream_cfg.set_buffer_count(4);

    match cfg.validate() {
        CameraConfigurationStatus::Valid => {}
        CameraConfigurationStatus::Adjusted => {
            warn!("Configuration was adjusted by libcamera — check actual format/size");
            if let Some(actual) = cfg.get(0) {
                warn!(
                    "Adjusted to {:?} {}x{}",
                    actual.get_pixel_format(),
                    actual.get_size().width,
                    actual.get_size().height
                );
            }
        }
        CameraConfigurationStatus::Invalid => {
            anyhow::bail!("Configuration is invalid");
        }
    }

    cam.configure(&mut cfg).context("configure() failed")?;

    // --- Allocate and memory-map frame buffers ---
    let mut alloc = FrameBufferAllocator::new(&cam);
    let stream = cfg
        .get(0)
        .context("no stream config")?
        .stream()
        .context("no stream handle")?;

    // alloc() allocates buffers AND returns them directly.
    // Wrapping each in MemoryMappedFrameBuffer gives us &[u8] access to pixel data
    // after capture without an extra mmap call in the hot path.
    let buffers = alloc
        .alloc(&stream)
        .context("FrameBufferAllocator::alloc failed")?
        .into_iter()
        .map(|fb| MemoryMappedFrameBuffer::new(fb).expect("mmap failed"))
        .collect::<Vec<_>>();

    info!("Allocated {} frame buffers", buffers.len());

    // --- Subscribe to completed requests via channel ---
    let rx = cam.subscribe_request_completed();

    // Build one request per buffer and set manual exposure on the first one.
    // Controls applied to a request persist to all subsequent requests until changed.
    let mut requests = buffers
        .into_iter()
        .enumerate()
        .map(|(i, buf)| {
            let mut req = cam.create_request(Some(i as u64)).expect("create_request failed");
            req.add_buffer(&stream, buf).expect("add_buffer failed");
            req
        })
        .collect::<Vec<_>>();

    // Disable AE and apply manual exposure on request 0.
    {
        let ctrls = requests[0].controls_mut();
        ctrls.set(controls::AeEnable(false)).ok();
        ctrls.set(controls::ExposureTime(exposure_us)).ok();
        ctrls.set(controls::AnalogueGain(gain)).ok();
        // Clamp frame duration so the sensor doesn't stretch the frame period.
        let min_frame_us = exposure_us as i64 + 2_000;
        ctrls
            .set(controls::FrameDurationLimits([min_frame_us, min_frame_us]))
            .ok();
    }

    cam.start(None).context("camera start() failed")?;

    for req in requests.drain(..) {
        cam.queue_request(req).map_err(|(_, e)| e).context("queue_request failed")?;
    }

    // --- Capture loop ---
    let mut frame_idx = 0usize;
    while frame_idx < n_frames {
        let mut req = rx
            .recv_timeout(Duration::from_secs(2))
            .context("Timed out waiting for frame")?;

        let meta = req.metadata();

        let sensor_ts_ns: i64 = meta
            .get::<controls::SensorTimestamp>()
            .map(|ts| *ts)
            .unwrap_or(0);
        let actual_exposure_us: i32 = meta
            .get::<controls::ExposureTime>()
            .map(|e| *e)
            .unwrap_or(exposure_us);
        let actual_gain: f32 = meta
            .get::<controls::AnalogueGain>()
            .map(|g| *g)
            .unwrap_or(gain);

        let coi_ns = center_of_integration_ns(sensor_ts_ns, actual_exposure_us);

        info!(
            frame = frame_idx,
            sensor_ts_ns,
            coi_ns,
            actual_exposure_us,
            actual_gain,
            "Frame received"
        );

        // Access raw pixel data from the memory-mapped Y plane (NV12 plane 0).
        let framebuffer: &MemoryMappedFrameBuffer<FrameBuffer> = req.buffer(&stream).unwrap();
        let planes = framebuffer.data();
        if let Some(y_plane) = planes.first() {
            debug!(frame = frame_idx, bytes = y_plane.len(), "Y-plane size");
            // process_frame(y_plane, width, height);
        }

        frame_idx += 1;

        if frame_idx < n_frames {
            req.reuse(ReuseFlag::REUSE_BUFFERS);
            cam.queue_request(req).map_err(|(_, e)| e).context("re-queue failed")?;
        }
    }

    cam.stop().context("camera stop() failed")?;
    info!("Done — captured {} frames", n_frames);
    Ok(())
}

fn arg_value<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .and_then(|w| w[1].parse().ok())
}
