use anyhow::{Context, Result};
use clap::Parser;
use gmsl_picam_rs::{
    frame_store::{CameraState, FrameStore},
    gateway, http,
    transmitter::{PostcardListener, StreamEndpoint},
};
use iris_client::IrisClient;
use std::{
    future::IntoFuture,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Debug, Parser)]
#[command(
    name = "iris-view",
    version,
    about = "HTTP live view gateway for Photon image postcards"
)]
struct Args {
    #[arg(long, env = "IRIS_VIEW_BIND", default_value = "0.0.0.0:8080")]
    bind: SocketAddr,
    #[arg(long, env = "IRIS_VIEW_CONTROL_BIND", default_value = "0.0.0.0:8081")]
    control_bind: SocketAddr,
    #[arg(long, env = "IRIS_VIEW_LISTEN", default_value = "tcp://127.0.0.1:9001")]
    listen: StreamEndpoint,
    #[arg(
        long,
        env = "IRIS_VIEW_COMMAND_ENDPOINT",
        default_value = "udp://127.0.0.1:8081"
    )]
    command_endpoint: String,
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "iris_view=info,gmsl_picam_rs=info".into()),
        )
        .init();
    let args = Args::parse();
    anyhow::ensure!(
        args.command_endpoint.starts_with("udp://"),
        "iris-view command endpoint must use UDP"
    );
    let client = IrisClient::new(&args.command_endpoint)?;
    let preview = tokio::net::TcpListener::bind(args.bind)
        .await
        .context("bind preview HTTP")?;
    let commands = tokio::net::TcpListener::bind(args.control_bind)
        .await
        .context("bind gateway control HTTP")?;
    let listener = PostcardListener::bind(&args.listen).context("bind postcard receiver")?;
    let frames = Arc::new(FrameStore::new(CameraState::Stopped, 0, 0, "NV12"));
    let stop = Arc::new(AtomicBool::new(false));
    let receiver_frames = Arc::clone(&frames);
    let receiver_stop = Arc::clone(&stop);
    let mut receiver = tokio::task::spawn_blocking(move || {
        gateway::receive(listener, receiver_frames, receiver_stop)
    });
    tracing::info!(preview=%args.bind,control=%args.control_bind,images=?args.listen,"iris-view listening");
    let mut receiver_finished = false;
    let result = tokio::select! {
        result=axum::serve(preview,http::router(Arc::clone(&frames))).into_future()=>result.context("preview server failed"),
        result=axum::serve(commands,gateway::control_router(client,frames)).into_future()=>result.context("gateway control server failed"),
        result=&mut receiver=> { receiver_finished=true; result.context("image receiver panicked")?.context("image receiver failed") },
        result=shutdown_signal()=>result,
    };
    stop.store(true, Ordering::Release);
    if !receiver_finished {
        receiver.await.context("image receiver panicked")??;
    }
    result
}
async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { r=tokio::signal::ctrl_c()=>r?, _=terminate.recv()=>{} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}
