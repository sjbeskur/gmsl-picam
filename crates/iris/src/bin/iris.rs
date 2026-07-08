use anyhow::{Context, Result};
use clap::Parser;
use gmsl_picam_rs::camera::{run_capture, CameraConfig};
use gmsl_picam_rs::frame_store::{CameraState, FrameStore};
use gmsl_picam_rs::http::router;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(
    name = "iris",
    about = "GMSL IMX477 camera service",
    long_about = "GMSL IMX477 camera service.\n\nSelect the active camera at startup with --camera-index. Run one iris process per camera if you want to switch between two physically connected cameras.\n\nThe --fps flag is currently advisory: it is reported in configuration and status, but exposure timing is driven by the working libcamera control path."
)]
struct Args {
    #[arg(long, default_value = "0.0.0.0:8080")]
    bind: SocketAddr,

    #[arg(
        long,
        default_value_t = 0,
        help = "Zero-based camera index from libcamera::CameraManager::cameras(). Use a separate iris process per camera."
    )]
    camera_index: usize,

    #[arg(long, default_value_t = 1_280)]
    width: u32,

    #[arg(long, default_value_t = 720)]
    height: u32,

    #[arg(
        long,
        default_value_t = 30,
        help = "Requested frame rate in frames per second. Advisory for now; exposure controls drive sensor timing."
    )]
    fps: u32,

    #[arg(
        long,
        help = "Manual exposure in microseconds. When set, iris disables AE and uses the requested exposure."
    )]
    exposure_us: Option<i32>,

    #[arg(
        long,
        help = "Manual analogue gain. When set, iris applies the requested gain alongside exposure control."
    )]
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
    let store = Arc::new(FrameStore::new(
        CameraState::Starting,
        args.width,
        args.height,
        "NV12",
    ));
    let camera_store = Arc::clone(&store);
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
        if let Err(err) = run_capture(camera_config, Arc::clone(&camera_store)) {
            error!(error = ?err, "camera capture stopped");
            camera_store.set_camera_state(CameraState::Failed);
            camera_store.set_last_error(format!("{err:#}"));
        }
    });

    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("failed to bind {}", args.bind))?;
    info!(
        camera_index = args.camera_index,
        bind = %args.bind,
        "iris listening"
    );
    axum::serve(listener, router(store))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("http server failed")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
