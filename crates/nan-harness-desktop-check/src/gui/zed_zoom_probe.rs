//! Closed, diagnostic-only correlation of immutable source icons and owned AX buttons.
//! Cairo candidates cannot authorize zoom input or establish a native toggle state.
use super::native_icon_probe::ZoomMatches;
use serde::Serialize;
#[cfg(any(not(target_os = "linux"), test))]
use xa11y::Toggled;
use xa11y::{ElementData, Rect, Role};

// One allocation covers setup, all candidates and confirmation; polls never
// renew it, and input/provider phases retain their own existing clocks.
#[cfg(any(target_os = "linux", test))]
pub(super) const ZED_ZOOM_PROOF_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Observation {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    pub(super) status: &'static str,
    maximize_matches: usize,
    minimize_matches: usize,
    stable_maximize_matches: usize,
    stable_minimize_matches: usize,
    correlated_buttons: usize,
    #[serde(skip_serializing_if = "omit_atspi_roles")]
    matched_push_buttons: usize,
    #[serde(skip_serializing_if = "omit_atspi_roles")]
    matched_toggle_buttons: usize,
    #[serde(skip_serializing_if = "omit_atspi_roles")]
    nested_containing_controls: usize,
    #[serde(skip_serializing_if = "omit_atspi_roles")]
    matched_role: &'static str,
    checked_state: &'static str,
    unique_correlation: bool,
    activation_attempted: bool,
    tooltip_status: &'static str,
    tooltip_candidates: usize,
    tooltip_matches: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip_start_remaining_ms: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip_end_remaining_ms: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip_phase: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon_calibration: Option<super::native_icon_probe::ZoomCalibration>,
}

// Raw AT-SPI roles must not be inferred from another platform's semantic role mapping.
fn omit_atspi_roles<T>(_: &T) -> bool {
    !cfg!(target_os = "linux")
}

impl Observation {
    pub(super) fn proves_zoomed(&self) -> bool {
        self.status == "observed"
            && self.unique_correlation
            && self.correlated_buttons == 1
            && self.stable_minimize_matches == 1
            && self.stable_maximize_matches == 0
            && self.matched_toggle_buttons == 1
            && self.matched_push_buttons == 0
            && self.checked_state == "on"
    }

    #[cfg(any(target_os = "linux", test))]
    pub(super) fn record_tooltip(
        &mut self,
        status: &'static str,
        candidates: usize,
        matches: usize,
    ) {
        self.tooltip_status = status;
        self.tooltip_candidates = candidates;
        self.tooltip_matches = matches;
    }

    #[cfg(any(target_os = "linux", test))]
    pub(super) fn tooltip_progress(&mut self, phase: &'static str, deadline: std::time::Instant) {
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .min(30_000) as u16;
        self.tooltip_start_remaining_ms.get_or_insert(remaining);
        self.tooltip_end_remaining_ms = Some(remaining);
        self.tooltip_phase = Some(phase);
    }
    #[cfg(target_os = "linux")]
    pub(super) fn finish_tooltip_progress(&mut self, deadline: std::time::Instant) {
        self.tooltip_end_remaining_ms = Some(
            deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis()
                .min(30_000) as u16,
        );
    }
    pub(super) fn unavailable(status: &'static str) -> Self {
        Self {
            schema_version: 1,
            mechanism: "zed-panel-zoom",
            diagnostics_only: true,
            status,
            maximize_matches: 0,
            minimize_matches: 0,
            stable_maximize_matches: 0,
            stable_minimize_matches: 0,
            correlated_buttons: 0,
            matched_push_buttons: 0,
            matched_toggle_buttons: 0,
            nested_containing_controls: 0,
            matched_role: "none",
            checked_state: "unavailable",
            unique_correlation: false,
            activation_attempted: false,
            tooltip_status: "unmeasured",
            tooltip_candidates: 0,
            tooltip_matches: 0,
            tooltip_start_remaining_ms: None,
            tooltip_end_remaining_ms: None,
            tooltip_phase: None,
            icon_calibration: None,
        }
    }
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.width > 0
        && outer.height > 0
        && inner.width > 0
        && inner.height > 0
        && inner.x >= outer.x
        && inner.y >= outer.y
        && i64::from(inner.x) + i64::from(inner.width)
            <= i64::from(outer.x) + i64::from(outer.width)
        && i64::from(inner.y) + i64::from(inner.height)
            <= i64::from(outer.y) + i64::from(outer.height)
}

