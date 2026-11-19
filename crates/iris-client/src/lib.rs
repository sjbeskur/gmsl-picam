//! Typed camera control for the CLI, native backends, and browser gateways.
//! Native clients use udp://host:port. Browser clients use HTTP to iris-view.
//! ```no_run
//! # async fn example() -> Result<(), iris_client::ClientError> {
//! use iris_client::{IrisClient, SettingsPatch};
//! let camera = IrisClient::new("udp://127.0.0.1:8081")?;
//! camera.start().await?;
//! camera.set_settings(&SettingsPatch { gain: Some(1.5), ..Default::default() }).await?;
//! # Ok(()) }
//! ```
#[cfg(all(feature = "config", not(target_arch = "wasm32")))]
pub mod config;
pub use photon_protocol::{
    CameraSettings, CameraState, DaemonStatus, FrameMetadata, ServiceStatus, SettingsPatch,
};
use photon_protocol::{Command, CommandResult};
use std::sync::Arc;
use url::Url;

#[derive(Debug)]
pub enum ClientError {
    InvalidEndpoint(String),
    InvalidSettings(String),
    #[cfg(feature = "http")]
    Transport(reqwest::Error),
    Io(std::io::Error),
    Protocol(String),
    Timeout,
    Api {
        status: u16,
        message: String,
    },
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEndpoint(m) => write!(f, "invalid irisd endpoint: {m}"),
            Self::InvalidSettings(m) => write!(f, "invalid camera settings: {m}"),
            #[cfg(feature = "http")]
            Self::Transport(e) => write!(f, "irisd request failed: {e}"),
            Self::Io(e) => write!(f, "irisd UDP request failed: {e}"),
            Self::Protocol(m) => write!(f, "invalid photon response: {m}"),
            Self::Timeout => write!(
                f,
                "irisd UDP request timed out; command outcome may be unknown"
            ),
            Self::Api { status, message } => write!(f, "irisd returned {status}: {message}"),
        }
    }
}
impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "http")]
            Self::Transport(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
#[cfg(feature = "http")]
impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        Self::Transport(e)
    }
}
impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug)]
enum Transport {
    #[cfg(not(any(feature = "http", all(feature = "udp", not(target_arch = "wasm32")))))]
    Disabled,
    #[cfg(feature = "http")]
    Http { client: reqwest::Client, base: Url },
    #[cfg(all(feature = "udp", not(target_arch = "wasm32")))]
    Udp { host: String, port: u16 },
}
#[derive(Debug, Clone)]
pub struct IrisClient {
    transport: Arc<Transport>,
}
impl IrisClient {
    pub fn new(endpoint: &str) -> Result<Self, ClientError> {
        let base = Url::parse(endpoint).map_err(|e| ClientError::InvalidEndpoint(e.to_string()))?;
        if base.host_str().is_none()
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
        {
            return Err(ClientError::InvalidEndpoint(
                "expected an endpoint without credentials, query or fragment".into(),
            ));
        }
        #[cfg(all(feature = "udp", not(target_arch = "wasm32")))]
        if base.scheme() == "udp" && matches!(base.path(), "" | "/") {
            let port = base.port().ok_or_else(|| {
                ClientError::InvalidEndpoint("UDP endpoint requires a port".into())
            })?;
            return Ok(Self {
                transport: Arc::new(Transport::Udp {
                    host: base.host_str().unwrap().trim_matches(['[', ']']).into(),
                    port,
                }),
            });
        }
        #[cfg(feature = "http")]
        if matches!(base.scheme(), "http" | "https") {
            #[cfg(not(target_arch = "wasm32"))]
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(12))
                .build()?;
            #[cfg(target_arch = "wasm32")]
            let client = reqwest::Client::new();
            return Self::with_http_client(endpoint, client);
        }
        Err(ClientError::InvalidEndpoint("unsupported endpoint; enable the matching HTTP/UDP transport feature (browsers require HTTP)".into()))
    }
    #[cfg(feature = "http")]
    pub fn with_http_client(endpoint: &str, client: reqwest::Client) -> Result<Self, ClientError> {
        let mut base =
            Url::parse(endpoint).map_err(|e| ClientError::InvalidEndpoint(e.to_string()))?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
        {
            return Err(ClientError::InvalidEndpoint(
                "expected an absolute HTTP(S) URL without credentials, query or fragment".into(),
            ));
        }
        base.set_path(&format!("{}/", base.path().trim_end_matches('/')));
        Ok(Self {
            transport: Arc::new(Transport::Http { client, base }),
        })
    }
    pub async fn ping(&self) -> Result<(), ClientError> {
        match self.command(Command::Ping).await? {
            CommandResult::Pong => Ok(()),
            _ => Err(ClientError::Protocol("expected pong".into())),
        }
    }
    pub async fn status(&self) -> Result<DaemonStatus, ClientError> {
        self.status_command(Command::Status).await
    }
    pub async fn start(&self) -> Result<DaemonStatus, ClientError> {
        self.status_command(Command::Start).await
    }
    pub async fn stop(&self) -> Result<DaemonStatus, ClientError> {
        self.status_command(Command::Stop).await
    }
    pub async fn set_settings(&self, patch: &SettingsPatch) -> Result<DaemonStatus, ClientError> {
        patch
            .validate()
            .map_err(|e| ClientError::InvalidSettings(e.to_string()))?;
        self.status_command(Command::Settings(patch.clone())).await
    }
    async fn status_command(&self, command: Command) -> Result<DaemonStatus, ClientError> {
        match self.command(command).await? {
            CommandResult::Status(s) => Ok(*s),
            _ => Err(ClientError::Protocol("expected status".into())),
        }
    }
    async fn command(&self, command: Command) -> Result<CommandResult, ClientError> {
        let response: CommandResult = match self.transport.as_ref() {
            #[cfg(not(any(feature = "http", all(feature = "udp", not(target_arch = "wasm32")))))]
            Transport::Disabled => {
                return Err(ClientError::InvalidEndpoint("no transport enabled".into()))
            }
            #[cfg(feature = "http")]
            Transport::Http { client, base } => http_command(client, base, command).await?,
            #[cfg(all(feature = "udp", not(target_arch = "wasm32")))]
            Transport::Udp { host, port } => udp_command(host, *port, command).await?,
        };
        if let CommandResult::Error { code, message } = response {
            use photon_protocol::ErrorCode;
            return Err(ClientError::Api {
                status: match code {
                    ErrorCode::Invalid => 400,
                    ErrorCode::Conflict => 409,
                    ErrorCode::Unavailable => 503,
                },
                message,
            });
        }
        Ok(response)
    }
}

