use std::io::Cursor;

use crate::ocr::ImageInput;

const DARK_PIXEL_THRESHOLD: u8 = 200;
const MIN_DARK_PIXELS: u64 = 24;
const DESKEW_LIMIT_DEGREES: i32 = 5;
const DESKEW_APPLY_THRESHOLD_DEGREES: f32 = 0.75;

pub(crate) fn preprocess_image(image: ImageInput) -> ImageInput {
    preprocess_image_impl(image)
}

#[cfg(feature = "pdf-input")]
fn preprocess_image_impl(image: ImageInput) -> ImageInput {
    use image::ImageFormat as EncodedImageFormat;

    let decoded = match image::load_from_memory(&image.bytes) {
        Ok(decoded) => decoded,
        Err(_) => return image,
    };
    let mut gray = decoded.to_luma8();
    if dark_pixel_count(&gray) < MIN_DARK_PIXELS {
        return image;
    }

    gray = best_right_angle_orientation(&gray);
    let deskew_degrees = estimate_deskew_degrees(&gray);
    if deskew_degrees.abs() >= DESKEW_APPLY_THRESHOLD_DEGREES {
        gray = rotate_luma(&gray, deskew_degrees);
    }

    let mut bytes = Vec::new();
    let mut cursor = Cursor::new(&mut bytes);
    if image::DynamicImage::ImageLuma8(gray)
        .write_to(&mut cursor, EncodedImageFormat::Png)
        .is_err()
    {
        return image;
    }

    ImageInput {
        bytes,
        format: crate::ocr::ImageFormat::Png,
        dpi: image.dpi,
    }
}

#[cfg(not(feature = "pdf-input"))]
fn preprocess_image_impl(image: ImageInput) -> ImageInput {
    image
}

#[cfg(feature = "pdf-input")]
fn best_right_angle_orientation(image: &image::GrayImage) -> image::GrayImage {
    let candidates = [
        image.clone(),
        image::imageops::rotate90(image),
        image::imageops::rotate180(image),
        image::imageops::rotate270(image),
    ];

    let mut best = image.clone();
    let mut best_score = horizontal_projection_score(&best);
    for candidate in candidates.into_iter().skip(1) {
        let score = horizontal_projection_score(&candidate);
        if score > best_score {
            best = candidate;
            best_score = score;
        }
    }
    best
}

#[cfg(feature = "pdf-input")]
fn estimate_deskew_degrees(image: &image::GrayImage) -> f32 {
    let mut best_degrees = 0;
    let mut best_score = horizontal_projection_score(image);
    for degrees in -DESKEW_LIMIT_DEGREES..=DESKEW_LIMIT_DEGREES {
        if degrees == 0 {
            continue;
        }
        let candidate = rotate_luma(image, degrees as f32);
        let score = horizontal_projection_score(&candidate);
        if score > best_score {
            best_score = score;
            best_degrees = degrees;
        }
    }
    best_degrees as f32
}

#[cfg(feature = "pdf-input")]
fn horizontal_projection_score(image: &image::GrayImage) -> u64 {
    let mut score = 0u64;
    for y in 0..image.height() {
        let mut dark = 0u64;
        for x in 0..image.width() {
            if image.get_pixel(x, y).0[0] < DARK_PIXEL_THRESHOLD {
                dark += 1;
            }
        }
        score = score.saturating_add(dark.saturating_mul(dark));
    }
    score
}

#[cfg(feature = "pdf-input")]
fn dark_pixel_count(image: &image::GrayImage) -> u64 {
    image
        .pixels()
        .filter(|pixel| pixel.0[0] < DARK_PIXEL_THRESHOLD)
        .count() as u64
}

#[cfg(feature = "pdf-input")]
fn rotate_luma(image: &image::GrayImage, degrees: f32) -> image::GrayImage {
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let width = image.width();
    let height = image.height();
    let center_x = (width as f32 - 1.0) / 2.0;
    let center_y = (height as f32 - 1.0) / 2.0;
    let mut out = image::GrayImage::from_pixel(width, height, image::Luma([255]));

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center_x;
            let dy = y as f32 - center_y;
            let src_x = cos.mul_add(dx, sin * dy) + center_x;
            let src_y = (-sin).mul_add(dx, cos * dy) + center_y;
            if src_x >= 0.0 && src_y >= 0.0 && src_x < width as f32 && src_y < height as f32 {
                let src_x = src_x.round() as u32;
                let src_y = src_y.round() as u32;
                if src_x < width && src_y < height {
                    out.put_pixel(x, y, *image.get_pixel(src_x, src_y));
                }
            }
        }
    }

    out
}

#[cfg(all(test, feature = "pdf-input"))]
mod tests {
    use image::{GrayImage, ImageFormat as EncodedImageFormat, Luma};

    use crate::ocr::ImageFormat;

    use super::*;

    fn png_bytes(image: &GrayImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), EncodedImageFormat::Png)
            .expect("encode png");
        bytes
    }

    fn horizontal_rule_image() -> GrayImage {
        let mut image = GrayImage::from_pixel(120, 80, Luma([255]));
        for y in 38..42 {
            for x in 16..104 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        image
    }

    #[test]
    fn preprocessing_rotates_sideways_payload_before_ocr() {
        let sideways = image::imageops::rotate90(&horizontal_rule_image());
        let preprocessed = preprocess_image(ImageInput {
            bytes: png_bytes(&sideways),
            format: ImageFormat::Png,
            dpi: None,
        });
        let decoded = image::load_from_memory(&preprocessed.bytes)
            .expect("preprocessed image decodes")
            .to_luma8();

        assert!(horizontal_projection_score(&decoded) > horizontal_projection_score(&sideways));
    }

    #[test]
    fn preprocessing_deskews_small_angle_payload() {
        let skewed = rotate_luma(&horizontal_rule_image(), 4.0);
        let preprocessed = preprocess_image(ImageInput {
            bytes: png_bytes(&skewed),
            format: ImageFormat::Png,
            dpi: None,
        });
        let decoded = image::load_from_memory(&preprocessed.bytes)
            .expect("preprocessed image decodes")
            .to_luma8();

        assert!(horizontal_projection_score(&decoded) > horizontal_projection_score(&skewed));
    }
}
