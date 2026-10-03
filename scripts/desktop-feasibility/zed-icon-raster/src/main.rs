//! Offline pinned GPUI SVG alpha rasterization; never reads a desktop image.
//! Source contract: Zed 76659a55a8c10ed355a070f8764a0b1733e3c115,
//! gpui/src/window.rs paint_svg; gpui_wgpu/src/shaders.wgsl fs_mono_sprite;
//! gpui/src/platform.rs get_gamma_correction_ratios. Integer-aligned opaque
//! One Dark toolbar icons only; this does not model arbitrary themes/transforms.
use std::io::{Read, Write};

/// Pinned One Dark default icon RGB (#dce0e5), before shader correction.
const ICON_RGB: [f32; 3] = [220.0 / 255.0, 224.0 / 255.0, 229.0 / 255.0];

fn corrected_alpha(alpha: f32, rgb: [f32; 3]) -> f32 {
    // GPUI shader defaults: gamma 1.8, grayscale enhanced contrast 1.0.
    let brightness = rgb[0] * 0.30 + rgb[1] * 0.59 + rgb[2] * 0.11;
    let contrast = (4.0 * (0.75 - brightness)).clamp(0.0, 1.0);
    let a = alpha * (contrast + 1.0) / (alpha * contrast + 1.0);
    let ratios = [
        0.1469 * 65536.0 / (255.0 * 255.0),
        -0.8911 * 256.0 / 255.0,
        1.4644 * 65536.0 / (255.0 * 255.0),
        -0.3234 * 256.0 / 255.0,
    ];
    let correction = (ratios[0] * brightness + ratios[1]) * a + ratios[2] * brightness + ratios[3];
    (a + a * (1.0 - a) * correction).clamp(0.0, 1.0)
}

fn sampled_mask(alpha: &[u8], side: usize, rgb: [f32; 3]) -> Result<Vec<u8>, &'static str> {
    if !matches!(side, 14 | 28) || alpha.len() != side * side * 4 {
        return Err("invalid raster dimensions");
    }
    let width = side * 2;
    let mut result = Vec::with_capacity(side * side);
    for y in 0..side {
        for x in 0..side {
            // An integer-aligned destination pixel centre maps to the midpoint
            // of four 2x atlas texels, as GPUI's linear texture sampler does.
            let at = y * 2 * width + x * 2;
            let average = (f32::from(alpha[at])
                + f32::from(alpha[at + 1])
                + f32::from(alpha[at + width])
                + f32::from(alpha[at + width + 1]))
                / (4.0 * 255.0);
            result.push((corrected_alpha(average, rgb) * 255.0).round() as u8);
        }
    }
    Ok(result)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("fixed raster side and palette required".into());
    }
    let side: usize = args[0].parse()?;
    if !matches!(side, 14 | 28) {
        return Err("unsupported raster side".into());
    }
    let rgb = match args[1].as_str() {
        "default" => ICON_RGB,
        "selected" => [116.0 / 255.0, 173.0 / 255.0, 232.0 / 255.0],
        _ => return Err("unsupported source palette".into()),
    };
    let mut bytes = Vec::new();
    std::io::stdin().take(16385).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > 16384 {
        return Err("bounded SVG required".into());
    }
    let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default())?;
    let mut image = tiny_skia::Pixmap::new((side * 2) as u32, (side * 2) as u32)
        .ok_or("invalid raster allocation")?;
    let transform = tiny_skia::Transform::from_scale(
        side as f32 * 2.0 / tree.size().width(),
        side as f32 * 2.0 / tree.size().height(),
    );
    resvg::render(&tree, transform, &mut image.as_mut());
    let alpha: Vec<_> = image.pixels().iter().map(|pixel| pixel.alpha()).collect();
    std::io::stdout().write_all(&sampled_mask(&alpha, side, rgb)?)?;
    Ok(())
}

fn main() {
    if run().is_err() {
        eprintln!("reference rasterization failed");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_sampling_uses_each_texel_and_shader_preserves_endpoints() {
        let mut alpha = vec![0; 28 * 28];
        alpha[0] = 255;
        alpha[1] = 255;
        let mask = sampled_mask(&alpha, 14, ICON_RGB).unwrap();
        assert_eq!(
            mask[0],
            (corrected_alpha(0.5, ICON_RGB) * 255.0).round() as u8
        );
        assert_eq!(mask[1], 0);
        assert_eq!(corrected_alpha(0.0, ICON_RGB), 0.0);
        assert_eq!(corrected_alpha(1.0, ICON_RGB), 1.0);
        assert!(corrected_alpha(0.5, ICON_RGB) > 0.5);
    }

    #[test]
    fn source_rasterizer_and_palette_correction_preserve_synthetic_edges() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14"><rect x="0" y="0" width="7" height="14" fill="white"/></svg>"#;
        let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).unwrap();
        let mut image = tiny_skia::Pixmap::new(28, 28).unwrap();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(2.0, 2.0),
            &mut image.as_mut(),
        );
        let alpha: Vec<_> = image.pixels().iter().map(|pixel| pixel.alpha()).collect();
        let mask = sampled_mask(&alpha, 14, ICON_RGB).unwrap();
        assert_eq!(mask.len(), 196);
        for row in mask.chunks_exact(14) {
            assert!(row[..7].iter().all(|value| *value == 255));
            assert!(row[7..].iter().all(|value| *value == 0));
        }
        let selected = [116.0 / 255.0, 173.0 / 255.0, 232.0 / 255.0];
        assert_ne!(
            corrected_alpha(0.5, selected),
            corrected_alpha(0.5, ICON_RGB)
        );
    }

    #[test]
    fn exact_scale_dimensions_are_required_and_correction_is_monotone() {
        assert!(sampled_mask(&[0; 8], 14, ICON_RGB).is_err());
        assert!(sampled_mask(&vec![0; 24 * 24], 12, ICON_RGB).is_err());
        let mut previous = 0.0;
        for value in 0..=255 {
            let current = corrected_alpha(value as f32 / 255.0, ICON_RGB);
            assert!(current >= previous && current <= 1.0);
            previous = current;
        }
    }
}
