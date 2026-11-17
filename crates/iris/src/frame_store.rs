use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub const MAX_FRAME_AGE: Duration = Duration::from_secs(2);

#[cfg_attr(feature = "http-support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraState {
    Starting,
    Running,
    Degraded,
    Failed,
}

#[cfg_attr(feature = "http-support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct FrameMetadata {
    pub frame_index: u64,
    pub sensor_ts_ns: i64,
    pub exposure_us: Option<i32>,
    pub analogue_gain: Option<f32>,
    pub coi_ns: Option<i64>,
    pub first_row_coi_ns: Option<i64>,
    pub frame_duration_us: Option<i64>,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub captured_at_unix_ms: u64,
}

#[cfg_attr(feature = "http-support", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceStatus {
    pub camera_state: CameraState,
    pub frame_count: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub latest_frame: Option<FrameMetadata>,
    pub last_error: Option<String>,
    pub frame_age_ms: Option<u64>,
    pub frame_stale: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LatestFrame {
    pub jpeg: Arc<[u8]>,
    pub metadata: FrameMetadata,
}

#[derive(Debug)]
pub struct FrameStore {
    inner: RwLock<FrameStoreState>,
}

#[derive(Debug)]
struct FrameStoreState {
    status: ServiceStatus,
    latest_frame: Option<LatestFrame>,
    received_at: Option<Instant>,
}

impl FrameStore {
    pub fn new(
        camera_state: CameraState,
        width: u32,
        height: u32,
        pixel_format: impl Into<String>,
    ) -> Self {
        let pixel_format = pixel_format.into();
        Self {
            inner: RwLock::new(FrameStoreState {
                status: ServiceStatus {
                    camera_state,
                    frame_count: 0,
                    width,
                    height,
                    pixel_format,
                    latest_frame: None,
                    last_error: None,
                    frame_age_ms: None,
                    frame_stale: true,
                },
                latest_frame: None,
                received_at: None,
            }),
        }
    }

    pub fn status(&self) -> ServiceStatus {
        let inner = self.inner.read().expect("frame store lock poisoned");
        let mut status = inner.status.clone();
        let age = inner.received_at.map(|at| at.elapsed());
        status.frame_age_ms = age.map(|age| age.as_millis().try_into().unwrap_or(u64::MAX));
        status.frame_stale = status.camera_state != CameraState::Running
            || age.is_none_or(|age| age > MAX_FRAME_AGE);
        if status.camera_state == CameraState::Running && status.frame_stale {
            status.camera_state = CameraState::Degraded;
            status
                .last_error
                .get_or_insert_with(|| "No fresh camera frame".into());
        }
        status
    }

    /// Return a frame only while capture is healthy and its monotonic age is bounded.
    pub fn fresh_frame(&self) -> Option<LatestFrame> {
        let inner = self.inner.read().expect("frame store lock poisoned");
        if inner.status.camera_state != CameraState::Running
            || inner.received_at?.elapsed() > MAX_FRAME_AGE
        {
            return None;
        }
        inner.latest_frame.clone()
    }

    pub fn latest_frame(&self) -> Option<LatestFrame> {
        self.inner
            .read()
            .expect("frame store lock poisoned")
            .latest_frame
            .clone()
    }

    pub fn update_frame(&self, jpeg: impl Into<Vec<u8>>, metadata: FrameMetadata) {
        let mut inner = self.inner.write().expect("frame store lock poisoned");
        inner.received_at = Some(Instant::now());
        inner.status.camera_state = CameraState::Running;
        inner.status.frame_count += 1;
        inner.status.width = metadata.width;
        inner.status.height = metadata.height;
        inner.status.pixel_format = metadata.pixel_format.clone();
        inner.status.latest_frame = Some(metadata.clone());
        inner.status.last_error = None;
        inner.latest_frame = Some(LatestFrame {
            jpeg: Arc::from(jpeg.into()),
            metadata,
        });
    }

    pub fn set_camera_state(&self, camera_state: CameraState) {
        self.inner
            .write()
            .expect("frame store lock poisoned")
            .status
            .camera_state = camera_state;
    }

