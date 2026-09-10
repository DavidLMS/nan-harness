use crate::report::Reason;
use num_traits::ToPrimitive as _;
use xa11y::Rect;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Window {
    pub(crate) id: u64,
    pub(crate) pid: u32,
    pub(crate) bounds: Rect,
    pub(crate) name: String,
    /// CoreGraphics window level (`kCGWindowLayer`), 0 when the platform does
    /// not report a level. Used only by the transient occlusion diagnostic.
    pub(crate) layer: i32,
}

pub(crate) struct Snapshot {
    foreground_pid: u32,
    foreground_window: u64,
    displays: Vec<Rect>,
    pub(crate) windows: Vec<Window>,
}

impl Snapshot {
    pub(super) fn parse(text: &str) -> Result<Self, Reason> {
        let mut lines = text.lines();
        let fields = lines
            .next()
            .ok_or(Reason::DesktopUnavailable)?
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 3 || fields[0] != "FG" {
            return Err(Reason::DesktopUnavailable);
        }
        let mut snapshot = Self {
            foreground_pid: parse(fields[1])?,
            foreground_window: parse(fields[2])?,
            displays: Vec::new(),
            windows: Vec::new(),
        };
        for line in lines {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            match fields.as_slice() {
                ["DISPLAY", x, y, width, height] => {
                    snapshot.displays.push(rect(x, y, width, height)?);
                }
                ["WIN", id, pid, x, y, width, height, name] => {
                    snapshot.windows.push(Window {
                        id: parse(id)?,
                        pid: parse(pid)?,
                        bounds: rect(x, y, width, height)?,
                        name: decode_name(name)?,
                        layer: 0,
                    });
                }
                ["WIN", id, pid, x, y, width, height, name, layer] => {
                    snapshot.windows.push(Window {
                        id: parse(id)?,
                        pid: parse(pid)?,
                        bounds: rect(x, y, width, height)?,
                        name: decode_name(name)?,
                        layer: parse(layer)?,
                    });
                }
                _ => return Err(Reason::DesktopUnavailable),
            }
            if snapshot.displays.len() > 32 || snapshot.windows.len() > 1024 {
                return Err(Reason::DesktopUnavailable);
            }
        }
        if snapshot.displays.is_empty() {
            return Err(Reason::DesktopUnavailable);
        }
        Ok(snapshot)
    }

    pub(crate) fn require_clear(&self, expected: &Window) -> Result<(), Reason> {
        let index = self
            .windows
            .iter()
            .position(|window| window.id == expected.id && window.pid == expected.pid)
            .ok_or(Reason::WindowChanged)?;
        let current = &self.windows[index];
        if current.bounds != expected.bounds {
            return Err(Reason::WindowChanged);
        }
        if self.foreground_pid != expected.pid
            || (cfg!(windows) && self.foreground_window != expected.id)
            || self.windows[..index]
                .iter()
                .any(|window| window.pid == expected.pid)
        {
            return Err(Reason::FocusChanged);
        }
        if !self
            .displays
            .iter()
            .any(|display| contains(*display, current.bounds))
        {
            return Err(Reason::WindowChanged);
        }
        if self.windows[..index]
            .iter()
            .any(|window| intersects(window.bounds, current.bounds))
        {
            return Err(Reason::WindowOccluded);
        }
        Ok(())
    }

    /// Recompute the occluders for the exact snapshot that produces
    /// `Reason::WindowOccluded`, returning a closed classification only when an
    /// intersecting window sits ahead of the target. The verdict is unchanged;
    /// this is read-only diagnostic evidence for the transient wave10 research.
    pub(crate) fn occluders(
        &self,
        expected: &Window,
    ) -> Option<crate::occlusion::OcclusionDiagnostic> {
        use crate::occlusion::{Occluder, classify_owner};
        // Only classify when the guard has actually rejected with
        // WindowOccluded. A same-PID window ahead (FocusChanged), a moved or
        // changed target (WindowChanged), or any other guard rejection is not
        // occlusion evidence and must never be classified.
        if self.require_clear(expected) != Err(Reason::WindowOccluded) {
            return None;
        }
        let index = self
            .windows
            .iter()
            .position(|window| window.id == expected.id && window.pid == expected.pid)?;
        let current = &self.windows[index];
        let target_area = i64::from(current.bounds.width) * i64::from(current.bounds.height);
        let mut occluders = Vec::new();
        for window in &self.windows[..index] {
            if !intersects(window.bounds, current.bounds) {
                continue;
            }
            let overlap = overlap_area(window.bounds, current.bounds);
            let overlap_per_mille = if target_area > 0 {
                ((overlap * 1000) / target_area).clamp(0, 1000)
            } else {
                0
            };
            occluders.push(Occluder {
                class: classify_owner(&window.name),
                same_process: window.pid == expected.pid,
                layer: window.layer,
                overlap_area: u64::try_from(overlap)
                    .expect("the overlap is bounded by the coordinate contract"),
                overlap_per_mille: u16::try_from(overlap_per_mille)
                    .expect("overlap per mille is clamped to 0..=1000"),
            });
        }
        if occluders.is_empty() {
            None
        } else {
            Some(crate::occlusion::OcclusionDiagnostic::new(occluders))
        }
    }
}

