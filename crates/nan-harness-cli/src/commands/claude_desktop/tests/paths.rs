use super::super::{DesktopEnvironment, DesktopPaths, DesktopPlatform, tasklist_reports_desktop};
use std::path::PathBuf;

#[test]
fn macos_paths_use_application_support_and_accept_a_nan_override() {
    let environment = DesktopEnvironment {
        home: Some(PathBuf::from("/Users/tester")),
        nan_config: Some(PathBuf::from("/private/nan")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Macos, &environment)
        .expect("macOS paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from(
            "/Users/tester/Library/Application Support/Claude/claude_desktop_config.json"
        )
    );
    assert_eq!(
        paths.profile,
        PathBuf::from(
            "/Users/tester/Library/Application Support/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
        )
    );
    assert_eq!(
        paths.receipt,
        PathBuf::from("/private/nan/claude-desktop-receipt.json")
    );
}

#[test]
fn linux_paths_follow_xdg_config_home() {
    let environment = DesktopEnvironment {
        home: Some(PathBuf::from("/home/tester")),
        xdg_config: Some(PathBuf::from("/var/lib/tester/config")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
        .expect("Linux XDG paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from("/var/lib/tester/config/Claude/claude_desktop_config.json")
    );
    assert_eq!(
        paths.third_party_config,
        PathBuf::from("/var/lib/tester/config/Claude-3p/claude_desktop_config.json")
    );
    assert_eq!(
        paths.lock,
        PathBuf::from("/var/lib/tester/config/nan-harness/claude-desktop.lock")
    );
}

#[test]
fn linux_paths_fall_back_to_the_home_config_directory() {
    let environment = DesktopEnvironment {
        home: Some(PathBuf::from("/home/tester")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
        .expect("Linux home paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from("/home/tester/.config/Claude/claude_desktop_config.json")
    );
    assert_eq!(
        paths.profile,
        PathBuf::from(
            "/home/tester/.config/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
        )
    );
}

#[test]
fn windows_paths_separate_roaming_standard_state_from_local_third_party_state() {
    let environment = DesktopEnvironment {
        app_data: Some(PathBuf::from("roaming")),
        local_app_data: Some(PathBuf::from("local")),
        ..DesktopEnvironment::default()
    };

    let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Windows, &environment)
        .expect("Windows paths");

    assert_eq!(
        paths.normal_config,
        PathBuf::from("roaming/Claude/claude_desktop_config.json")
    );
    assert_eq!(
        paths.third_party_config,
        PathBuf::from("local/Claude-3p/claude_desktop_config.json")
    );
    assert_eq!(
        paths.receipt,
        PathBuf::from("roaming/nan-harness/claude-desktop-receipt.json")
    );
}

#[test]
fn windows_tasklist_detection_ignores_localized_empty_output() {
    assert!(!tasklist_reports_desktop(
        b"INFO: No tasks are running which match the specified criteria.\r\n"
    ));
    assert!(tasklist_reports_desktop(
        b"\"Claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
    ));
    assert!(tasklist_reports_desktop(
        b"\"claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
    ));
}
