use super::*;
use crate::cli::{SessionMode, VerificationPolicy};
use std::ffi::{OsStr, OsString};

struct Fixture {
    journal: Journal,
    facts: PathBuf,
    _temporary: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let facts = temporary.path().join("facts");
        create_private_dir(&facts).unwrap();
        Self {
            journal: Journal::create(temporary.path()).unwrap(),
            facts,
            _temporary: temporary,
        }
    }

    fn probe(&mut self, app: DesktopHarnessKind, index: usize) -> (ProbeSpec, PathBuf) {
        let root = self
            .journal
            .reserve(&format!("{app}-deterministic-{index}"))
            .unwrap();
        let spec = ProbeSpec {
            kind: app,
            nan_harness: PathBuf::from("/synthetic/nanh"),
            nan_harness_sha256: "a".repeat(64),
            executable: PathBuf::from("/synthetic/app"),
            workspace: root.join("workspace"),
            model: "qwen3.6".into(),
            live: false,
            probe_index: Some(index),
            session: SessionMode::GithubHosted,
            verification: VerificationPolicy::SemanticOnly,
            launch_wrapper: None,
        };
        (spec, root)
    }

    fn prepare(&self, spec: &ProbeSpec, root: &Path) -> Result<Option<WorkerFacts>, Reason> {
        prepare_worker_facts(spec, root, "windows", Some(&self.facts), Some(&self.facts))
    }
}

#[test]
fn same_owner_in_distinct_factory_probes_keeps_current_connection_records_separate() {
    let mut fixture = Fixture::new();
    let (first, first_root) = fixture.probe(DesktopHarnessKind::ChatGpt, 0);
    let (second, second_root) = fixture.probe(DesktopHarnessKind::ChatGpt, 1);
    let first = fixture.prepare(&first, &first_root).unwrap().unwrap();
    let second = fixture.prepare(&second, &second_root).unwrap().unwrap();
    let filename = "connection-41.json";
    let first_path = first.directory.join(filename);
    let second_path = second.directory.join(filename);
    assert_ne!(first_path, second_path);
    for (path, child) in [(&first_path, 17), (&second_path, 18)] {
        let mut file = open_private_new(path).unwrap();
        write!(
            file,
            "{{\"schemaVersion\":1,\"port\":43210,\"launcherPid\":{child}}}"
        )
        .unwrap();
        file.sync_all().unwrap();
    }
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&first_path).unwrap()).unwrap()
            ["launcherPid"],
        17
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&second_path).unwrap()).unwrap()
            ["launcherPid"],
        18
    );
    assert_eq!(
        open_private_new(&first_path).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert!(std::fs::read_to_string(&first_path).unwrap().contains("17"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&first.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    #[cfg(windows)]
    assert_eq!(
        nan_harness_private_fs::classify_owned_windows_dacl(
            &retain_facts_directory(&first.directory).unwrap(),
            nan_harness_private_fs::PrivatePathKind::Directory
        ),
        nan_harness_private_fs::OwnedWindowsDacl::Protected
    );
}

#[test]
fn scoped_command_binds_both_roots_without_mutating_parent_or_other_environment() {
    let mut fixture = Fixture::new();
    let (spec, root) = fixture.probe(DesktopHarnessKind::ChatGpt, 2);
    let facts = fixture.prepare(&spec, &root).unwrap().unwrap();
    let keys = ["FEASIBILITY_FACTS", "NANH_DESKTOP_QUALIFICATION_FACTS"];
    let before = keys.map(std::env::var_os);
    let mut command = tokio::process::Command::new("synthetic-never-executed");
    command
        .env("KEEP_UNRELATED", "yes")
        .env_remove("REMOVE_UNRELATED");
    facts.bind(&mut command);
    let environment = command
        .as_std()
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(OsStr::to_os_string)))
        .collect::<std::collections::BTreeMap<_, _>>();
    for key in keys {
        assert_eq!(
            environment[OsStr::new(key)],
            Some(facts.directory.clone().into_os_string())
        );
    }
    assert_eq!(
        environment[OsStr::new("KEEP_UNRELATED")],
        Some(OsString::from("yes"))
    );
    assert_eq!(environment[OsStr::new("REMOVE_UNRELATED")], None);
    assert_eq!(keys.map(std::env::var_os), before);
}

#[test]
fn all_hosted_windows_non_zed_apps_use_the_existing_closed_probe_name() {
    let mut fixture = Fixture::new();
    for app in [
        DesktopHarnessKind::ChatGpt,
        DesktopHarnessKind::Claude,
        DesktopHarnessKind::Hermes,
        DesktopHarnessKind::Pen,
    ] {
        let (spec, root) = fixture.probe(app, 0);
        let facts = fixture.prepare(&spec, &root).unwrap().unwrap();
        assert_eq!(
            facts.directory,
            fixture.facts.join(format!("{app}-deterministic-0"))
        );
    }
}