fn parse<T: std::str::FromStr>(value: &str) -> Result<T, Reason> {
    value.parse().map_err(|_| Reason::DesktopUnavailable)
}

fn rect(x: &str, y: &str, width: &str, height: &str) -> Result<Rect, Reason> {
    let values = [parse::<f64>(x)?, parse(y)?, parse(width)?, parse(height)?];
    if values
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 65536.0)
        || values[2] <= 0.0
        || values[3] <= 0.0
    {
        return Err(Reason::DesktopUnavailable);
    }
    Ok(Rect {
        x: values[0].to_i32().ok_or(Reason::DesktopUnavailable)?,
        y: values[1].to_i32().ok_or(Reason::DesktopUnavailable)?,
        width: values[2].to_u32().ok_or(Reason::DesktopUnavailable)?,
        height: values[3].to_u32().ok_or(Reason::DesktopUnavailable)?,
    })
}

fn decode_name(value: &str) -> Result<String, Reason> {
    if value == "-" {
        return Ok(String::new());
    }
    if value.len() > 512
        || !value.len().is_multiple_of(2)
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Reason::DesktopUnavailable);
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| Reason::DesktopUnavailable)?;
            u8::from_str_radix(text, 16).map_err(|_| Reason::DesktopUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| Reason::DesktopUnavailable)
}

pub(crate) fn contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && i64::from(inner.x) + i64::from(inner.width)
            <= i64::from(outer.x) + i64::from(outer.width)
        && i64::from(inner.y) + i64::from(inner.height)
            <= i64::from(outer.y) + i64::from(outer.height)
}

fn intersects(left: Rect, right: Rect) -> bool {
    overlap_area(left, right) > 0
}

