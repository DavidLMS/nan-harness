use crate::report::Reason;
use xa11y::Screenshot;

pub(crate) fn prepare_ocr_image(image: Screenshot) -> Result<Screenshot, Reason> {
    super::process::validate_image(&image)?;
    // Small 1x UI glyphs lose strokes during OCR thresholding. Interpolate in
    // memory, preserving the exact coordinate-to-window scale and helper limits.
    if image.scale >= 2.0
        || image.width > 4096
        || image.height > 4096
        || u64::from(image.width) * u64::from(image.height) > 4 * 1024 * 1024
    {
        return Ok(image);
    }
    let width = image.width as usize;
    let height = image.height as usize;
    let mut horizontal = vec![0; width * 2 * height * 4];
    for y in 0..height {
        for x in 0..width * 2 {
            let (left, right, weight) = neighbors(x, width);
            for channel in 0..4 {
                horizontal[(y * width * 2 + x) * 4 + channel] = interpolate(
                    image.pixels[(y * width + left) * 4 + channel],
                    image.pixels[(y * width + right) * 4 + channel],
                    weight,
                );
            }
        }
    }
    let stride = width * 2 * 4;
    let mut pixels = vec![0; stride * height * 2];
    for y in 0..height * 2 {
        let (top, bottom, weight) = neighbors(y, height);
        for x in 0..stride {
            pixels[y * stride + x] = interpolate(
                horizontal[top * stride + x],
                horizontal[bottom * stride + x],
                weight,
            );
        }
    }
    Ok(Screenshot {
        width: image.width * 2,
        height: image.height * 2,
        scale: image.scale * 2.0,
        pixels,
    })
}

fn neighbors(index: usize, size: usize) -> (usize, usize, u16) {
    (
        index.saturating_sub(1) / 2,
        index.div_ceil(2).min(size - 1),
        if index.is_multiple_of(2) { 3 } else { 1 },
    )
}

fn interpolate(first: u8, second: u8, second_weight: u16) -> u8 {
    u8::try_from(
        (u16::from(first) * (4 - second_weight) + u16::from(second) * second_weight + 2) / 4,
    )
    .expect("a weighted average of bytes is a byte")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_pixels_interpolate_with_clamped_edges_and_matching_scale() {
        let image = prepare_ocr_image(Screenshot {
            width: 2,
            height: 1,
            scale: 1.0,
            pixels: vec![0, 0, 0, 255, 100, 100, 100, 255],
        })
        .unwrap();
        assert_eq!((image.width, image.height, image.scale), (4, 2, 2.0));
        let row = [0, 25, 75, 100]
            .into_iter()
            .flat_map(|v| [v, v, v, 255])
            .collect::<Vec<_>>();
        assert_eq!(image.pixels, row.repeat(2));
    }

    #[test]
    fn native_retina_pixels_and_invalid_inputs_are_not_rescaled() {
        let pixels = vec![25; 16];
        let image = prepare_ocr_image(Screenshot {
            width: 2,
            height: 2,
            scale: 2.0,
            pixels: pixels.clone(),
        })
        .unwrap();
        assert_eq!(image.pixels, pixels);
        assert_eq!((image.width, image.height, image.scale), (2, 2, 2.0));
        assert!(
            prepare_ocr_image(Screenshot {
                width: 0,
                height: 2,
                scale: 1.0,
                pixels: vec![],
            })
            .is_err()
        );
    }

    #[test]
    fn interpolation_keeps_the_native_pixel_budget() {
        let pixels = vec![255; 4097 * 4];
        let image = prepare_ocr_image(Screenshot {
            width: 4097,
            height: 1,
            scale: 1.0,
            pixels: pixels.clone(),
        })
        .unwrap();
        assert_eq!(image.width, 4097);
        assert_eq!(image.scale.to_bits(), 1.0_f32.to_bits());
        assert_eq!(image.pixels, pixels);
        let image = prepare_ocr_image(Screenshot {
            width: 1,
            height: 2,
            scale: 1.0,
            pixels: vec![0, 0, 0, 255, 100, 100, 100, 255],
        })
        .unwrap();
        let expected = [0, 25, 75, 100]
            .into_iter()
            .flat_map(|v| [v, v, v, 255].repeat(2))
            .collect::<Vec<_>>();
        assert_eq!(image.pixels, expected);
    }
}
