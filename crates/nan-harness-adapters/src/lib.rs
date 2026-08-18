#![forbid(unsafe_code)]

mod aider;
mod claude_code;
mod cline;
mod codex;
mod deepseek_harness;
mod direct;
mod goose;
mod hermes;
mod openclaw;
mod opencode;
mod pi;
mod qwen_code;

pub use aider::AiderAdapter;
pub use claude_code::ClaudeCodeAdapter;
pub use cline::ClineAdapter;
pub use codex::CodexAdapter;
pub use deepseek_harness::DeepSeekHarnessAdapter;
pub use direct::{ModelDescription, describe_model};
pub use goose::GooseAdapter;
pub use hermes::HermesAdapter;
pub use openclaw::OpenClawAdapter;
pub use opencode::OpenCodeAdapter;
pub use pi::{
    PersistentPiAdapter, PersistentPrimeAgentAdapter, PiAdapter, PrimeAgentAdapter,
    persistent_provider_extension,
};
pub use qwen_code::QwenCodeAdapter;
