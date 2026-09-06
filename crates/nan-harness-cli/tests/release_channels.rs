//! End-to-end coverage for the two release-discovery sources: an explicit update installs the
//! newest published release, while the maintainer-recommended release stays the startup default.

#[path = "release_channels/fixture.rs"]
mod fixture;
#[path = "release_channels/installer.rs"]
mod installer;
#[cfg(unix)]
#[path = "release_channels/startup.rs"]
mod startup;

#[cfg(unix)]
use fixture::missing_response;
use fixture::{Fixture, candidate_script, failing_response, manifest_response};

#[cfg(unix)]
const PUBLISHED_VERSION: &str = "9.9.9";
const RECOMMENDED_VERSION: &str = "9.9.8";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const OLDER_VERSION: &str = "0.0.1";

#[cfg(unix)]
#[test]
fn an_explicit_update_installs_the_newest_published_release() {
    let candidate = candidate_script(PUBLISHED_VERSION);
    let fixture = Fixture::new(vec![
        (
            "/available.json".to_owned(),
            manifest_response(PUBLISHED_VERSION, &candidate),
        ),
        (
            "/recommended.json".to_owned(),
            manifest_response(CURRENT_VERSION, &candidate),
        ),
        ("/nan".to_owned(), (200, candidate.clone())),
    ]);

    let output = fixture.run_update();

    assert!(output.succeeded(), "{output}");
    assert!(
        output.stdout.contains(&format!(
            "Updating nan-harness {CURRENT_VERSION} -> {PUBLISHED_VERSION}"
        )),
        "{output}"
    );
    assert_eq!(fixture.installed_version(), PUBLISHED_VERSION);
    assert!(
        !fixture.state_path().exists(),
        "an explicit update must not write the startup cache"
    );
}

#[cfg(unix)]
#[test]
fn an_explicit_update_falls_back_to_the_recommended_release_without_a_published_feed() {
    let candidate = candidate_script(RECOMMENDED_VERSION);
    let fixture = Fixture::new(vec![
        ("/available.json".to_owned(), missing_response()),
        (
            "/recommended.json".to_owned(),
            manifest_response(RECOMMENDED_VERSION, &candidate),
        ),
        ("/nan".to_owned(), (200, candidate.clone())),
    ]);

    let output = fixture.run_update();

    assert!(output.succeeded(), "{output}");
    assert_eq!(fixture.installed_version(), RECOMMENDED_VERSION);
    assert!(
        !fixture.state_path().exists(),
        "a fallback explicit update must not write the startup cache"
    );
}

#[test]
fn an_explicit_update_reports_up_to_date_when_neither_source_is_ahead() {
    let candidate = candidate_script(CURRENT_VERSION);
    let fixture = Fixture::new(vec![
        (
            "/available.json".to_owned(),
            manifest_response(CURRENT_VERSION, &candidate),
        ),
        (
            "/recommended.json".to_owned(),
            manifest_response(CURRENT_VERSION, &candidate),
        ),
    ]);

    let output = fixture.run_update();

    assert!(output.succeeded(), "{output}");
    assert!(
        output
            .stdout
            .contains(&format!("nan-harness {CURRENT_VERSION} is up to date")),
        "{output}"
    );
    assert!(
        !fixture.state_path().exists(),
        "an explicit update must not write the startup cache"
    );
    assert_eq!(fixture.requests(), vec!["/available.json".to_owned()]);
}

#[test]
fn an_explicit_update_never_downgrades_an_installation_that_is_ahead() {
    let candidate = candidate_script(OLDER_VERSION);
    let fixture = Fixture::new(vec![
        (
            "/available.json".to_owned(),
            manifest_response(OLDER_VERSION, &candidate),
        ),
        (
            "/recommended.json".to_owned(),
            manifest_response(OLDER_VERSION, &candidate),
        ),
    ]);

    let output = fixture.run_update();

    assert!(output.succeeded(), "{output}");
    assert!(
        output
            .stdout
            .contains(&format!("nan-harness {CURRENT_VERSION} is up to date")),
        "{output}"
    );
    assert_eq!(fixture.installed_version(), CURRENT_VERSION);
    assert_eq!(
        fixture.requests(),
        vec!["/available.json".to_owned()],
        "an explicit update asks the published feed, not the recommended source"
    );
}

/// A feed that answers 404 is absent; anything else is a failure, not an invitation to install
/// whatever the recommended source happens to offer.
#[test]
fn an_explicit_update_reports_a_failing_published_feed_instead_of_falling_back() {
    let candidate = candidate_script(RECOMMENDED_VERSION);
    let fixture = Fixture::new(vec![
        ("/available.json".to_owned(), failing_response()),
        (
            "/recommended.json".to_owned(),
            manifest_response(RECOMMENDED_VERSION, &candidate),
        ),
        ("/nan".to_owned(), (200, candidate.clone())),
    ]);

    let output = fixture.run_update();

    assert!(!output.succeeded(), "{output}");
    assert_eq!(fixture.installed_version(), CURRENT_VERSION);
    assert!(
        !fixture.state_path().exists(),
        "a failed explicit update must not write the startup cache"
    );
}
