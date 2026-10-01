//! Diagnostic-only exact-source icon candidates. Never authorizes input.
//!
//! Cairo alpha references have not been calibrated against GPUI native pixels.
//! A cluster is a candidate, even when unique/new/stable. A future controller
//! must separately prove the error via native Copy Error clipboard output.
use crate::report::Reason;
use serde::Serialize;
use std::{
    io::Read as _,
    path::Path,
    time::{Duration, Instant},
};
use xa11y::Screenshot;
use zeroize::{Zeroize as _, Zeroizing};

const MAX_PIXELS: usize = 4_194_304;
const MAX_MATCHES: usize = 32;

pub(super) struct PrivateIconFrame(Screenshot);

impl PrivateIconFrame {
    pub(super) fn scale(&self) -> f32 {
        self.0.scale
    }

    pub(super) fn new(screenshot: Screenshot) -> Self {
        Self(screenshot)
    }
}

impl Drop for PrivateIconFrame {
    fn drop(&mut self) {
        self.0.pixels.zeroize();
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IconDiagnostics {
    pub(super) diagnostics_only: bool,
    pub(super) template_side: u32,
    pub(super) retry_matches: usize,
    pub(super) copy_matches: usize,
    pub(super) close_matches: usize,
    pub(super) baseline_clusters: usize,
    pub(super) first_clusters: usize,
    pub(super) second_clusters: usize,
    pub(super) new_stable_clusters: usize,
}

impl IconDiagnostics {
    pub(super) fn unsupported() -> Self {
        Self {
            diagnostics_only: true,
            template_side: 0,
            retry_matches: 0,
            copy_matches: 0,
            close_matches: 0,
            baseline_clusters: 0,
            first_clusters: 0,
            second_clusters: 0,
            new_stable_clusters: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Position {
    x: u32,
    y: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cluster {
    retry: Position,
    copy: Position,
    close: Position,
}

pub(super) struct Templates {
    side: u32,
    retry: Vec<u8>,
    copy: Vec<u8>,
    close: Vec<u8>,
}

impl Templates {
    pub(super) fn load(directory: &Path, scale: f32) -> Result<Self, Reason> {
        let side = supported_side(scale)?;
        if !directory.is_absolute() {
            return Err(Reason::IsolationUnavailable);
        }
        let load = |name| {
            let path = directory.join(format!("{name}-{side}.alpha"));
            let metadata =
                std::fs::symlink_metadata(&path).map_err(|_| Reason::ActionUnsupported)?;
            if !metadata.is_file() || metadata.len() != u64::from(side * side) {
                return Err(Reason::ActionUnsupported);
            }
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|_| Reason::ActionUnsupported)?
                .take(u64::from(side * side) + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Reason::ActionUnsupported)?;
            if bytes.len() != (side * side) as usize {
                return Err(Reason::ActionUnsupported);
            }
            Ok(bytes)
        };
        Ok(Self {
            side,
            retry: load("rotate_cw")?,
            copy: load("copy")?,
            close: load("close")?,
        })
    }
}

fn supported_side(scale: f32) -> Result<u32, Reason> {
    if scale.to_bits() == 1.0_f32.to_bits() {
        Ok(14)
    } else if scale.to_bits() == 2.0_f32.to_bits() {
        Ok(28)
    } else {
        Err(Reason::ActionUnsupported)
    }
}

fn validate(frame: &Screenshot) -> Result<(), Reason> {
    let pixels = usize::try_from(u64::from(frame.width) * u64::from(frame.height))
        .map_err(|_| Reason::ActionUnsupported)?;
    if pixels > MAX_PIXELS {
        return Err(Reason::BudgetExceeded);
    }
    if pixels == 0 || pixels.checked_mul(4) != Some(frame.pixels.len()) {
        return Err(Reason::ActionUnsupported);
    }
    supported_side(frame.scale).map(|_| ())
}

struct LumaFrame {
    width: u32,
    height: u32,
    pixels: Zeroizing<Vec<f64>>,
}

impl LumaFrame {
    fn new(frame: &Screenshot) -> Self {
        let pixels = frame
            .pixels
            .chunks_exact(4)
            .map(|pixel| (f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2])) / 3.0)
            .collect();
        Self {
            width: frame.width,
            height: frame.height,
            pixels: Zeroizing::new(pixels),
        }
    }
}

fn gray(frame: &LumaFrame, x: u32, y: u32) -> f64 {
    frame.pixels[y as usize * frame.width as usize + x as usize]
}

fn correlation(frame: &LumaFrame, mask: &[u8], side: u32, at: Position) -> f64 {
    let mut sum = [0.0; 5];
    for y in 0..side {
        for x in 0..side {
            let alpha = f64::from(mask[(y * side + x) as usize]);
            let pixel = gray(frame, at.x + x, at.y + y);
            sum[0] += alpha;
            sum[1] += pixel;
            sum[2] += alpha * alpha;
            sum[3] += pixel * pixel;
            sum[4] += alpha * pixel;
        }
    }
    let count = f64::from(side * side);
    let denominator =
        ((count * sum[2] - sum[0] * sum[0]) * (count * sum[3] - sum[1] * sum[1])).sqrt();
    if denominator > 0.0 {
        ((count * sum[4] - sum[0] * sum[1]) / denominator).abs()
    } else {
        0.0
    }
}

fn find(
    frame: &LumaFrame,
    mask: &[u8],
    side: u32,
    deadline: Instant,
) -> Result<Vec<Position>, Reason> {
    if frame.width < side || frame.height < side {
        return Ok(Vec::new());
    }
    let max_alpha = mask.iter().copied().max().unwrap_or(0);
    if max_alpha < 16 {
        return Err(Reason::ActionUnsupported);
    }
    let minimum_alpha = max_alpha - max_alpha / 10;
    let foreground: Vec<_> = mask
        .iter()
        .enumerate()
        .filter(|(_, alpha)| **alpha >= minimum_alpha)
        .map(|(i, _)| i)
        .collect();
    if foreground.len() < 4 {
        return Err(Reason::ActionUnsupported);
    }
    let probes = [
        foreground[0],
        foreground[foreground.len() / 3],
        foreground[foreground.len() * 2 / 3],
        foreground[foreground.len() - 1],
    ];
    let probes = probes.map(u32::try_from);
    let probes = [
        probes[0].map_err(|_| Reason::ActionUnsupported)?,
        probes[1].map_err(|_| Reason::ActionUnsupported)?,
        probes[2].map_err(|_| Reason::ActionUnsupported)?,
        probes[3].map_err(|_| Reason::ActionUnsupported)?,
    ];
    let mut matches = Vec::new();
    for y in 0..=frame.height - side {
        if Instant::now() >= deadline {
            return Err(Reason::BudgetExceeded);
        }
        for x in 0..=frame.width - side {
            let at = Position { x, y };
            let background = gray(frame, x, y).midpoint(gray(frame, x + side - 1, y + side - 1));
            if (gray(frame, x + probes[0] % side, y + probes[0] / side) - background).abs() < 24.0 {
                continue;
            }
            let values = probes.map(|i| gray(frame, x + (i % side), y + (i / side)));
            let mean = values.iter().sum::<f64>() / 4.0;
            if (mean - background).abs() < 24.0 || values.iter().any(|v| (v - mean).abs() > 24.0) {
                continue;
            }
            if correlation(frame, mask, side, at) >= 0.985 {
                matches.push(at);
                if matches.len() > MAX_MATCHES {
                    return Err(Reason::SelectorNotMatched);
                }
            }
        }
    }
    Ok(matches)
}

fn clusters(
    retry: &[Position],
    copy: &[Position],
    close: &[Position],
    side: u32,
) -> Result<Vec<Cluster>, Reason> {
    let same_row = |a: Position, b: Position| a.y.abs_diff(b.y) <= side / 7;
    let follows = |a: Position, b: Position| b.x > a.x && (side..=side * 3).contains(&(b.x - a.x));
    let mut result = Vec::new();
    for &r in retry {
        for &c in copy {
            for &d in close {
                if same_row(r, c)
                    && same_row(c, d)
                    && follows(r, c)
                    && follows(c, d)
                    && (c.x - r.x).abs_diff(d.x - c.x) <= side / 4
                {
                    if result.len() >= MAX_MATCHES {
                        return Err(Reason::SelectorNotMatched);
                    }
                    result.push(Cluster {
                        retry: r,
                        copy: c,
                        close: d,
                    });
                }
            }
        }
    }
    Ok(result)
}

struct FrameCandidates {
    retry: Vec<Position>,
    copy: Vec<Position>,
    close: Vec<Position>,
    grouped: Vec<Cluster>,
}

fn frame_candidates(
    frame: &Screenshot,
    masks: &Templates,
    deadline: Instant,
) -> Result<FrameCandidates, Reason> {
    let luma = LumaFrame::new(frame);
    let retry = find(&luma, &masks.retry, masks.side, deadline)?;
    let copy = find(&luma, &masks.copy, masks.side, deadline)?;
    let close = find(&luma, &masks.close, masks.side, deadline)?;
    let grouped = clusters(&retry, &copy, &close, masks.side)?;
    Ok(FrameCandidates {
        retry,
        copy,
        close,
        grouped,
    })
}

pub(super) fn observe(
    masks: &Templates,
    baseline: &PrivateIconFrame,
    first: &PrivateIconFrame,
    second: &PrivateIconFrame,
) -> Result<IconDiagnostics, Reason> {
    for frame in [&baseline.0, &first.0, &second.0] {
        validate(frame)?;
        if supported_side(frame.scale)? != masks.side {
            return Err(Reason::WindowChanged);
        }
    }
    if baseline.0.width != first.0.width
        || baseline.0.height != first.0.height
        || first.0.width != second.0.width
        || first.0.height != second.0.height
        || baseline.0.scale.to_bits() != first.0.scale.to_bits()
        || first.0.scale.to_bits() != second.0.scale.to_bits()
    {
        return Err(Reason::WindowChanged);
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    let before = frame_candidates(&baseline.0, masks, deadline)?;
    let one = frame_candidates(&first.0, masks, deadline)?;
    let two = frame_candidates(&second.0, masks, deadline)?;
    let stable = one
        .grouped
        .iter()
        .filter(|candidate| two.grouped.contains(candidate) && !before.grouped.contains(candidate))
        .count();
    Ok(IconDiagnostics {
        diagnostics_only: true,
        template_side: masks.side,
        retry_matches: one.retry.len(),
        copy_matches: one.copy.len(),
        close_matches: one.close.len(),
        baseline_clusters: before.grouped.len(),
        first_clusters: one.grouped.len(),
        second_clusters: two.grouped.len(),
        new_stable_clusters: stable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_masks() -> Templates {
        let mut retry = vec![0; 196];
        let mut copy = vec![0; 196];
        let mut close = vec![0; 196];
        for y in 3..11 {
            for x in 3..11 {
                if x == 3 || y == 3 {
                    retry[y * 14 + x] = 255;
                }
                if x == 3 || x == 10 || y == 3 || y == 10 {
                    copy[y * 14 + x] = 255;
                }
                if x == y || x + y == 13 {
                    close[y * 14 + x] = 255;
                }
            }
        }
        Templates {
            side: 14,
            retry,
            copy,
            close,
        }
    }

    fn frame(masks: &Templates, rows: &[u32]) -> PrivateIconFrame {
        let mut screenshot = Screenshot {
            width: 120,
            height: 80,
            pixels: vec![20; 120 * 80 * 4],
            scale: 1.0,
        };
        for &y in rows {
            for (x, mask) in [(10, &masks.retry), (34, &masks.copy), (58, &masks.close)] {
                for row in 0..14 {
                    for col in 0..14 {
                        let index = ((y + row) * 120 + x + col) as usize * 4;
                        let value = 20 + mask[(row * 14 + col) as usize] / 2;
                        screenshot.pixels[index..index + 3].fill(value);
                        screenshot.pixels[index + 3] = 255;
                    }
                }
            }
        }
        PrivateIconFrame::new(screenshot)
    }

    #[test]
    fn diagnostic_distinguishes_new_stable_existing_ambiguous_and_moving_clusters() {
        let masks = synthetic_masks();
        let empty = frame(&masks, &[]);
        let one = frame(&masks, &[10]);
        let observed = observe(&masks, &empty, &one, &one).unwrap();
        assert!(observed.diagnostics_only);
        assert_eq!(observed.new_stable_clusters, 1);
        assert_eq!(
            observe(&masks, &one, &one, &one)
                .unwrap()
                .new_stable_clusters,
            0
        );
        let two = frame(&masks, &[10, 40]);
        assert_eq!(
            observe(&masks, &empty, &two, &two)
                .unwrap()
                .new_stable_clusters,
            2
        );
        let moved = frame(&masks, &[11]);
        assert_eq!(
            observe(&masks, &empty, &one, &moved)
                .unwrap()
                .new_stable_clusters,
            0
        );
    }

    #[test]
    fn antialiased_reference_without_opaque_pixels_still_matches() {
        let mut masks = synthetic_masks();
        for mask in [&mut masks.retry, &mut masks.copy, &mut masks.close] {
            for (index, alpha) in mask.iter_mut().enumerate().filter(|(_, alpha)| **alpha > 0) {
                *alpha = [238, 220, 214][index % 3];
            }
        }
        let empty = frame(&masks, &[]);
        let one = frame(&masks, &[10]);
        assert_eq!(
            observe(&masks, &empty, &one, &one)
                .unwrap()
                .new_stable_clusters,
            1
        );
        masks.retry.fill(0);
        assert!(observe(&masks, &empty, &one, &one).is_err());
    }

    #[test]
    fn pixel_and_deadline_limits_are_distinct_from_malformed_layout() {
        let masks = synthetic_masks();
        let mut image = frame(&masks, &[]);
        image.0.width = u32::try_from(MAX_PIXELS + 1).unwrap();
        image.0.height = 1;
        assert_eq!(validate(&image.0), Err(Reason::BudgetExceeded));
        image.0.width = 120;
        image.0.height = 80;
        image.0.pixels.pop();
        assert_eq!(validate(&image.0), Err(Reason::ActionUnsupported));
        let image = frame(&masks, &[]);
        let luma = LumaFrame::new(&image.0);
        assert_eq!(
            find(&luma, &masks.retry, 14, Instant::now()),
            Err(Reason::BudgetExceeded)
        );
    }

    #[test]
    fn ambiguous_cluster_combinations_stop_at_the_closed_budget() {
        let retry = [Position { x: 0, y: 0 }];
        let copy: Vec<_> = (14..=42).map(|x| Position { x, y: 0 }).collect();
        let close: Vec<_> = (28..=84).map(|x| Position { x, y: 0 }).collect();
        assert_eq!(
            clusters(&retry, &copy, &close, 14),
            Err(Reason::SelectorNotMatched)
        );
    }

    #[test]
    fn diagnostic_rejects_untrusted_geometry_scale_and_plain_pixels() {
        let masks = synthetic_masks();
        let empty = frame(&masks, &[]);
        assert_eq!(
            observe(&masks, &empty, &empty, &empty)
                .unwrap()
                .new_stable_clusters,
            0
        );
        let mut wrong = frame(&masks, &[]);
        wrong.0.scale = 1.5;
        assert!(observe(&masks, &empty, &wrong, &empty).is_err());
        wrong.0.scale = 2.0;
        assert!(observe(&masks, &empty, &wrong, &empty).is_err());
        wrong.0.pixels.pop();
        assert!(validate(&wrong.0).is_err());
    }
}
