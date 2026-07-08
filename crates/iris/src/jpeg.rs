use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JpegError {
    InvalidDimensions,
    InvalidBufferLength,
    EncodeFailed,
}

pub fn encode_nv12_to_jpeg(
    width: u32,
    height: u32,
    nv12: &[u8],
    quality: u8,
) -> Result<Vec<u8>, JpegError> {
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
        return Err(JpegError::InvalidDimensions);
    }

    let width = usize::try_from(width).map_err(|_| JpegError::InvalidDimensions)?;
    let height = usize::try_from(height).map_err(|_| JpegError::InvalidDimensions)?;
    let y_len = width
        .checked_mul(height)
        .ok_or(JpegError::InvalidDimensions)?;
    let expected_len = y_len
        .checked_add(y_len / 2)
        .ok_or(JpegError::InvalidDimensions)?;
    if nv12.len() != expected_len {
        return Err(JpegError::InvalidBufferLength);
    }

    let mut rgb = vec![0_u8; y_len * 3];
    let uv_offset = y_len;
    for row in 0..height {
        let y_row = row * width;
        let uv_row = uv_offset + (row / 2) * width;
        for col in 0..width {
            let y = nv12[y_row + col] as i32;
            let uv_index = uv_row + (col / 2) * 2;
            let u = nv12[uv_index] as i32;
            let v = nv12[uv_index + 1] as i32;

            let c = y - 16;
            let d = u - 128;
            let e = v - 128;

            let r = (298 * c + 409 * e + 128) >> 8;
            let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
            let b = (298 * c + 516 * d + 128) >> 8;

            let rgb_index = (y_row + col) * 3;
            rgb[rgb_index] = clamp_to_u8(r);
            rgb[rgb_index + 1] = clamp_to_u8(g);
            rgb[rgb_index + 2] = clamp_to_u8(b);
        }
    }

    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, quality)
        .encode(&rgb, width as u32, height as u32, ExtendedColorType::Rgb8)
        .map_err(|_| JpegError::EncodeFailed)?;
    Ok(encoded)
}

fn clamp_to_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_nv12_into_decodable_jpeg() {
        let width = 2;
        let height = 2;
        let nv12 = vec![
            235, 235, // y plane
            235, 235, //
            128, 128, // uv plane
        ];

        let jpeg = encode_nv12_to_jpeg(width, height, &nv12, 90).expect("jpeg");
        let decoded = image::load_from_memory(&jpeg).expect("decode jpeg");

        assert_eq!(decoded.width(), width);
        assert_eq!(decoded.height(), height);
    }

    #[test]
    fn rejects_short_buffer() {
        let err = encode_nv12_to_jpeg(2, 2, &[235, 235, 235], 90).expect_err("buffer too short");
        assert_eq!(err, JpegError::InvalidBufferLength);
    }

    #[test]
    fn rejects_odd_dimensions() {
        let err = encode_nv12_to_jpeg(3, 2, &[0; 9], 90).expect_err("odd width");
        assert_eq!(err, JpegError::InvalidDimensions);
    }
}
