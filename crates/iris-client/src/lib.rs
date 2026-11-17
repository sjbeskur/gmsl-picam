//! Typed async camera control, shared by the Iris CLI and Rust web applications.
//!
//! ```no_run
//! # async fn example() -> Result<(), iris_client::ClientError> {
//! use iris_client::{IrisClient, SettingsPatch};
//! let camera = IrisClient::new("http://127.0.0.1:8081")?;
//! camera.start().await?;
//! camera.set_settings(&SettingsPatch { gain: Some(1.5), ..Default::default() }).await?;
//! let status = camera.status().await?;
//! println!("{:?}", status.camera.camera_state);
//! # Ok(()) }
//! ```
//!
//! The crate has no libcamera, Axum, or Clap dependency. Browser builds use Fetch;
//! proxy the daemon API on the web application's origin (including the control
//! header), and attach its token for remote access. Keep browser authentication in the application proxy.
#[cfg(all(feature = "config", not(target_arch = "wasm32")))]
pub mod config;

use iris_api::CONTROL_HEADER;
pub use iris_api::{
    CameraSettings, CameraState, DaemonStatus, FrameMetadata, ServiceStatus, SettingsPatch,
};
use reqwest::{Client, Method, Url};

#[derive(Debug)]
pub enum ClientError {
    InvalidEndpoint(String),
    InvalidSettings(String),
    InvalidToken(String),
    Transport(reqwest::Error),
    Api { status: u16, message: String },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEndpoint(message) => write!(f, "invalid irisd endpoint: {message}"),
            Self::InvalidToken(message) => write!(f, "invalid control token: {message}"),
            Self::InvalidSettings(message) => write!(f, "invalid camera settings: {message}"),
            Self::Transport(error) => write!(f, "irisd request failed: {error}"),
            Self::Api { status, message } => write!(f, "irisd returned HTTP {status}: {message}"),
        }
    }
}
impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}
impl From<reqwest::Error> for ClientError {
    fn from(error: reqwest::Error) -> Self {
        Self::Transport(error)
    }
}

/// Cheaply cloneable, asynchronous client. A successful mutation reports accepted
/// settings; use actual frame metadata to check hardware application/clamping.
#[derive(Debug, Clone)]
pub struct IrisClient {
    http: Client,
    base: Url,
    authorization: Option<reqwest::header::HeaderValue>,
}

