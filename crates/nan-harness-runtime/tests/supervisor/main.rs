// Supervisor integration coverage, split by concern: process lifecycle and
// cancellation, direct launches, bridge launches, provider usage propagation,
// and model-catalog materialization. Shared fixtures, launch helpers, and fake
// providers live in `support`. Every test creates its own working directory,
// session tokens, and temporary artifacts, so tests stay deterministic and
// independent of execution order.
#![cfg(unix)]

mod bridges;
mod catalogs;
mod direct;
mod lifecycle;
mod support;
mod usage;
