// Chat Completions bridge integration coverage, split by concern: local
// authentication with model discovery and upstream error pass-through, the
// search endpoints and their feature gate, streaming versus non-streaming
// responses, per-model usage accounting, request and response size bounds,
// and responses that never complete.
//
// The fake upstream provider, the bridge launcher, and the usage fixtures live
// in `support`. Every test starts its own bridge and upstream on loopback
// ports with a fresh `FakeState`, so test order and parallelism cannot couple
// them.

mod auth;
mod incomplete_responses;
mod limits;
mod responses;
mod search;
mod support;
mod usage;
