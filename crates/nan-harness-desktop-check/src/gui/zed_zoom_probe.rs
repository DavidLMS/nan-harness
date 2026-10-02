//! Closed, diagnostic-only correlation of immutable source icons and owned AX buttons.
//! Cairo candidates cannot authorize zoom input or establish a native toggle state.
use super::native_icon_probe::ZoomMatches;
use serde::Serialize;
use xa11y::{ElementData, Rect, Role, Toggled};

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
    checked_state: &'static str,
    unique_correlation: bool,
    activation_attempted: bool,
}

impl Observation {
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
            checked_state: "unavailable",
            unique_correlation: false,
            activation_attempted: false,
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
    result.maximize_matches = matches.maximize_matches;
    result.minimize_matches = matches.minimize_matches;
    result.stable_maximize_matches = matches.maximize.len();
    result.stable_minimize_matches = matches.minimize.len();
    result.correlated_buttons = candidates.len();
    result.unique_correlation =
        candidates.len() == 1 && matches.maximize.len() + matches.minimize.len() == 1;
    result.checked_state = match candidates.as_slice() {
        [item] => match item.toggle.as_str() {
            "on" => "on",
            "off" => "off",
            _ => "unavailable",
        },
        [] => "unavailable",
        _ => "ambiguous",
    };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
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
