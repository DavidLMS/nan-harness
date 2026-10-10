#![cfg(unix)]

// These ignored tests exercise installed clients through actual generated configuration.
// See conformance_reasoning/README.md for binary overrides and the verified versions.
#[path = "conformance_reasoning/aider.rs"]
mod aider;
#[path = "conformance_reasoning/bridged.rs"]
mod bridged;
#[path = "conformance_reasoning/deepseek.rs"]
mod deepseek;
#[path = "conformance_reasoning/opencode.rs"]
mod opencode;
#[path = "conformance_reasoning/prime.rs"]
mod prime;
#[path = "conformance_reasoning/qwen.rs"]
mod qwen;
#[path = "conformance_reasoning/support.rs"]
mod support;
