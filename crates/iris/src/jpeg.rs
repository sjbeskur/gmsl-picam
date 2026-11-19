use turbojpeg::{Compressor, Image, PixelFormat, Subsamp};

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

/// Reuse one encoder per capture worker. Scratch allocations grow only when the
/// image size grows; each returned JPEG owns its data independently of the next frame.
pub struct Nv12JpegEncoder {
    compressor: Compressor,
    nv12: Vec<u8>,
    rgb: Vec<u8>,
    output: Vec<u8>,
}

impl Nv12JpegEncoder {
    pub fn new() -> Result<Self, JpegError> {
        let mut compressor = Compressor::new().map_err(|_| JpegError::EncodeFailed)?;
        // Match the previous encoder's actual 1x1 component sampling factors.
        compressor
            .set_subsamp(Subsamp::None)
            .map_err(|_| JpegError::EncodeFailed)?;
        Ok(Self {
            compressor,
            nv12: Vec::new(),
            rgb: Vec::new(),
            output: Vec::new(),
        })
    }

    pub fn encode(&mut self, image: Nv12Image<'_>, quality: u8) -> Result<Vec<u8>, JpegError> {
        let started = std::time::Instant::now();
        let (width, height, _) = validate_nv12(image)?;
        let y_len = width * height;
        self.nv12.resize(y_len + y_len / 2, 0);
        let (y, uv) = self.nv12.split_at_mut(y_len);
        // Camera DMA mappings can be very slow for individual byte loads.
        // Read each plane sequentially into reusable cacheable memory before
        // the pixel loop. Preserve the negotiated row layout while copying.
        copy_plane(image.y, y, width, image.y_stride);
        copy_plane(image.uv, uv, width, image.uv_stride);
        let copied = std::time::Instant::now();
        nv12_to_rgb_into(
            Nv12Image {
                y,
                uv,
                y_stride: width,
                uv_stride: width,
                ..image
            },
            &mut self.rgb,
        )?;
        let converted = std::time::Instant::now();
        self.compressor
            .set_quality(i32::from(quality.clamp(1, 100)))
            .map_err(|_| JpegError::EncodeFailed)?;
        let width = image.width as usize;
        let height = image.height as usize;
        let capacity = self
            .compressor
            .buf_len(width, height)
            .map_err(|_| JpegError::InvalidDimensions)?;
        self.output.resize(capacity, 0);
        let len = self
            .compressor
            .compress_to_slice(
                Image {
                    pixels: self.rgb.as_slice(),
                    width,
                    height,
                    pitch: width * 3,
                    format: PixelFormat::RGB,
                },
                &mut self.output,
            )
            .map_err(|_| JpegError::EncodeFailed)?;
        let jpeg = self.output[..len].to_vec();
        tracing::debug!(
            copy_us = copied.duration_since(started).as_micros(),
            conversion_us = converted.duration_since(copied).as_micros(),
            compression_us = converted.elapsed().as_micros(),
            jpeg_bytes = len,
            "encoded camera frame"
        );
        Ok(jpeg)
    }
}

/// Convenience wrapper. Streaming callers should keep an `Nv12JpegEncoder`.
pub fn encode_nv12_planes_to_jpeg(image: Nv12Image<'_>, quality: u8) -> Result<Vec<u8>, JpegError> {
    Nv12JpegEncoder::new()?.encode(image, quality)
}

#[cfg(test)]
fn nv12_to_rgb(image: Nv12Image<'_>) -> Result<Vec<u8>, JpegError> {
    let mut rgb = Vec::new();
    nv12_to_rgb_into(image, &mut rgb)?;
    Ok(rgb)
}

fn validate_nv12(image: Nv12Image<'_>) -> Result<(usize, usize, usize), JpegError> {
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
    Ok((width, height, len))
}

fn copy_plane(source: &[u8], destination: &mut [u8], width: usize, stride: usize) {
    if stride == width {
        destination.copy_from_slice(&source[..destination.len()]);
    } else {
        for (row, dest) in destination.chunks_exact_mut(width).enumerate() {
            dest.copy_from_slice(&source[row * stride..][..width]);
        }
    }
}

