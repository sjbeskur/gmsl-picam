//! Shared wire types and validation. This crate has no camera, server, or CLI dependencies.
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub mod wire;
pub use wire::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraSettings {
    pub camera_index: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub exposure_us: Option<i32>,
    pub gain: Option<f32>,
    pub jpeg_quality: u8,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            camera_index: 0,
            width: 1_280,
            height: 720,
            fps: 30,
            exposure_us: None,
            gain: None,
            jpeg_quality: 85,
        }
    }
}

impl CameraSettings {
    pub fn frame_duration_us(&self) -> i64 {
        let fps = self.fps.max(1) as i64;
        1_000_000 / fps
    }
    /// A requested fixed frame period must accommodate manual exposure.
    pub fn effective_frame_duration_us(&self) -> i64 {
        self.frame_duration_us().max(
            self.exposure_us
                .map_or(0, |exposure| i64::from(exposure) + 2000),
        )
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.width > 0
                && self.height > 0
                && self.width.is_multiple_of(2)
                && self.height.is_multiple_of(2),
            "NV12 width and height must be positive even numbers"
        );
        anyhow::ensure!(
            (1..=240).contains(&self.fps),
            "fps must be between 1 and 240"
        );
        anyhow::ensure!(
            self.exposure_us.is_none_or(|value| value > 0),
            "exposure must be positive"
        );
        anyhow::ensure!(
            self.gain
                .is_none_or(|value| value.is_finite() && value > 0.0),
            "gain must be finite and positive"
        );
        anyhow::ensure!(
            (1..=100).contains(&self.jpeg_quality),
            "JPEG quality must be between 1 and 100"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraState {
    Stopped,
    Stopping,
    Starting,
    Running,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsPatch {
    pub exposure_us: Option<i32>,
    pub gain: Option<f32>,
    pub fps: Option<u32>,
    pub jpeg_quality: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub camera_index: Option<usize>,
    pub auto_exposure: bool,
    pub auto_gain: bool,
}

impl SettingsPatch {
    pub fn validate(&self) -> Result<()> {
        self.apply(&CameraSettings::default()).map(|_| ())
    }

    pub fn apply(&self, original: &CameraSettings) -> Result<CameraSettings> {
        anyhow::ensure!(
            !(self.auto_exposure && self.exposure_us.is_some()),
            "choose exposure-us or auto-exposure"
        );
        anyhow::ensure!(
            !(self.auto_gain && self.gain.is_some()),
            "choose gain or auto-gain"
        );
        let mut next = original.clone();
        if let Some(value) = self.exposure_us {
            next.exposure_us = Some(value);
        }
        if let Some(value) = self.gain {
            next.gain = Some(value);
        }
        if self.auto_exposure {
            next.exposure_us = None;
        }
        if self.auto_gain {
            next.gain = None;
        }
        if let Some(value) = self.fps {
            next.fps = value;
        }
        if let Some(value) = self.jpeg_quality {
            next.jpeg_quality = value;
        }
        if let Some(value) = self.width {
            next.width = value;
        }
        if let Some(value) = self.height {
            next.height = value;
        }
        if let Some(value) = self.camera_index {
            next.camera_index = value;
        }
        next.validate()?;
        Ok(next)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub camera: ServiceStatus,
    /// Requested values; actual exposure/gain are in camera.latest_frame.
    pub requested_settings: CameraSettings,
    pub capture_active: bool,
}
