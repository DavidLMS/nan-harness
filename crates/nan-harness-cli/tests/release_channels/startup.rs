//! Startup discovery on the real command path: a copied `nan-harness` runs under a pseudo
//! terminal, so `check_on_start` decides what to offer exactly as it does for a user. These are
//! not inferences from `nanh update`; the update command never reaches this code.

use super::fixture::{Fixture, candidate_script, failing_response, manifest_response};
use nan_harness_test_support::terminal::TerminalCommand;
use std::fs;
use std::time::Duration;

const PUBLISHED_VERSION: &str = "9.9.9";
const RECOMMENDED_VERSION: &str = "9.9.8";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROMPT: &str = "Select an option";

/// Runs one interactive startup, answering the update prompt with `response`. `telemetry off` is
/// the payload command: it reaches the real startup path, touches only the isolated configuration
/// directory, and leaves telemetry disabled in the child.
async fn start(fixture: &Fixture, response: &str) -> String {
    let mut command = TerminalCommand::new(fixture.executable_path(), fixture.home_path())
        .args(["telemetry", "off"])
        .clear_environment()
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "dumb")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .respond_when(PROMPT, response)
        .timeout(Duration::from_secs(30));
    for (name, value) in fixture.environment() {
        command = command.env(name, value);
    }
    for (name, value) in fixture.sources() {
        command = command.env(name, value);
    }
    let output = command.run().await.expect("startup should complete");
    assert!(
        output.status.success(),
        "startup should stay usable: {}",
        output.diagnostic()
    );
    format!("{}{}", output.stdout, output.stderr)
}

fn two_source_fixture() -> Fixture {
    let candidate = candidate_script(RECOMMENDED_VERSION);
    Fixture::new(vec![
        (
            "/available.json".to_owned(),
            manifest_response(PUBLISHED_VERSION, &candidate),
        ),
        (
            "/recommended.json".to_owned(),
            manifest_response(RECOMMENDED_VERSION, &candidate),
        ),
        ("/nan".to_owned(), (200, candidate)),
    ])
}

#[cfg(unix)]
#[tokio::test]
async fn startup_offers_the_recommended_release_and_never_reads_the_published_feed() {
    let fixture = two_source_fixture();

    let transcript = start(&fixture, "2").await;

    assert!(
        transcript.contains(&format!("{CURRENT_VERSION} -> {RECOMMENDED_VERSION}")),
        "startup should offer the recommended release: {transcript}"
    );
    assert!(
        !transcript.contains(PUBLISHED_VERSION),
        "startup must not mention the newer published release: {transcript}"
    );
    assert_eq!(fixture.requests(), vec!["/recommended.json".to_owned()]);
    let state = fs::read_to_string(fixture.state_path()).expect("startup state should exist");
    assert!(
        state.contains(&format!("\"version\": \"{RECOMMENDED_VERSION}\"")),
        "deferring should cache the recommended release: {state}"
    );
    assert_eq!(fixture.installed_version(), CURRENT_VERSION);
}

#[cfg(unix)]
#[tokio::test]
async fn a_second_startup_reuses_the_cached_recommendation() {
    let fixture = two_source_fixture();

    start(&fixture, "2").await;
    let transcript = start(&fixture, "2").await;

    assert!(
        transcript.contains(&format!("{CURRENT_VERSION} -> {RECOMMENDED_VERSION}")),
        "a deferred release stays visible: {transcript}"
    );
    assert_eq!(
        fixture.requests(),
        vec!["/recommended.json".to_owned()],
        "the one-hour cache should answer the second startup"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_skipped_release_is_not_offered_again() {
    let fixture = two_source_fixture();

    start(&fixture, "3").await;
    let state = fs::read_to_string(fixture.state_path()).expect("startup state should exist");
    assert!(
        state.contains(&format!("\"skippedVersion\": \"{RECOMMENDED_VERSION}\"")),
        "skipping should record the exact version: {state}"
    );

    let transcript = start(&fixture, "2").await;

    assert!(
        !transcript.contains(PROMPT),
        "a skipped release must not be offered again: {transcript}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn startup_stays_silent_when_the_recommended_release_is_not_newer() {
    let candidate = candidate_script(CURRENT_VERSION);
    let fixture = Fixture::new(vec![
        (
            "/available.json".to_owned(),
            manifest_response(PUBLISHED_VERSION, &candidate),
        ),
        (
            "/recommended.json".to_owned(),
            manifest_response(CURRENT_VERSION, &candidate),
        ),
    ]);

    let transcript = start(&fixture, "2").await;

    assert!(
        !transcript.contains(PROMPT),
        "an installation that is not behind the recommendation must not be prompted: {transcript}"
    );
    assert_eq!(fixture.requests(), vec!["/recommended.json".to_owned()]);
}

#[cfg(unix)]
#[tokio::test]
async fn a_failing_release_source_leaves_startup_usable() {
    let fixture = Fixture::new(vec![
        ("/recommended.json".to_owned(), failing_response()),
        (
            "/available.json".to_owned(),
            manifest_response(PUBLISHED_VERSION, &candidate_script(PUBLISHED_VERSION)),
        ),
    ]);

    let transcript = start(&fixture, "2").await;

    assert!(
        transcript.contains("Telemetry is off."),
        "the command itself should still run: {transcript}"
    );
    assert!(
        !fixture.state_path().exists(),
        "a failed check must not write a cache entry"
    );
}
