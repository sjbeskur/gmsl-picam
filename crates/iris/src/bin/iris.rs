use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use iris_client::config::{default_config_path, CameraProfile, ClientConfig};
use iris_client::{IrisClient, SettingsPatch};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "iris", about = "Control camera capture in the irisd service")]
struct Cli {
    #[arg(long, env = "IRIS_CONTROL_ADDRESS", global = true)]
    address: Option<String>,
    #[arg(long, env = "IRIS_ENDPOINT", global = true)]
    endpoint: Option<String>,
    #[arg(long, env = "IRIS_CONFIG", global = true)]
    config: Option<PathBuf>,
    #[arg(long, env = "IRIS_TOKEN_FILE", global = true, conflicts_with = "token")]
    token_file: Option<std::path::PathBuf>,
    #[arg(long, env = "IRIS_TOKEN", hide_env_values = true, global = true)]
    token: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    /// Remove the token-file reference from this profile.
    #[arg(long)]
    clear_token: bool,
}

#[derive(Debug, Subcommand)]
enum CameraCommand {
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
                format!("http://{address}")
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
        println!("{}", config.to_toml()?);
        return Ok(());
    }
    anyhow::ensure!(
        args.token.is_none(),
        "save a token-file reference with --token-file; raw tokens are never written to TOML"
    );
    anyhow::ensure!(
        !(options.clear_token && args.token_file.is_some()),
        "choose --clear-token or --token-file"
    );
    let mut profile = config
        .cameras
        .get(&options.camera_name)
        .cloned()
        .unwrap_or_default();
    if let Some(endpoint) = endpoint_override(args) {
        profile.endpoint = endpoint;
    }
    if let Some(token) = &args.token_file {
        profile.token_file = Some(if token.is_absolute() {
            token.clone()
        } else {
            std::env::current_dir()?.join(token)
        });
    }
    if options.clear_token {
        profile.token_file = None;
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

fn connect(args: &Cli, mut profile: CameraProfile, path: &Path) -> Result<IrisClient> {
    if let Some(endpoint) = endpoint_override(args) {
        profile.endpoint = endpoint;
    }
    if let Some(token) = &args.token_file {
        profile.token_file = Some(if token.is_absolute() {
            token.clone()
        } else {
            std::env::current_dir()?.join(token)
        });
    }
    if args.token.is_some() {
        profile.token_file = None;
    }
    let mut client = profile.client(path.parent().unwrap_or_else(|| Path::new(".")))?;
    if let Some(token) = &args.token {
        client = client.with_token(token)?;
    }
    Ok(client)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let path = config_path(&args)?;
    let config = ClientConfig::load(&path)?;
    if let Command::Config(options) = &args.command {
        return save_config(&args, options, &path, config);
    }
    let profile = match &args.command {
        Command::Camera { camera_name, .. } => config.camera(camera_name)?.clone(),
        _ => config.cameras.get("default").cloned().unwrap_or_default(),
    };
    let endpoint = endpoint_override(&args).unwrap_or_else(|| profile.endpoint.clone());
    let client = connect(&args, profile, &path)?;
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
        Command::Config(_) => unreachable!(),
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
