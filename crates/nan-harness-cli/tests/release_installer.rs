#[path = "release_installer/fixtures.rs"]
mod fixtures;

#[path = "release_installer/guardrails.rs"]
mod guardrails;

#[path = "release_installer/installation.rs"]
mod installation;

#[path = "release_installer/platform.rs"]
mod platform;

#[path = "release_installer/support.rs"]
mod support;

#[cfg(windows)]
#[path = "release_installer/windows_architecture.rs"]
mod windows_architecture;