#[test]
fn missing_mismatched_or_invalid_factory_bindings_refuse_before_namespace_creation() {
    let mut fixture = Fixture::new();
    let (spec, root) = fixture.probe(DesktopHarnessKind::ChatGpt, 0);
    for (facts, qualification) in [
        (None, None),
        (Some(fixture.facts.as_path()), None),
        (None, Some(fixture.facts.as_path())),
        (Some(fixture.facts.as_path()), Some(root.as_path())),
        (Some(Path::new("relative")), Some(Path::new("relative"))),
    ] {
        assert_eq!(
            prepare_worker_facts(&spec, &root, "windows", facts, qualification).err(),
            Some(Reason::IsolationUnavailable)
        );
    }
    for invalid in [
        ProbeSpec {
            probe_index: None,
            ..spec.clone()
        },
        ProbeSpec {
            probe_index: Some(3),
            ..spec.clone()
        },
        ProbeSpec {
            probe_index: Some(1),
            ..spec.clone()
        },
        ProbeSpec {
            workspace: root.join("other"),
            ..spec.clone()
        },
    ] {
        assert_eq!(
            fixture.prepare(&invalid, &root).err(),
            Some(Reason::IsolationUnavailable)
        );
    }
    assert_eq!(
        fixture
            .prepare(&spec, Path::new("chatgpt-desktop-deterministic-0"))
            .err(),
        Some(Reason::IsolationUnavailable)
    );
    let absent = fixture.facts.join("absent");
    assert_eq!(
        prepare_worker_facts(&spec, &root, "windows", Some(&absent), Some(&absent)).err(),
        Some(Reason::IsolationUnavailable)
    );
    assert_eq!(std::fs::read_dir(&fixture.facts).unwrap().count(), 0);
}

#[test]
fn existing_namespace_is_preserved_instead_of_adopted_or_emptied() {
    let mut fixture = Fixture::new();
    let (spec, root) = fixture.probe(DesktopHarnessKind::ChatGpt, 0);
    let namespace = fixture.facts.join("chatgpt-desktop-deterministic-0");
    create_private_dir(&namespace).unwrap();
    let original = namespace.join("connection-41.json");
    std::fs::write(&original, b"PRIVATE_ORIGINAL").unwrap();
    assert_eq!(
        fixture.prepare(&spec, &root).err(),
        Some(Reason::IsolationUnavailable)
    );
    assert_eq!(std::fs::read(&original).unwrap(), b"PRIVATE_ORIGINAL");
}

#[cfg(unix)]
#[test]
fn symlink_roots_or_namespaces_and_nonprivate_roots_are_rejected_without_repair() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let mut fixture = Fixture::new();
    let (spec, root) = fixture.probe(DesktopHarnessKind::ChatGpt, 0);
    let link = fixture.facts.parent().unwrap().join("linked-facts");
    symlink(&fixture.facts, &link).unwrap();
    assert_eq!(
        prepare_worker_facts(&spec, &root, "windows", Some(&link), Some(&link)).err(),
        Some(Reason::IsolationUnavailable)
    );
    let namespace = fixture.facts.join("chatgpt-desktop-deterministic-0");
    symlink(&root, &namespace).unwrap();
    assert_eq!(
        fixture.prepare(&spec, &root).err(),
        Some(Reason::IsolationUnavailable)
    );
    assert!(namespace.is_symlink());
    std::fs::remove_file(namespace).unwrap();
    std::fs::set_permissions(&fixture.facts, std::fs::Permissions::from_mode(0o750)).unwrap();
    assert_eq!(
        fixture.prepare(&spec, &root).err(),
        Some(Reason::IsolationUnavailable)
    );
    assert_eq!(
        std::fs::metadata(&fixture.facts)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o750
    );
}

#[test]
fn unrelated_modes_ignore_scope_only_inputs_and_keep_their_environment() {
    let mut fixture = Fixture::new();
    let (spec, _) = fixture.probe(DesktopHarnessKind::ChatGpt, 0);
    for platform in ["linux", "macos"] {
        assert!(
            prepare_worker_facts(&spec, Path::new("relative"), platform, None, None)
                .unwrap()
                .is_none()
        );
    }
    for other in [
        ProbeSpec {
            kind: DesktopHarnessKind::Zed,
            ..spec.clone()
        },
        ProbeSpec {
            live: true,
            ..spec.clone()
        },
        ProbeSpec {
            session: SessionMode::PrivateProfile,
            ..spec.clone()
        },
        ProbeSpec {
            verification: VerificationPolicy::AccessibilityOrOcr,
            ..spec
        },
    ] {
        assert!(
            prepare_worker_facts(&other, Path::new("relative"), "windows", None, None)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(std::fs::read_dir(&fixture.facts).unwrap().count(), 0);
}
