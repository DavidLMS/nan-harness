use super::child_process::{ChildScenario, scenario_completed};
use super::{CoordinatorClient, DISABLE_ENVIRONMENT};
use crate::CoordinatorError;
use nan_harness_core::SecretValue;
use std::path::Path;
use std::time::Duration;

/// Exact path of the child scenario that applies one gate environment.
const GATE_SCENARIO: &str = "client::gate_tests::the_managed_gate_child_applies_its_environment";
/// Marks a process as a managed nan-harness launch; owned by `crate::paths`.
const MANAGED_PROCESS_ENVIRONMENT: &str = "NAN_HARNESS_INTERNAL_MANAGED_PROCESS";
/// Redirects private coordinator state; owned by `crate::paths`.
const CONFIG_DIRECTORY_ENVIRONMENT: &str = "NAN_HARNESS_CONFIG_DIR";
const EXPECTATION_ENVIRONMENT: &str = "NAN_HARNESS_TEST_GATE_EXPECTATION";
const CLIENT_EXPECTED: &str = "client";
const NO_CLIENT_EXPECTED: &str = "no-client";
const CHILD_SCENARIO_BUDGET: Duration = Duration::from_secs(30);
const PROVIDER_URL: &str = "https://api.example.com/v1";
const INVALID_PROVIDER_URL: &str = "api.example.com";
const SALT_BYTES: usize = 32;

/// One managed-process gate environment and the client it must produce.
struct GateCase {
    disabled: bool,
    managed: bool,
    expects_client: bool,
}

const GATE_CASES: [GateCase; 4] = [
    GateCase {
        disabled: false,
        managed: false,
        expects_client: false,
    },
    GateCase {
        disabled: true,
        managed: false,
        expects_client: false,
    },
    GateCase {
        disabled: true,
        managed: true,
        expects_client: false,
    },
    GateCase {
        disabled: false,
        managed: true,
        expects_client: true,
    },
];

#[test]
fn only_an_enabled_managed_process_builds_a_client_and_its_private_state() {
    for case in GATE_CASES {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let configuration = temporary.path().join("config");

        run_gate_child(&case, &configuration, &temporary.path().join("marker"));

        let state = configuration.join("coordinator/v1");
        if case.expects_client {
            let salt = std::fs::read(state.join("scope.salt")).expect("a salt should be published");
            assert_eq!(salt.len(), SALT_BYTES);
        } else {
            assert!(
                !configuration.exists(),
                "a closed gate must not create coordinator state"
            );
        }
    }
}

/// Applies one gate environment inside a child process, because the gate reads
/// environment state that is global to the process it runs in.
#[test]
#[ignore = "bounded child scenario of the managed-process gate"]
fn the_managed_gate_child_applies_its_environment() {
    let expects_client = match std::env::var(EXPECTATION_ENVIRONMENT).as_deref() {
        Ok(CLIENT_EXPECTED) => true,
        Ok(NO_CLIENT_EXPECTED) => false,
        expectation => panic!("the parent should state an expectation, not {expectation:?}"),
    };
    let api_key = SecretValue::new("gate-scenario-key").expect("a test credential");

    let client = CoordinatorClient::try_new(PROVIDER_URL, &api_key, "gate-scenario-launch")
        .expect("the gate itself should not fail");
    let invalid =
        CoordinatorClient::try_new(INVALID_PROVIDER_URL, &api_key, "gate-scenario-launch");

    assert_eq!(client.is_some(), expects_client);
    if let Some(client) = client {
        let reused = CoordinatorClient::try_new(PROVIDER_URL, &api_key, "gate-scenario-launch")
            .expect("a published salt should stay usable")
            .expect("an enabled managed process should keep building clients");
        assert_eq!(
            reused.scope, client.scope,
            "a second client must reuse the published salt"
        );
        assert!(
            matches!(invalid, Err(CoordinatorError::Protocol(_))),
            "an open gate should report an unusable provider URL"
        );
    } else {
        assert!(
            matches!(invalid, Ok(None)),
            "a closed gate should not inspect the provider URL"
        );
    }
    scenario_completed();
}

fn run_gate_child(case: &GateCase, configuration: &Path, marker: &Path) {
    let mut scenario = ChildScenario::new(GATE_SCENARIO, marker);
    scenario
        .with_env(CONFIG_DIRECTORY_ENVIRONMENT, configuration)
        .with_env(
            EXPECTATION_ENVIRONMENT,
            if case.expects_client {
                CLIENT_EXPECTED
            } else {
                NO_CLIENT_EXPECTED
            },
        );
    if case.disabled {
        scenario.with_env(DISABLE_ENVIRONMENT, "1");
    } else {
        scenario.without_env(DISABLE_ENVIRONMENT);
    }
    if case.managed {
        scenario.with_env(MANAGED_PROCESS_ENVIRONMENT, "1");
    } else {
        scenario.without_env(MANAGED_PROCESS_ENVIRONMENT);
    }
    scenario.run(CHILD_SCENARIO_BUDGET);
}