#[cfg(feature = "http")]
async fn http_command(
    client: &reqwest::Client,
    base: &Url,
    command: Command,
) -> Result<CommandResult, ClientError> {
    use reqwest::Method;
    let (method, path) = match &command {
        Command::Ping => (Method::GET, "ping"),
        Command::Status => (Method::GET, "status"),
        Command::Start => (Method::POST, "camera/start"),
        Command::Stop => (Method::POST, "camera/stop"),
        Command::Settings(_) => (Method::PATCH, "settings"),
    };
    let url = base
        .join(path)
        .map_err(|e| ClientError::InvalidEndpoint(e.to_string()))?;
    let mut request = client
        .request(method, url)
        .header(iris_api::CONTROL_HEADER, "1");
    if let Command::Settings(patch) = &command {
        request = request.json(patch);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await?;
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|v| v.as_str()).map(str::to_owned))
            .unwrap_or(body);
        return Err(ClientError::Api { status, message });
    }
    if matches!(command, Command::Ping) {
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(ClientError::Api {
                status: response.status().as_u16(),
                message: "expected HTTP 204 pong".into(),
            });
        }
        Ok(CommandResult::Pong)
    } else {
        Ok(CommandResult::Status(Box::new(response.json().await?)))
    }
}

#[cfg(all(feature = "udp", not(target_arch = "wasm32")))]
async fn udp_command(
    host: &str,
    port: u16,
    command: Command,
) -> Result<CommandResult, ClientError> {
    use photon_protocol::{CommandPacket, CommandResponse, MAX_RESPONSE_BYTES};
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    static IDS: AtomicU64 = AtomicU64::new(0);
    let id = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64)
        .wrapping_add(IDS.fetch_add(1, Ordering::Relaxed));
    let packet = CommandPacket::new(id, command).encode()?;
    let request = async {
        let peer = tokio::net::lookup_host((host, port))
            .await?
            .next()
            .ok_or_else(|| ClientError::InvalidEndpoint("host has no address".into()))?;
        let socket = tokio::net::UdpSocket::bind(if peer.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .await?;
        socket.connect(peer).await?;
        let mut retry = tokio::time::interval(Duration::from_millis(400));
        retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut buffer = [0; MAX_RESPONSE_BYTES + 1];
        loop {
            tokio::select! {
                _ = retry.tick() => { socket.send(&packet).await?; },
                received = socket.recv(&mut buffer) => {
                    let len = received?;
                    if let Ok(reply) = CommandResponse::decode(&buffer[..len]) {
                        if reply.request_id == id { return Ok(reply.result); }
                    }
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(12), request)
        .await
        .map_err(|_| ClientError::Timeout)?
}
#[cfg(all(test, feature = "http", not(target_arch = "wasm32")))]
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
    async fn ping_uses_small_request_and_empty_response() {
        let (url, server) = server("204 No Content", "");
        IrisClient::new(&url).unwrap().ping().await.unwrap();
        let request = server.join().unwrap().to_lowercase();
        assert!(request.starts_with("get /api/iris/ping "));
        assert!(!request.contains("authorization:"));
        assert!(request.ends_with("\r\n\r\n"));
        let (url, server) = self::server("200 OK", "not a daemon ping response");
        assert!(matches!(
            IrisClient::new(&url).unwrap().ping().await,
            Err(ClientError::Api { status: 200, .. })
        ));
        server.join().unwrap();
    }
}

#[cfg(all(test, feature = "udp", not(target_arch = "wasm32")))]
mod udp_tests {
    use super::*;
    use photon_protocol::{CommandPacket, CommandResponse};
    #[tokio::test]
    async fn lost_reply_retries_same_request_and_ignores_unrelated_reply() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut buffer = [0; 1200];
            let (n, peer) = server.recv_from(&mut buffer).await.unwrap();
            let first = buffer[..n].to_vec();
            let packet = CommandPacket::decode(&first).unwrap();
            assert_eq!(packet.command, Command::Ping);
            let (n, retry_peer) = server.recv_from(&mut buffer).await.unwrap();
            assert_eq!(peer, retry_peer);
            assert_eq!(&buffer[..n], first);
            server
                .send_to(
                    &CommandResponse::new(packet.request_id.wrapping_add(1), CommandResult::Pong)
                        .encode()
                        .unwrap(),
                    peer,
                )
                .await
                .unwrap();
            server
                .send_to(
                    &CommandResponse::new(packet.request_id, CommandResult::Pong)
                        .encode()
                        .unwrap(),
                    peer,
                )
                .await
                .unwrap();
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            IrisClient::new(&format!("udp://{address}")).unwrap().ping(),
        )
        .await
        .unwrap()
        .unwrap();
        task.await.unwrap();
    }
}
