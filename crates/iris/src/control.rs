//! Serialized camera ownership for the daemon's local control API.
use crate::camera::{run_capture_with_settings, supervise_capture, CameraConfig};
use crate::frame_store::{CameraState, FrameStore};
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread::{self, JoinHandle};

pub use iris_api::{DaemonStatus, SettingsPatch};

type Capture = dyn Fn(CameraConfig, Arc<FrameStore>, Arc<AtomicBool>, Arc<RwLock<CameraConfig>>) -> Result<()>
    + Send
    + Sync;
struct Worker {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}
struct Controller {
    config: CameraConfig,
    worker: Option<Worker>,
}

pub struct CameraService {
    pub frames: Arc<FrameStore>,
    inner: Mutex<Controller>,
    live: Arc<RwLock<CameraConfig>>,
    capture: Arc<Capture>,
    shutting_down: AtomicBool,
}

impl CameraService {
    pub fn new(config: CameraConfig) -> Result<Self> {
        Self::with_capture(config, Arc::new(run_capture_with_settings))
    }

    fn with_capture(config: CameraConfig, capture: Arc<Capture>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            frames: Arc::new(FrameStore::new(
                CameraState::Stopped,
                config.width,
                config.height,
                "NV12",
            )),
            live: Arc::new(RwLock::new(config.clone())),
            inner: Mutex::new(Controller {
                config,
                worker: None,
            }),
            capture,
            shutting_down: AtomicBool::new(false),
        })
    }

    fn status_locked(&self, inner: &Controller) -> DaemonStatus {
        DaemonStatus {
            camera: self.frames.status(),
            requested_settings: inner.config.clone(),
            capture_active: inner
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.thread.is_finished()),
        }
    }

    pub fn status(&self) -> Result<DaemonStatus> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("camera controller lock poisoned"))?;
        Ok(self.status_locked(&inner))
    }

    /// Start is idempotent; a completed failed worker is reaped before reacquiring the camera.
    pub fn start(&self) -> Result<DaemonStatus> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("camera controller lock poisoned"))?;
        anyhow::ensure!(
            !self.shutting_down.load(Ordering::Acquire),
            "irisd is shutting down"
        );
        if inner
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.thread.is_finished())
        {
            return Ok(self.status_locked(&inner));
        }
        if let Some(old) = inner.worker.take() {
            old.thread
                .join()
                .map_err(|_| anyhow::anyhow!("previous camera worker panicked"))?;
        }
        self.frames.reset(
            CameraState::Starting,
            inner.config.width,
            inner.config.height,
        );
        let config = inner.config.clone();
        let frames = Arc::clone(&self.frames);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let settings = Arc::clone(&self.live);
        let capture = Arc::clone(&self.capture);
        let thread = thread::spawn(move || {
            supervise_capture(Arc::clone(&frames), || {
                capture(config, frames, worker_stop, settings)
            });
        });
        inner.worker = Some(Worker { stop, thread });
        Ok(self.status_locked(&inner))
    }

    /// Serialize stop/start and do not release ownership until libcamera has stopped.
    pub fn stop(&self) -> Result<DaemonStatus> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("camera controller lock poisoned"))?;
        self.frames.set_camera_state(CameraState::Stopping);
        if let Some(worker) = inner.worker.take() {
            worker.stop.store(true, Ordering::Release);
            worker
                .thread
                .join()
                .map_err(|_| anyhow::anyhow!("camera worker panicked during stop"))?;
        }
        self.frames.reset(
            CameraState::Stopped,
            inner.config.width,
            inner.config.height,
        );
        Ok(self.status_locked(&inner))
    }

    pub fn shutdown(&self) -> Result<DaemonStatus> {
        self.shutting_down.store(true, Ordering::Release);
        self.stop()
    }

    pub fn update(&self, patch: SettingsPatch) -> std::result::Result<DaemonStatus, ControlError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| ControlError::Internal("camera controller lock poisoned".into()))?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ControlError::Internal("irisd is shutting down".into()));
        }
        let next = patch
            .apply(&inner.config)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let running = inner
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.thread.is_finished());
        if running
            && (next.width != inner.config.width
                || next.height != inner.config.height
                || next.camera_index != inner.config.camera_index)
        {
            return Err(ControlError::Conflict(
                "stop capture before changing dimensions or camera index".into(),
            ));
        }
        *self
            .live
            .write()
            .map_err(|_| ControlError::Internal("settings lock poisoned".into()))? = next.clone();
        inner.config = next;
        Ok(self.status_locked(&inner))
    }
}

