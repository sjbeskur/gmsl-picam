use anyhow::{Context, Result};
use clap::Parser;
use gmsl_picam_rs::{
    camera::CameraConfig,
    control::{self, CameraService},
    http,
};
use std::{future::IntoFuture, net::SocketAddr, sync::Arc};
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "irisd",
    about = "IMX477 camera daemon with authenticated TCP control"
)]
struct Args {
    #[arg(long, env = "IRIS_BIND", default_value = "0.0.0.0:8080")]
    bind: SocketAddr,
    #[arg(long, env = "IRIS_CONTROL_BIND", default_value = "127.0.0.1:8081")]
    control_bind: SocketAddr,
    #[arg(long, env = "IRIS_CONTROL_TOKEN_FILE")]
    control_token_file: Option<std::path::PathBuf>,
    #[arg(long, env = "IRIS_CAMERA_INDEX", default_value_t = 0)]
    camera_index: usize,
    #[arg(long, env = "IRIS_WIDTH", default_value_t = 1280)]
    width: u32,
    #[arg(long, env = "IRIS_HEIGHT", default_value_t = 720)]
    height: u32,
    #[arg(long, env = "IRIS_FPS", default_value_t = 30)]
    fps: u32,
    #[arg(long, env = "IRIS_EXPOSURE_US", default_value_t = 59000)]
    exposure_us: i32,
    #[arg(long, env = "IRIS_GAIN", default_value_t = 1.5)]
    gain: f32,
    #[arg(long, env = "IRIS_AUTO_EXPOSURE", default_value_t = false)]
    auto_exposure: bool,
    #[arg(long, env = "IRIS_AUTO_GAIN", default_value_t = false)]
    auto_gain: bool,
    #[arg(long, env = "IRIS_JPEG_QUALITY", default_value_t = 85)]
    jpeg_quality: u8,
    /// Keep daemon running with capture initially stopped.
    #[arg(long, env = "IRIS_IDLE", default_value_t = false)]
    idle: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "irisd=info,gmsl_picam_rs=info".into()),
        )
        .init();
    let args = Args::parse();
    let token = args
        .control_token_file
        .as_ref()
        .map(|path| std::fs::read_to_string(path).context("cannot read control token file"))
        .transpose()?
        .map(|value| value.trim().to_owned());
    if let Some(token) = &token {
        control::validate_token(token)?;
    }
    anyhow::ensure!(
        args.control_bind.ip().is_loopback() || token.is_some(),
        "a control token file is required for non-loopback TCP control"
    );
    // Bind both listeners before starting capture so an occupied port cannot strand a worker.
    let preview = tokio::net::TcpListener::bind(args.bind)
        .await
        .context("failed to bind preview listener")?;
    let commands = tokio::net::TcpListener::bind(args.control_bind)
        .await
        .context("failed to bind control listener")?;
    let service = Arc::new(CameraService::new(CameraConfig {
        camera_index: args.camera_index,
        width: args.width,
        height: args.height,
        fps: args.fps,
        exposure_us: (!args.auto_exposure).then_some(args.exposure_us),
        gain: (!args.auto_gain).then_some(args.gain),
        jpeg_quality: args.jpeg_quality,
    })?);
    if !args.idle {
        service.start()?;
    }
    info!(preview = %args.bind, control = %args.control_bind, "irisd listening");
    let result = tokio::select! {
        result = axum::serve(preview, http::router(Arc::clone(&service.frames))).into_future() => result.context("preview server failed"),
        result = axum::serve(commands, control::router_with_token(Arc::clone(&service), token).into_make_service_with_connect_info::<SocketAddr>()).into_future() => result.context("control server failed"),
        result = shutdown_signal() => result,
    };
    // Drop HTTP connections, then stop and join capture independently of open MJPEG clients.
    tokio::task::spawn_blocking(move || service.shutdown())
        .await
        .context("shutdown task failed")??;
    result
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