#[cfg(any(target_os = "linux", test))]
fn describe_controls(result: &mut Observation, controls: &[(u32, Rect)]) {
    result.matched_push_buttons = controls.iter().filter(|(role, _)| *role == 43).count();
    result.matched_toggle_buttons = controls.iter().filter(|(role, _)| *role == 62).count();
    result.matched_role = match (result.matched_push_buttons, result.matched_toggle_buttons) {
        (0, 0) => "none",
        (_, 0) => "push-button",
        (0, _) => "toggle-button",
        _ => "mixed",
    };
    result.nested_containing_controls = controls
        .iter()
        .enumerate()
        .filter(|(index, (_, outer))| {
            controls
                .iter()
                .enumerate()
                .any(|(other, (_, inner))| other != *index && contains(*outer, *inner))
        })
        .count();
}

fn held_button(before: &ElementData, after: &ElementData) -> bool {
    matches!(before.role, Role::Button | Role::Switch)
        && before.role == after.role
        && before.pid.is_some()
        && before.pid == after.pid
        && before.stable_id.as_ref().is_some_and(|id| !id.is_empty())
        && before.stable_id == after.stable_id
        && before.bounds == after.bounds
        && before.states.enabled
        && after.states.enabled
        && before.states.visible
        && after.states.visible
        && before.states.checked == after.states.checked
}

#[cfg(any(not(target_os = "linux"), test))]
pub(super) fn correlate(
    matches: &ZoomMatches,
    before: &[ElementData],
    after: &[ElementData],
) -> Observation {
    if before.len() > 64 || after.len() > 64 {
        return Observation::unavailable("budget-exceeded");
    }
    let mut result = Observation::unavailable("observed");
    result.icon_calibration.clone_from(&matches.calibration);
    result.maximize_matches = matches.maximize_matches;
    result.minimize_matches = matches.minimize_matches;
    result.stable_maximize_matches = matches.maximize.len();
    result.stable_minimize_matches = matches.minimize.len();
    let candidates: Vec<_> = before
        .iter()
        .filter(|button| {
            let Some(bounds) = button.bounds else {
                return false;
            };
            after
                .iter()
                .filter(|other| held_button(button, other))
                .count()
                == 1
                && matches
                    .maximize
                    .iter()
                    .chain(&matches.minimize)
                    .any(|icon| contains(bounds, *icon))
        })
        .collect();
    result.correlated_buttons = candidates.len();
    result.unique_correlation =
        candidates.len() == 1 && matches.maximize.len() + matches.minimize.len() == 1;
    result.checked_state = match candidates.as_slice() {
        // xa11y maps AT-SPI ToggleButton to Switch but reads CHECKED rather
        // than AccessKit's PRESSED bit; only the raw sampler can measure it.
        [button] if button.role == Role::Switch => "unavailable",
        [button] => match button.states.checked {
            Some(Toggled::Off) => "off",
            Some(Toggled::On) => "on",
            Some(Toggled::Mixed) => "mixed",
            None => "unavailable",
        },
        [] => "unavailable",
        _ => "ambiguous",
    };
    result
}

#[cfg(target_os = "linux")]
pub(super) fn remap_canonical(
    mut records: Vec<CanonicalButton>,
    held: &[&ElementData],
    inventory: &[ElementData],
) -> Vec<CanonicalButton> {
    if records.len() > 64 {
        return Vec::new();
    }
    for record in &mut records {
        let Some(button) = held.get(record.index) else {
            return Vec::new();
        };
        let Some(index) = inventory
            .iter()
            .position(|candidate| std::ptr::eq(candidate, *button))
        else {
            return Vec::new();
        };
        record.index = index;
    }
    records
}

/// Private handoff: object indices refer only to the fresh held inventory.
#[cfg(any(target_os = "linux", test))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CanonicalButton {
    index: usize,
    role: u32,
    bounds: [i32; 4],
    toggle: String,
}

