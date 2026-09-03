mod args;
mod commands;
mod parser;
mod targets;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use args::{
    AuthLogoutArgs, BridgedHarnessRunArgs, ChatGptDesktopArgs, ClaudeDesktopArgs, ConfigArgs,
    DirectHarnessRunArgs, DoctorArgs, HarnessRunArgs, HermesDesktopArgs, PenDesktopArgs,
    RecordInstallationArgs, UninstallArgs, WebSearchArgs, ZedDesktopArgs,
};
pub(crate) use commands::{AuthCommand, Command, CompletionShell, TelemetryCommand};
pub(crate) use parser::Cli;
pub(crate) use targets::{ConfigTarget, DoctorTarget};