impl IrisClient {
    /// Native defaults: direct HTTP, a 12-second request timeout, and no proxy.
    /// Browser transport uses Fetch and the browser's network policy.
    pub fn new(base: &str) -> Result<Self, ClientError> {
        #[cfg(not(target_arch = "wasm32"))]
        let http = Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(12))
            .build()?;
        #[cfg(target_arch = "wasm32")]
        let http = Client::new();
        Self::with_http_client(base, http)
    }

    /// Supply a configured reqwest client for a web backend's proxy, authentication,
    /// or timeout policy. The base may include a reverse-proxy prefix, e.g. /api/iris.
    pub fn with_http_client(base: &str, http: Client) -> Result<Self, ClientError> {
        let mut base =
            Url::parse(base).map_err(|error| ClientError::InvalidEndpoint(error.to_string()))?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(ClientError::InvalidEndpoint(
                "expected an absolute HTTP(S) URL without query or fragment".into(),
            ));
        }
        base.set_path(&format!("{}/", base.path().trim_end_matches('/')));
        Ok(Self {
            http,
            base,
            authorization: None,
        })
    }

    /// Add a bearer token for direct remote TCP control. Debug output redacts it.
    pub fn with_token(mut self, token: &str) -> Result<Self, ClientError> {
        if token.is_empty() || token.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(ClientError::InvalidToken(
                "expected a non-empty token without whitespace".into(),
            ));
        }
        let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|error| ClientError::InvalidToken(error.to_string()))?;
        value.set_sensitive(true);
        self.authorization = Some(value);
        Ok(self)
    }

    pub async fn status(&self) -> Result<DaemonStatus, ClientError> {
        self.request(Method::GET, "status", None).await
    }
    pub async fn start(&self) -> Result<DaemonStatus, ClientError> {
        self.request(Method::POST, "camera/start", None).await
    }
    pub async fn stop(&self) -> Result<DaemonStatus, ClientError> {
        self.request(Method::POST, "camera/stop", None).await
    }
    pub async fn set_settings(&self, patch: &SettingsPatch) -> Result<DaemonStatus, ClientError> {
        patch
            .validate()
            .map_err(|error| ClientError::InvalidSettings(error.to_string()))?;
        self.request(Method::PATCH, "settings", Some(patch)).await
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        patch: Option<&SettingsPatch>,
    ) -> Result<DaemonStatus, ClientError> {
        let url = self
            .base
            .join(path)
            .map_err(|error| ClientError::InvalidEndpoint(error.to_string()))?;
        let mut request = self.http.request(method, url).header(CONTROL_HEADER, "1");
        if let Some(token) = &self.authorization {
            request = request.header(reqwest::header::AUTHORIZATION, token);
        }
        if let Some(patch) = patch {
            request = request.json(patch);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await?;
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("error")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                })
                .unwrap_or(body);
            return Err(ClientError::Api { status, message });
        }
        Ok(response.json().await?)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn server(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/iris", listener.local_addr().unwrap());
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let len = socket.read(&mut buf).unwrap();
                assert!(len > 0);
                bytes.extend_from_slice(&buf[..len]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .map(|n| n.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            socket.write_all(response.as_bytes()).unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (url, handle)
    }

    #[tokio::test]
    async fn preserves_api_errors_prefix_and_control_header() {
        let (url, server) = server("409 Conflict", r#"{"error":"stop capture first"}"#);
        let error = IrisClient::new(&url).unwrap().start().await.unwrap_err();
        assert!(
            matches!(error, ClientError::Api { status: 409, message } if message == "stop capture first")
        );
        let request = server.join().unwrap().to_lowercase();
        assert!(request.starts_with("post /api/iris/camera/start "));
        assert!(request.contains("x-iris-control: 1"));
    }

    #[tokio::test]
    async fn decodes_status_and_sends_typed_patch() {
        let status = DaemonStatus {
            camera: ServiceStatus {
                camera_state: CameraState::Stopped,
                frame_count: 0,
                width: 1280,
                height: 720,
                pixel_format: "NV12".into(),
                latest_frame: None,
                last_error: None,
                frame_age_ms: None,
                frame_stale: true,
            },
            requested_settings: CameraSettings::default(),
            capture_active: false,
        };
        let (url, server) = server("200 OK", &serde_json::to_string(&status).unwrap());
        let response = IrisClient::new(&url)
            .unwrap()
            .set_settings(&SettingsPatch {
                gain: Some(1.5),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.camera.camera_state, CameraState::Stopped);
        let request = server.join().unwrap();
        assert!(request.starts_with("PATCH /api/iris/settings "));
        let json: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(json["gain"], 1.5);
    }

    #[tokio::test]
    async fn rejects_invalid_input_without_network_io() {
        assert!(IrisClient::new("file:///tmp/camera").is_err());
        let client = IrisClient::new("http://127.0.0.1:1").unwrap();
        assert!(matches!(
            client
                .set_settings(&SettingsPatch {
                    gain: Some(f32::NAN),
                    ..Default::default()
                })
                .await,
            Err(ClientError::InvalidSettings(_))
        ));
    }
    #[tokio::test]
    async fn includes_remote_token_without_exposing_it_in_debug() {
        let (url, server) = server("401 Unauthorized", r#"{"error":"invalid token"}"#);
        let client = IrisClient::new(&url)
            .unwrap()
            .with_token("secret-token")
            .unwrap();
        assert!(!format!("{client:?}").contains("secret-token"));
        assert!(matches!(
            client.status().await,
            Err(ClientError::Api { status: 401, .. })
        ));
        assert!(server
            .join()
            .unwrap()
            .to_lowercase()
            .contains("authorization: bearer secret-token"));
    }
}
