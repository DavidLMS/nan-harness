use super::{DesktopCompatibilityStatus, evaluate_desktop_compatibility};

// The evaluator resolves its row from the build target, so platform-specific
// tests are the only way to exercise the actual embedded ChatGPT contract.
// The registry rows currently provide all four bounds; therefore the evaluator's
// Unavailable and missing-evidence branches are outside this test scope.
#[cfg(target_os = "macos")]
mod macos {
    use super::{DesktopCompatibilityStatus, evaluate_desktop_compatibility};
    use semver::Version;

    const APP_EQ: Version = Version::new(26, 825, 51511);
    const RUNTIME_EQ: &str = "0.151.0-alpha.7.2";

    fn app_neighbor(delta: i64) -> Version {
        let patch = if delta < 0 {
            APP_EQ
                .patch
                .checked_sub(delta.unsigned_abs())
                .expect("app patch boundary should have a predecessor")
        } else {
            APP_EQ
                .patch
                .checked_add(delta.unsigned_abs())
                .expect("app patch boundary should have a successor")
        };
        Version::new(APP_EQ.major, APP_EQ.minor, patch)
    }

    fn runtime_neighbor(final_segment: u64) -> Version {
        Version::parse(&format!("0.151.0-alpha.7.{final_segment}"))
            .expect("runtime prerelease version should parse")
    }

    #[test]
    fn equal_bounds_are_tested_and_propagate_the_complete_report() {
        let report = evaluate_desktop_compatibility(&APP_EQ, &runtime_neighbor(2))
            .expect("macOS ChatGPT evaluation should succeed");

        assert_eq!(report.status, DesktopCompatibilityStatus::Tested);
        assert_eq!(report.minimum_app_version, APP_EQ);
        assert_eq!(report.last_compatible_app_version, APP_EQ);
        assert_eq!(
            report.minimum_bundled_codex_version,
            Version::parse(RUNTIME_EQ).expect("runtime minimum should parse")
        );
        assert_eq!(
            report.last_compatible_bundled_codex_version,
            Version::parse(RUNTIME_EQ).expect("runtime maximum should parse")
        );
        assert_eq!(report.compatible_at, "2026-08-30");
    }

    #[test]
    fn independently_newer_app_or_runtime_is_newer_untested() {
        let newer_app = evaluate_desktop_compatibility(&app_neighbor(1), &runtime_neighbor(2))
            .expect("newer app evaluation should succeed");
        let newer_runtime = evaluate_desktop_compatibility(&APP_EQ, &runtime_neighbor(3))
            .expect("newer runtime evaluation should succeed");

        assert_eq!(newer_app.status, DesktopCompatibilityStatus::NewerUntested);
        assert_eq!(
            newer_runtime.status,
            DesktopCompatibilityStatus::NewerUntested
        );
    }

    #[test]
    fn independently_older_app_or_runtime_is_older_unsupported() {
        let older_app = evaluate_desktop_compatibility(&app_neighbor(-1), &runtime_neighbor(2))
            .expect("older app evaluation should succeed");
        let older_runtime = evaluate_desktop_compatibility(&APP_EQ, &runtime_neighbor(1))
            .expect("older runtime evaluation should succeed");

        assert_eq!(
            older_app.status,
            DesktopCompatibilityStatus::OlderUnsupported
        );
        assert_eq!(
            older_runtime.status,
            DesktopCompatibilityStatus::OlderUnsupported
        );
    }