impl Drop for CameraService {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[derive(Debug)]
pub enum ControlError {
    Invalid(String),
    Conflict(String),
    Internal(String),
}
impl IntoResponse for ControlError {
    fn into_response(self) -> Response {
        let (code, error) = match self {
            Self::Invalid(error) => (StatusCode::BAD_REQUEST, error),
            Self::Conflict(error) => (StatusCode::CONFLICT, error),
            Self::Internal(error) => (StatusCode::SERVICE_UNAVAILABLE, error),
        };
        (code, Json(serde_json::json!({ "error": error }))).into_response()
    }
}

/// Local-only convenience router used by tests and development.
pub fn router(service: Arc<CameraService>) -> Router {
    router_with_token(service, None)
}

pub fn validate_token(token: &str) -> Result<()> {
    anyhow::ensure!(
        token.len() >= 32
            && token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte)),
        "control token must contain at least 32 ASCII letters, digits, '-' or '_'"
    );
    Ok(())
}

/// Non-loopback listeners require a token at daemon startup. Local OS users are
/// trusted; other peers must authenticate, including for status reads.
pub fn router_with_token(service: Arc<CameraService>, token: Option<String>) -> Router {
    Router::new()
        .route("/ping", get(|| async { StatusCode::NO_CONTENT }))
        .route("/status", get(status))
        .route("/camera/start", post(start))
        .route("/camera/stop", post(stop))
        .route("/settings", axum::routing::patch(settings))
        .layer(axum::middleware::from_fn_with_state(
            token,
            require_control_header,
        ))
        .with_state(service)
}

async fn require_control_header(
    State(token): State<Option<String>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let local = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .is_some_and(|peer| peer.0.ip().is_loopback());
    if let Some(token) = token.filter(|_| !local) {
        let expected = format!("Bearer {token}");
        if request
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .is_none_or(|value| value.as_bytes() != expected.as_bytes())
        {
            return (
                StatusCode::UNAUTHORIZED,
                [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
                Json(serde_json::json!({"error": "a valid control token is required"})),
            )
                .into_response();
        }
    }
    if request.method() != axum::http::Method::GET
        && request
            .headers()
            .get(iris_api::CONTROL_HEADER)
            .is_none_or(|value| value != "1")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "X-Iris-Control: 1 is required"})),
        )
            .into_response();
    }
    next.run(request).await
}

