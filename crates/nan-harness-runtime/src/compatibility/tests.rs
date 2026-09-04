// Compatibility coverage, split by concern: feed validation and evidence rules,
// release selection and atomic merge, cached state and expiry, and the remote
// download path with its redirect, size, and URL-redaction limits. Feed
// fixtures and the fake manifest server live in `support`. Each test still owns
// its manifests and temporary state directory, so order and parallelism cannot
// couple them.
mod cache_state;
mod evidence_validation;
mod release_merge;
mod remote_download;
mod support;
