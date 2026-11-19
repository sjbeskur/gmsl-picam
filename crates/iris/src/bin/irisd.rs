use anyhow::{Context, Result};
use clap::Parser;
use gmsl_picam_rs::{
    camera::CameraConfig,
    control::CameraService,
    transmitter::{self, StreamEndpoint},
    udp,
};
use std::{net::SocketAddr, sync::Arc};
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "irisd",
    version,
    about = "Camera daemon with UDP commands and pluggable image output"
)]
struct Args {
    #[arg(long, env = "IRIS_CONTROL_BIND", default_value = "0.0.0.0:8081")]
    control_bind: SocketAddr,
    /// Optional image receiver; tcp://IP:port or unix:///absolute/path.
    #[arg(long, env = "IRIS_TRANSMIT")]
    transmit: Option<StreamEndpoint>,
    /// Disable image output, including an endpoint inherited from the environment.
    #[arg(long, env = "IRIS_NO_TRANSMIT", default_value_t = false)]
    no_transmit: bool,
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
    let commands = tokio::net::UdpSocket::bind(args.control_bind)
        .await
        .context("bind UDP command listener")?;
    let service = Arc::new(CameraService::new(CameraConfig {
        camera_index: args.camera_index,
        width: args.width,
        height: args.height,
        fps: args.fps,
        exposure_us: (!args.auto_exposure).then_some(args.exposure_us),
        gain: (!args.auto_gain).then_some(args.gain),
        jpeg_quality: args.jpeg_quality,
    })?);
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let mut output = args.transmit.filter(|_| !args.no_transmit).map(|endpoint| {
        tokio::spawn(transmitter::run(
            endpoint.transmitter(),
            Arc::clone(&service.frames),
            receiver,
        ))
    });
    if !args.idle {
        service.start()?;
    }
    info!(udp = %args.control_bind, "irisd listening");
    let mut output_finished = false;
    let result = tokio::select! {
        result = udp::serve(commands, Arc::clone(&service)) => result.context("UDP command listener failed"),
        result = async {
            match output.as_mut() {
                Some(task) => task.await.context("image output task panicked")?.context("image output failed"),
                None => std::future::pending::<Result<()>>().await,
            }
        } => { output_finished = true; result },
        result = shutdown_signal() => result,
    };
    shutdown.send_replace(true);
    tokio::task::spawn_blocking(move || service.shutdown())
        .await
        .context("shutdown task failed")??;
    if !output_finished {
        if let Some(output) = output {
            output.await.context("image output task failed")??;
        }
    }
    result
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("register SIGTERM handler")?;
        tokio::select! { result = tokio::signal::ctrl_c() => result.context("Ctrl-C handler failed")?, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .context("Ctrl-C handler failed")?;
    Ok(())
}
