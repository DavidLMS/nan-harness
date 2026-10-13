use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::openpty;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn fixture(node: Option<&str>, npm: Option<i32>) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join(".npm-global/bin");
    fs::create_dir_all(&bin).unwrap();
    if let Some(version) = node {
        executable(&bin.join("node"), &format!("#!/bin/sh\necho '{version}'\n"));
    }
    if let Some(code) = npm {
        executable(
            &bin.join("npm"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/npm-calls"
if [ "$1" = --version ]; then echo 10.9.0; exit 0; fi
if [ {code} != 0 ]; then exit {code}; fi
printf '#!/bin/sh\nprintf version-probed > "$HOME/launcher-probe"\nexit 27\n' > "$HOME/.npm-global/bin/gentle-shell"
/bin/chmod 700 "$HOME/.npm-global/bin/gentle-shell"
"#
            ),
        );
    }
    root
}

fn run(root: &Path, input: &[u8]) -> (ExitStatus, String) {
    let pty = openpty(None, None).unwrap();
    let mut master = File::from(pty.master);
    let slave = File::from(pty.slave);
    fcntl(&master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .arg("gentle-shell")
        .env("PATH", root.join(".npm-global/bin"))
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("NAN_HARNESS_STATE_DIR", root.join("state"))
        .env("NAN_HARNESS_LANG", "en")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        .spawn()
        .unwrap();
    master.write_all(input).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut captured = Vec::new();
    loop {
        let status = child.try_wait().unwrap();
        let mut buffer = [0; 4096];
        while let Ok(count) = master.read(&mut buffer) {
            if count == 0 {
                break;
            }
            captured.extend_from_slice(&buffer[..count]);
        }
        if let Some(status) = status {
            return (status, String::from_utf8_lossy(&captured).into_owned());
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "interactive installer timed out: {}",
                String::from_utf8_lossy(&captured)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn gentle_shell_interactive_acceptance_installs_then_validates_launcher() {
    let root = fixture(Some("v22.19.0"), Some(0));
    let (status, output) = run(root.path(), b"y\nn\n");
    assert!(
        !status.success(),
        "synthetic version probe deliberately fails"
    );
    assert!(output.contains("Install Gentle Shell"), "{output}");
    assert_eq!(
        fs::read_to_string(root.path().join("npm-calls")).unwrap(),
        "--version\ninstall --global gentle-pi@latest\n"
    );
    assert!(root.path().join(".npm-global/bin/gentle-shell").is_file());
    assert!(root.path().join("launcher-probe").is_file(), "{output}");
    assert!(output.contains("27"), "{output}");
}

#[test]
fn gentle_shell_interactive_refusal_does_not_install() {
    let root = fixture(Some("v22.19.0"), Some(0));
    let (status, output) = run(root.path(), b"n\n");
    assert!(status.success(), "{output}");
    assert_eq!(
        fs::read_to_string(root.path().join("npm-calls")).unwrap(),
        "--version\n"
    );
    assert!(!root.path().join(".npm-global/bin/gentle-shell").exists());
}

#[test]
fn gentle_shell_interactive_missing_or_old_node_never_reaches_npm() {
    for node in [None, Some("v22.18.0")] {
        let root = fixture(node, Some(0));
        let (status, output) = run(root.path(), b"n\n");
        assert!(!status.success());
        assert!(output.contains("22.19.0"), "{output}");
        assert!(output.contains("nanh gentle-shell"), "{output}");
        assert!(!output.contains("nanh pi"), "{output}");
        assert!(!output.contains("anonymous error report"), "{output}");
        assert!(!root.path().join("npm-calls").exists());
    }
}

#[test]
fn gentle_shell_interactive_missing_npm_is_setup_and_failed_install_is_error() {
    let missing = fixture(Some("v22.19.0"), None);
    let (status, output) = run(missing.path(), b"n\n");
    assert!(!status.success());
    assert!(output.contains("npm"), "{output}");
    assert!(!output.contains("nanh pi"), "{output}");
    assert!(!output.contains("NH-INSTALL-001"), "{output}");
    let failed = fixture(Some("v22.19.0"), Some(42));
    let (status, output) = run(failed.path(), b"y\nn\n");
    assert!(!status.success());
    assert!(output.contains("NH-INSTALL-001"), "{output}");
    assert!(output.contains("42"), "{output}");
    assert!(!failed.path().join(".npm-global/bin/gentle-shell").exists());
}