fn nv12_to_rgb_into(image: Nv12Image<'_>, rgb: &mut Vec<u8>) -> Result<(), JpegError> {
    let (width, height, len) = validate_nv12(image)?;
    rgb.resize(len, 0);
    // Q16 coefficients preserve BT.601/709 and full/limited-range semantics.
    // Chroma is shared by a 2x2 block; compute its contribution just once.
    let (kr, kb) = match image.encoding {
        YuvEncoding::Bt601 => (0.299, 0.114),
        YuvEncoding::Bt709 => (0.2126, 0.0722),
    };
    let kg = 1.0 - kr - kb;
    let (y_scale, c_scale, y_offset) = if image.full_range {
        (1.0, 1.0, 0)
    } else {
        (255.0 / 219.0, 255.0 / 224.0, 16)
    };
    let fixed = |v: f64| (v * 65536.0).round() as i32;
    let cy = fixed(y_scale);
    let rv = fixed(2.0 * (1.0 - kr) * c_scale);
    let bu = fixed(2.0 * (1.0 - kb) * c_scale);
    let gu = fixed(-2.0 * kb * (1.0 - kb) / kg * c_scale);
    let gv = fixed(-2.0 * kr * (1.0 - kr) / kg * c_scale);
    let byte = |v: i32| ((v + 32768) >> 16).clamp(0, 255) as u8;
    for row in (0..height).step_by(2) {
        for col in (0..width).step_by(2) {
            let uv = (row / 2) * image.uv_stride + col;
            let u = i32::from(image.uv[uv]) - 128;
            let v = i32::from(image.uv[uv + 1]) - 128;
            let r = rv * v;
            let g = gu * u + gv * v;
            let b = bu * u;
            for dy in 0..2 {
                for dx in 0..2 {
                    let y = (i32::from(image.y[(row + dy) * image.y_stride + col + dx]) - y_offset)
                        * cy;
                    let out = ((row + dy) * width + col + dx) * 3;
                    rgb[out] = byte(y + r);
                    rgb[out + 1] = byte(y + g);
                    rgb[out + 2] = byte(y + b);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_matches_float_reference_across_color_ranges() {
        for encoding in [YuvEncoding::Bt601, YuvEncoding::Bt709] {
            let (kr, kb) = match encoding {
                YuvEncoding::Bt601 => (0.299, 0.114),
                YuvEncoding::Bt709 => (0.2126, 0.0722),
            };
            for full_range in [false, true] {
                for y in (0..=255).step_by(7) {
                    for u in (0..=255).step_by(17) {
                        for v in (0..=255).step_by(17) {
                            let luma = [y as u8; 4];
                            let chroma = [u as u8, v as u8];
                            let rgb = nv12_to_rgb(Nv12Image {
                                width: 2,
                                height: 2,
                                y: &luma,
                                uv: &chroma,
                                y_stride: 2,
                                uv_stride: 2,
                                encoding,
                                full_range,
                            })
                            .unwrap();
                            let (y, u, v) =
                                (f64::from(y), f64::from(u) - 128.0, f64::from(v) - 128.0);
                            let (y, u, v) = if full_range {
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
                            for (actual, expected) in rgb
                                .as_chunks::<3>()
                                .0
                                .iter()
                                .flat_map(|p| p.iter().zip([r, g, b]))
                            {
                                let expected = expected.round().clamp(0.0, 255.0) as u8;
                                assert!(
                                    actual.abs_diff(expected) <= 1,
                                    "{encoding:?} full={full_range}: {actual} vs {expected}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn encoder_preserves_outputs_across_quality_and_size_changes() {
        let mut encoder = Nv12JpegEncoder::new().unwrap();
        let image = Nv12Image {
            width: 2,
            height: 2,
            y: &[63; 4],
            uv: &[102, 240],
            y_stride: 2,
            uv_stride: 2,
            encoding: YuvEncoding::Bt709,
            full_range: false,
        };
        let first = encoder.encode(image, 100).unwrap();
        let saved = first.clone();
        let larger = Nv12Image {
            width: 16,
            height: 16,
            y: &[235; 256],
            uv: &[128; 128],
            y_stride: 16,
            uv_stride: 16,
            ..image
        };
        let second = encoder.encode(larger, 40).unwrap();
        assert_eq!(first, saved);
        let red = image::load_from_memory(&first).unwrap().to_rgb8();
        assert!(red.pixels().all(|p| p[0] >= 250 && p[1] <= 4 && p[2] <= 4));
        let white = image::load_from_memory(&second).unwrap().to_rgb8();
        assert_eq!(white.dimensions(), (16, 16));
        assert!(white.pixels().all(|p| p.0.iter().all(|v| *v >= 250)));
        assert_eq!(encoder.encode(image, 100).unwrap(), first);
    }

    #[test]
    fn padded_planes_encode_identically_to_packed_planes() {
        let y = [
            16, 64, 128, 235, 81, 63, 128, 90, 50, 100, 150, 200, 0, 255, 235, 16,
        ];
        let uv = [90, 240, 128, 128, 240, 90, 102, 240];
        let mut padded_y = [255; 22];
        let mut padded_uv = [255; 10];
        for row in 0..4 {
            padded_y[row * 6..row * 6 + 4].copy_from_slice(&y[row * 4..row * 4 + 4]);
        }
        for row in 0..2 {
            padded_uv[row * 6..row * 6 + 4].copy_from_slice(&uv[row * 4..row * 4 + 4]);
        }
        let mut encoder = Nv12JpegEncoder::new().unwrap();
        for encoding in [YuvEncoding::Bt601, YuvEncoding::Bt709] {
            for full_range in [false, true] {
                let packed = Nv12Image {
                    width: 4,
                    height: 4,
                    y: &y,
                    uv: &uv,
                    y_stride: 4,
                    uv_stride: 4,
                    encoding,
                    full_range,
                };
                let padded = Nv12Image {
                    y: &padded_y,
                    uv: &padded_uv,
                    y_stride: 6,
                    uv_stride: 6,
                    ..packed
                };
                assert_eq!(encoder.encode(packed, 85), encoder.encode(padded, 85));
            }
        }
    }

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
