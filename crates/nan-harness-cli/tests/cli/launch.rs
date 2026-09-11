#[cfg(unix)]
#[path = "launch/claude.rs"]
mod claude;
#[cfg(unix)]
#[path = "launch/desktop.rs"]
mod desktop;
#[cfg(unix)]
#[path = "launch/discovery.rs"]
mod discovery;
#[cfg(unix)]
#[path = "launch/harnesses.rs"]
mod harnesses;

#[cfg(unix)]
#[path = "launch/model_cache.rs"]
mod model_cache;
