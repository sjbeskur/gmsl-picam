use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use iris_client::config::{default_config_path, CameraProfile, ClientConfig};
use iris_client::{IrisClient, SettingsPatch};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "iris",
    version,
    about = "Control camera capture in the irisd service"
)]
struct Cli {
    #[arg(long, env = "IRIS_CONTROL_ADDRESS", global = true)]
    address: Option<String>,
    #[arg(long, env = "IRIS_ENDPOINT", global = true)]
    endpoint: Option<String>,
    #[arg(long, env = "IRIS_CONFIG", global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Test the configured daemon connection and report round-trip time.
    Ping { camera_name: Option<String> },
    /// Show saved TOML configuration without connecting to the daemon.
    Show,
    /// Save a named connection in the client TOML file; no network request is made.
    Config(ConfigArgs),
    /// Control the camera served by a saved connection profile.
    Camera {
        camera_name: String,
        #[command(subcommand)]
        command: CameraCommand,
    },
    /// Show requested settings and actual capture metadata.
    Status,
    /// Start camera capture (the irisd daemon must already be running).
    Start,
    /// Stop and release the camera while leaving irisd running.
    Stop,
    /// Change live settings; dimensions and camera index require stopped capture.
    Set(SettingsArgs),
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[arg(default_value = "default")]
    camera_name: String,
    /// Show saved TOML without changing it.
    #[arg(long)]
    show: bool,
}

#[derive(Debug, Subcommand)]
enum CameraCommand {
    /// Test this camera profile's daemon connection.
    Ping,
    Status,
    Start,
    Stop,
    Set(SettingsArgs),
}

#[derive(Debug, Args)]
struct SettingsArgs {
    #[arg(long, conflicts_with = "auto_exposure")]
    exposure_us: Option<i32>,
    #[arg(long, conflicts_with = "auto_gain")]
    gain: Option<f32>,
    #[arg(long)]
    fps: Option<u32>,
    #[arg(long)]
    jpeg_quality: Option<u8>,
    #[arg(long)]
    width: Option<u32>,
    #[arg(long)]
    height: Option<u32>,
    #[arg(long)]
    camera_index: Option<usize>,
    #[arg(long)]
    auto_exposure: bool,
    #[arg(long)]
    auto_gain: bool,
}
impl From<SettingsArgs> for SettingsPatch {
    fn from(args: SettingsArgs) -> Self {
        Self {
            exposure_us: args.exposure_us,
            gain: args.gain,
            fps: args.fps,
            jpeg_quality: args.jpeg_quality,
            width: args.width,
            height: args.height,
            camera_index: args.camera_index,
            auto_exposure: args.auto_exposure,
            auto_gain: args.auto_gain,
        }
    }
}

fn endpoint_override(args: &Cli) -> Option<String> {
    args.endpoint.clone().or_else(|| {
        args.address.as_ref().map(|address| {
            if address.contains("://") {
                address.clone()
            } else {
                format!("udp://{address}")
            }
        })
    })
}

fn config_path(args: &Cli) -> Result<PathBuf> {
    let path = args.config.clone().map_or_else(default_config_path, Ok)?;
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    })
}

fn save_config(
    args: &Cli,
    options: &ConfigArgs,
    path: &Path,
    mut config: ClientConfig,
) -> Result<()> {
    if options.show {
        return show_config(&config);
    }
    let mut profile = config
        .cameras
        .get(&options.camera_name)
        .cloned()
        .unwrap_or_default();
    if let Some(endpoint) = endpoint_override(args) {
        profile.endpoint = endpoint;
    }
    config.cameras.insert(options.camera_name.clone(), profile);
    config.save(path)?;
    println!(
        "Saved camera '{}' to {}",
        options.camera_name,
        path.display()
    );
    Ok(())
}

fn show_config(config: &ClientConfig) -> Result<()> {
    println!("{}", config.to_toml()?);
    Ok(())
}

fn connect(args: &Cli, mut profile: CameraProfile) -> Result<IrisClient> {
    if let Some(endpoint) = endpoint_override(args) {
        profile.endpoint = endpoint;
    }
    profile.client()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let path = config_path(&args)?;
    let config = ClientConfig::load(&path)?;
    if let Command::Show = &args.command {
        return show_config(&config);
    }
    if let Command::Config(options) = &args.command {
        return save_config(&args, options, &path, config);
    }
    let profile = match &args.command {
        Command::Camera { camera_name, .. }
        | Command::Ping {
            camera_name: Some(camera_name),
        } => config.camera(camera_name)?.clone(),
        _ => config.cameras.get("default").cloned().unwrap_or_default(),
    };
    let endpoint = endpoint_override(&args).unwrap_or_else(|| profile.endpoint.clone());
    let client = connect(&args, profile)?;
    if matches!(
        &args.command,
        Command::Ping { .. }
            | Command::Camera {
                command: CameraCommand::Ping,
                ..
            }
    ) {
        let started = std::time::Instant::now();
        tokio::time::timeout(std::time::Duration::from_secs(3), client.ping())
            .await
            .with_context(|| format!("ping to {endpoint} timed out after 3 seconds"))?
            .with_context(|| format!("ping to {endpoint} failed"))?;
        println!(
            "pong {endpoint} ({:.2} ms)",
            started.elapsed().as_secs_f64() * 1000.0
        );
        return Ok(());
    }
    let status = match args.command {
        Command::Status
        | Command::Camera {
            command: CameraCommand::Status,
            ..
        } => client.status().await,
        Command::Start
        | Command::Camera {
            command: CameraCommand::Start,
            ..
        } => client.start().await,
        Command::Stop
        | Command::Camera {
            command: CameraCommand::Stop,
            ..
        } => client.stop().await,
        Command::Set(patch)
        | Command::Camera {
            command: CameraCommand::Set(patch),
            ..
        } => client.set_settings(&patch.into()).await,
        Command::Config(_)
        | Command::Show
        | Command::Ping { .. }
        | Command::Camera {
            command: CameraCommand::Ping,
            ..
        } => unreachable!(),
    }
    .with_context(|| format!("iris command at {endpoint}; check systemctl status irisd"))?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_saved_profile_commands_and_connection_flags() {
        let args = Cli::try_parse_from([
            "iris",
            "config",
            "front",
            "--endpoint",
            "http://pi.local:8081",
        ])
        .unwrap();
        assert!(
            matches!(args.command, Command::Config(ConfigArgs { camera_name, .. }) if camera_name == "front")
        );
        assert_eq!(args.endpoint.as_deref(), Some("http://pi.local:8081"));
        assert!(matches!(
            Cli::try_parse_from(["iris", "camera", "front", "stop"])
                .unwrap()
                .command,
            Command::Camera {
                command: CameraCommand::Stop,
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["iris", "camera", "front", "start"])
                .unwrap()
                .command,
            Command::Camera {
                command: CameraCommand::Start,
                ..
            }
        ));
    }
}
