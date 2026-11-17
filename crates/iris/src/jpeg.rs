use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JpegError {
    InvalidDimensions,
    InvalidBufferLength,
    EncodeFailed,
}

#[derive(Debug, Clone, Copy)]
pub enum YuvEncoding {
    Bt601,
    Bt709,
}

#[derive(Debug, Clone, Copy)]
pub struct Nv12Image<'a> {
    pub width: u32,
    pub height: u32,
    pub y: &'a [u8],
    pub uv: &'a [u8],
    pub y_stride: usize,
    pub uv_stride: usize,
    pub encoding: YuvEncoding,
    pub full_range: bool,
}

/// Compatibility entry point for tightly packed, limited-range BT.601 NV12.
pub fn encode_nv12_to_jpeg(
    width: u32,
    height: u32,
    nv12: &[u8],
    quality: u8,
) -> Result<Vec<u8>, JpegError> {
    let y_len = (width as usize)
        .checked_mul(height as usize)
        .ok_or(JpegError::InvalidDimensions)?;
    let y = nv12.get(..y_len).ok_or(JpegError::InvalidBufferLength)?;
    let uv = nv12.get(y_len..).ok_or(JpegError::InvalidBufferLength)?;
    encode_nv12_planes_to_jpeg(
        Nv12Image {
            width,
            height,
            y,
            uv,
            y_stride: width as usize,
            uv_stride: width as usize,
            encoding: YuvEncoding::Bt601,
            full_range: false,
        },
        quality,
    )
}

pub fn encode_nv12_planes_to_jpeg(image: Nv12Image<'_>, quality: u8) -> Result<Vec<u8>, JpegError> {
    let rgb = nv12_to_rgb(image)?;
    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, quality)
        .encode(&rgb, image.width, image.height, ExtendedColorType::Rgb8)
        .map_err(|_| JpegError::EncodeFailed)?;
    Ok(encoded)
}

fn nv12_to_rgb(image: Nv12Image<'_>) -> Result<Vec<u8>, JpegError> {
    let (width, height) = (image.width as usize, image.height as usize);
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(JpegError::InvalidDimensions);
    }
    fn required_len(width: usize, rows: usize, stride: usize) -> Option<usize> {
        if stride < width {
            return None;
        }
        (rows - 1).checked_mul(stride)?.checked_add(width)
    }
    let y_len =
        required_len(width, height, image.y_stride).ok_or(JpegError::InvalidBufferLength)?;
    let uv_len =
        required_len(width, height / 2, image.uv_stride).ok_or(JpegError::InvalidBufferLength)?;
    if image.y.len() < y_len || image.uv.len() < uv_len {
        return Err(JpegError::InvalidBufferLength);
    }
    let len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3))
        .ok_or(JpegError::InvalidDimensions)?;
    let mut rgb = vec![0; len];
    let (kr, kb) = match image.encoding {
        YuvEncoding::Bt601 => (0.299, 0.114),
        YuvEncoding::Bt709 => (0.2126, 0.0722),
    };
    for row in 0..height {
        for col in 0..width {
            let y = f64::from(image.y[row * image.y_stride + col]);
            let uv = (row / 2) * image.uv_stride + (col / 2) * 2;
            let u = f64::from(image.uv[uv]) - 128.0;
            let v = f64::from(image.uv[uv + 1]) - 128.0;
            let (y, u, v) = if image.full_range {
                (y, u, v)
            } else {
                (
                    (y - 16.0) * 255.0 / 219.0,
                    u * 255.0 / 224.0,
                    v * 255.0 / 224.0,
                )
            };
            let r = y + 2.0 * (1.0 - kr) * v;
            let b = y + 2.0 * (1.0 - kb) * u;
            let g = (y - kr * r - kb * b) / (1.0 - kr - kb);
            let out = (row * width + col) * 3;
            for (dst, value) in rgb[out..out + 3].iter_mut().zip([r, g, b]) {
                *dst = value.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(rgb)
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
    #[test]
    fn ignores_padding_and_checks_plane_bounds() {
        let mut image = Nv12Image {
            width: 2,
            height: 2,
            y: &[235, 235, 0, 0, 235, 235],
            uv: &[128, 128],
            y_stride: 4,
            uv_stride: 4,
            encoding: YuvEncoding::Bt709,
            full_range: false,
        };
        assert_eq!(nv12_to_rgb(image).unwrap(), vec![255; 12]);
        image.y = &[235; 5];
        assert_eq!(nv12_to_rgb(image), Err(JpegError::InvalidBufferLength));
        image.y = &[235; 8];
        image.uv = &[128];
        assert_eq!(nv12_to_rgb(image), Err(JpegError::InvalidBufferLength));
        image.uv = &[128; 2];
        image.y_stride = 1;
        assert_eq!(nv12_to_rgb(image), Err(JpegError::InvalidBufferLength));
    }

    #[test]
    fn converts_known_red_and_full_range_black() {
        for (encoding, y, u, v) in [
            (YuvEncoding::Bt709, 63, 102, 240),
            (YuvEncoding::Bt601, 81, 90, 240),
        ] {
            let y = [y; 4];
            let uv = [u, v];
            let image = Nv12Image {
                width: 2,
                height: 2,
                y: &y,
                uv: &uv,
                y_stride: 2,
                uv_stride: 2,
                encoding,
                full_range: false,
            };
            let rgb = nv12_to_rgb(image).unwrap();
            assert!(rgb[0] >= 253 && rgb[1] <= 2 && rgb[2] <= 2, "{rgb:?}");
        }
        let image = Nv12Image {
            width: 2,
            height: 2,
            y: &[0; 4],
            uv: &[128; 2],
            y_stride: 2,
            uv_stride: 2,
            encoding: YuvEncoding::Bt709,
            full_range: true,
        };
        assert_eq!(nv12_to_rgb(image).unwrap(), vec![0; 12]);
    }
}
