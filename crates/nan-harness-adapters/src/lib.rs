#![forbid(unsafe_code)]

mod aider;
mod claude_code;
mod cline;
mod codex;
mod deepseek_harness;
mod direct;
mod fx;
mod goose;
mod hermes;
mod kimi_code;
mod omp;
mod openclaw;
mod opencode;
mod pi;
mod qwen_code;
mod search;

pub use aider::AiderAdapter;
pub use claude_code::ClaudeCodeAdapter;
pub use cline::ClineAdapter;
pub use codex::CodexAdapter;
pub use deepseek_harness::DeepSeekHarnessAdapter;
pub use direct::{ModelDescription, describe_model};
pub use fx::FxAdapter;
pub use goose::GooseAdapter;
pub use hermes::{
    HermesAdapter, hermes_search_provider_files, render_hermes_desktop_provider_block,
};
pub use kimi_code::KimiCodeAdapter;
pub use omp::{OmpAdapter, OmpSearchMode, render_omp_search_extension};
pub use openclaw::OpenClawAdapter;
pub use opencode::OpenCodeAdapter;
pub use pi::{PiAdapter, PiSearchMode, PrimeAgentAdapter, render_pi_search_extension};
pub use qwen_code::QwenCodeAdapter;
