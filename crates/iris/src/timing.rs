#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingError {
    InvalidFrameHeight,
    InvalidRow,
    InvalidDuration,
    Overflow,
}

impl std::fmt::Display for TimingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid sensor timing: {self:?}")
    }
}
impl std::error::Error for TimingError {}

/// Midpoint of the first active row's exposure, in libcamera CLOCK_BOOTTIME ns.
pub fn center_of_integration_ns(sensor_ts_ns: i64, exposure_us: i32) -> Result<i64, TimingError> {
    if sensor_ts_ns < 0 || exposure_us <= 0 {
        return Err(TimingError::InvalidDuration);
    }
    sensor_ts_ns
        .checked_add(i64::from(exposure_us) * 500)
        .ok_or(TimingError::Overflow)
}

/// Midpoint of the active sensor readout. `readout_duration_us` must come from
/// sensor mode/line timing, not the frame period (which includes blanking).
/// Exposure and readout duration are independent; either may be longer.
pub fn center_of_integration_frame_center_ns(
    sensor_ts_ns: i64,
    exposure_us: i32,
    readout_duration_us: i64,
) -> Result<i64, TimingError> {
    offset_coi(sensor_ts_ns, exposure_us, readout_duration_us, 1, 2)
}

/// Row position refers to the active sensor array, not a resized output image.
pub fn center_of_integration_row_ns(
    sensor_ts_ns: i64,
    exposure_us: i32,
    readout_duration_us: i64,
    row: u32,
    frame_height: u32,
) -> Result<i64, TimingError> {
    if frame_height == 0 {
        return Err(TimingError::InvalidFrameHeight);
    }
    if row >= frame_height {
        return Err(TimingError::InvalidRow);
    }
    offset_coi(
        sensor_ts_ns,
        exposure_us,
        readout_duration_us,
        row,
        frame_height,
    )
}

fn offset_coi(
    ts: i64,
    exposure_us: i32,
    readout_us: i64,
    row: u32,
    height: u32,
) -> Result<i64, TimingError> {
    if readout_us < 0 {
        return Err(TimingError::InvalidDuration);
    }
    let offset = i128::from(readout_us) * 1000 * i128::from(row) / i128::from(height);
    let result = i128::from(center_of_integration_ns(ts, exposure_us)?) + offset;
    result.try_into().map_err(|_| TimingError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exposure_can_exceed_active_readout_time() {
        assert_eq!(
            center_of_integration_frame_center_ns(1_000_000_000, 59_000, 10_000),
            Ok(1_034_500_000)
        );
        assert_eq!(
            center_of_integration_row_ns(1_000_000_000, 59_000, 10_000, 0, 720),
            Ok(1_029_500_000)
        );
        assert_eq!(
            center_of_integration_row_ns(1_000_000_000, 59_000, 10_000, 719, 720),
            Ok(1_039_486_111)
        );
    }
    #[test]
    fn rejects_invalid_timing_and_overflow() {
        assert_eq!(
            center_of_integration_ns(0, -1),
            Err(TimingError::InvalidDuration)
        );
        assert_eq!(
            center_of_integration_frame_center_ns(0, 8000, -1),
            Err(TimingError::InvalidDuration)
        );
        assert_eq!(
            center_of_integration_ns(i64::MAX, 8000),
            Err(TimingError::Overflow)
        );
        assert_eq!(
            center_of_integration_frame_center_ns(0, 8000, i64::MAX),
            Err(TimingError::Overflow)
        );
        assert_eq!(
            center_of_integration_row_ns(0, 8000, 10000, 0, 0),
            Err(TimingError::InvalidFrameHeight)
        );
        assert_eq!(
            center_of_integration_row_ns(0, 8000, 10000, 720, 720),
            Err(TimingError::InvalidRow)
        );
    }
}