async fn blocking<T: Send + 'static>(
    action: impl FnOnce() -> Result<T> + Send + 'static,
) -> std::result::Result<T, ControlError> {
    tokio::task::spawn_blocking(action)
        .await
        .map_err(|error| ControlError::Internal(error.to_string()))?
        .map_err(|error| ControlError::Internal(format!("{error:#}")))
}
async fn status(
    State(service): State<Arc<CameraService>>,
) -> std::result::Result<Json<DaemonStatus>, ControlError> {
    blocking(move || service.status()).await.map(Json)
}
async fn start(
    State(service): State<Arc<CameraService>>,
) -> std::result::Result<Json<DaemonStatus>, ControlError> {
    blocking(move || service.start()).await.map(Json)
}
async fn stop(
    State(service): State<Arc<CameraService>>,
) -> std::result::Result<Json<DaemonStatus>, ControlError> {
    blocking(move || service.stop()).await.map(Json)
}
async fn settings(
    State(service): State<Arc<CameraService>>,
    Json(patch): Json<SettingsPatch>,
) -> std::result::Result<Json<DaemonStatus>, ControlError> {
    tokio::task::spawn_blocking(move || service.update(patch))
        .await
        .map_err(|error| ControlError::Internal(error.to_string()))?
        .map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !condition() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }

    #[test]
    fn lifecycle_serializes_ownership_and_live_settings_do_not_restart() {
        let starts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&starts);
        let observed_gain = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&observed_gain);
        let capture = Arc::new(
            move |_: CameraConfig,
                  _: Arc<FrameStore>,
                  stop: Arc<AtomicBool>,
                  live: Arc<RwLock<CameraConfig>>| {
                count.fetch_add(1, Ordering::SeqCst);
                while !stop.load(Ordering::Acquire) {
                    observed.store(
                        (live.read().unwrap().gain.unwrap_or(0.0) * 100.0) as usize,
                        Ordering::SeqCst,
                    );
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            },
        );
        let service =
            Arc::new(CameraService::with_capture(CameraConfig::default(), capture).unwrap());
        service.start().unwrap();
        service.start().unwrap();
        wait_until(|| starts.load(Ordering::SeqCst) == 1);
        service
            .update(SettingsPatch {
                gain: Some(1.5),
                ..Default::default()
            })
            .unwrap();
        wait_until(|| observed_gain.load(Ordering::SeqCst) == 150);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(matches!(
            service.update(SettingsPatch {
                width: Some(640),
                ..Default::default()
            }),
            Err(ControlError::Conflict(_))
        ));
        let stopped = service.stop().unwrap();
        assert_eq!(stopped.camera.camera_state, CameraState::Stopped);
        assert!(stopped.camera.latest_frame.is_none());
        assert!(service.frames.fresh_frame().is_none());
        service
            .update(SettingsPatch {
                width: Some(640),
                ..Default::default()
            })
            .unwrap();
        service.start().unwrap();
        wait_until(|| starts.load(Ordering::SeqCst) == 2);
        service.stop().unwrap();
        service.stop().unwrap();
    }

    #[test]
    fn invalid_patch_is_atomic_and_auto_modes_clear_manual_values() {
        let config = CameraConfig {
            exposure_us: Some(59000),
            gain: Some(1.5),
            ..Default::default()
        };
        let service = CameraService::new(config.clone()).unwrap();
        assert!(service
            .update(SettingsPatch {
                exposure_us: Some(-1),
                gain: Some(2.0),
                ..Default::default()
            })
            .is_err());
        assert_eq!(service.status().unwrap().requested_settings, config);
        assert!(service
            .update(SettingsPatch {
                gain: Some(f32::NAN),
                ..Default::default()
            })
            .is_err());
        assert!(service
            .update(SettingsPatch {
                fps: Some(0),
                ..Default::default()
            })
            .is_err());
        assert!(service
            .update(SettingsPatch {
                auto_gain: true,
                gain: Some(2.0),
                ..Default::default()
            })
            .is_err());
        let status = service
            .update(SettingsPatch {
                auto_exposure: true,
                auto_gain: true,
                ..Default::default()
            })
            .unwrap();
        assert!(status.requested_settings.gain.is_none());
        assert!(status.requested_settings.exposure_us.is_none());
    }

    #[test]
    fn failed_worker_can_be_started_again() {
        let starts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&starts);
        let capture = Arc::new(
            move |_: CameraConfig,
                  _: Arc<FrameStore>,
                  _: Arc<AtomicBool>,
                  _: Arc<RwLock<CameraConfig>>| {
                count.fetch_add(1, Ordering::SeqCst);
                bail!("injected camera failure");
            },
        );
        let service = CameraService::with_capture(CameraConfig::default(), capture).unwrap();
        service.start().unwrap();
        wait_until(|| !service.status().unwrap().capture_active);
        assert_eq!(
            service.status().unwrap().camera.camera_state,
            CameraState::Failed
        );
        service.start().unwrap();
        wait_until(|| {
            starts.load(Ordering::SeqCst) == 2 && !service.status().unwrap().capture_active
        });
        assert!(service
            .status()
            .unwrap()
            .camera
            .last_error
            .unwrap()
            .contains("injected camera failure"));
        service.stop().unwrap();
    }
    #[tokio::test]
    async fn control_api_requires_explicit_header_and_validates_settings() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let service = Arc::new(CameraService::new(CameraConfig::default()).unwrap());
        let app = router(Arc::clone(&service));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
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
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/camera/stop")
                    .header("X-Iris-Control", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        for (json, expected) in [
            (r#"{"gain":0}"#, StatusCode::BAD_REQUEST),
            (r#"{"gain":2.0}"#, StatusCode::OK),
            (r#"{"unknown":1}"#, StatusCode::UNPROCESSABLE_ENTITY),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PATCH")
                        .uri("/settings")
                        .header("X-Iris-Control", "1")
                        .header("Content-Type", "application/json")
                        .body(Body::from(json))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
        assert_eq!(service.status().unwrap().requested_settings.gain, Some(2.0));
    }
    #[test]
    fn shutdown_prevents_late_commands_from_reopening_camera() {
        let service = CameraService::new(CameraConfig::default()).unwrap();
        service.shutdown().unwrap();
        assert!(service.start().is_err());
        assert!(matches!(
            service.update(SettingsPatch {
                gain: Some(1.5),
                ..Default::default()
            }),
            Err(ControlError::Internal(_))
        ));
        assert_eq!(
            service.status().unwrap().camera.camera_state,
            CameraState::Stopped
        );
        assert!(SettingsPatch {
            gain: Some(f32::NAN),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
    #[tokio::test]
    async fn remote_commands_require_token_but_local_os_users_can_control_camera() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let token = "0123456789abcdef0123456789abcdef";
        validate_token(token).unwrap();
        assert!(validate_token("short").is_err());
        let service = Arc::new(CameraService::new(CameraConfig::default()).unwrap());
        let app = router_with_token(service, Some(token.into()));
        for auth in [None, Some("Bearer wrong")] {
            let mut request = Request::builder().uri("/status");
            if let Some(auth) = auth {
                request = request.header("Authorization", auth);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/status")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut request = Request::builder()
            .uri("/status")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
        ));
        assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::OK);
    }
    #[tokio::test]
    async fn ping_succeeds_without_starting_or_changing_capture() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let service = Arc::new(CameraService::new(CameraConfig::default()).unwrap());
        let response = router(Arc::clone(&service))
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(axum::body::to_bytes(response.into_body(), 1)
            .await
            .unwrap()
            .is_empty());
        let status = service.status().unwrap();
        assert_eq!(status.camera.camera_state, CameraState::Stopped);
        assert!(!status.capture_active);
        assert_eq!(status.camera.frame_count, 0);
        let response = router_with_token(service, Some("0123456789abcdef0123456789abcdef".into()))
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
