#![cfg(unix)]

#[path = "conformance_direct/assertions.rs"]
mod assertions;
#[path = "conformance_direct/execution.rs"]
mod execution;
#[path = "conformance_direct/fixtures.rs"]
mod fixtures;
#[path = "conformance_direct/inventories.rs"]
#[allow(dead_code)]
mod inventories;
#[path = "conformance_direct/scenarios.rs"]
#[allow(dead_code)]
mod scenarios;
