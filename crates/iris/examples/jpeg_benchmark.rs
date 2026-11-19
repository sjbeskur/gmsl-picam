//! Compare the previous floating-point/image-rs pipeline with the live encoder.
//! Run a release build on the target device; this does not access the camera.
use gmsl_picam_rs::jpeg::{JpegError, Nv12Image, Nv12JpegEncoder, YuvEncoding};
use image::{codecs::jpeg::JpegEncoder, ExtendedColorType};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn main() {
    let (width, height) = (1280, 720);
    // Textured, deterministic NV12, including limited-range luma/chroma.
    let y: Vec<u8> = (0..width * height)
        .map(|i| (16 + (i * 13 + i / width * 7) % 220) as u8)
        .collect();
    let uv: Vec<u8> = (0..width * height / 2)
        .map(|i| (96 + (i * 3 + i / width) % 64) as u8)
        .collect();
    let image = Nv12Image {
        width: width as u32,
        height: height as u32,
        y: &y,
        uv: &uv,
        y_stride: width,
        uv_stride: width,
        encoding: YuvEncoding::Bt709,
        full_range: false,
    };
    let iterations = std::env::args()
        .nth(1)
        .map(|n| n.parse::<u32>().expect("iteration count"))
        .unwrap_or(30)
        .max(1);
    let mut conversion = Duration::ZERO;
    let mut compression = Duration::ZERO;
    for _ in 0..iterations {
        let start = Instant::now();
        let rgb = reference_rgb(black_box(image)).unwrap();
        conversion += start.elapsed();
        let start = Instant::now();
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 85)
            .encode(&rgb, width as u32, height as u32, ExtendedColorType::Rgb8)
            .unwrap();
        black_box(jpeg);
        compression += start.elapsed();
    }
    let mut encoder = Nv12JpegEncoder::new().unwrap();
    encoder.encode(image, 85).unwrap(); // allocate scratch before timing
    let start = Instant::now();
    let mut bytes = 0;
    for _ in 0..iterations {
        bytes = black_box(encoder.encode(black_box(image), 85).unwrap()).len();
    }
    let optimized = start.elapsed();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0 / f64::from(iterations);
    println!("1280x720 BT.709 limited, JPEG quality 85, {iterations} iterations");
    println!(
        "previous conversion: {:.3} ms, JPEG: {:.3} ms, total: {:.3} ms",
        ms(conversion),
        ms(compression),
        ms(conversion + compression)
    );
    println!(
        "optimized total: {:.3} ms, speedup: {:.2}x, JPEG bytes: {bytes}",
        ms(optimized),
        ms(conversion + compression) / ms(optimized)
    );
}

fn reference_rgb(image: Nv12Image<'_>) -> Result<Vec<u8>, JpegError> {
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
