//! HTTP belongs to iris-view. Camera control is forwarded over UDP and images
//! arrive as framed postcards from a separately running camera daemon.
use crate::{
    frame_store::{CameraState, FrameStore},
    transmitter::{ImageStream, PostcardListener},
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use iris_client::{ClientError, DaemonStatus, IrisClient, SettingsPatch};
use photon_protocol::read_postcard;
use std::{
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Gateway {
    client: IrisClient,
    frames: Arc<FrameStore>,
}

pub fn control_router(client: IrisClient, frames: Arc<FrameStore>) -> Router {
    Router::new()
        .route("/ping", get(ping))
        .route("/status", get(status))
        .route("/camera/start", post(start))
        .route("/camera/stop", post(stop))
        .route("/settings", axum::routing::patch(settings))
        .layer(axum::middleware::from_fn(require_control_header))
        .with_state(Gateway { client, frames })
}
async fn require_control_header(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if request.method() != axum::http::Method::GET
        && request
            .headers()
            .get("x-iris-control")
            .is_none_or(|v| v != "1")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error":"X-Iris-Control: 1 is required"})),
        )
            .into_response();
    }
    next.run(request).await
}
struct GatewayError(ClientError);
impl From<ClientError> for GatewayError {
    fn from(e: ClientError) -> Self {
        Self(e)
    }
}
impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let code = match &self.0 {
            ClientError::Api { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            ClientError::InvalidSettings(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::BAD_GATEWAY,
        };
        (code, Json(serde_json::json!({"error":self.0.to_string()}))).into_response()
    }
}
async fn ping(State(g): State<Gateway>) -> Result<StatusCode, GatewayError> {
    g.client.ping().await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn status(State(g): State<Gateway>) -> Result<Json<DaemonStatus>, GatewayError> {
    Ok(Json(g.client.status().await?))
}
async fn start(State(g): State<Gateway>) -> Result<Json<DaemonStatus>, GatewayError> {
    Ok(Json(g.client.start().await?))
}
async fn stop(State(g): State<Gateway>) -> Result<Json<DaemonStatus>, GatewayError> {
    let status = g.client.stop().await?;
    g.frames.reset(
        CameraState::Stopped,
        status.camera.width,
        status.camera.height,
    );
    Ok(Json(status))
}
async fn settings(
    State(g): State<Gateway>,
    Json(patch): Json<SettingsPatch>,
) -> Result<Json<DaemonStatus>, GatewayError> {
    Ok(Json(g.client.set_settings(&patch).await?))
}

struct FrameReader<'a> {
    stream: &'a mut ImageStream,
    stop: &'a AtomicBool,
    deadline: Instant,
}
impl Read for FrameReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stop.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "gateway stopped",
                ));
            }
            if Instant::now() >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "postcard receive deadline",
                ));
            }
            match self.stream.read(bytes) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                result => return result,
            }
        }
    }
}

