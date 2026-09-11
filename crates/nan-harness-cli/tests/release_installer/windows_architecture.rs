use crate::fixtures::serve_release;
use crate::platform::{assert_alias, assert_success, assert_version, binary_file_name};
use crate::support::installer_process_with_architecture;

#[test]
fn release_installer_accepts_native_and_wow64_x64_environments() {
    for (process, native) in [("AMD64", ""), ("x86", "AMD64")] {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let install_directory = directory.path().join("bin");
        let (base_url, server) = serve_release();
        let output = installer_process_with_architecture(
            directory.path(),
            &directory.path().join("home"),
            &install_directory,
            &directory.path().join("state"),
            &base_url,
            process,
            native,
        )
        .output()
        .expect("Windows PowerShell installer should start");
        assert_success("Windows architecture installation", &output);
        server
            .join()
            .expect("release server should finish")
            .expect("release server should deliver every file");
        assert_version(&install_directory.join(binary_file_name()));
        assert_alias(&install_directory);
    }
}

#[test]
fn release_installer_rejects_unsupported_or_missing_windows_architectures() {
    for (process, native, reason) in [
        ("x86", "", "does not publish a Windows binary for x86"),
        ("ARM64", "", "does not publish a Windows binary for ARM64"),
        (
            "AMD64",
            "ARM64",
            "does not publish a Windows binary for ARM64",
        ),
        (
            "unknown",
            "",
            "does not publish a Windows binary for unknown",
        ),
        ("", "", "could not determine the Windows architecture"),
    ] {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let install_directory = directory.path().join("bin");
        let output = installer_process_with_architecture(
            directory.path(),
            &directory.path().join("home"),
            &install_directory,
            &directory.path().join("state"),
            "http://127.0.0.1:9",
            process,
            native,
        )
        .output()
        .expect("Windows PowerShell installer should start");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(reason),
            "unexpected installer error: {stderr}"
        );
        assert!(!install_directory.exists());
    }
}