    pub fn set_last_error(&self, last_error: impl Into<String>) {
        self.inner
            .write()
            .expect("frame store lock poisoned")
            .status
            .last_error = Some(last_error.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata(frame_index: u64) -> FrameMetadata {
        FrameMetadata {
            frame_index,
            sensor_ts_ns: 1_000_000_000,
            exposure_us: Some(8_000),
            analogue_gain: Some(1.5),
            coi_ns: None,
            first_row_coi_ns: Some(1_004_000_000),
            frame_duration_us: Some(33_333),
            width: 1_280,
            height: 720,
            pixel_format: "NV12".to_string(),
            captured_at_unix_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn empty_store_reports_initial_status() {
        let store = FrameStore::new(CameraState::Starting, 1_280, 720, "NV12");

        let status = store.status();
        assert_eq!(status.camera_state, CameraState::Starting);
        assert_eq!(status.frame_count, 0);
        assert_eq!(status.width, 1_280);
        assert_eq!(status.height, 720);
        assert_eq!(status.pixel_format, "NV12");
        assert!(status.latest_frame.is_none());
        assert!(status.last_error.is_none());
        assert!(store.latest_frame().is_none());
    }

    #[test]
    fn update_frame_stores_latest_frame_and_status() {
        let store = FrameStore::new(CameraState::Starting, 1_280, 720, "NV12");
        let jpeg = vec![1, 2, 3, 4];
        let metadata = sample_metadata(7);

        store.update_frame(jpeg.clone(), metadata.clone());

        let status = store.status();
        assert_eq!(status.frame_count, 1);
        assert_eq!(status.camera_state, CameraState::Running);
        assert_eq!(status.width, 1_280);
        assert_eq!(status.height, 720);
        assert_eq!(status.pixel_format, "NV12");
        assert_eq!(status.latest_frame, Some(metadata.clone()));
        assert!(status.last_error.is_none());

        let latest = store.latest_frame().expect("latest frame");
        assert_eq!(&*latest.jpeg, jpeg.as_slice());
        assert_eq!(latest.metadata, metadata);
    }

    #[test]
    fn update_frame_replaces_previous_frame() {
        let store = FrameStore::new(CameraState::Starting, 1_280, 720, "NV12");

        store.update_frame(vec![1, 2, 3], sample_metadata(1));
        store.update_frame(vec![9, 8, 7, 6], sample_metadata(2));

        let status = store.status();
        assert_eq!(status.frame_count, 2);
        assert_eq!(status.latest_frame.as_ref().map(|m| m.frame_index), Some(2));

        let latest = store.latest_frame().expect("latest frame");
        assert_eq!(&*latest.jpeg, &[9, 8, 7, 6]);
        assert_eq!(latest.metadata.frame_index, 2);
    }

    #[test]
    fn set_last_error_is_reflected_in_status() {
        let store = FrameStore::new(CameraState::Running, 1_280, 720, "NV12");

        store.set_last_error("jpeg encode failed");

        let status = store.status();
        assert_eq!(status.last_error.as_deref(), Some("jpeg encode failed"));
    }

    #[test]
    fn set_camera_state_updates_status() {
        let store = FrameStore::new(CameraState::Starting, 1_280, 720, "NV12");

        store.set_camera_state(CameraState::Failed);

        assert_eq!(store.status().camera_state, CameraState::Failed);
    }
    #[test]
    fn failure_and_monotonic_expiry_hide_previous_frames() {
        let store = FrameStore::new(CameraState::Starting, 1280, 720, "NV12");
        store.update_frame(vec![1], sample_metadata(0));
        assert!(store.fresh_frame().is_some());
        store.set_camera_state(CameraState::Failed);
        assert!(store.fresh_frame().is_none());
        assert!(store.status().frame_stale);
        store.update_frame(vec![2], sample_metadata(1));
        store.inner.write().unwrap().received_at =
            Some(Instant::now() - MAX_FRAME_AGE - Duration::from_millis(1));
        assert!(store.fresh_frame().is_none());
        assert_eq!(store.status().camera_state, CameraState::Degraded);
        assert!(store.status().frame_age_ms.unwrap() > 2000);
    }
}