/// Retain only fresh, enabled source `ToggleButtons` with independently normalized bounds.
#[cfg(any(target_os = "linux", test))]
pub(super) fn active_candidates(
    before: &[ElementData],
    after: &[ElementData],
    canonical: &[CanonicalButton],
) -> Result<Vec<(usize, Rect)>, crate::report::Reason> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in canonical {
        let Some(button) = before.get(item.index) else {
            return Err(crate::report::Reason::ActionUnsupported);
        };
        if !seen.insert(item.index) || item.bounds[2] <= 0 || item.bounds[3] <= 0 {
            return Err(crate::report::Reason::ActionUnsupported);
        }
        if item.role != 62 || item.toggle != "on" {
            continue;
        }
        if button.role != Role::Switch
            || after
                .iter()
                .filter(|other| held_button(button, other))
                .count()
                != 1
        {
            return Err(crate::report::Reason::ActionUnsupported);
        }
        result.push((
            item.index,
            Rect {
                x: item.bounds[0],
                y: item.bounds[1],
                width: item.bounds[2].cast_unsigned(),
                height: item.bounds[3].cast_unsigned(),
            },
        ));
        if result.len() > 3 {
            return Err(crate::report::Reason::BudgetExceeded);
        }
    }
    Ok(result)
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn correlate_canonical(
    matches: &ZoomMatches,
    before: &[ElementData],
    after: &[ElementData],
    canonical: &[CanonicalButton],
) -> Observation {
    if canonical.len() > 64 || before.len() > 64 || after.len() > 64 {
        return Observation::unavailable("budget-exceeded");
    }
    let mut seen = std::collections::HashSet::new();
    let mut candidates = Vec::new();
    for item in canonical {
        let Some(button) = before.get(item.index) else {
            return Observation::unavailable("inventory-unavailable");
        };
        if !seen.insert(item.index)
            || !matches!(
                (item.role, button.role),
                (43, Role::Button) | (62, Role::Switch)
            )
            || !matches!(item.toggle.as_str(), "on" | "off" | "unknown")
            || item.bounds[2] <= 0
            || item.bounds[3] <= 0
        {
            return Observation::unavailable("inventory-unavailable");
        }
        let rect = Rect {
            x: item.bounds[0],
            y: item.bounds[1],
            width: item.bounds[2].cast_unsigned(),
            height: item.bounds[3].cast_unsigned(),
        };
        if after
            .iter()
            .filter(|other| held_button(button, other))
            .count()
            == 1
            && matches
                .maximize
                .iter()
                .chain(&matches.minimize)
                .any(|icon| contains(rect, *icon))
        {
            candidates.push(item);
        }
    }
    let mut result = Observation::unavailable("observed");
    result.icon_calibration.clone_from(&matches.calibration);
    result.maximize_matches = matches.maximize_matches;
    result.minimize_matches = matches.minimize_matches;
    result.stable_maximize_matches = matches.maximize.len();
    result.stable_minimize_matches = matches.minimize.len();
    describe_controls(
        &mut result,
        &candidates
            .iter()
            .map(|item| {
                (
                    item.role,
                    Rect {
                        x: item.bounds[0],
                        y: item.bounds[1],
                        width: item.bounds[2].cast_unsigned(),
                        height: item.bounds[3].cast_unsigned(),
                    },
                )
            })
            .collect::<Vec<_>>(),
    );
    result.correlated_buttons = candidates.len();
    result.unique_correlation =
        candidates.len() == 1 && matches.maximize.len() + matches.minimize.len() == 1;
    result.checked_state = match candidates.as_slice() {
        [item] if item.role == 62 => match item.toggle.as_str() {
            "on" => "on",
            "off" => "off",
            _ => "unavailable",
        },
        [] | [_] => "unavailable",
        _ => "ambiguous",
    };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tooltip_progress_preserves_expired_budget_without_claiming_proof() {
        let mut observation = Observation::unavailable("observed");
        observation.tooltip_progress("initial-clear", std::time::Instant::now());
        let value = serde_json::to_value(observation).unwrap();
        assert_eq!(value["tooltipStartRemainingMs"], 0);
        assert_eq!(value["tooltipEndRemainingMs"], 0);
        assert_eq!(value["tooltipPhase"], "initial-clear");
        assert_eq!(value["tooltipStatus"], "unmeasured");
        assert_eq!(value["uniqueCorrelation"], false);

        let mut future = Observation::unavailable("observed");
        future.tooltip_progress(
            "initial-clear",
            std::time::Instant::now() + ZED_ZOOM_PROOF_BUDGET + std::time::Duration::from_secs(1),
        );
        let value = serde_json::to_value(future).unwrap();
        assert_eq!(value["tooltipStartRemainingMs"], 30_000);
        assert_eq!(value["tooltipEndRemainingMs"], 30_000);
        assert_eq!(value["tooltipStatus"], "unmeasured");
        assert_eq!(value["uniqueCorrelation"], false);
    }

    #[test]
    fn zoom_postcondition_requires_stable_minimize_and_selected_owned_toggle() {
        let mut held = button();
        held.role = Role::Switch;
        let mut matches = icons();
        matches.minimize = std::mem::take(&mut matches.maximize);
        matches.minimize_matches = 1;
        matches.maximize_matches = 0;
        let canonical = |toggle: &str| CanonicalButton {
            index: 0,
            role: 62,
            bounds: [10, 20, 30, 30],
            toggle: toggle.into(),
        };
        let before = std::slice::from_ref(&held);
        let observed = correlate_canonical(&matches, before, before, &[canonical("on")]);
        assert!(observed.proves_zoomed());
        assert!(
            !correlate_canonical(&matches, before, before, &[canonical("off")]).proves_zoomed()
        );
        assert!(
            !correlate_canonical(&matches, before, before, &[canonical("unknown")]).proves_zoomed()
        );
        matches.maximize = matches.minimize.clone();
        assert!(!correlate_canonical(&matches, before, before, &[canonical("on")]).proves_zoomed());
    }

    #[test]
    fn canonical_root_bounds_correlate_without_double_origin_or_identity_loss() {
        let mut held = button();
        held.role = Role::Switch;
        let mut matches = icons();
        matches.maximize[0].x += 100;
        matches.maximize[0].y += 200;
        let records = vec![CanonicalButton {
            index: 0,
            role: 62,
            bounds: [110, 220, 30, 30],
            toggle: "on".into(),
        }];
        let result = correlate_canonical(&matches, &[held.clone()], &[held.clone()], &records);
        assert!(result.unique_correlation);
        assert_eq!(result.checked_state, "on");
        let mut changed = held.clone();
        changed.stable_id = Some("replacement".into());
        assert!(
            !correlate_canonical(&matches, &[held.clone()], &[changed], &records)
                .unique_correlation
        );
        let duplicate = vec![
            CanonicalButton {
                index: 0,
                role: 62,
                bounds: [110, 220, 30, 30],
                toggle: "on".into(),
            },
            CanonicalButton {
                index: 0,
                role: 62,
                bounds: [110, 220, 30, 30],
                toggle: "on".into(),
            },
        ];
        assert_eq!(
            correlate_canonical(&matches, &[held.clone()], &[held], &duplicate).status,
            "inventory-unavailable"
        );
    }

    #[test]
    fn nested_push_button_and_source_toggle_remain_distinct_and_ambiguous() {
        let push = button();
        let mut observation = Observation::unavailable("observed");
        observation.record_tooltip("proved", 3, 1);
        assert!(!observation.proves_zoomed());
        let mut toggle = button();
        toggle.role = Role::Switch;
        toggle.stable_id = Some("separate-held-toggle".into());
        let inventory = [push, toggle];
        let records = [
            CanonicalButton {
                index: 0,
                role: 43,
                bounds: [0, 0, 100, 100],
                toggle: "on".into(),
            },
            CanonicalButton {
                index: 1,
                role: 62,
                bounds: [10, 20, 30, 30],
                toggle: "off".into(),
            },
        ];
        let result = correlate_canonical(&icons(), &inventory, &inventory, &records);
        assert_eq!(result.matched_push_buttons, 1);
        assert_eq!(result.matched_toggle_buttons, 1);
        assert_eq!(result.nested_containing_controls, 1);
        assert_eq!(result.matched_role, "mixed");
        assert_eq!(result.checked_state, "ambiguous");
        assert!(!result.unique_correlation);
        let push_only = correlate_canonical(&icons(), &inventory, &inventory, &records[..1]);
        assert_eq!(push_only.matched_role, "push-button");
        assert_eq!(push_only.checked_state, "unavailable");
        assert!(push_only.unique_correlation);
    }

    #[test]
    fn tooltip_candidates_require_unique_live_enabled_source_identity() {
        let mut toggle = button();
        toggle.role = Role::Switch;
        let record = CanonicalButton {
            index: 0,
            role: 62,
            bounds: [10, 20, 30, 30],
            toggle: "on".into(),
        };
        assert_eq!(
            active_candidates(
                std::slice::from_ref(&toggle),
                std::slice::from_ref(&toggle),
                &[record]
            )
            .unwrap()
            .len(),
            1
        );
        let mut disabled = toggle.clone();
        disabled.states.enabled = false;
        let record = CanonicalButton {
            index: 0,
            role: 62,
            bounds: [10, 20, 30, 30],
            toggle: "on".into(),
        };
        assert!(active_candidates(&[toggle.clone()], &[disabled], &[record]).is_err());
        let record = CanonicalButton {
            index: 0,
            role: 62,
            bounds: [10, 20, 30, 30],
            toggle: "on".into(),
        };
        assert!(
            active_candidates(
                std::slice::from_ref(&toggle),
                &[toggle.clone(), toggle.clone()],
                &[record]
            )
            .is_err()
        );
        let off = CanonicalButton {
            index: 0,
            role: 62,
            bounds: [10, 20, 30, 30],
            toggle: "off".into(),
        };
        assert!(
            active_candidates(
                std::slice::from_ref(&toggle),
                std::slice::from_ref(&toggle),
                &[off]
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn tooltip_inventory_caps_work_before_hover_and_rejects_stale_geometry() {
        let inventory: Vec<_> = (0..4)
            .map(|index| {
                let mut element = button();
                element.role = Role::Switch;
                element.stable_id = Some(format!("owned-source-{index}"));
                element
            })
            .collect();
        let records: Vec<_> = (0..4)
            .map(|index| CanonicalButton {
                index,
                role: 62,
                bounds: [10, 20, 30, 30],
                toggle: "on".into(),
            })
            .collect();
        assert_eq!(
            active_candidates(&inventory[..3], &inventory[..3], &records[..3])
                .unwrap()
                .len(),
            3
        );
        assert!(matches!(
            active_candidates(&inventory, &inventory, &records),
            Err(crate::report::Reason::BudgetExceeded)
        ));
        let mut moved = inventory.clone();
        moved[0].bounds.as_mut().unwrap().x += 1;
        assert!(active_candidates(&inventory[..3], &moved[..3], &records[..3]).is_err());
    }

    fn button() -> ElementData {
        let mut element = ElementData {
            role: Role::Button,
            name: None,
            value: None,
            description: None,
            bounds: None,
            actions: Vec::new(),
            states: xa11y::StateSet::default(),
            numeric_value: None,
            min_value: None,
            max_value: None,
            stable_id: None,
            pid: None,
            raw: std::collections::HashMap::default(),
            handle: 0,
        };
        element.pid = Some(17);
        element.stable_id = Some("private-object-identity".into());
        element.bounds = Some(Rect {
            x: 10,
            y: 20,
            width: 30,
            height: 30,
        });
        element.states.enabled = true;
        element.states.visible = true;
        element
    }
    fn icons() -> ZoomMatches {
        ZoomMatches {
            maximize: vec![Rect {
                x: 15,
                y: 25,
                width: 14,
                height: 14,
            }],
            minimize: vec![],
            calibration: None,
            maximize_matches: 1,
            minimize_matches: 0,
        }
    }
    #[test]
    fn accesskit_toggle_button_mapped_to_switch_can_correlate_but_needs_raw_state() {
        let mut button = button();
        button.role = Role::Switch;
        button.states.checked = Some(Toggled::Off);
        let observation = correlate(
            &icons(),
            std::slice::from_ref(&button),
            std::slice::from_ref(&button),
        );
        assert!(observation.unique_correlation);
        assert_eq!(observation.checked_state, "unavailable");
        assert!(!observation.activation_attempted);
    }

    #[test]
    fn stable_owned_button_correlation_does_not_infer_toggle_or_authorize_input() {
        let button = button();
        let observation = correlate(
            &icons(),
            std::slice::from_ref(&button),
            std::slice::from_ref(&button),
        );
        assert!(observation.unique_correlation);
        assert_eq!(observation.checked_state, "unavailable");
        assert!(!observation.activation_attempted);
        let bytes = serde_json::to_string(&observation).unwrap();
        assert!(!bytes.contains("private-object-identity"));
        assert!(!bytes.contains("bounds"));
        assert!(!bytes.contains("pid"));
    }
    #[test]
    fn foreign_changed_disabled_or_duplicate_identity_is_not_unique() {
        let before = button();
        for changed in ["pid", "identity", "bounds", "disabled", "hidden"] {
            let mut after = before.clone();
            match changed {
                "pid" => after.pid = Some(99),
                "identity" => after.stable_id = Some("foreign".into()),
                "bounds" => after.bounds.as_mut().unwrap().x += 1,
                "disabled" => after.states.enabled = false,
                _ => after.states.visible = false,
            }
            assert!(
                !correlate(&icons(), std::slice::from_ref(&before), &[after]).unique_correlation
            );
        }
        assert!(
            !correlate(
                &icons(),
                &[before.clone(), before.clone()],
                &[before.clone(), before]
            )
            .unique_correlation
        );
    }
}
