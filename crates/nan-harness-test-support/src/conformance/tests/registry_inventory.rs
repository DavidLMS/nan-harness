use crate::conformance::{
    HarnessRegistration, PrimeCleanupTargets, embedded_manifest, harness_registry,
    inventory_drift_fingerprint, inventory_matches, owned_prime_pids_from_status,
    prime_status_path, round_trip_probe, validate_harness_registry,
};
use nan_harness_core::HarnessKind;
use serde_json::json;
use std::path::Path;

#[cfg(unix)]
use crate::conformance::signal_prime_targets_now;

#[test]
fn registry_covers_every_harness_kind_and_manifest() {
    validate_harness_registry().expect("the conformance registry should be complete");
    let kinds = harness_registry()
        .iter()
        .map(|registration| registration.kind)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds.len(), HarnessKind::ALL.len());
    assert!(HarnessKind::ALL.iter().all(|kind| kinds.contains(kind)));
    for registration in harness_registry() {
        assert_eq!(registration.binary_name(), registration.kind.binary_name());
        assert_eq!(
            registration.manifest().expect("embedded manifest").harness,
            registration.kind
        );
    }
}

#[test]
fn codex_inventory_accepts_version_dependent_native_tools() {
    let manifest = harness_registry()
        .iter()
        .find(|registration| registration.kind == HarnessKind::Codex)
        .expect("Codex registration should exist")
        .manifest()
        .expect("Codex manifest should parse");
    let baseline = ["apply_patch", "exec_command", "update_plan", "write_stdin"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    assert!(inventory_matches(HarnessKind::Codex, &manifest, &baseline));

    let current = manifest.tool_names();
    assert!(inventory_matches(HarnessKind::Codex, &manifest, &current));
}

#[test]
fn managed_search_inventory_accepts_connected_and_pending_mcp_servers() {
    for (kind, managed_search_tool) in [
        (HarnessKind::OpenCode, "nan-search_web_search"),
        (HarnessKind::KimiCode, "mcp__nan-search__web_search"),
    ] {
        let manifest = harness_registry()
            .iter()
            .find(|registration| registration.kind == kind)
            .expect("harness registration should exist")
            .manifest()
            .expect("harness manifest should parse");
        let baseline = manifest
            .inventory
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert!(inventory_matches(kind, &manifest, &baseline));

        let mut connected = baseline;
        connected.insert(managed_search_tool.to_owned());
        assert!(inventory_matches(kind, &manifest, &connected));

        connected.insert("undeclared_tool".to_owned());
        assert!(!inventory_matches(kind, &manifest, &connected));
    }
}

#[test]
fn hermes_inventory_accepts_only_declared_dynamic_variants() {
    let manifest = harness_registry()
        .iter()
        .find(|registration| registration.kind == HarnessKind::Hermes)
        .expect("Hermes registration should exist")
        .manifest()
        .expect("Hermes manifest should parse");
    let mut browser_inventory = manifest
        .inventory
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    browser_inventory.insert("browser_exec".to_owned());
    assert!(inventory_matches(
        HarnessKind::Hermes,
        &manifest,
        &browser_inventory
    ));

    browser_inventory.insert("undeclared_tool".to_owned());
    assert!(!inventory_matches(
        HarnessKind::Hermes,
        &manifest,
        &browser_inventory
    ));
}

#[test]
fn inventory_drift_fingerprint_is_stable_and_content_addressed() {
    let expected = ["read_file", "write_file"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    let actual = ["tool_search", "write_file"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    let reordered = ["write_file", "tool_search"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    let fingerprint = inventory_drift_fingerprint(HarnessKind::Hermes, &expected, &actual);
    assert_eq!(
        fingerprint,
        inventory_drift_fingerprint(HarnessKind::Hermes, &expected, &reordered)
    );
    assert_ne!(
        fingerprint,
        inventory_drift_fingerprint(HarnessKind::Codex, &expected, &actual)
    );
    assert_eq!(fingerprint.len(), 64);
    assert!(fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn registry_registration_is_derived_from_canonical_identity() {
    let registration = HarnessRegistration {
        kind: HarnessKind::KimiCode,
    };
    assert_eq!(registration.binary_name(), "kimi");
}

#[test]
fn published_round_trip_probes_are_declared_by_embedded_manifests() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    for registration in harness_registry() {
        let manifest = registration.manifest().expect("embedded manifest");
        let probe = round_trip_probe(registration.kind, workspace.path(), &manifest)
            .expect("published probe should satisfy the manifest contract");
        assert!(manifest.tool_names().contains(&probe.call.name));
    }
}

#[test]
fn prime_round_trip_probe_uses_an_absolute_json_python_path() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let manifest =
        embedded_manifest(HarnessKind::PrimeAgent).expect("Prime manifest should be embedded");
    let probe = round_trip_probe(HarnessKind::PrimeAgent, workspace.path(), &manifest)
        .expect("Prime probe should satisfy the manifest contract");
    let output_path = workspace.path().join("tool-output.txt");
    let literal = serde_json::to_string(&output_path.to_string_lossy())
        .expect("output path should serialize");
    let code = probe.call.input["code"]
        .as_str()
        .expect("Prime probe should contain Python code");
    assert!(output_path.is_absolute());
    assert!(code.contains(&format!("output_path = Path({literal})")));
    assert_eq!(probe.filesystem.path, output_path);
}

#[cfg(unix)]
#[test]
fn prime_status_path_contains_required_system_directories() {
    let path = std::env::split_paths(&prime_status_path()).collect::<Vec<_>>();
    assert!(path.iter().any(|entry| entry == Path::new("/usr/sbin")));
    assert!(path.iter().any(|entry| entry == Path::new("/sbin")));
}

#[test]
fn prime_status_ownership_uses_the_exact_workspace_socket() {
    let status = json!([
        {"socketPath": "/workspace/prime-agent.sock", "pid": 42},
        {"socketPath": "/workspace/prime-agent.sock", "pid": 0},
        {"socketPath": "/other/prime-agent.sock", "pid": 43}
    ]);
    assert_eq!(
        owned_prime_pids_from_status(&status, Path::new("/workspace/prime-agent.sock"))
            .expect("status should parse"),
        vec![42]
    );
}

#[cfg(unix)]
#[test]
fn prime_cleanup_terminates_the_owned_process() {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("owned test process should start");
    let targets = PrimeCleanupTargets::from_pids(&[child.id()]);
    signal_prime_targets_now(&targets, false).expect("owned process should terminate");
    let status = child.wait().expect("owned process should be reaped");
    assert!(!status.success());
}

#[cfg(unix)]
#[test]
fn prime_cleanup_does_not_signal_an_unrelated_shared_process_group_member() {
    use nix::unistd::{Pid, getpgid, getpgrp};
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    let mut owned = std::process::Command::new("/bin/sh")
        .args(["-c", "trap '' TERM; while :; do :; done"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .expect("owned Prime-like process should start");
    let owned_pid = i32::try_from(owned.id()).expect("owned pid should fit");
    let mut unrelated = std::process::Command::new("sleep")
        .arg("30")
        .process_group(owned_pid)
        .spawn()
        .expect("unrelated process should start");

    let process_group =
        getpgid(Some(Pid::from_raw(owned_pid))).expect("owned process group should be readable");
    assert_eq!(process_group.as_raw(), owned_pid);
    assert_ne!(process_group, getpgrp());
    assert_eq!(
        getpgid(Some(Pid::from_raw(
            i32::try_from(unrelated.id()).expect("unrelated pid should fit",)
        )))
        .expect("unrelated process group should be readable"),
        process_group
    );

    let targets = PrimeCleanupTargets::from_pids(&[owned.id()]);
    signal_prime_targets_now(&targets, false).expect("TERM should be delivered");
    signal_prime_targets_now(&targets, true).expect("KILL should be delivered");
    let status = owned.wait().expect("owned process should be reaped");
    assert!(!status.success());
    assert!(
        unrelated
            .try_wait()
            .expect("unrelated status should work")
            .is_none()
    );
    let _ = unrelated.kill();
    let _ = unrelated.wait();
}
