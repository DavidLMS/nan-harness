use super::child_process::{ChildScenario, scenario_completed};
use super::spawn_daemon;
use std::io::{self, Read, Seek};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Exact path of the scenario that calls the real spawn from its own process.
const SPAWN_SCENARIO: &str = "client::spawn_tests::the_real_spawn_is_observed_in_a_child";
/// Names the file the spawned probe reports into; without it the probe stays inert.
const PROBE_REPORT_ENVIRONMENT: &str = "NAN_HARNESS_TEST_SPAWN_PROBE_REPORT";
/// Far above the scenario's own bounded waits, so a stuck scenario still fails as a test.
const CHILD_SCENARIO_BUDGET: Duration = Duration::from_secs(30);
/// Bounds every wait the scenario performs on its synthetic child.
const PROBE_BUDGET: Duration = Duration::from_secs(10);
const PROBE_POLL: Duration = Duration::from_millis(10);

/// `spawn_daemon` runs the current executable with `__coordinator`. Under a test binary
/// that executable is the test binary and the argument is a libtest name filter, so the
/// spawn selects the synthetic probe below instead of a real daemon.
///
/// The scenario runs in its own process because the report path reaches the probe only
/// through inherited environment, and because the spawn detaches a child that this
/// process would otherwise have no handle on.
#[test]
fn the_real_spawn_starts_a_detached_child_in_its_own_process_group() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let marker = temporary.path().join("scenario.marker");
    let report = temporary.path().join("probe.report");

    ChildScenario::new(SPAWN_SCENARIO, &marker)
        .with_env(PROBE_REPORT_ENVIRONMENT, &report)
        .run(CHILD_SCENARIO_BUDGET);
}

/// Calls the real private spawn and observes the child it detaches. The parent above
/// bounds this externally; every wait here is bounded again so a spawn that never
/// produces a child fails as an ordinary assertion.
#[test]
#[ignore = "bounded child scenario of the real daemon spawn"]
fn the_real_spawn_is_observed_in_a_child() {
    let report = report_path().expect("the parent should name the probe report");
    let scenario = std::process::id();
    let scenario_group = process_group_of(scenario)
        .expect("the scenario process query should succeed")
        .expect("the scenario should have a group");

    spawn_daemon();

    let probe = wait_for_probe(&report);
    // Reap before assertions so the intentional detach mutant cannot skip cleanup.
    wait_until_reaped(probe.pid);
    assert_ne!(
        probe.pid, scenario,
        "the spawn must run a separate process rather than work in this one"
    );
    assert_eq!(
        probe.group, probe.pid,
        "a detached daemon leads a process group of its own"
    );
    assert_ne!(
        probe.group, scenario_group,
        "a detached daemon must leave the group that started it"
    );
    scenario_completed();
}

/// The synthetic daemon. `spawn_daemon` passes `__coordinator`, which libtest reads as a
/// name filter, so this is the only test the spawned command selects; the name carries
/// that argument as its prefix. It does finite metadata work and exits, never spawning a
/// daemon itself, and an ordinary suite run finds no report path and does nothing at all.
#[test]
fn __coordinator_probe_reports_its_own_process_group() {
    let Some(report) = report_path() else {
        return;
    };
    let pid = std::process::id();
    let group = process_group_of(pid)
        .expect("the probe process query should succeed")
        .expect("the probe should have a process group");
    std::fs::write(report, format!("{pid} {group}\n")).expect("the probe should report");
}

/// What the spawned probe observed about itself.
struct Probe {
    pid: u32,
    group: u32,
}

fn report_path() -> Option<PathBuf> {
    std::env::var_os(PROBE_REPORT_ENVIRONMENT).map(PathBuf::from)
}

fn wait_for_probe(report: &Path) -> Probe {
    let deadline = Instant::now() + PROBE_BUDGET;
    loop {
        if let Ok(reported) = std::fs::read_to_string(report)
            && let Some(probe) = parse_report(&reported)
        {
            return probe;
        }
        assert!(
            Instant::now() < deadline,
            "no spawned probe reported within {PROBE_BUDGET:?}"
        );
        std::thread::sleep(PROBE_POLL);
    }
}

/// Accepts only a complete report, so a read that catches the write half-done retries.
fn parse_report(reported: &str) -> Option<Probe> {
    let (pid, group) = reported.strip_suffix('\n')?.split_once(' ')?;
    Some(Probe {
        pid: pid.parse().ok()?,
        group: group.parse().ok()?,
    })
}

/// Waits for the detached probe to exit and be reaped, so the scenario never leaves a
/// process behind for the parent to inherit.
fn wait_until_reaped(pid: u32) {
    let deadline = Instant::now() + PROBE_BUDGET;
    while process_group_of(pid)
        .expect("process inspection must succeed before claiming the probe was reaped")
        .is_some()
    {
        assert!(
            Instant::now() < deadline,
            "the spawned probe was still present after {PROBE_BUDGET:?}"
        );
        std::thread::sleep(PROBE_POLL);
    }
}

// A file avoids pipe reads blocking after exit if a helper inherits the output handle.
fn bounded_query(command: &mut Command, budget: Duration) -> io::Result<(ExitStatus, String)> {
    let mut output = tempfile::tempfile()?;
    let child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .spawn()?;
    let mut child = QueryChild(child);
    let deadline = Instant::now() + budget;
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process query exceeded its deadline",
            ));
        }
        std::thread::sleep(PROBE_POLL);
    };
    output.rewind()?;
    let mut text = String::new();
    output.read_to_string(&mut text)?;
    Ok((status, text))
}

struct QueryChild(Child);

impl Drop for QueryChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// ps returns status 1 with no rows when the requested PID is absent. Other
// failures or malformed output are inspection errors, never evidence of reaping.
fn process_group_of(pid: u32) -> io::Result<Option<u32>> {
    let mut command = Command::new("/bin/ps");
    command.args(["-o", "pgid=", "-p", &pid.to_string()]);
    let (status, text) = bounded_query(&mut command, Duration::from_secs(2))?;
    parse_process_group(status, &text)
}

fn parse_process_group(status: ExitStatus, text: &str) -> io::Result<Option<u32>> {
    if status.code() == Some(1) && text.trim().is_empty() {
        return Ok(None);
    }
    if !status.success() {
        return Err(io::Error::other("process inspection failed"));
    }
    text.trim()
        .parse()
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[test]
fn process_queries_time_out_and_inspection_errors_do_not_mean_absence() {
    let mut blocked = Command::new("/bin/sh");
    // exec keeps the synthetic delay in the one child owned by QueryChild.
    blocked.args(["-c", "exec /bin/sleep 30"]);
    let error = bounded_query(&mut blocked, Duration::from_millis(50))
        .expect_err("the synthetic query must time out");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);

    assert_eq!(
        parse_process_group(ExitStatus::from_raw(256), "").unwrap(),
        None
    );
    assert!(parse_process_group(ExitStatus::from_raw(512), "").is_err());
    assert!(parse_process_group(ExitStatus::from_raw(0), "invalid").is_err());
    assert_eq!(
        parse_process_group(ExitStatus::from_raw(0), "123\n").unwrap(),
        Some(123)
    );
}
