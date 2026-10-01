//! Controlled harness double for the supervised standard-stream regression.
//!
//! The fixture stands in for a real harness in the repository's own Windows regression: it answers
//! the version probe, replays a fixed marker on standard output, and can leave a controlled
//! descendant holding the inherited standard streams after the harness itself exits. It ignores
//! provider configuration, never contacts the network, and never reads user data, so the
//! regression can exercise the real supervisor without depending on a live harness conversation.

use std::process::Command;
use std::time::Duration;

const MODE_VARIABLE: &str = "NAN_CODEX_HARNESS_FIXTURE_MODE";
const MARKER: &str = "NAN_CODEX_DIAGNOSTIC_OK";
/// The holder always exits on its own, so a diagnostic failure cannot leak a process forever.
const HOLD: Duration = Duration::from_mins(2);

fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    // The version probe must stay inside the supported forward-compatibility path, so the fixture
    // reports a version newer than the pinned entry instead of asking for a bypass flag.
    if arguments.iter().any(|argument| argument == "--version") {
        println!("codex-cli 99.0.0");
        return;
    }
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("Usage: codex [--profile <name>] [--model <id>] <prompt>");
        return;
    }
    match std::env::var(MODE_VARIABLE).as_deref() {
        Ok("holder") => hold(),
        Ok("descendant") => {
            spawn_holder();
            println!("{MARKER}");
        }
        _ => println!("{MARKER}"),
    }
}

/// Starts a descendant that inherits this process's standard streams and deliberately outlives it.
fn spawn_holder() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new(executable)
        .env(MODE_VARIABLE, "holder")
        .spawn();
}

fn hold() {
    std::thread::sleep(HOLD);
}