    #[test]
    fn an_older_component_wins_over_a_newer_component() {
        let older_app_newer_runtime =
            evaluate_desktop_compatibility(&app_neighbor(-1), &runtime_neighbor(3))
                .expect("mixed app/runtime evaluation should succeed");
        let newer_app_older_runtime =
            evaluate_desktop_compatibility(&app_neighbor(1), &runtime_neighbor(1))
                .expect("mixed app/runtime evaluation should succeed");

        assert_eq!(
            older_app_newer_runtime.status,
            DesktopCompatibilityStatus::OlderUnsupported
        );
        assert_eq!(
            newer_app_older_runtime.status,
            DesktopCompatibilityStatus::OlderUnsupported
        );
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{DesktopCompatibilityStatus, evaluate_desktop_compatibility};
    use semver::Version;

    const APP_EQ: Version = Version::new(26, 825, 51511);
    const RUNTIME_EQ: &str = "0.151.0-alpha.7.2";

    fn app_neighbor(delta: i64) -> Version {
        let patch = if delta < 0 {
            APP_EQ
                .patch
                .checked_sub(delta.unsigned_abs())
                .expect("app patch boundary should have a predecessor")
        } else {
            APP_EQ
                .patch
                .checked_add(delta.unsigned_abs())
                .expect("app patch boundary should have a successor")
        };
        Version::new(APP_EQ.major, APP_EQ.minor, patch)
    }

    fn runtime_neighbor(final_segment: u64) -> Version {
        Version::parse(&format!("0.151.0-alpha.7.{final_segment}"))
            .expect("runtime prerelease version should parse")
    }

    #[test]
    fn equal_and_above_bounds_are_contract_only() {
        let equal = evaluate_desktop_compatibility(&APP_EQ, &runtime_neighbor(2))
            .expect("Linux ChatGPT evaluation should succeed");
        let above = evaluate_desktop_compatibility(&app_neighbor(1), &runtime_neighbor(3))
            .expect("above-bound evaluation should succeed");

        assert_eq!(equal.status, DesktopCompatibilityStatus::ContractOnly);
        assert_eq!(above.status, DesktopCompatibilityStatus::ContractOnly);
        assert_eq!(equal.minimum_app_version, APP_EQ);
        assert_eq!(equal.last_compatible_app_version, APP_EQ);
        assert_eq!(
            equal.minimum_bundled_codex_version,
            Version::parse(RUNTIME_EQ).expect("runtime minimum should parse")
        );
        assert_eq!(
            equal.last_compatible_bundled_codex_version,
            Version::parse(RUNTIME_EQ).expect("runtime maximum should parse")
        );
        assert_eq!(equal.compatible_at, "2026-08-31");
    }

    #[test]
    fn below_minimum_app_or_runtime_is_older_unsupported() {
        let older_app = evaluate_desktop_compatibility(&app_neighbor(-1), &runtime_neighbor(2))
            .expect("older app evaluation should succeed");
        let older_runtime = evaluate_desktop_compatibility(&APP_EQ, &runtime_neighbor(1))
            .expect("older runtime evaluation should succeed");

        assert_eq!(
            older_app.status,
            DesktopCompatibilityStatus::OlderUnsupported
        );
        assert_eq!(
            older_runtime.status,
            DesktopCompatibilityStatus::OlderUnsupported
        );
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{DesktopCompatibilityStatus, evaluate_desktop_compatibility};
    use semver::Version;

    const APP_EQ: Version = Version::new(0, 0, 0);
    const APP_LAST: Version = Version::new(999_999, 0, 0);
    const RUNTIME_EQ: Version = Version::new(0, 0, 0);
    const RUNTIME_LAST: Version = Version::new(999_999, 0, 0);

    fn major_successor(version: &Version) -> Version {
        let major = version
            .major
            .checked_add(1)
            .expect("upper bound should have a successor");
        Version::new(major, 0, 0)
    }

    #[test]
    fn minimum_and_upper_bounds_are_contract_only_and_propagate_fields() {
        let at_minimum = evaluate_desktop_compatibility(&APP_EQ, &RUNTIME_EQ)
            .expect("zero minimum evaluation should succeed");
        let at_upper = evaluate_desktop_compatibility(&APP_LAST, &RUNTIME_LAST)
            .expect("upper bound evaluation should succeed");
        let above_upper = evaluate_desktop_compatibility(
            &major_successor(&APP_LAST),
            &major_successor(&RUNTIME_LAST),
        )
        .expect("above upper bound evaluation should succeed");

        assert_eq!(at_minimum.status, DesktopCompatibilityStatus::ContractOnly);
        assert_eq!(at_upper.status, DesktopCompatibilityStatus::ContractOnly);
        assert_eq!(above_upper.status, DesktopCompatibilityStatus::ContractOnly);
        assert_eq!(at_minimum.minimum_app_version, APP_EQ);
        assert_eq!(at_minimum.last_compatible_app_version, APP_LAST);
        assert_eq!(at_minimum.minimum_bundled_codex_version, RUNTIME_EQ);
        assert_eq!(
            at_minimum.last_compatible_bundled_codex_version,
            RUNTIME_LAST
        );
        assert_eq!(at_minimum.compatible_at, "2026-08-31");
    }
}
