//! Serialized camera ownership for the daemon's local control API.
use crate::camera::{run_capture_with_settings, supervise_capture, CameraConfig};
use crate::frame_store::{CameraState, FrameStore};
use anyhow::Result;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread::{self, JoinHandle};

pub use photon_protocol::{DaemonStatus, SettingsPatch};

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

    pub(crate) fn with_capture(config: CameraConfig, capture: Arc<Capture>) -> Result<Self> {
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
}