/// Non-negative overlap area in squared units, using max(lefts)/min(rights) on
/// both axes so contained rectangles, disjoint and touching boundaries are all
/// handled correctly. Coordinate values are bounded by the parse contract, so
/// the product always fits, but the computation stays in `i64` to avoid any
/// wrapping (`65536 * 65536` would otherwise overflow a `u32`).
fn overlap_area(left: Rect, right: Rect) -> i64 {
    let width = (i64::from(left.x) + i64::from(left.width))
        .min(i64::from(right.x) + i64::from(right.width))
        - i64::from(left.x).max(i64::from(right.x));
    let height = (i64::from(left.y) + i64::from(left.height))
        .min(i64::from(right.y) + i64::from(right.height))
        - i64::from(left.y).max(i64::from(right.y));
    width.max(0) * height.max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot::parse("FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564\nWIN 43 11 0 0 1920 1080 6f74686572\n").unwrap()
    }

    #[test]
    fn only_the_unchanged_foreground_unoccluded_window_is_safe() {
        let mut state = snapshot();
        let target = state.windows[0].clone();
        assert!(state.require_clear(&target).is_ok());
        state.foreground_pid = 11;
        assert!(state.require_clear(&target).is_err());
        state.foreground_pid = 10;
        let mut other_owned = target.clone();
        other_owned.id = 100;
        other_owned.bounds.x = 1000;
        state.windows.insert(0, other_owned);
        assert_eq!(state.require_clear(&target), Err(Reason::FocusChanged));
        state.windows.remove(0);
        state.windows.swap(0, 1);
        assert!(state.require_clear(&target).is_err());
        state.windows.swap(0, 1);
        state.windows[0].bounds.x += 1;
        assert!(state.require_clear(&target).is_err());
        state.windows[0] = target.clone();
        state.windows[0].id = 99;
        assert!(state.require_clear(&target).is_err());
    }

    #[test]
    fn metadata_is_bounded_and_offscreen_capture_is_rejected() {
        assert!(Snapshot::parse("FG 1 1\nDISPLAY NaN 0 100 100\n").is_err());
        assert!(Snapshot::parse("FG 1 1\nWIN 1 1 0 0 10 10 zz\n").is_err());
        let mut state = snapshot();
        state.windows[0].bounds.x = -1;
        assert!(state.require_clear(&state.windows[0]).is_err());
    }

    #[test]
    fn occluders_report_closed_classes_without_changing_the_guard_verdict() {
        use crate::occlusion::OccluderClass;
        // FG 10 42, target only window 42 (pid 10) at 100,100 800x600.
        let mut state = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564 0\n",
        )
        .unwrap();
        let target = state.windows[0].clone();
        assert!(state.require_clear(&target).is_ok());
        assert!(state.occluders(&target).is_none());

        // A same-PID window ahead is a FocusChanged verdict; it is not an
        // occluder we may classify (the guard never reaches WindowOccluded).
        let mut owned = target.clone();
        owned.id = 100;
        owned.bounds.x = 100;
        state.windows.insert(0, owned);
        assert_eq!(state.require_clear(&target), Err(Reason::FocusChanged));
        assert!(state.occluders(&target).is_none());
        state.windows.remove(0);

        // A different-PID WindowServer window ahead, overlapping the target,
        // keeps the WindowOccluded verdict and yields a closed classification.
        state.windows.insert(
            0,
            Window {
                id: 200,
                pid: 11,
                bounds: Rect {
                    x: 100,
                    y: 100,
                    width: 800,
                    height: 600,
                },
                name: "WindowServer".into(),
                layer: -1,
            },
        );
        assert_eq!(state.require_clear(&target), Err(Reason::WindowOccluded));
        let diagnostic = state.occluders(&target).expect("must classify an occluder");
        assert_eq!(diagnostic.occluder_count, 1);
        assert_eq!(diagnostic.occluders[0].class, OccluderClass::WindowServer);
        assert!(!diagnostic.occluders[0].same_process);
        assert_eq!(diagnostic.occluders[0].layer, -1);
        assert_eq!(diagnostic.occluders[0].overlap_per_mille, 1000);
    }

    #[test]
    fn occluders_reject_a_private_external_app_without_emitting_its_name() {
        use crate::occlusion::OccluderClass;
        let mut state = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564 0\n",
        )
        .unwrap();
        let target = state.windows[0].clone();
        // A real, unknown external application window ahead (different PID).
        state.windows.insert(
            0,
            Window {
                id: 300,
                pid: 99,
                bounds: Rect {
                    x: 100,
                    y: 100,
                    width: 800,
                    height: 600,
                },
                name: "PrivateExternalAppName".into(),
                layer: 0,
            },
        );
        assert_eq!(state.require_clear(&target), Err(Reason::WindowOccluded));
        let diagnostic = state.occluders(&target).unwrap();
        // The closed class is Other, never the raw process/window name.
        assert_eq!(diagnostic.occluders[0].class, OccluderClass::Other);
        assert!(!diagnostic.occluders[0].same_process);
        assert_eq!(diagnostic.occluders[0].layer, 0);
    }

    #[test]
    fn intersection_uses_max_lefts_and_min_rights_across_all_cases() {
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        // Contained rectangle: the inner target is fully inside the occluder.
        assert!(intersects(rect(0, 0, 100, 100), rect(25, 25, 10, 10)));
        assert_eq!(
            overlap_area(rect(0, 0, 100, 100), rect(25, 25, 10, 10)),
            100
        );
        // Disjoint rectangles never intersect.
        assert!(!intersects(rect(0, 0, 50, 50), rect(60, 0, 50, 50)));
        assert_eq!(overlap_area(rect(0, 0, 50, 50), rect(60, 0, 50, 50)), 0);
        // Touching boundaries are not an intersection.
        assert!(!intersects(rect(0, 0, 50, 50), rect(50, 0, 50, 50)));
        assert!(!intersects(rect(0, 0, 50, 50), rect(0, 50, 50, 50)));
        assert_eq!(overlap_area(rect(0, 0, 50, 50), rect(50, 0, 50, 50)), 0);
        // Partial overlap computes the exact shared area.
        assert_eq!(
            overlap_area(rect(0, 0, 100, 100), rect(50, 50, 100, 100)),
            2500
        );
        // Coordinate extremes stay in i64 and never wrap a u32.
        assert!(intersects(
            rect(-65536, -65536, 65536, 65536),
            rect(-65536, -65536, 65536, 65536)
        ));
        assert_eq!(
            overlap_area(rect(0, 0, 65536, 65536), rect(0, 0, 65536, 65536)),
            65536 * 65536
        );
        // Touching at the coordinate boundary is still not an intersection.
        assert!(!intersects(
            rect(-65536, -65536, 65536, 65536),
            rect(0, 0, 65536, 65536)
        ));
        assert_eq!(
            overlap_area(rect(-65536, -65536, 65536, 65536), rect(0, 0, 65536, 65536)),
            0
        );
    }

    #[test]
    fn occluders_never_classify_a_same_process_focus_changed_snapshot() {
        // A same-PID window ahead must hold the FocusChanged verdict and must
        // never be misclassified as WindowOccluded occluder evidence.
        let mut state = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564 0\n",
        )
        .unwrap();
        let target = state.windows[0].clone();
        let mut owned = target.clone();
        owned.id = 77;
        state.windows.insert(0, owned);
        assert_eq!(state.require_clear(&target), Err(Reason::FocusChanged));
        assert!(
            state.occluders(&target).is_none(),
            "a FocusChanged snapshot must not yield occlusion evidence"
        );
    }

    #[test]
    fn occluders_do_not_classify_a_changed_target_snapshot() {
        // A moved or resized target yields WindowChanged, not WindowOccluded,
        // so there is no occluder evidence to classify.
        let state = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564 0\n",
        )
        .unwrap();
        let mut target = state.windows[0].clone();
        target.bounds.x += 1;
        assert_eq!(state.require_clear(&target), Err(Reason::WindowChanged));
        assert!(state.occluders(&target).is_none());
    }
}
