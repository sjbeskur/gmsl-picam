/// Example: GStreamer appsink capture for frame processing.
///
/// Build:  cargo build --example gstreamer_capture --features gstreamer-example
/// Run:    GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0 \
///             ./target/debug/examples/gstreamer_capture --frames 30 --width 1280 --height 720
///
/// For exposure control from GStreamer you must set libcamera controls via the
/// libcamerasrc element properties *before* the pipeline moves to PLAYING.
/// True sensor timestamps are not recoverable from GStreamer buffers; if you need
/// center-of-integration precision, use the libcamera_capture example instead.
use anyhow::{Context, Result};
use gstreamer::{self as gst, prelude::*};
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_video::{self as gst_video, prelude::*};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tracing::{debug, info, warn};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "gmsl_picam_rs=debug".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let n_frames: usize = arg_value(&args, "--frames").unwrap_or(30);
    let width: u32 = arg_value(&args, "--width").unwrap_or(1280);
    let height: u32 = arg_value(&args, "--height").unwrap_or(720);
    // Exposure in microseconds; 0 = leave AE in auto.
    let exposure_us: u32 = arg_value(&args, "--exposure-us").unwrap_or(0);

    gst::init().context("GStreamer init failed")?;

    // --- Build pipeline ---
    // libcamerasrc can accept exposure-time-us and awb-mode as GObject properties.
    let src_props = if exposure_us > 0 {
        format!(
            "libcamerasrc exposure-time={exposure_us} ae-enable=false"
        )
    } else {
        "libcamerasrc".to_string()
    };

    let pipeline_str = format!(
        "{src_props} ! \
         video/x-raw,format=NV12,width={width},height={height},framerate=30/1 ! \
         videoconvert ! \
         video/x-raw,format=RGB ! \
         appsink name=sink max-buffers=4 drop=true"
    );

    info!("Pipeline: {}", pipeline_str);

    let pipeline = gst::parse::launch(&pipeline_str)
        .context("gst::parse::launch failed — is GST_PLUGIN_PATH set to /usr/local/lib/gstreamer-1.0?")?
        .dynamic_cast::<gst::Pipeline>()
        .unwrap();

    let sink = pipeline
        .by_name("sink")
        .context("appsink 'sink' not found in pipeline")?
        .dynamic_cast::<AppSink>()
        .unwrap();

    // --- Frame counter shared with callback ---
    let frame_count = Arc::new(AtomicUsize::new(0));
    let frame_count_cb = Arc::clone(&frame_count);
    let total = n_frames;

    sink.set_callbacks(
        AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = match sink.pull_sample() {
                    Ok(s) => s,
                    Err(_) => return Err(gst::FlowError::Eos),
                };

                let buf = sample.buffer().ok_or(gst::FlowError::Error)?;
                let caps = sample.caps().ok_or(gst::FlowError::Error)?;

                // Decode frame geometry from caps
                let info = gst_video::VideoInfo::from_caps(caps)
                    .map_err(|_| gst::FlowError::Error)?;
                let w = info.width();
                let h = info.height();

                // GStreamer pipeline-clock timestamp — NOT the sensor hardware timestamp.
                //
                // Limitations:
                //   1. Epoch is arbitrary (pipeline start), not wall time or IMU clock.
                //   2. Carries 1–5 ms of queuing jitter from videoconvert + appsink scheduling.
                //   3. No exposure midpoint: does not encode when photons were collected.
                //
                // If you need center-of-integration for IMU fusion, use the
                // libcamera_capture example instead — it exposes SensorTimestamp and
                // ExposureTime metadata directly from the completed request.
                let pts = buf.pts();

                let map = buf
                    .map_readable()
                    .map_err(|_| gst::FlowError::Error)?;

                let idx = frame_count_cb.fetch_add(1, Ordering::SeqCst);
                debug!(
                    frame = idx,
                    pts_ns = pts.map(|t| t.nseconds()).unwrap_or(0),
                    width = w,
                    height = h,
                    bytes = map.len(),
                    "Frame received (RGB24)"
                );

                // --- Your processing here ---
                //
                // `map.as_slice()` is a flat &[u8] of RGB24 pixels, row-major.
                // Pixel at (x, y): &map[y as usize * w as usize * 3 + x as usize * 3 ..]
                //
                // Example: compute mean luminance of centre 100×100 patch
                let mean = mean_luminance_patch(map.as_slice(), w, h, 100, 100);
                info!(frame = idx, mean_luminance = mean, "Centre patch luminance");

                if idx + 1 >= total {
                    Err(gst::FlowError::Eos)
                } else {
                    Ok(gst::FlowSuccess::Ok)
                }
            })
            .build(),
    );

    // --- Run ---
    pipeline
        .set_state(gst::State::Playing)
        .context("Failed to set pipeline to PLAYING")?;

    info!("Pipeline running — waiting for {} frames", n_frames);

    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;
        match msg.view() {
            MessageView::Eos(_) => {
                info!("EOS received");
                break;
            }
            MessageView::Error(err) => {
                let src = msg.src().map(|s| s.path_string()).unwrap_or_default();
                anyhow::bail!("GStreamer error from {src}: {}", err.error());
            }
            MessageView::Warning(w) => {
                warn!("GStreamer warning: {}", w.error());
            }
            _ => {}
        }
    }

    pipeline
        .set_state(gst::State::Null)
        .context("Failed to stop pipeline")?;

    info!(
        "Done — processed {} frames",
        frame_count.load(Ordering::SeqCst)
    );
    Ok(())
}

/// Mean luminance (R channel of RGB24) of a centred patch of `pw`×`ph` pixels.
fn mean_luminance_patch(data: &[u8], width: u32, height: u32, pw: u32, ph: u32) -> f32 {
    let x0 = (width.saturating_sub(pw) / 2) as usize;
    let y0 = (height.saturating_sub(ph) / 2) as usize;
    let x1 = (x0 + pw as usize).min(width as usize);
    let y1 = (y0 + ph as usize).min(height as usize);
    let stride = width as usize * 3;

    let mut sum = 0u64;
    let mut count = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += data[y * stride + x * 3] as u64; // R channel ≈ luminance proxy
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum as f32 / count as f32 }
}

fn arg_value<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .and_then(|w| w[1].parse().ok())
}
