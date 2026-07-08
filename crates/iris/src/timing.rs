#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingError {
    InvalidFrameHeight,
    InvalidRow,
    ExposureLongerThanFrame,
}

/// Center-of-integration timestamp for the first image row.
///
/// `sensor_ts_ns` is libcamera `SensorTimestamp`, defined as the time when the
/// first row of the image sensor active array is exposed. This compatibility
/// helper returns the midpoint of that first row's exposure.
pub fn center_of_integration_ns(sensor_ts_ns: i64, exposure_us: i32) -> i64 {
    sensor_ts_ns + (exposure_us as i64 * 1_000 / 2)
}

pub fn center_of_integration_frame_center_ns(
    sensor_ts_ns: i64,
    exposure_us: i32,
    frame_duration_us: i64,
) -> Result<i64, TimingError> {
    let exposure_ns = exposure_us as i64 * 1_000;
    let frame_duration_ns = frame_duration_us * 1_000;
    ensure_valid_duration(exposure_ns, frame_duration_ns)?;

    Ok(sensor_ts_ns + exposure_ns / 2 + frame_duration_ns / 2)
}

pub fn center_of_integration_row_ns(
    sensor_ts_ns: i64,
    exposure_us: i32,
    frame_duration_us: i64,
    row: u32,
    frame_height: u32,
) -> Result<i64, TimingError> {
    if frame_height == 0 {
        return Err(TimingError::InvalidFrameHeight);
    }
    if row >= frame_height {
        return Err(TimingError::InvalidRow);
    }

    let exposure_ns = exposure_us as i64 * 1_000;
    let frame_duration_ns = frame_duration_us * 1_000;
    ensure_valid_duration(exposure_ns, frame_duration_ns)?;
    let row_offset_ns = row as i64 * frame_duration_ns / frame_height as i64;

    Ok(sensor_ts_ns + exposure_ns / 2 + row_offset_ns)
}

fn ensure_valid_duration(exposure_ns: i64, frame_duration_ns: i64) -> Result<(), TimingError> {
    if exposure_ns > frame_duration_ns {
        return Err(TimingError::ExposureLongerThanFrame);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_center_adds_half_exposure_and_half_frame_duration() {
        let coi = center_of_integration_frame_center_ns(1_000_000_000, 8_000, 33_333)
            .expect("valid timing");
        assert_eq!(coi, 1_020_666_500);
    }

    #[test]
    fn row_zero_is_first_row_center_of_integration() {
        let coi = center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 0, 720)
            .expect("valid timing");
        assert_eq!(coi, 1_004_000_000);
    }

    #[test]
    fn bottom_row_is_later_than_top_row() {
        let top = center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 0, 720)
            .expect("valid timing");
        let bottom = center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 719, 720)
            .expect("valid timing");
        assert_eq!(bottom, 1_037_286_704);
        assert!(bottom > top);
    }

    #[test]
    fn rejects_zero_frame_height() {
        assert_eq!(
            center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 0, 0),
            Err(TimingError::InvalidFrameHeight)
        );
    }

    #[test]
    fn rejects_row_at_frame_height() {
        assert_eq!(
            center_of_integration_row_ns(1_000_000_000, 8_000, 33_333, 720, 720),
            Err(TimingError::InvalidRow)
        );
    }

    #[test]
    fn rejects_exposure_longer_than_frame() {
        assert_eq!(
            center_of_integration_frame_center_ns(1_000_000_000, 40_000, 33_333),
            Err(TimingError::ExposureLongerThanFrame)
        );
    }

    #[test]
    fn compatibility_helper_returns_first_row_center_of_integration() {
        assert_eq!(
            center_of_integration_ns(1_000_000_000, 8_000),
            1_004_000_000
        );
    }
}
