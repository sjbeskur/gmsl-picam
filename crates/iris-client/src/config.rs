//! Optional native TOML profiles. The async transport remains usable without filesystem configuration.
use crate::IrisClient;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraProfile {
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_file: Option<PathBuf>,
}
impl Default for CameraProfile {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8081".into(),
            token_file: None,
        }
    }
}
impl CameraProfile {
    pub fn validate(&self) -> Result<()> {
        let endpoint =
            reqwest::Url::parse(&self.endpoint).context("invalid camera endpoint URL")?;
        anyhow::ensure!(
            matches!(endpoint.scheme(), "http" | "https")
                && endpoint.host_str().is_some()
                && endpoint.query().is_none()
                && endpoint.fragment().is_none()
                && endpoint.username().is_empty()
                && endpoint.password().is_none(),
            "endpoint must be an absolute HTTP(S) URL without credentials, query or fragment"
        );
        anyhow::ensure!(
            self.token_file
                .as_ref()
                .is_none_or(|path| !path.as_os_str().is_empty()),
            "token file path cannot be empty"
        );
        Ok(())
    }

    /// Relative token paths in TOML resolve against the config file's directory.
    pub fn client(&self, config_directory: &Path) -> Result<IrisClient> {
        self.validate()?;
        let mut client = IrisClient::new(&self.endpoint)?;
        if let Some(path) = &self.token_file {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                config_directory.join(path)
            };
            let token = fs::read_to_string(&path)
                .with_context(|| format!("cannot read token file {}", path.display()))?;
            client = client.with_token(token.trim())?;
        }
        Ok(client)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    #[serde(default)]
    pub cameras: BTreeMap<String, CameraProfile>,
}

impl ClientConfig {
    /// A missing file is an empty configuration; malformed files are never overwritten silently.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()))
            }
        };
        let config: Self =
            toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn camera(&self, name: &str) -> Result<&CameraProfile> {
        self.cameras.get(name).with_context(|| format!("camera profile '{name}' is not configured; run iris config {name} --endpoint <url>"))
    }

    pub fn validate(&self) -> Result<()> {
        for (name, profile) in &self.cameras {
            anyhow::ensure!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)),
                "camera names must use letters, digits, '-', '_' or '.'"
            );
            profile
                .validate()
                .with_context(|| format!("invalid camera profile '{name}'"))?;
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String> {
        self.validate()?;
        toml::to_string_pretty(self).context("cannot serialize client configuration")
    }

    /// Atomic replacement, with mode 0600 on Unix. No token contents are persisted.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = self.to_toml()?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let (temporary, mut file) = loop {
            let temporary = parent.join(format!(
                ".iris-config-{}-{}.tmp",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&temporary) {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).context("cannot create temporary configuration file")
                }
            }
        };
        let result = (|| -> Result<()> {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
                .with_context(|| format!("cannot save {}", path.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        anyhow::ensure!(path.is_absolute(), "XDG_CONFIG_HOME must be absolute");
        return Ok(path.join("iris/config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home).join(".config/iris/config.toml"));
    }
    if let Some(path) = std::env::var_os("APPDATA").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path).join("iris/config.toml"));
    }
    anyhow::bail!("cannot locate client configuration directory; use --config <path>")
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "iris-config-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn round_trip_preserves_other_cameras_and_keeps_files_private() {
        let dir = Temp::new();
        let path = dir.0.join("nested/config.toml");
        let mut config = ClientConfig::load(&path).unwrap();
        config.cameras.insert(
            "front".into(),
            CameraProfile {
                endpoint: "http://192.168.50.150:8081".into(),
                token_file: Some("front.token".into()),
            },
        );
        config.save(&path).unwrap();
        let mut loaded = ClientConfig::load(&path).unwrap();
        loaded
            .cameras
            .insert("rear".into(), CameraProfile::default());
        loaded.save(&path).unwrap();
        assert_eq!(ClientConfig::load(&path).unwrap().cameras.len(), 2);
        assert_eq!(
            loaded.camera("front").unwrap().token_file.as_deref(),
            Some(Path::new("front.token"))
        );
        assert!(loaded.camera("missing").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn malformed_or_invalid_config_is_rejected_without_overwrite() {
        let dir = Temp::new();
        let path = dir.0.join("config.toml");
        fs::write(&path, "not [valid TOML").unwrap();
        assert!(ClientConfig::load(&path).is_err());
        let mut config = ClientConfig::default();
        config.cameras.insert(
            "front".into(),
            CameraProfile {
                endpoint: "file:///tmp/camera".into(),
                token_file: None,
            },
        );
        assert!(config.save(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "not [valid TOML");
    }

    #[tokio::test]
    async fn relative_credentials_resolve_against_config_directory() {
        let dir = Temp::new();
        fs::write(dir.0.join("token"), "some-private-token\n").unwrap();
        let profile = CameraProfile {
            token_file: Some("token".into()),
            ..Default::default()
        };
        let client = profile.client(&dir.0).unwrap();
        assert!(!format!("{client:?}").contains("some-private-token"));
        assert!(profile.client(&dir.0.join("wrong")).is_err());
    }
}
