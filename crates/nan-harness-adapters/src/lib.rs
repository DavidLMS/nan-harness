#![forbid(unsafe_code)]

mod claude_code;
mod deepseek_harness;
mod direct;
mod hermes;
mod opencode;
mod pi;

pub use claude_code::ClaudeCodeAdapter;
pub use deepseek_harness::DeepSeekHarnessAdapter;
pub use hermes::HermesAdapter;
pub use opencode::OpenCodeAdapter;
pub use pi::{PiAdapter, PrimeAgentAdapter};
