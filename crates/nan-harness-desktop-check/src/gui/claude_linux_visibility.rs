//! One supervised, passive state inspection of a source-labelled held editor.

use super::Gui;
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::open_private_new;
use serde_json::{Value, json};
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xa11y::{ElementData, Role};
use zeroize::Zeroizing;

pub(super) fn policy() -> bool {
    [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "Linux"),
        ("NANH_CLAUDE_LINUX_SOURCE_POLICY", "official-2.9939.4"),
        ("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline"),
    ]
    .into_iter()
    .all(|(key, expected)| std::env::var(key).as_deref() == Ok(expected))
}

const OBSERVATIONS: [&str; 5] = [
    "visible",
    "showing",
    "boundsPositive",
    "checkedAncestorCount",
    "hiddenAncestorCount",
];

fn unavailable(stage: &str) -> Value {
    let mut value = json!({"schemaVersion":1,"mechanism":"claude-linux-classic-visibility",
        "diagnosticsOnly":true,"status":"unavailable","stage":stage});
    for key in OBSERVATIONS {
        value[key] = Value::Null;
    }
    value
}

fn root_endpoint(data: &ElementData, pid: u32) -> Option<(&str, &str)> {
    if data.role != Role::Application || data.pid != Some(pid) {
        return None;
    }
    let bus = data.raw.get("bus_name")?.as_str()?;
    let path = data.stable_id.as_deref()?;
    (!bus.is_empty() && bus.len() <= 256 && !path.is_empty() && path.len() <= 256)
        .then_some((bus, path))
}

fn decode(bytes: &[u8]) -> Option<Value> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object()?;
    if object.len() != 10
        || value["schemaVersion"] != 1
        || value["mechanism"] != "claude-linux-classic-visibility"
        || value["diagnosticsOnly"] != true
        || object.keys().any(|key| {
            !OBSERVATIONS.contains(&key.as_str())
                && ![
                    "schemaVersion",
                    "mechanism",
                    "diagnosticsOnly",
                    "status",
                    "stage",
                ]
                .contains(&key.as_str())
        })
    {
        return None;
    }
    let stage = value["stage"].as_str()?;
    match value["status"].as_str()? {
        "complete" => {
            let checked = value["checkedAncestorCount"].as_u64()?;
            let hidden = value["hiddenAncestorCount"].as_u64()?;
            if stage != "complete"
                || checked > 32
                || hidden > checked
                || OBSERVATIONS[..3]
                    .iter()
                    .any(|key| !value[*key].is_boolean())
            {
                return None;
            }
        }
        status @ ("unavailable" | "changed" | "limit") => {
            if ![
                "policy", "deadline", "guard", "source", "owner", "state", "bounds", "parent",
                "identity",
            ]
            .contains(&stage)
                || OBSERVATIONS.iter().any(|key| !value[*key].is_null())
                || (status == "changed" && stage != "identity")
                || (status == "limit" && stage != "parent")
            {
                return None;
            }
        }
        _ => return None,
    }
    Some(value)
}

