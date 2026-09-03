mod support {
    pub(super) use super::super::assertions::assert_hermes_inventory;
    pub(super) use super::super::execution::{
        harness_command, inventory, run_controlled_tool, run_openclaw_yield_tool, run_round_trip,
    };
    pub(super) use super::super::fixtures::{
        HERMES_OPTIONAL_CREDENTIALS_CLEARED, OPENCLAW_MEDIA_CREDENTIALS_CLEARED, write_png,
    };
    pub(super) use nan_harness_test_support::assertions::assert_aider_edit_protocol;
    pub(super) use nan_harness_test_support::conformance::{
        assert_file, assert_success, call, write_fixture,
    };
    pub(super) use nan_harness_test_support::scripted_provider::{
        ProviderScenario, ScriptedProvider, ScriptedToolCall,
    };
    pub(super) use serde_json::json;
    pub(super) use std::ffi::OsString;
    pub(super) use std::path::Path;
}

#[path = "scenarios/aider.rs"]
mod aider;
#[path = "scenarios/cline.rs"]
mod cline;
#[path = "scenarios/deepseek_harness.rs"]
mod deepseek_harness;
#[path = "scenarios/goose.rs"]
mod goose;
#[path = "scenarios/hermes.rs"]
mod hermes;
#[path = "scenarios/kimi_code.rs"]
mod kimi_code;
#[path = "scenarios/omp.rs"]
mod omp;
#[path = "scenarios/openclaw.rs"]
mod openclaw;
#[path = "scenarios/opencode.rs"]
mod opencode;
#[path = "scenarios/pi.rs"]
mod pi;
#[path = "scenarios/prime_agent.rs"]
mod prime_agent;
#[path = "scenarios/qwen_code.rs"]
mod qwen_code;
