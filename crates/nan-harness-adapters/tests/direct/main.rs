// Direct-protocol adapter coverage, organized by harness: each module mirrors
// its adapter in `src/` and keeps the original test names, fixtures, and
// assertions for overlay contracts, routing precedence, user-state
// preservation, launch-scoped secrets, search, and model catalogs. Shared
// plan fixtures and the common search-block, search-MCP, and
// launch-scoped-secret assertions live in `support`; `routing` owns the
// cross-harness contract that user arguments cannot bypass NaN routing.
// Every test builds its own plan from the shared context fixture, so cases
// stay deterministic and independent of execution order.
mod aider;
mod cline;
mod codex;
mod deepseek_harness;
mod goose;
mod hermes;
mod kimi_code;
mod omp;
mod openclaw;
mod opencode;
mod pi;
mod qwen_code;
mod routing;
mod support;