fn supervise(driver: &Path, request: Zeroizing<String>, deadline: Instant) -> Option<Value> {
    if Instant::now() >= deadline {
        return None;
    }
    let mut child = Command::new("/usr/bin/python3")
        .arg(driver)
        .env_clear()
        .envs(
            [
                "DBUS_SESSION_BUS_ADDRESS",
                "GITHUB_ACTIONS",
                "RUNNER_ENVIRONMENT",
                "RUNNER_OS",
                "NANH_CLAUDE_LINUX_SOURCE_POLICY",
                "NANH_DESKTOP_QUALIFICATION_MODE",
            ]
            .into_iter()
            .filter_map(|key| std::env::var_os(key).map(|value| (key, value))),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let outcome = std::thread::scope(|scope| {
        let mut input = child.stdin.take()?;
        let output = child.stdout.take()?;
        let writer = scope.spawn(move || input.write_all(request.as_bytes()));
        let reader = scope.spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            output.take(4097).read_to_end(&mut bytes).ok()?;
            Some(bytes)
        });
        let status = loop {
            if Instant::now() >= deadline {
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                ),
                Err(_) => break None,
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let written = writer.join().ok()?.ok();
        let bytes = reader.join().ok()??;
        if written.is_none()
            || !status.is_some_and(|status| status.success())
            || bytes.len() > 4096
            || Instant::now() >= deadline
        {
            return None;
        }
        decode(&bytes)
    });
    if outcome.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

impl Gui {
    fn linux_visibility(&self, deadline: Instant) -> Value {
        if Instant::now() >= deadline {
            return unavailable("deadline");
        }
        if !self.visual.linux_passive_composer_guard_until(deadline) {
            return unavailable("guard");
        }
        let request = (|| {
            // App::by_pid already retained this endpoint during acquisition.
            // Resolve uniqueness inside the supervised helper, not an uncancellable locator.
            let data = &self.app.as_ref()?.data;
            let (bus, path) = root_endpoint(data, self.visual.pid())?;
            let driver = std::env::var_os("FEASIBILITY_CLAUDE_VISIBILITY_DRIVER")
                .map(std::path::PathBuf::from)?;
            if !driver.is_absolute() || !driver.is_file() || driver.is_symlink() {
                return None;
            }
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64();
            if remaining <= 0.0 {
                return None;
            }
            let payload = Zeroizing::new(
                json!({"pid":self.visual.pid(),"bus":bus,"path":path,"remaining":remaining})
                    .to_string(),
            );
            supervise(&driver, payload, deadline)
        })();
        if Instant::now() >= deadline {
            return unavailable("deadline");
        }
        if !self.visual.linux_passive_composer_guard_until(deadline) {
            return unavailable("guard");
        }
        if Instant::now() >= deadline {
            return unavailable("deadline");
        }
        request.unwrap_or_else(|| unavailable("source"))
    }

    pub(super) fn record_linux_classic_visibility(&self, directory: &Path, owner: u32) {
        if self.kind != DesktopHarnessKind::Claude || !policy() {
            return;
        }
        let value = self.initial_deadline.get().map_or_else(
            || unavailable("deadline"),
            |deadline| self.linux_visibility(deadline),
        );
        let _ = open_private_new(
            &directory.join(format!("claude-linux-classic-visibility-{owner}.json")),
        )
        .and_then(|mut file| file.write_all(value.to_string().as_bytes()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_never_publishes_partial_states_or_private_values() {
        let value = unavailable("deadline");
        assert!(decode(value.to_string().as_bytes()).is_some());
        let mut partial = value.clone();
        partial["visible"] = json!(false);
        assert!(decode(partial.to_string().as_bytes()).is_none());
        let mut private = value;
        private["bus"] = json!("PRIVATE");
        assert!(decode(private.to_string().as_bytes()).is_none());
    }
    #[test]
    fn cached_root_requires_owned_application_and_private_endpoint() {
        let mut root = ElementData {
            role: Role::Application,
            name: None,
            value: None,
            description: None,
            bounds: None,
            actions: Vec::new(),
            states: xa11y::StateSet::default(),
            numeric_value: None,
            min_value: None,
            max_value: None,
            stable_id: Some("private-root".into()),
            pid: Some(12),
            raw: Default::default(),
            handle: 0,
        };
        root.raw
            .insert("bus_name".into(), String::from("private-bus").into());
        assert!(root_endpoint(&root, 12).is_some());
        assert!(root_endpoint(&root, 13).is_none());
        root.role = Role::Window;
        assert!(root_endpoint(&root, 12).is_none());
        root.role = Role::Application;
        root.stable_id = None;
        assert!(root_endpoint(&root, 12).is_none());
    }

    #[test]
    fn expired_supervisor_never_spawns_a_missing_driver() {
        assert!(
            supervise(
                Path::new("/missing"),
                Zeroizing::new(String::new()),
                Instant::now()
            )
            .is_none()
        );
    }
}
