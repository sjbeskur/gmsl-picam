use anyhow::{Context, Result};
use clap::Parser;
use gmsl_picam_rs::camera::{run_capture_until_stopped, supervise_capture, CameraConfig};
use gmsl_picam_rs::frame_store::{CameraState, FrameStore};
use gmsl_picam_rs::http::router;
use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tracing::info;

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

    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("failed to bind {}", args.bind))?;
    let stop = Arc::new(AtomicBool::new(false));
    let camera_stop = Arc::clone(&stop);
    let camera_thread = thread::spawn(move || {
        supervise_capture(Arc::clone(&camera_store), || {
            run_capture_until_stopped(camera_config, camera_store, camera_stop)
        });
    });

    info!(
        camera_index = args.camera_index,
        bind = %args.bind,
        "iris listening"
    );
    // Drop HTTP streams on shutdown so an open MJPEG client cannot prevent
    // the camera thread from stopping and releasing its buffers.
    let server_result = tokio::select! {
        result = axum::serve(listener, router(store)).into_future() => {
            result.context("http server failed")
        }
        result = shutdown_signal() => result,
    };
    stop.store(true, Ordering::Release);
    tokio::task::spawn_blocking(move || camera_thread.join())
        .await
        .context("failed to join camera task")?
        .map_err(|_| anyhow::anyhow!("camera thread panicked"))?;
    server_result
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("failed to register SIGTERM handler")?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("Ctrl-C handler failed")?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .context("Ctrl-C handler failed")?;
    Ok(())
}
