use super::constants::ROUND_TRIP_MARKER;
use crate::assertions::ProbeAssertionError;
use crate::manifest::{ConformanceManifest, ToolManifestEntry};
use crate::scripted_provider::ScriptedToolCall;
use nan_harness_core::HarnessKind;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone)]
pub(crate) struct FilesystemContract {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    pub(crate) must_change: bool,
    pub(crate) before: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RoundTripProbe {
    pub(crate) call: ScriptedToolCall,
    pub(crate) filesystem: FilesystemContract,
}

#[derive(Debug, Error)]
#[error("round-trip probe tool '{tool}' is not declared by the {kind} manifest")]
pub(crate) struct ProbeSelectionError {
    kind: HarnessKind,
    tool: String,
}

#[allow(clippy::too_many_lines)]
pub(crate) fn round_trip_probe(
    kind: HarnessKind,
    workspace: &Path,
    manifest: &ConformanceManifest,
) -> Result<RoundTripProbe, ProbeSelectionError> {
    let read_path = workspace.join("read-target.txt");
    let read_path_string = read_path.to_string_lossy().into_owned();
    let (name, input, filesystem) = match kind {
        HarnessKind::ClaudeCode => (
            "Write",
            json!({
                "file_path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Codex => (
            "exec_command",
            json!({"cmd": "printf NAN_HARNESS_TOOL_OK > tool-output.txt"}),
            filesystem_contract(
                workspace.join("tool-output.txt"),
                "NAN_HARNESS_TOOL_OK",
                true,
            ),
        ),
        HarnessKind::OpenCode => (
            "bash",
            json!({"command": "printf NAN_HARNESS_TOOL_OK > tool-output.txt"}),
            filesystem_contract(
                workspace.join("tool-output.txt"),
                "NAN_HARNESS_TOOL_OK",
                true,
            ),
        ),
        HarnessKind::Hermes | HarnessKind::Fx => (
            "write_file",
            json!({
                "path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Pi | HarnessKind::Omp | HarnessKind::OpenClaw => (
            "write",
            json!({
                "path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::PrimeAgent => {
            let output_path = workspace.join("tool-output.txt");
            let output_path_literal = serde_json::to_string(&output_path.to_string_lossy())
                .expect("Prime output path should serialize as a JSON string literal");
            (
                "ipython",
                json!({
                    "code": format!(
                        "from pathlib import Path; output_path = Path({output_path_literal}); output_path.write_text('{ROUND_TRIP_MARKER}', encoding='utf-8'); output_path.read_text(encoding='utf-8')"
                    )
                }),
                filesystem_contract(output_path, ROUND_TRIP_MARKER, true),
            )
        }
        HarnessKind::DeepSeekHarness => (
            "write",
            json!({
                "file_path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Cline => (
            "run_commands",
            json!({
                "commands": [format!(
                    "printf NAN_HARNESS_TOOL_OK > '{}'",
                    workspace.join("tool-output.txt").display()
                )]
            }),
            filesystem_contract(
                workspace.join("tool-output.txt"),
                "NAN_HARNESS_TOOL_OK",
                true,
            ),
        ),
        HarnessKind::QwenCode => (
            "read_file",
            json!({"file_path": read_path_string}),
            filesystem_contract(read_path, "READ_TARGET_CONTENT\n", false),
        ),
        HarnessKind::KimiCode => (
            "Write",
            json!({"path": "tool-output.txt", "content": ROUND_TRIP_MARKER}),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Aider => (
            "edit-protocol",
            json!({}),
            filesystem_contract(workspace.join("edit-target.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Goose => (
            "write",
            json!({"path": "round-trip.txt", "content": "NAN_HARNESS_TOOL_OK\n"}),
            filesystem_contract(
                workspace.join("round-trip.txt"),
                "NAN_HARNESS_TOOL_OK\n",
                true,
            ),
        ),
    };
    if !manifest.tool_names().contains(name) {
        return Err(ProbeSelectionError {
            kind,
            tool: name.to_owned(),
        });
    }
    Ok(RoundTripProbe {
        call: ScriptedToolCall {
            name: name.to_owned(),
            input,
            result_expected: true,
        },
        filesystem,
    })
}

fn filesystem_contract(path: PathBuf, text: &str, must_change: bool) -> FilesystemContract {
    FilesystemContract {
        path,
        text: text.to_owned(),
        must_change,
        before: must_change.then(|| "EDIT_TARGET_BEFORE\n".to_owned()),
    }
}

pub(crate) fn verify_probe_side_effect(probe: &RoundTripProbe) -> Result<(), ProbeAssertionError> {
    let contents = fs::read_to_string(&probe.filesystem.path)
        .map_err(|error| ProbeAssertionError::Filesystem(error.to_string()))?;
    if !contents.contains(&probe.filesystem.text) {
        return Err(ProbeAssertionError::MissingFilesystemSideEffect(
            probe.filesystem.path.clone(),
        ));
    }
    if probe.filesystem.must_change
        && probe
            .filesystem
            .before
            .as_ref()
            .is_some_and(|before| contents == *before)
    {
        return Err(ProbeAssertionError::MissingFilesystemSideEffect(
            probe.filesystem.path.clone(),
        ));
    }
    Ok(())
}

pub(crate) fn inventory_matches(
    kind: HarnessKind,
    manifest: &ConformanceManifest,
    actual: &BTreeSet<String>,
) -> bool {
    if kind == HarnessKind::Aider {
        return actual.is_empty();
    }
    let expected = manifest.tool_names();
    if kind == HarnessKind::Hermes {
        let required = manifest
            .inventory
            .iter()
            .map(String::as_str)
            .chain(manifest.tools.iter().flat_map(ToolManifestEntry::names))
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();
        let optional = manifest
            .optional_inventory
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let dynamic = actual
            .difference(&required)
            .filter(|name| !optional.contains(*name))
            .cloned()
            .collect::<BTreeSet<_>>();
        let configured_variant = manifest
            .dynamic_inventory
            .iter()
            .any(|variant| variant.iter().cloned().collect::<BTreeSet<_>>() == dynamic);
        return required.is_subset(actual)
            && actual.iter().all(|name| {
                required.contains(name) || optional.contains(name) || dynamic.contains(name)
            })
            && configured_variant;
    }
    let required = manifest.inventory.iter().all(|name| actual.contains(name))
        && manifest
            .tools
            .iter()
            .all(|entry| entry.names().any(|name| actual.contains(name)));
    required && actual.is_subset(&expected)
}

pub(crate) fn inventory_drift_fingerprint(
    kind: HarnessKind,
    expected: &BTreeSet<String>,
    actual: &BTreeSet<String>,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"inventory-drift\0");
    digest.update(kind.to_string().as_bytes());
    for name in expected {
        digest.update(b"\0expected\0");
        digest.update(name.as_bytes());
    }
    for name in actual {
        digest.update(b"\0actual\0");
        digest.update(name.as_bytes());
    }
    let mut fingerprint = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(fingerprint, "{byte:02x}").expect("writing to a String cannot fail");
    }
    fingerprint
}
