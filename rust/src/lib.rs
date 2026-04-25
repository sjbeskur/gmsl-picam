/// Center-of-integration timestamp (nanoseconds, same clock as SensorTimestamp).
///
/// `sensor_ts_ns` — `controls::SensorTimestamp` from the completed request.
///                  This is the V4L2 kernel timestamp at **end of last-row readout**.
/// `exposure_us`  — `controls::ExposureTime` in microseconds.
///
/// Returns the timestamp corresponding to the photon-collection midpoint of the
/// **centre row** of the frame. Suitable for IMU synchronisation when sub-frame
/// row-level precision is not required.
///
/// # Rolling-shutter per-row correction
///
/// The IMX477 is a rolling-shutter sensor: each row starts its exposure at a
/// slightly different absolute time. To get the CoI for a specific row:
///
/// ```text
/// readout_ns       = frame_duration_ns - exposure_ns
/// row_offset_ns    = row * readout_ns / frame_height
/// coi_for_row_ns   = sensor_ts_ns
///                    - exposure_ns / 2      // mid-exposure
///                    - readout_ns           // sensor_ts is end-of-last-row
///                    + row_offset_ns        // advance to this row
/// ```
///
/// At 1280×720/30fps with 8 ms exposure, readout ≈ 25 ms, so the top/bottom
/// row CoI values differ by ~25 ms — large enough to matter for a 1 kHz IMU.
pub fn center_of_integration_ns(sensor_ts_ns: i64, exposure_us: i32) -> i64 {
    sensor_ts_ns - (exposure_us as i64 * 1_000 / 2)
}

/// Per-row center-of-integration for rolling-shutter correction.
///
/// `sensor_ts_ns`      — end-of-last-row readout timestamp (SensorTimestamp).
/// `exposure_us`       — ExposureTime in microseconds.
/// `frame_duration_us` — FrameDuration in microseconds (1_000_000 / fps).
/// `row`               — row index (0 = top).
/// `frame_height`      — total frame height in pixels.
pub fn center_of_integration_row_ns(
    sensor_ts_ns: i64,
    exposure_us: i32,
    frame_duration_us: i64,
    row: u32,
    frame_height: u32,
) -> i64 {
    let exposure_ns = exposure_us as i64 * 1_000;
    let frame_duration_ns = frame_duration_us * 1_000;
    let readout_ns = frame_duration_ns - exposure_ns;
    let row_offset_ns = row as i64 * readout_ns / frame_height as i64;
    sensor_ts_ns - exposure_ns / 2 - readout_ns + row_offset_ns
}
