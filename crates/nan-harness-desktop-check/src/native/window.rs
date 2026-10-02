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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuardFailure {
    IdentityMissing,
    BoundsChanged,
    ForegroundChanged,
    SameProcessWindow,
    OffDisplay,
    Occluded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForegroundRelation {
    IdentityUnavailable,
    DifferentProcess,
    SameProcessDifferentWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisplayRelation {
    PartialMonitorOverlap,
    NoMonitorOverlap,
}

impl GuardFailure {
    pub(crate) const fn reason(self) -> Reason {
        match self {
            Self::IdentityMissing | Self::BoundsChanged | Self::OffDisplay => Reason::WindowChanged,
            Self::ForegroundChanged | Self::SameProcessWindow => Reason::FocusChanged,
            Self::Occluded => Reason::WindowOccluded,
        }
    }
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FocusStatus {
    Proved,
    Untrusted,
    QueryError,
    FocusMismatch,
    NotStandard,
    IdentityChanged,
    NoMatch,
    Ambiguous,
}
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FocusQuery {
    phase: FocusQueryPhase,
    stage: FocusQueryStage,
    error: FocusQueryError,
}
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum FocusQueryPhase {
    Before,
    After,
}
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum FocusQueryStage {
    AppCreate,
    AppTimeout,
    FocusedWindow,
    MainWindow,
    FocusedElement,
    InputTimeout,
    InputWindow,
    ElementType,
    Pid,
    WindowTimeout,
    Role,
    Subrole,
    Position,
    Size,
    Geometry,
}
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum FocusQueryError {
    Failure,
    IllegalArgument,
    InvalidElement,
    CannotComplete,
    AttributeUnsupported,
    NotImplemented,
    ApiDisabled,
    NoValue,
    Other,
    EmptyValue,
    TypeMismatch,
    OwnerMismatch,
    GeometryInvalid,
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone)]
struct FocusProof {
    status: FocusStatus,
    window: u64,
}

#[cfg(any(test, target_os = "macos"))]
impl FocusProof {
    // A proved receipt must carry one private window identity; failures carry none.
    fn parse(status: &str, id: &str) -> Result<Self, Reason> {
        let status = match status {
            "proved" => FocusStatus::Proved,
            "untrusted" => FocusStatus::Untrusted,
            "query-error" => FocusStatus::QueryError,
            "focus-mismatch" => FocusStatus::FocusMismatch,
            "not-standard" => FocusStatus::NotStandard,
            "identity-changed" => FocusStatus::IdentityChanged,
            "no-match" => FocusStatus::NoMatch,
            "ambiguous" => FocusStatus::Ambiguous,
            _ => return Err(Reason::DesktopUnavailable),
        };
        let window = parse(id)?;
        if (status == FocusStatus::Proved) != (window != 0) {
            return Err(Reason::DesktopUnavailable);
        }
        Ok(Self { status, window })
    }
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    foreground_pid: u32,
    foreground_window: u64,
    #[cfg(any(test, target_os = "macos"))]
    focus: Option<FocusProof>,
    #[cfg(any(test, target_os = "macos"))]
    focus_query: Option<FocusQuery>,
    #[cfg(any(test, target_os = "macos"))]
    window_focus: Option<FocusProof>,
    displays: Vec<Rect>,
    pub(crate) windows: Vec<Window>,
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StackObservation {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    status: &'static str,
    same_pid_ahead_count: Option<usize>,
    same_pid_ahead_eligible_count: Option<usize>,
    same_pid_ahead_intersects_held_count: Option<usize>,
    same_pid_ahead_normal_layer_count: Option<usize>,
    same_pid_ahead_other_layer_count: Option<usize>,
    foreground_pid_matches_held: bool,
    frontmost_window_same_pid: bool,
}

impl Snapshot {
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn stack_observation(&self, held: &Window) -> StackObservation {
        let index = self
            .windows
            .iter()
            .position(|window| window.id == held.id && window.pid == held.pid);
        let ahead = index.map(|index| {
            self.windows[..index]
                .iter()
                .filter(|window| window.pid == held.pid)
                .collect::<Vec<_>>()
        });
        let complete = ahead.as_ref().is_some_and(|windows| windows.len() <= 32);
        let count = |predicate: fn(&Window) -> bool| {
            complete.then(|| {
                ahead
                    .as_ref()
                    .unwrap()
                    .iter()
                    .filter(|window| predicate(window))
                    .count()
            })
        };
        StackObservation {
            schema_version: 1,
            mechanism: "claude-window-stack",
            diagnostics_only: true,
            status: if complete {
                "complete"
            } else if ahead.is_some() {
                "overflow"
            } else {
                "unavailable"
            },
            same_pid_ahead_count: count(|_| true),
            same_pid_ahead_eligible_count: count(|window| {
                window.bounds.width >= 300 && window.bounds.height >= 200
            }),
            same_pid_ahead_intersects_held_count: complete.then(|| {
                ahead
                    .as_ref()
                    .unwrap()
                    .iter()
                    .filter(|window| intersects(window.bounds, held.bounds))
                    .count()
            }),
            same_pid_ahead_normal_layer_count: count(|window| window.layer == 0),
            same_pid_ahead_other_layer_count: count(|window| window.layer != 0),
            foreground_pid_matches_held: self.foreground_pid == held.pid,
            frontmost_window_same_pid: self
                .windows
                .first()
                .is_some_and(|window| window.pid == held.pid),
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn focus_observation(
        &self,
        held: &Window,
    ) -> Option<(FocusStatus, Option<bool>, Option<FocusQuery>)> {
        self.focus.as_ref().map(|proof| {
            let matches = (proof.status == FocusStatus::Proved).then(|| {
                proof.window == held.id
                    && self.foreground_pid == held.pid
                    && self
                        .windows
                        .iter()
                        .filter(|window| {
                            window.id == proof.window
                                && window.pid == held.pid
                                && window.bounds == held.bounds
                                && window.layer == 0
                        })
                        .count()
                        == 1
            });
            (proof.status, matches, self.focus_query)
        })
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn window_focus_observation(
        &self,
        held: &Window,
    ) -> Option<(FocusStatus, Option<bool>)> {
        self.window_focus.as_ref().map(|proof| {
            let matched = (proof.status == FocusStatus::Proved).then(|| {
                proof.window == held.id
                    && self.foreground_pid == held.pid
                    && self
                        .windows
                        .iter()
                        .filter(|window| {
                            window.id == proof.window
                                && window.pid == held.pid
                                && window.bounds == held.bounds
                                && window.layer == 0
                        })
                        .count()
                        == 1
            });
            (proof.status, matched)
        })
    }

    pub(crate) fn parse(text: &str) -> Result<Self, Reason> {
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
            #[cfg(any(test, target_os = "macos"))]
            focus: None,
            #[cfg(any(test, target_os = "macos"))]
            focus_query: None,
            #[cfg(any(test, target_os = "macos"))]
            window_focus: None,
            displays: Vec::new(),
            windows: Vec::new(),
        };
        for line in lines {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            match fields.as_slice() {
                #[cfg(any(test, target_os = "macos"))]
                [tag @ ("FOCUS" | "FOCUS_WINDOW"), status, id]
                    if if *tag == "FOCUS" {
                        snapshot.focus.is_none()
                    } else {
                        snapshot.window_focus.is_none()
                    } =>
                {
                    let proof = Some(FocusProof::parse(status, id)?);
                    if *tag == "FOCUS" {
                        snapshot.focus = proof;
                    } else {
                        snapshot.window_focus = proof;
                    }
                }
                #[cfg(any(test, target_os = "macos"))]
                ["FOCUS_QUERY", phase, stage, error] if snapshot.focus_query.is_none() => {
                    snapshot.focus_query = Some(
                        serde_json::from_value(serde_json::json!({
                            "phase": phase, "stage": stage, "error": error,
                        }))
                        .map_err(|_| Reason::DesktopUnavailable)?,
                    );
                }
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
        #[cfg(any(test, target_os = "macos"))]
        if let Some(query) = snapshot.focus_query {
            let expected_status = match query.phase {
                FocusQueryPhase::Before => FocusStatus::QueryError,
                FocusQueryPhase::After => FocusStatus::IdentityChanged,
            };
            if snapshot
                .focus
                .as_ref()
                .is_none_or(|proof| proof.status != expected_status)
            {
                return Err(Reason::DesktopUnavailable);
            }
        }
        if snapshot.displays.is_empty() {
            return Err(Reason::DesktopUnavailable);
        }
        Ok(snapshot)
    }

    pub(crate) fn require_clear(&self, expected: &Window) -> Result<(), Reason> {
        self.guard_failure(expected).map_err(GuardFailure::reason)
    }

    pub(crate) fn guard_failure(&self, expected: &Window) -> Result<(), GuardFailure> {
        let index = self
            .windows
            .iter()
            .position(|window| window.id == expected.id && window.pid == expected.pid)
            .ok_or(GuardFailure::IdentityMissing)?;
        let current = &self.windows[index];
        if current.bounds != expected.bounds {
            return Err(GuardFailure::BoundsChanged);
        }
        if self.foreground_pid != expected.pid
            || (cfg!(windows) && self.foreground_window != expected.id)
        {
            return Err(GuardFailure::ForegroundChanged);
        }
        if self.windows[..index]
            .iter()
            .any(|window| window.pid == expected.pid)
        {
            return Err(GuardFailure::SameProcessWindow);
        }
        if !self
            .displays
            .iter()
            .any(|display| contains(*display, current.bounds))
        {
            return Err(GuardFailure::OffDisplay);
        }
        if self.windows[..index]
            .iter()
            .any(|window| intersects(window.bounds, current.bounds))
        {
            return Err(GuardFailure::Occluded);
        }
        Ok(())
    }

    /// A focused standard window may have a nonintersecting auxiliary panel above it.
    /// Both independent public AX proofs must identify this exact retained CG window.
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn claude_focused_guard_failure(
        &self,
        expected: &Window,
    ) -> Result<(), GuardFailure> {
        let original = self.guard_failure(expected);
        if original != Err(GuardFailure::SameProcessWindow) {
            return original;
        }
        if self
            .windows
            .iter()
            .filter(|window| window.id == expected.id && window.pid == expected.pid)
            .count()
            != 1
        {
            return Err(GuardFailure::IdentityMissing);
        }
        if !self
            .focus_observation(expected)
            .is_some_and(|value| value.0 == FocusStatus::Proved && value.1 == Some(true))
            || self.window_focus_observation(expected) != Some((FocusStatus::Proved, Some(true)))
        {
            return original;
        }
        let index = self
            .windows
            .iter()
            .position(|window| window.id == expected.id && window.pid == expected.pid)
            .ok_or(GuardFailure::IdentityMissing)?;
        if self.windows[..index]
            .iter()
            .any(|window| window.pid == expected.pid && window.layer == 0)
        {
            return original;
        }
        if !self
            .displays
            .iter()
            .any(|display| contains(*display, expected.bounds))
        {
            return Err(GuardFailure::OffDisplay);
        }
        if self.windows[..index]
            .iter()
            .any(|window| intersects(window.bounds, expected.bounds))
        {
            return Err(GuardFailure::Occluded);
        }
        Ok(())
    }

    /// Validate every guard condition except foreground ownership. This is
    /// used only by macOS's initial focus recovery: a foreground mismatch may
    /// be repaired, but identity, geometry, display, same-process stacking,
    /// and occlusion failures must block activation. Keep this separate from
    /// `guard_failure` so its established diagnostic precedence is unchanged.
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn non_foreground_failure(&self, expected: &Window) -> Result<(), GuardFailure> {
        let index = self
            .windows
            .iter()
            .position(|window| window.id == expected.id && window.pid == expected.pid)
            .ok_or(GuardFailure::IdentityMissing)?;
        let current = &self.windows[index];
        if current.bounds != expected.bounds {
            return Err(GuardFailure::BoundsChanged);
        }
        if self.windows[..index]
            .iter()
            .any(|window| window.pid == expected.pid)
        {
            return Err(GuardFailure::SameProcessWindow);
        }
        if !self
            .displays
            .iter()
            .any(|display| contains(*display, current.bounds))
        {
            return Err(GuardFailure::OffDisplay);
        }
        if self.windows[..index]
            .iter()
            .any(|window| intersects(window.bounds, current.bounds))
        {
            return Err(GuardFailure::Occluded);
        }
        Ok(())
    }

    pub(crate) fn foreground_relation(&self, expected: &Window) -> Option<ForegroundRelation> {
        if self.foreground_pid == 0 || (cfg!(windows) && self.foreground_window == 0) {
            Some(ForegroundRelation::IdentityUnavailable)
        } else if self.foreground_pid != expected.pid {
            Some(ForegroundRelation::DifferentProcess)
        } else if cfg!(windows) && self.foreground_window != expected.id {
            Some(ForegroundRelation::SameProcessDifferentWindow)
        } else {
            None
        }
    }

    pub(crate) fn off_display_relation(&self, expected: &Window) -> Option<DisplayRelation> {
        let index = self
            .windows
            .iter()
            .position(|window| window.id == expected.id && window.pid == expected.pid)?;
        let current = &self.windows[index];
        if self.displays.is_empty() {
            return None;
        }
        if current.bounds.width == 0 || current.bounds.height == 0 {
            return None;
        }
        if self
            .displays
            .iter()
            .any(|display| contains(*display, current.bounds))
        {
            return None;
        }
        if self
            .displays
            .iter()
            .any(|display| intersects(*display, current.bounds))
        {
            Some(DisplayRelation::PartialMonitorOverlap)
        } else {
            Some(DisplayRelation::NoMonitorOverlap)
        }
    }

    #[cfg(windows)]
    pub(crate) fn contains_display(&self, expected: &Window) -> bool {
        self.displays
            .iter()
            .any(|display| contains(*display, expected.bounds))
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
    contains_edges(EdgeRect::from(outer), EdgeRect::from(inner))
}

#[derive(Clone, Copy)]
struct EdgeRect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl From<Rect> for EdgeRect {
    fn from(rect: Rect) -> Self {
        Self {
            left: i64::from(rect.x),
            top: i64::from(rect.y),
            right: i64::from(rect.x) + i64::from(rect.width),
            bottom: i64::from(rect.y) + i64::from(rect.height),
        }
    }
}

fn contains_edges(outer: EdgeRect, inner: EdgeRect) -> bool {
    outer.right > outer.left
        && outer.bottom > outer.top
        && inner.right > inner.left
        && inner.bottom > inner.top
        && inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
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
    fn non_foreground_preconditions_require_all_activation_safety_properties() {
        let state = snapshot();
        let target = state.windows[0].clone();
        assert!(state.non_foreground_failure(&target).is_ok());

        let mut missing = state.clone();
        missing.windows.clear();
        assert_eq!(
            missing.non_foreground_failure(&target),
            Err(GuardFailure::IdentityMissing)
        );

        let mut changed = state.clone();
        changed.windows[0].bounds.x += 1;
        assert_eq!(
            changed.non_foreground_failure(&target),
            Err(GuardFailure::BoundsChanged)
        );

        let mut same_process = state.clone();
        same_process.windows.insert(
            0,
            Window {
                id: 99,
                ..target.clone()
            },
        );
        assert_eq!(
            same_process.non_foreground_failure(&target),
            Err(GuardFailure::SameProcessWindow)
        );

        let mut off_display = state.clone();
        off_display.displays[0].width = 100;
        assert_eq!(
            off_display.non_foreground_failure(&target),
            Err(GuardFailure::OffDisplay)
        );

        let mut occluded = state;
        occluded.windows.insert(
            0,
            Window {
                id: 100,
                pid: 11,
                bounds: target.bounds,
                ..target.clone()
            },
        );
        assert_eq!(
            occluded.non_foreground_failure(&target),
            Err(GuardFailure::Occluded)
        );
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
    fn off_display_relation_handles_partial_and_disjoint_multi_monitor_geometry() {
        let mut state = snapshot();
        let target = state.windows[0].clone();
        state.displays = vec![Rect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            state.off_display_relation(&target),
            Some(DisplayRelation::NoMonitorOverlap)
        );

        state.displays.push(Rect {
            x: 500,
            y: 0,
            width: 1920,
            height: 1080,
        });
        assert_eq!(
            state.off_display_relation(&target),
            Some(DisplayRelation::PartialMonitorOverlap)
        );

        state.windows[0].bounds.x = -4000;
        assert_eq!(
            state.off_display_relation(&state.windows[0]),
            Some(DisplayRelation::NoMonitorOverlap)
        );

        state.displays.clear();
        assert_eq!(state.off_display_relation(&state.windows[0]), None);
    }

    #[test]
    fn display_inventory_rejects_invalid_and_handles_boundaries() {
        assert!(Snapshot::parse("FG 1 1\nDISPLAY 0 0 0 100\n").is_err());
        assert!(Snapshot::parse("FG 1 1\nDISPLAY 0 0 100 0\n").is_err());
        let mut state = snapshot();
        state.displays = vec![Rect {
            x: -100,
            y: -100,
            width: 100,
            height: 100,
        }];
        state.windows[0].bounds = Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        assert_eq!(
            state.off_display_relation(&state.windows[0]),
            Some(DisplayRelation::NoMonitorOverlap)
        );
        state.windows[0].bounds = Rect {
            x: -100,
            y: -100,
            width: 0,
            height: 0,
        };
        assert_eq!(state.off_display_relation(&state.windows[0]), None);
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
    fn claude_auxiliary_panel_requires_both_exact_proofs_and_clear_owned_window() {
        let base = "FG 7 0\nDISPLAY 0 0 2000 2000\nWIN 99 7 1500 1500 10 10 50616e656c 3\nWIN 1 7 10 20 800 600 436c61756465 0\n";
        let state =
            Snapshot::parse(&format!("{base}FOCUS proved 1\nFOCUS_WINDOW proved 1\n")).unwrap();
        let held = state.windows[1].clone();
        assert_eq!(state.claude_focused_guard_failure(&held), Ok(()));
        assert_eq!(
            state.guard_failure(&held),
            Err(GuardFailure::SameProcessWindow)
        );
        assert_eq!(
            state.non_foreground_failure(&held),
            Err(GuardFailure::SameProcessWindow)
        );
        for proofs in [
            "",
            "FOCUS proved 1\n",
            "FOCUS_WINDOW proved 1\n",
            "FOCUS query-error 0\nFOCUS_WINDOW proved 1\n",
            "FOCUS proved 1\nFOCUS_WINDOW ambiguous 0\n",
            "FOCUS proved 99\nFOCUS_WINDOW proved 1\n",
        ] {
            let missing = Snapshot::parse(&format!("{base}{proofs}")).unwrap();
            assert_eq!(
                missing.claude_focused_guard_failure(&held),
                Err(GuardFailure::SameProcessWindow)
            );
        }
        let mut normal = state.clone();
        normal.windows[0].layer = 0;
        assert_eq!(
            normal.claude_focused_guard_failure(&held),
            Err(GuardFailure::SameProcessWindow)
        );
        for pid in [7, 8] {
            let mut overlap = state.clone();
            overlap.windows[0].pid = pid;
            overlap.windows[0].bounds = held.bounds;
            assert_eq!(
                overlap.claude_focused_guard_failure(&held),
                Err(GuardFailure::Occluded)
            );
        }
        let mut changed = state.clone();
        changed.windows[1].bounds.x += 1;
        assert_eq!(
            changed.claude_focused_guard_failure(&held),
            Err(GuardFailure::BoundsChanged)
        );
        let mut foreign = state.clone();
        foreign.foreground_pid = 8;
        assert_eq!(
            foreign.claude_focused_guard_failure(&held),
            Err(GuardFailure::ForegroundChanged)
        );
        let mut off_display = state.clone();
        off_display.displays.clear();
        assert_eq!(
            off_display.claude_focused_guard_failure(&held),
            Err(GuardFailure::OffDisplay)
        );
        let mut duplicate = state.clone();
        duplicate.windows.push(held.clone());
        assert_eq!(
            duplicate.claude_focused_guard_failure(&held),
            Err(GuardFailure::IdentityMissing)
        );
    }

    #[test]
    fn independent_window_proof_preserves_failed_input_and_occlusion_guard() {
        let base = "FG 7 0\nDISPLAY 0 0 2000 2000\nWIN 99 7 1500 1500 10 10 50616e656c 3\nWIN 1 7 10 20 800 600 436c61756465 0\n";
        let state = Snapshot::parse(&format!("{base}FOCUS query-error 0\nFOCUS_QUERY before focused-element no-value\nFOCUS_WINDOW proved 1\n")).unwrap();
        let held = &state.windows[1];
        assert_eq!(
            state.window_focus_observation(held),
            Some((FocusStatus::Proved, Some(true)))
        );
        assert_eq!(
            state.focus_observation(held).unwrap().0,
            FocusStatus::QueryError
        );
        assert_eq!(
            state.guard_failure(held),
            Err(GuardFailure::SameProcessWindow)
        );
        for receipt in [
            "FOCUS_WINDOW proved 0\n",
            "FOCUS_WINDOW ambiguous 1\n",
            "FOCUS_WINDOW proved 1\nFOCUS_WINDOW proved 1\n",
        ] {
            assert!(Snapshot::parse(&format!("{base}{receipt}")).is_err());
        }
    }

    #[test]
    fn focus_query_protocol_preserves_stage_and_rejects_inconsistent_receipts() {
        let base = "FG 7 0\nDISPLAY 0 0 2000 2000\nWIN 1 7 10 20 800 600 436c61756465 0\n";
        let query = "FOCUS_QUERY before input-window attribute-unsupported\n";
        let state = Snapshot::parse(&format!("{base}FOCUS query-error 0\n{query}")).unwrap();
        let observation = state.focus_observation(&state.windows[0]).unwrap();
        assert_eq!(observation.0, FocusStatus::QueryError);
        assert_eq!(observation.1, None);
        let value = serde_json::to_value(observation.2.unwrap()).unwrap();
        assert_eq!(value["stage"], "input-window");
        assert_eq!(value["error"], "attribute-unsupported");
        for receipt in [
            format!("{base}FOCUS proved 1\n{query}"),
            format!("{base}FOCUS query-error 0\n{query}{query}"),
            format!("{base}FOCUS query-error 0\nFOCUS_QUERY before private-label failure\n"),
            format!("{base}{query}"),
        ] {
            assert!(Snapshot::parse(&receipt).is_err());
        }
    }

    #[test]
    fn focus_proof_is_advisory_and_rejects_ambiguous_protocols() {
        let base = "FG 7 0\nDISPLAY 0 0 2000 2000\nWIN 1 7 10 20 800 600 436c61756465 0\n";
        let proved = Snapshot::parse(&format!("{base}FOCUS proved 1\n")).unwrap();
        let held = proved.windows[0].clone();
        assert_eq!(
            proved.focus_observation(&held),
            Some((FocusStatus::Proved, Some(true), None))
        );
        assert_eq!(proved.guard_failure(&held), Ok(()));
        let mut with_panel = proved.clone();
        let mut panel = held.clone();
        panel.id = 99;
        panel.layer = 3;
        panel.bounds = Rect {
            x: 1800,
            y: 1800,
            width: 10,
            height: 10,
        };
        with_panel.windows.insert(0, panel);
        assert_eq!(
            with_panel.focus_observation(&held),
            Some((FocusStatus::Proved, Some(true), None))
        );
        assert_eq!(
            with_panel.guard_failure(&held),
            Err(GuardFailure::SameProcessWindow)
        );
        for status in ["ambiguous", "identity-changed", "query-error", "untrusted"] {
            let state = Snapshot::parse(&format!("{base}FOCUS {status} 0\n")).unwrap();
            assert_eq!(state.focus_observation(&held).unwrap().1, None);
            assert_eq!(state.guard_failure(&held), Ok(()));
        }
        for suffix in [
            "FOCUS proved 0\n",
            "FOCUS ambiguous 1\n",
            "FOCUS invented 0\n",
            "FOCUS proved 1\nFOCUS proved 1\n",
        ] {
            assert!(Snapshot::parse(&format!("{base}{suffix}")).is_err());
        }
        let different = Snapshot::parse(&format!("{base}FOCUS proved 2\n")).unwrap();
        assert_eq!(different.focus_observation(&held).unwrap().1, Some(false));
    }

    #[test]
    fn stack_observation_separates_tiny_panels_and_closes_overflow() {
        let mut state = snapshot();
        let held = state.windows[0].clone();
        let mut tiny = held.clone();
        tiny.id = 99;
        tiny.bounds = Rect {
            x: 2000,
            y: 2000,
            width: 10,
            height: 10,
        };
        tiny.layer = 3;
        state.windows.insert(0, tiny.clone());
        let value = serde_json::to_value(state.stack_observation(&held)).unwrap();
        assert_eq!(value["samePidAheadCount"], 1);
        assert_eq!(value["samePidAheadEligibleCount"], 0);
        assert_eq!(value["samePidAheadIntersectsHeldCount"], 0);
        assert_eq!(value["samePidAheadOtherLayerCount"], 1);
        assert_eq!(
            state.guard_failure(&held),
            Err(GuardFailure::SameProcessWindow)
        );
        for id in 100..132 {
            tiny.id = id;
            state.windows.insert(0, tiny.clone());
        }
        let overflow = serde_json::to_value(state.stack_observation(&held)).unwrap();
        assert_eq!(overflow["status"], "overflow");
        assert!(overflow["samePidAheadCount"].is_null());
        assert!(overflow.get("bounds").is_none());
        assert!(overflow.get("pid").is_none());
    }

    #[test]
    fn guard_failure_keeps_window_invariants_closed_and_distinct() {
        let mut state = snapshot();
        let target = state.windows[0].clone();
        assert_eq!(state.guard_failure(&target), Ok(()));

        state.foreground_pid = 11;
        assert_eq!(
            state.foreground_relation(&target),
            Some(ForegroundRelation::DifferentProcess)
        );
        assert_eq!(
            state.guard_failure(&target),
            Err(GuardFailure::ForegroundChanged)
        );
        state.foreground_pid = 10;
        assert_eq!(state.foreground_relation(&target), None);
        state.foreground_pid = 0;
        assert_eq!(
            state.foreground_relation(&target),
            Some(ForegroundRelation::IdentityUnavailable)
        );
        assert_eq!(
            state.guard_failure(&target),
            Err(GuardFailure::ForegroundChanged)
        );
        state.foreground_pid = 10;

        state.windows[0].bounds.x += 1;
        assert_eq!(
            state.guard_failure(&target),
            Err(GuardFailure::BoundsChanged)
        );
        state.windows[0] = target.clone();

        state.windows.remove(0);
        assert_eq!(
            state.guard_failure(&target),
            Err(GuardFailure::IdentityMissing)
        );

        let mut off_display = snapshot();
        off_display.windows[0].bounds.x = 1500;
        off_display.windows[0].bounds.y = 700;
        assert_eq!(
            off_display.guard_failure(&off_display.windows[0].clone()),
            Err(GuardFailure::OffDisplay)
        );

        let mut same_process = snapshot();
        let mut sibling = target.clone();
        sibling.id = 99;
        sibling.bounds.x = 1000;
        sibling.pid = target.pid;
        same_process.windows.insert(0, sibling);
        assert_eq!(
            same_process.guard_failure(&target),
            Err(GuardFailure::SameProcessWindow)
        );

        let mut occlusion_state = snapshot();
        let mut covering_window = occlusion_state.windows[1].clone();
        covering_window.pid = 11;
        covering_window.bounds = target.bounds;
        occlusion_state.windows.insert(0, covering_window);
        assert_eq!(
            occlusion_state.guard_failure(&target),
            Err(GuardFailure::Occluded)
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

    #[test]
    fn system_surface_preserves_occlusion_without_a_blanket_dock_bypass() {
        use crate::occlusion::OccluderClass;
        // A macOS system surface (the Dock, process "Dock", CoreGraphics layer
        // 20) remains subject to the conservative geometry guard. This fixture
        // establishes the rejection contract, not the surface's visible
        // opacity; a process name or layer must not grant a blanket exemption.
        let dock = |x, y| Window {
            id: 200,
            pid: 11,
            bounds: Rect {
                x,
                y,
                width: 1920,
                height: 1080,
            },
            name: "Dock".into(),
            layer: 20,
        };

        // Overlapping geometry (the rejected snapshot) stays WindowOccluded.
        let mut overlapping = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564 0\n",
        )
        .unwrap();
        let target = overlapping.windows[0].clone();
        overlapping.windows.insert(0, dock(0, 0));
        assert_eq!(
            overlapping.require_clear(&target),
            Err(Reason::WindowOccluded)
        );
        let diagnostic = overlapping
            .occluders(&target)
            .expect("a system occluder is classified");
        assert_eq!(diagnostic.occluders[0].class, OccluderClass::Dock);
        assert!(!diagnostic.occluders[0].same_process);
        assert_eq!(diagnostic.occluders[0].layer, 20);
        assert_eq!(diagnostic.occluders[0].overlap_per_mille, 1000);

        // Non-overlapping geometry is not an occlusion.
        let mut disjoint = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564 0\n",
        )
        .unwrap();
        let target = disjoint.windows[0].clone();
        disjoint.windows.insert(0, dock(-3000, -3000));
        assert!(disjoint.require_clear(&target).is_ok());
        assert!(disjoint.occluders(&target).is_none());

        // A same-PID system-level window ahead is a focus change, never a
        // different-process occluder, and must not yield occlusion evidence.
        let mut owned = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564 0\n",
        )
        .unwrap();
        let target = owned.windows[0].clone();
        let mut same = dock(0, 0);
        same.pid = 10;
        owned.windows.insert(0, same);
        assert_eq!(owned.require_clear(&target), Err(Reason::FocusChanged));
        assert!(owned.occluders(&target).is_none());
    }
}