/// One configured camera publisher per gateway. A receive deadline bounds partial
/// frames and shutdown, and lengths are validated before allocating image data.
pub fn receive(
    listener: PostcardListener,
    frames: Arc<FrameStore>,
    stop: Arc<AtomicBool>,
) -> io::Result<()> {
    while !stop.load(Ordering::Acquire) {
        let mut stream = match listener.accept() {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(e) => return Err(e),
        };
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        while !stop.load(Ordering::Acquire) {
            let mut reader = FrameReader {
                stream: &mut stream,
                stop: &stop,
                deadline: Instant::now() + Duration::from_secs(2),
            };
            match read_postcard(&mut reader) {
                Ok(postcard) => {
                    frames.update_frame(postcard.jpeg.as_ref().to_vec(), postcard.metadata)
                }
                Err(error) => {
                    tracing::debug!(%error,"image publisher disconnected");
                    break;
                }
            }
        }
        frames.set_camera_state(CameraState::Degraded);
    }
    frames.set_camera_state(CameraState::Stopped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transmitter::{StreamEndpoint, TcpTransmitter, Transmitter};
    use photon_protocol::{FrameMetadata, ImagePostcard};
    use std::net::TcpListener;
    fn sample(index: u64) -> ImagePostcard {
        ImagePostcard {
            jpeg: Arc::from([255, 216, index as u8, 255, 217]),
            metadata: FrameMetadata {
                frame_index: index,
                sensor_ts_ns: 123 + index as i64,
                exposure_us: Some(59000),
                analogue_gain: Some(1.5),
                coi_ns: None,
                first_row_coi_ns: None,
                frame_duration_us: Some(60985),
                width: 1280,
                height: 720,
                pixel_format: "NV12".into(),
                captured_at_unix_ms: 12345,
            },
        }
    }
    fn wait_frame(store: &FrameStore, index: u64) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while store
            .latest_frame()
            .is_none_or(|f| f.metadata.frame_index != index)
        {
            assert!(Instant::now() < deadline, "frame not received");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn tcp_adapter_recovers_after_a_truncated_frame_and_shutdown_is_bounded() {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        socket.set_nonblocking(true).unwrap();
        let frames = Arc::new(FrameStore::new(CameraState::Stopped, 0, 0, "NV12"));
        let stop = Arc::new(AtomicBool::new(false));
        let (f, s) = (Arc::clone(&frames), Arc::clone(&stop));
        let receiver = std::thread::spawn(move || receive(PostcardListener::Tcp(socket), f, s));
        // A disconnected partial frame must not desynchronize the next connection.
        {
            use std::io::Write;
            let mut partial = std::net::TcpStream::connect(address).unwrap();
            partial.write_all(b"PHTN\x00").unwrap();
        }
        let expected = sample(7);
        let mut tx = TcpTransmitter::new(address);
        tx.transmit(&expected).unwrap();
        wait_frame(&frames, 7);
        assert_eq!(frames.fresh_frame().unwrap(), expected);
        stop.store(true, Ordering::Release);
        receiver.join().unwrap().unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn unix_adapter_preserves_postcards_and_cleans_its_socket() {
        let path = std::env::temp_dir().join(format!(
            "iris-view-{}-{}.sock",
            std::process::id(),
            sample(1).metadata.sensor_ts_ns
        ));
        let endpoint = StreamEndpoint::Unix(path.clone());
        let listener = PostcardListener::bind(&endpoint).unwrap();
        assert!(PostcardListener::bind(&endpoint).is_err());
        let frames = Arc::new(FrameStore::new(CameraState::Stopped, 0, 0, "NV12"));
        let stop = Arc::new(AtomicBool::new(false));
        let (f, s) = (Arc::clone(&frames), Arc::clone(&stop));
        let receiver = std::thread::spawn(move || receive(listener, f, s));
        let expected = sample(8);
        let mut transmitter = endpoint.transmitter();
        transmitter.transmit(&expected).unwrap();
        wait_frame(&frames, 8);
        assert_eq!(frames.fresh_frame().unwrap(), expected);
        stop.store(true, Ordering::Release);
        receiver.join().unwrap().unwrap();
        assert!(!path.exists());
    }
    #[tokio::test]
    async fn browser_control_forwards_to_udp_and_keeps_header_validation() {
        use crate::{camera::CameraConfig, control::CameraService, udp};
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let service = Arc::new(CameraService::new(CameraConfig::default()).unwrap());
        let server = tokio::spawn(udp::serve(socket, Arc::clone(&service)));
        let app = control_router(
            IrisClient::new(&format!("udp://{address}")).unwrap(),
            Arc::clone(&service.frames),
        );
        let pong = app
            .clone()
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(pong.status(), StatusCode::NO_CONTENT);
        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/camera/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let invalid = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/settings")
                    .header("X-Iris-Control", "1")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"gain":0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        let accepted = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/settings")
                    .header("X-Iris-Control", "1")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"gain":1.5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
        assert_eq!(service.status().unwrap().requested_settings.gain, Some(1.5));
        server.abort();
        let _ = server.await;
    }
}
