use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn native_launch_diagnostic_is_opt_in_and_successful_help_is_not_a_failure() {
    let directory = tempfile::tempdir().unwrap();
    for (argument, fails) in [("--invalid-native-fixture-option", true), ("--help", false)] {
        let path = directory
            .path()
            .join(if fails { "failed.json" } else { "help.json" });
        let mut child = Command::new(env!("CARGO_BIN_EXE_nanh"))
            .arg(argument)
            .env("NAN_NATIVE_LAUNCH_DIAGNOSTIC", &path)
            .env_remove("NAN_API_KEY")
            .env("NAN_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("synthetic parser child exceeded its execution bound");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(!status.success(), fails);
        assert_eq!(path.exists(), fails);
        if fails {
            let raw = std::fs::read(&path).unwrap();
            assert!(raw.len() <= 1024);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&raw).unwrap(),
                serde_json::json!({"schemaVersion":1,"failure":"argument-validation-failed"})
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }
}
