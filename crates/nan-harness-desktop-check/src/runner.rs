use crate::{
    catalog::{self, DiscoveryError, Installation},
    cli::{ExecutionMode, RunArgs, confirm, state_directory},
    install,
    journal::Journal,
    probe::ProbeSpec,
    report::{
        AppResult, Architecture, BinaryIdentity, Platform, ProbeResult, Reason, Report, Status,
        digest,
    },
};
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::open_private_new;
use semver::Version;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::io::AsyncReadExt as _;

mod prepared;
pub(crate) use prepared::prepare;

pub(crate) async fn run(mut args: RunArgs) -> Result<i32, String> {
    authorize_session(&args)?;
    let live = execution_live(&args)?;
    let apps = if args.apps.is_empty() {
        DesktopHarnessKind::ALL.to_vec()
    } else {
        let mut apps = args.apps.clone();
        apps.sort_unstable();
        apps.dedup();
        apps
    };
    launch_wrapper_scope(&args, &apps)?;
    let (inventory, existing_nanh, _prepared_owner) = if let Some(path) = &args.prepared {
        if args.nan_harness.is_some() {
            return Err("--prepared already selects the tested nanh executable".into());
        }
        let prepared = prepared::load(path, &apps, &args.model)?;
        (
            prepared.inventory,
            Some(prepared.nanh),
            Some(prepared.owner),
        )
    } else {
        (
            apps.iter()
                .map(|&app| {
                    (
                        app,
                        catalog::discover(app).map_err(prepared::discovery_reason),
                    )
                })
                .collect::<Vec<_>>(),
            discover_nanh(args.nan_harness.as_deref()).await?,
            None,
        )
    };
    bind_launch_wrapper(&mut args, existing_nanh.as_ref())?;
    print_inventory(&inventory, existing_nanh.as_ref(), live, args.ephemeral);
    if !args.yes && (args.non_interactive || !confirm("Proceed with these operations?")?) {
        return Ok(0);
    }
    let mut journal = Journal::create(&state_directory()?).map_err(|error| error.to_string())?;
    let mut report = Report {
        schema_version: 3,
        checker_version: Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|_| "invalid checker version")?,
        run_id: journal.run_id().into(),
        started_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| "cannot record time")?,
        platform: Platform::current(),
        architecture: Architecture::current(),
        model: Some(args.model.clone()),
        nan_harness: None,
        results: Vec::new(),
        cleanup: Status::Passed,
    };
    let nanh = match existing_nanh {
        Some(existing) => Ok(existing),
        None => install_nanh(&mut journal).await,
    };
    if let Ok((_, identity)) = &nanh {
        report.nan_harness = Some(identity.clone());
    }
    for (app, found) in inventory {
        eprintln!("Checking {app}...");
        let result = if let Ok((binary, _)) = &nanh {
            run_app(app, found, binary, &args, live, &mut journal).await
        } else {
            blocked_app(app, Reason::InstallationFailed, live)
        };
        eprintln!(
            "  deterministic: {:?}; live: {:?}",
            result
                .deterministic
                .iter()
                .map(|probe| probe.status)
                .collect::<Vec<_>>(),
            result.live.status
        );
        if let Some(reason) = executed_reason(&result, args.mode, live) {
            eprintln!("  {}", guidance(reason));
        }
        let cancelled = result
            .deterministic
            .iter()
            .chain(std::iter::once(&result.live))
            .any(|probe| probe.reason == Some(Reason::Cancelled));
        // A failed cleanup leaves ownership evidence and possibly native
        // descendants unresolved. Do not launch another app until recovery
        // has sealed this run; doing so would turn one uncertain tree into a
        // global absence failure for every later app.
        let cleanup_failed = result.cleanup != Status::Passed;
        report.results.push(result);
        if cancelled || cleanup_failed {
            break;
        }
    }
    finish_report(report, &mut journal, &args, live)
}

/// Check the account-access authority before discovering or launching any app.
fn authorize_session(args: &RunArgs) -> Result<(), String> {
    if args.session == crate::cli::SessionMode::GithubHosted {
        if !args.yes || !args.session.available() {
            return Err(
                "--session github-hosted requires --yes and a fresh GitHub-hosted runner".into(),
            );
        }
        eprintln!(
            "Disposable VM session authorized. Native apps may use this VM account's home and credential store."
        );
    }
    Ok(())
}

/// Refuse an incomplete or out-of-scope diagnostic binding before discovery
/// runs anything.
fn launch_wrapper_scope(args: &RunArgs, apps: &[DesktopHarnessKind]) -> Result<(), String> {
    if args.launch_wrapper.is_none() {
        return Ok(());
    }
    if cfg!(windows)
        || args.mode != ExecutionMode::Deterministic
        || apps != [DesktopHarnessKind::ChatGpt]
    {
        return Err("--launch-wrapper supports only deterministic ChatGPT Desktop checks".into());
    }
    if args.nan_harness.is_none() && args.prepared.is_none() {
        return Err("--launch-wrapper requires an explicit --nan-harness or --prepared".into());
    }
    let digest = args.launch_wrapper_sha256.as_deref().unwrap_or_default();
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("--launch-wrapper-sha256 must be a lowercase SHA-256 digest".into());
    }
    private_facts_root(args.launch_wrapper_facts.as_deref())?;
    Ok(())
}

/// Bind the wrapper after the tested nanh is known and before any operation.
/// The wrapper never becomes the tested identity; it is verified separately.
fn bind_launch_wrapper(
    args: &mut RunArgs,
    nanh: Option<&(PathBuf, BinaryIdentity)>,
) -> Result<(), String> {
    let Some(requested) = &args.launch_wrapper else {
        return Ok(());
    };
    let Some((nanh, identity)) = nanh else {
        return Err("--launch-wrapper requires an explicit --nan-harness or --prepared".into());
    };
    let path = std::fs::canonicalize(requested).map_err(|_| "the launch wrapper cannot be read")?;
    let digest =
        crate::probe::binary_digest(&path).map_err(|_| "the launch wrapper cannot be read")?;
    if args.launch_wrapper_sha256.as_deref() != Some(digest.as_str()) {
        return Err("the launch wrapper does not match its bound digest".into());
    }
    if path == *nanh || digest == identity.sha256 {
        return Err("the launch wrapper must be separate from the tested nanh".into());
    }
    args.launch_wrapper = Some(path);
    eprintln!(
        "Diagnostic: ChatGPT Desktop launches run through the bound wrapper; its closed facts stay outside the report."
    );
    Ok(())
}

fn private_facts_root(path: Option<&Path>) -> Result<(), String> {
    const REFUSED: &str = "--launch-wrapper-facts must be an existing owner-only directory";
    let path = path.filter(|path| path.is_absolute()).ok_or(REFUSED)?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| REFUSED)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.is_dir() && metadata.permissions().mode() & 0o7777 == 0o700 {
            return Ok(());
        }
    }
    let _ = metadata;
    Err(REFUSED.into())
}

fn has_live_key() -> bool {
    std::env::var("NAN_API_KEY")
        .map(zeroize::Zeroizing::new)
        .is_ok_and(|key| !key.trim().is_empty())
}

fn executed_reason(result: &AppResult, mode: ExecutionMode, live: bool) -> Option<Reason> {
    (mode != ExecutionMode::Live)
        .then(|| result.deterministic.iter().find_map(|probe| probe.reason))
        .flatten()
        .or_else(|| live.then_some(result.live.reason).flatten())
}

fn execution_live(args: &RunArgs) -> Result<bool, String> {
    if !catalog::frozen::valid_model(&args.model) {
        return Err("invalid model identifier".into());
    }
    match args.mode {
        ExecutionMode::Deterministic => Ok(false),
        ExecutionMode::Auto => Ok(has_live_key()),
        ExecutionMode::Live => {
            if !has_live_key() {
                return Err("live checks require a nonempty NAN_API_KEY".into());
            }
            if args.prepared.is_none() {
                return Err(
                    "live-only checks require --prepared from a credential-free prepare step"
                        .into(),
                );
            }
            Ok(true)
        }
    }
}

fn print_inventory(
    inventory: &[(DesktopHarnessKind, Result<Option<Installation>, Reason>)],
    nanh: Option<&(PathBuf, BinaryIdentity)>,
    live: bool,
    retain: bool,
) {
    eprintln!("Desktop compatibility checks:");
    for (app, found) in inventory {
        match found {
            Ok(Some(installed)) => eprintln!(
                "  {app}: use existing installation (version {})",
                installed
                    .app_version
                    .as_ref()
                    .map_or_else(|| "unknown".into(), ToString::to_string)
            ),
            Ok(None) => eprintln!("  {app}: install if an isolated official package is available"),
            Err(_) => eprintln!("  {app}: blocked; inventory needs attention"),
        }
    }
    if let Some((_, identity)) = nanh {
        eprintln!(
            "nanh: use existing version {} without upgrading",
            identity.version
        );
    } else {
        eprintln!("nanh: download the newest published version from the available channel");
    }
    eprintln!(
        "NaN calls: {}. Cleanup: {}.",
        if live {
            "enabled, bounded to four generations per app"
        } else {
            "disabled"
        },
        if retain {
            "retain installations"
        } else {
            "remove only owned installations"
        }
    );
}

fn guidance(reason: Reason) -> &'static str {
    match reason {
        Reason::AlreadyRunning => {
            "Close the application before checking it; its current session was left untouched."
        }
        Reason::PermissionRequired => {
            "Grant accessibility permission to the checker, then run the check again."
        }
        Reason::LoginRequired => {
            "Complete the application's sign-in or onboarding in a disposable test session."
        }
        Reason::InstallationUnavailable => {
            "Install an official application in the test environment; automatic isolated installation is unavailable."
        }
        Reason::UnsupportedArchitecture => {
            "Install a native build for this architecture; emulated or unidentified executables are not certified."
        }
        Reason::HarnessCapabilityUnavailable => {
            "The selected nanh build lacks a required provider or profile endpoint; select a compatible harness release."
        }
        Reason::InstallationAmbiguous => {
            "Resolve the multiple application installations before running this check."
        }
        Reason::CleanupFailed | Reason::CleanupConflict => {
            "Keep the private run directory and close the tested application before retrying cleanup."
        }
        Reason::InvalidKey => {
            "The provider rejected the supplied key; replace NAN_API_KEY before running live checks again."
        }
        Reason::BudgetExceeded => {
            "The live request limit was reached; inspect the reported failure without automatic retries."
        }
        Reason::IsolationUnavailable => {
            "The checker could not establish an owned test session; no passing evidence was recorded."
        }
        Reason::FocusChanged => {
            "The tested window lost focus. Run the check again and leave that window in front."
        }
        Reason::WindowChanged => {
            "The tested window or display changed. Run the check again without moving or resizing the window."
        }
        Reason::WindowOccluded => {
            "Another window covered the test window. Move it aside and run the check again."
        }
        Reason::ApplicationExited => {
            "The application launcher exited before a test window was available. Check that the native application can start in this environment."
        }
        _ => {
            "This check did not pass. Inspect the typed reason in the sanitized report; no evidence was published."
        }
    }
}

fn finish_report(
    mut report: Report,
    journal: &mut Journal,
    args: &RunArgs,
    live: bool,
) -> Result<i32, String> {
    if journal.cleanup(args.ephemeral).is_err()
        || report
            .results
            .iter()
            .any(|app| app.cleanup != Status::Passed)
    {
        report.cleanup = Status::Failed;
        eprintln!(
            "Cleanup needs attention. Recover with: nanh-desktop-check cleanup {}",
            journal.run_id()
        );
    }
    report.validate().map_err(|error| error.to_string())?;
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| journal.root().join("report.json"));
    let mut file = open_private_new(&output).map_err(
        |_| "report output already exists or cannot be created; recovery state retained",
    )?;
    let bytes = report_output(&report, args.launch_wrapper_sha256.as_deref())
        .map_err(|_| "cannot encode report")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "cannot save report")?;
    println!("Report: {}", output.display());
    println!("Report SHA-256: {}", digest(&bytes));
    if args.launch_wrapper.is_some() {
        eprintln!(
            "Diagnostic only. This instrumented result cannot be submitted or published as compatibility evidence."
        );
    } else {
        eprintln!(
            "No report was submitted. Use nanh-desktop-check submit <report> to review and share it."
        );
    }
    Ok(i32::from(
        report.cleanup != Status::Passed
            || report.results.iter().any(|app| {
                (args.mode != ExecutionMode::Live
                    && app
                        .deterministic
                        .iter()
                        .any(|probe| probe.status != Status::Passed))
                    || (live && app.live.status != Status::Passed)
            }),
    ))
}

fn report_output(
    report: &Report,
    wrapper_sha256: Option<&str>,
) -> Result<Vec<u8>, serde_json::Error> {
    // Keep instrumentation outside the public schema. All submission and feed
    // readers reject this envelope, including older checker versions.
    if let Some(wrapper_sha256) = wrapper_sha256 {
        serde_json::to_vec_pretty(&serde_json::json!({
            "diagnosticVersion": 1,
            "kind": "chatgpt-startup-wrapper",
            "wrapperSha256": wrapper_sha256,
            "observation": report,
        }))
    } else {
        serde_json::to_vec_pretty(report)
    }
}

async fn run_app(
    app: DesktopHarnessKind,
    found: Result<Option<Installation>, Reason>,
    binary: &Path,
    args: &RunArgs,
    live: bool,
    journal: &mut Journal,
) -> AppResult {
    let installed = match found {
        Ok(Some(installed)) => installed,
        Ok(None) => match install::install(app, journal).await {
            Ok(installed) => installed,
            Err(
                install::InstallError::ExternalInstallation | install::InstallError::Unavailable,
            ) => return blocked_app(app, Reason::InstallationUnavailable, live),
            Err(_) => return blocked_app(app, Reason::InstallationFailed, live),
        },
        Err(reason) => return blocked_app(app, reason, live),
    };
    let mut result = blocked_app(app, Reason::NotRun, live);
    result.app_version = installed.app_version.clone();
    result.runtime_version = installed.runtime_version.clone();
    match catalog::architecture::matches_host(&installed.executable) {
        Ok(true) => {}
        Ok(false) => return blocked_app(app, Reason::UnsupportedArchitecture, live),
        Err(_) => return blocked_app(app, Reason::InstallationUnreadable, live),
    }
    for repetition in 0..if args.mode == ExecutionMode::Live {
        0
    } else {
        3
    } {
        result.deterministic[repetition] =
            run_probe(app, &installed, binary, args, false, repetition, journal).await;
        if matches!(
            result.deterministic[repetition].reason,
            Some(
                Reason::CleanupFailed
                    | Reason::Cancelled
                    | Reason::AlreadyRunning
                    | Reason::PermissionRequired
                    | Reason::FocusChanged
                    | Reason::WindowChanged
                    | Reason::WindowOccluded
            )
        ) {
            result.cleanup = if matches!(
                result.deterministic[repetition].reason,
                Some(Reason::CleanupFailed | Reason::Cancelled)
            ) {
                Status::Failed
            } else {
                Status::Passed
            };
            break;
        }
    }
    if live && result.cleanup == Status::Passed {
        result.live = run_probe(app, &installed, binary, args, true, 0, journal).await;
        if matches!(
            result.live.reason,
            Some(Reason::CleanupFailed | Reason::Cancelled)
        ) {
            result.cleanup = Status::Failed;
        }
    }
    result
}

fn blocked_app(app: DesktopHarnessKind, reason: Reason, live: bool) -> AppResult {
    AppResult {
        app,
        app_version: None,
        runtime_version: None,
        deterministic: std::array::from_fn(|_| ProbeResult::blocked(reason)),
        live: ProbeResult::not_run(if live { reason } else { Reason::MissingKey }),
        cleanup: Status::Passed,
    }
}

async fn run_probe(
    app: DesktopHarnessKind,
    installed: &Installation,
    binary: &Path,
    args: &RunArgs,
    live: bool,
    repetition: usize,
    journal: &mut Journal,
) -> ProbeResult {
    let name = format!(
        "{app}-{}-{repetition}",
        if live { "live" } else { "deterministic" }
    );
    let Ok(root) = journal.reserve(&name) else {
        return ProbeResult::blocked(Reason::IsolationUnavailable);
    };
    let Ok(nan_harness_sha256) = crate::probe::binary_digest(binary) else {
        return ProbeResult::blocked(Reason::InstallationUnreadable);
    };
    let spec = ProbeSpec {
        kind: app,
        nan_harness: binary.into(),
        nan_harness_sha256,
        executable: installed.executable.clone(),
        workspace: root.join("workspace"),
        model: if live {
            args.model.clone()
        } else {
            "qwen3.6".into()
        },
        live,
        session: args.session,
        launch_wrapper: probe_launch_wrapper(args, &name),
    };
    let outcome = execute_probe(&spec, &root).await;
    seal_probe(outcome, journal, &name)
}

/// Give each probe its own facts directory, named by the closed probe name.
fn probe_launch_wrapper(args: &RunArgs, name: &str) -> Option<crate::probe::LaunchWrapper> {
    let ((path, sha256), facts) = args
        .launch_wrapper
        .as_ref()
        .zip(args.launch_wrapper_sha256.as_ref())
        .zip(args.launch_wrapper_facts.as_ref())?;
    Some(crate::probe::LaunchWrapper {
        path: path.clone(),
        sha256: sha256.clone(),
        facts: facts.join(name),
    })
}

fn seal_probe(mut outcome: ProbeResult, journal: &mut Journal, name: &str) -> ProbeResult {
    if !matches!(
        outcome.reason,
        Some(Reason::CleanupFailed | Reason::Cancelled)
    ) && let Err(error) = journal.seal(name)
    {
        report_seal_failure(&error, outcome.reason);
        outcome.status = Status::Failed;
        outcome.reason = Some(Reason::CleanupFailed);
    }
    outcome
}

fn report_seal_failure(error: &crate::journal::JournalError, original: Option<Reason>) {
    use crate::journal::JournalError;
    // Never format the I/O error itself; it may carry a private path/message.
    let (kind, code) = match error {
        JournalError::Io(error) => ("io", error.raw_os_error()),
        JournalError::Locked => ("locked", None),
        JournalError::Invalid => ("invalid", None),
        JournalError::Conflict => ("conflict", None),
    };
    eprintln!("Desktop seal diagnostic: {kind}, os-code={code:?}, original={original:?}");
}

async fn execute_probe(spec: &ProbeSpec, root: &Path) -> ProbeResult {
    let spec_path = root.join("spec.json");
    let output = root.join("probe.json");
    let saved = open_private_new(&spec_path)
        .and_then(|mut file| serde_json::to_writer(&mut file, spec).map_err(std::io::Error::other));
    if saved.is_err() {
        return ProbeResult::blocked(Reason::IsolationUnavailable);
    }
    let Ok(executable) = std::env::current_exe() else {
        return ProbeResult::blocked(Reason::NotRun);
    };
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("probe")
        .arg(&spec_path)
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if !spec.live {
        command.env_remove("NAN_API_KEY");
    }
    #[cfg(unix)]
    command.process_group(0);
    let Ok(mut child) = command.spawn() else {
        return ProbeResult::blocked(Reason::NotRun);
    };
    let (completed, cancelled) = {
        let wait = tokio::time::timeout(worker_timeout(spec.live), child.wait());
        tokio::pin!(wait);
        tokio::select! { result = &mut wait => (matches!(result, Ok(Ok(_))), false), _ = tokio::signal::ctrl_c() => (false, true) }
    };
    if !completed {
        terminate_worker(&mut child).await;
        return ProbeResult {
            status: Status::Failed,
            reason: Some(if cancelled {
                Reason::Cancelled
            } else {
                Reason::CleanupFailed
            }),
            ..ProbeResult::not_run(Reason::CleanupFailed)
        };
    }
    let exit_code = child
        .try_wait()
        .ok()
        .flatten()
        .and_then(|status| status.code());
    read_worker_result(&output, exit_code)
}

pub(crate) fn worker_timeout(live: bool) -> Duration {
    Duration::from_secs(if live { 180 } else { 240 })
}

#[derive(Debug)]
enum WorkerResultFailure {
    Missing,
    UnreadableOrOversized,
    Schema,
    ExitMismatch,
}

fn read_worker_result(output: &Path, exit_code: Option<i32>) -> ProbeResult {
    let uncertain = |stage: WorkerResultFailure| {
        eprintln!("Desktop worker diagnostic: {stage:?}, exit-code={exit_code:?}");
        ProbeResult::blocked(Reason::CleanupFailed)
    };
    let Ok(file) = std::fs::File::open(output) else {
        return uncertain(WorkerResultFailure::Missing);
    };
    let mut bytes = Vec::new();
    if file.take(8193).read_to_end(&mut bytes).is_err() || bytes.len() > 8192 {
        return uncertain(WorkerResultFailure::UnreadableOrOversized);
    }
    let Ok(outcome) = serde_json::from_slice::<crate::probe::WorkerOutcome>(&bytes) else {
        return uncertain(WorkerResultFailure::Schema);
    };
    let result = outcome.result;
    if exit_code != Some(i32::from(result.status != Status::Passed)) {
        return uncertain(WorkerResultFailure::ExitMismatch);
    }
    if let Some(exit) = outcome.launch_exit {
        // The private envelope contains only closed numeric status, not app output.
        eprintln!("Desktop launch diagnostic: {exit:?}");
    }
    if let Some(diagnostic) = outcome.cleanup {
        // This internal channel accepts closed enums only, never native messages.
        eprintln!("Desktop cleanup diagnostic: {diagnostic:?}");
    }
    result
}

async fn terminate_worker(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let _ = tokio::process::Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    if tokio::time::timeout(Duration::from_secs(20), child.wait())
        .await
        .is_err()
    {
        #[cfg(unix)]
        if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        let _ = child.kill().await;
    }
}

async fn discover_nanh(
    explicit: Option<&Path>,
) -> Result<Option<(PathBuf, BinaryIdentity)>, String> {
    if let Some(path) = explicit {
        let path = std::fs::canonicalize(path).map_err(|_| "selected nanh cannot be read")?;
        return identify(&path).await.map(|identity| Some((path, identity)));
    }
    let mut paths = BTreeSet::new();
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path).filter(|path| path.is_absolute()) {
            for name in if cfg!(windows) {
                ["nanh.exe", "nan-harness.exe"]
            } else {
                ["nanh", "nan-harness"]
            } {
                let candidate = directory.join(name);
                match std::fs::symlink_metadata(&candidate) {
                    Ok(_) => {
                        paths.insert(
                            std::fs::canonicalize(candidate)
                                .map_err(|_| "existing nanh installation is unreadable")?,
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("nanh installation inventory is unreadable".into()),
                }
            }
        }
    }
    let mut chosen: Option<(PathBuf, BinaryIdentity)> = None;
    for path in paths {
        let identity = identify(&path).await?;
        if chosen.as_ref().is_some_and(|(_, first)| first != &identity) {
            return Err(
                "multiple nanh versions are installed; select one with --nan-harness".into(),
            );
        }
        chosen = Some((path, identity));
    }
    Ok(chosen)
}

async fn identify(path: &Path) -> Result<BinaryIdentity, String> {
    let mut command = tokio::process::Command::new(path);
    command
        .arg("--version")
        .env_remove("NAN_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| "nanh version cannot be inspected")?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or("nanh version cannot be inspected")?;
    let mut bytes = Vec::new();
    let operation = async {
        (&mut stdout).take(4097).read_to_end(&mut bytes).await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>(status)
    };
    let status = tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .map_err(|_| "nanh version inspection timed out")?
        .map_err(|_| "nanh version cannot be inspected")?;
    if !status.success() || bytes.len() > 4096 {
        return Err("invalid nanh version response".into());
    }
    let version = String::from_utf8_lossy(&bytes)
        .split_whitespace()
        .find_map(|word| Version::parse(word).ok())
        .ok_or("nanh version is unknown")?;
    let file = std::fs::File::open(path).map_err(|_| "nanh cannot be read")?;
    let mut bytes = Vec::new();
    file.take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "nanh cannot be read")?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("nanh binary is too large".into());
    }
    Ok(BinaryIdentity {
        version,
        sha256: digest(&bytes),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Available {
    schema_version: u8,
    version: Version,
    notes_url: String,
    artifacts: Vec<Artifact>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    target: String,
    url: String,
    sha256: String,
}

async fn install_nanh(journal: &mut Journal) -> Result<(PathBuf, BinaryIdentity), String> {
    let root = journal.reserve("nanh").map_err(|error| error.to_string())?;
    let outcome = async {
        let manifest = root.join("available.json");
        install::download_file("https://github.com/DavidLMS/nan-harness/releases/download/available/update-manifest.json", &manifest, 1024 * 1024).await.map_err(|_| "cannot download available nanh manifest")?;
        let available: Available = serde_json::from_slice(&std::fs::read(manifest).map_err(|_| "cannot read available manifest")?).map_err(|_| "invalid available manifest")?;
        let prefix = format!("https://github.com/DavidLMS/nan-harness/releases/download/v{}/", available.version);
        if available.schema_version != 1 || available.notes_url != format!("https://github.com/DavidLMS/nan-harness/releases/tag/v{}", available.version) { return Err("invalid release identity".into()); }
        let matches = available.artifacts.iter().filter(|artifact| artifact.target == nanh_target()).collect::<Vec<_>>();
        if matches.len() != 1 || !matches[0].url.starts_with(&prefix) { return Err("no unique official nanh artifact for this platform".into()); }
        let artifact = matches[0];
        let binary = root.join(if cfg!(windows) { "nanh.exe" } else { "nanh" });
        install::download_file(&artifact.url, &binary, 256 * 1024 * 1024).await.map_err(|_| "cannot download nanh")?;
        let bytes = std::fs::read(&binary).map_err(|_| "cannot verify nanh")?;
        if digest(&bytes) != artifact.sha256 { return Err("nanh checksum mismatch".into()); }
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt as _; std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).map_err(|_| "cannot prepare nanh executable")?; }
        let identity = identify(&binary).await?;
        if identity.version != available.version { return Err("nanh version mismatch".into()); }
        Ok((binary, identity))
    }.await;
    journal.seal("nanh").map_err(|error| error.to_string())?;
    outcome
}

fn nanh_target() -> &'static str {
    match (Platform::current(), Architecture::current()) {
        (Platform::Linux, Architecture::X86_64) => "x86_64-unknown-linux-musl",
        (Platform::Linux, Architecture::Aarch64) => "aarch64-unknown-linux-musl",
        (Platform::Macos, Architecture::X86_64) => "x86_64-apple-darwin",
        (Platform::Macos, Architecture::Aarch64) => "aarch64-apple-darwin",
        (Platform::Windows, _) => "x86_64-pc-windows-msvc",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instrumented_results_are_not_public_compatibility_reports() {
        let report = Report {
            schema_version: 2,
            checker_version: Version::new(0, 1, 0),
            run_id: "a".repeat(32),
            started_at: "2026-09-11T00:00:00Z".into(),
            platform: Platform::Linux,
            architecture: Architecture::Aarch64,
            model: None,
            nan_harness: None,
            results: vec![blocked_app(
                DesktopHarnessKind::ChatGpt,
                Reason::NotRun,
                false,
            )],
            cleanup: Status::Passed,
        };
        let normal = report_output(&report, None).unwrap();
        assert_eq!(normal, serde_json::to_vec_pretty(&report).unwrap());
        assert!(Report::parse(&normal).is_ok());
        let wrapper_digest = "b".repeat(64);
        let diagnostic = report_output(&report, Some(&wrapper_digest)).unwrap();
        assert!(Report::parse(&diagnostic).is_err());
        let value: serde_json::Value = serde_json::from_slice(&diagnostic).unwrap();
        assert_eq!(value["wrapperSha256"], wrapper_digest);
        assert_eq!(value["observation"], serde_json::to_value(report).unwrap());
    }

    #[test]
    fn failed_sealing_preserves_observed_steps_and_pending_recovery() {
        let parent = tempfile::tempdir().unwrap();
        let mut journal = Journal::create(parent.path()).unwrap();
        let root = journal.reserve("synthetic").unwrap();
        std::fs::remove_dir(root).unwrap();
        let original = ProbeResult {
            status: Status::Failed,
            steps: vec![crate::report::CheckStep::Launched],
            duration_milliseconds: 123,
            ..ProbeResult::blocked(Reason::SelectorNotMatched)
        };
        let failed = seal_probe(original.clone(), &mut journal, "synthetic");
        assert_eq!(failed.status, Status::Failed);
        assert_eq!(failed.reason, Some(Reason::CleanupFailed));
        assert_eq!(failed.steps, original.steps);
        assert_eq!(failed.duration_milliseconds, 123);
        assert_eq!(journal.pending_names(), vec!["synthetic"]);
    }

    #[test]
    fn live_only_guidance_ignores_unexecuted_deterministic_checks() {
        let mut result = blocked_app(DesktopHarnessKind::Zed, Reason::NotRun, true);
        result.live.reason = None;
        result.live.status = Status::Passed;
        assert_eq!(executed_reason(&result, ExecutionMode::Live, true), None);
        result.live.reason = Some(Reason::InvalidKey);
        result.live.status = Status::Failed;
        assert_eq!(
            executed_reason(&result, ExecutionMode::Live, true),
            Some(Reason::InvalidKey)
        );
    }

    #[test]
    fn uncertain_worker_exit_always_retains_recovery_resources() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("probe.json");
        assert_eq!(
            read_worker_result(&output, Some(2)).reason,
            Some(Reason::CleanupFailed)
        );
        std::fs::write(&output, vec![b' '; 8193]).unwrap();
        assert_eq!(
            read_worker_result(&output, Some(0)).reason,
            Some(Reason::CleanupFailed)
        );
        let result = ProbeResult::blocked(Reason::LoginRequired);
        let outcome = crate::probe::WorkerOutcome {
            result: result.clone(),
            launch_exit: None,
            cleanup: None,
        };
        std::fs::write(&output, serde_json::to_vec(&outcome).unwrap()).unwrap();
        assert_eq!(
            read_worker_result(&output, None).reason,
            Some(Reason::CleanupFailed)
        );
        assert_eq!(
            read_worker_result(&output, Some(0)).reason,
            Some(Reason::CleanupFailed)
        );
        assert_eq!(read_worker_result(&output, Some(1)), result);
    }

    #[cfg(unix)]
    fn private_directory(path: &Path, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::create_dir(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        path.into()
    }

    #[cfg(unix)]
    fn wrapper_args(directory: &Path) -> RunArgs {
        let wrapper = directory.join("wrapper");
        std::fs::write(&wrapper, "synthetic wrapper").unwrap();
        std::fs::write(directory.join("nanh"), "synthetic nanh").unwrap();
        RunArgs {
            apps: vec![DesktopHarnessKind::ChatGpt],
            mode: ExecutionMode::Deterministic,
            model: "qwen3.6".into(),
            nan_harness: Some(directory.join("nanh")),
            launch_wrapper_sha256: Some(crate::probe::binary_digest(&wrapper).unwrap()),
            launch_wrapper: Some(wrapper),
            launch_wrapper_facts: Some(private_directory(&directory.join("facts"), 0o700)),
            ..RunArgs::default()
        }
    }

    #[cfg(unix)]
    fn nanh_identity(directory: &Path) -> (PathBuf, BinaryIdentity) {
        let path = std::fs::canonicalize(directory.join("nanh")).unwrap();
        let sha256 = crate::probe::binary_digest(&path).unwrap();
        let version = Version::new(0, 9, 9);
        (path, BinaryIdentity { version, sha256 })
    }

    #[test]
    fn normal_runs_carry_no_launch_wrapper() {
        let args = RunArgs::default();
        assert_eq!(
            launch_wrapper_scope(&args, &DesktopHarnessKind::ALL),
            Ok(())
        );
        let mut bound = RunArgs::default();
        assert_eq!(bind_launch_wrapper(&mut bound, None), Ok(()));
        assert!(bound.launch_wrapper.is_none());
        assert!(probe_launch_wrapper(&bound, "chatgpt-desktop-deterministic-0").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn out_of_scope_or_incomplete_wrapper_bindings_refuse_before_discovery() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        // Each case gets its own complete, otherwise valid binding, so a
        // refusal can only come from the one field the case changes.
        let fresh = |name: &str| {
            let path = directory.path().join(name);
            std::fs::create_dir(&path).unwrap();
            wrapper_args(&path)
        };
        let refusal = |args: &RunArgs, apps: &[DesktopHarnessKind]| {
            launch_wrapper_scope(args, apps).unwrap_err()
        };
        let chatgpt = [DesktopHarnessKind::ChatGpt];
        let args = fresh("valid");
        assert_eq!(launch_wrapper_scope(&args, &chatgpt), Ok(()));
        for mode in [ExecutionMode::Auto, ExecutionMode::Live] {
            let args = RunArgs {
                mode,
                ..fresh(&format!("{mode:?}"))
            };
            assert!(refusal(&args, &chatgpt).contains("deterministic ChatGPT"));
        }
        for apps in [
            vec![DesktopHarnessKind::Zed],
            vec![DesktopHarnessKind::ChatGpt, DesktopHarnessKind::Zed],
        ] {
            assert!(refusal(&args, &apps).contains("deterministic ChatGPT"));
        }
        let implicit = RunArgs {
            nan_harness: None,
            ..fresh("implicit")
        };
        assert!(refusal(&implicit, &chatgpt).contains("explicit --nan-harness"));
        for (index, digest) in ["A".repeat(64), "a".repeat(63), "g".repeat(64)]
            .into_iter()
            .enumerate()
        {
            let args = RunArgs {
                launch_wrapper_sha256: Some(digest),
                ..fresh(&format!("digest-{index}"))
            };
            assert!(refusal(&args, &chatgpt).contains("lowercase SHA-256"));
        }
        let facts = args.launch_wrapper_facts.clone().unwrap();
        std::fs::set_permissions(&facts, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(refusal(&args, &chatgpt).contains("owner-only"));
        std::fs::set_permissions(&facts, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = directory.path().join("facts-link");
        std::os::unix::fs::symlink(&facts, &link).unwrap();
        for refused in [
            link,
            directory.path().join("missing"),
            PathBuf::from("facts"),
        ] {
            let args = RunArgs {
                launch_wrapper_facts: Some(refused),
                ..args.clone_binding()
            };
            assert!(refusal(&args, &chatgpt).contains("owner-only"));
        }
    }

    #[cfg(unix)]
    impl RunArgs {
        fn clone_binding(&self) -> Self {
            Self {
                apps: self.apps.clone(),
                mode: self.mode,
                model: self.model.clone(),
                nan_harness: self.nan_harness.clone(),
                launch_wrapper: self.launch_wrapper.clone(),
                launch_wrapper_sha256: self.launch_wrapper_sha256.clone(),
                launch_wrapper_facts: self.launch_wrapper_facts.clone(),
                ..Self::default()
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_wrapper_is_bound_separately_from_the_tested_identity() {
        let directory = tempfile::tempdir().unwrap();
        let mut args = wrapper_args(directory.path());
        let nanh = nanh_identity(directory.path());
        assert!(bind_launch_wrapper(&mut args.clone_binding(), None).is_err());
        bind_launch_wrapper(&mut args, Some(&nanh)).unwrap();
        let wrapper = args.launch_wrapper.clone().unwrap();
        assert_eq!(
            wrapper,
            std::fs::canonicalize(directory.path().join("wrapper")).unwrap()
        );
        let probe = probe_launch_wrapper(&args, "chatgpt-desktop-deterministic-1").unwrap();
        assert_eq!(probe.path, wrapper);
        assert_eq!(probe.sha256, args.launch_wrapper_sha256.clone().unwrap());
        assert_eq!(
            probe.facts,
            args.launch_wrapper_facts
                .clone()
                .unwrap()
                .join("chatgpt-desktop-deterministic-1")
        );

        // A changed wrapper, the tested binary itself, or an identical copy of
        // it can never stand in for the separately bound wrapper.
        let mut changed = args.clone_binding();
        std::fs::write(&wrapper, "substituted synthetic wrapper").unwrap();
        assert!(
            bind_launch_wrapper(&mut changed, Some(&nanh))
                .unwrap_err()
                .contains("bound digest")
        );
        let copy = directory.path().join("copy-of-nanh");
        std::fs::copy(&nanh.0, &copy).unwrap();
        for substitute in [nanh.0.clone(), copy] {
            let mut same = RunArgs {
                launch_wrapper: Some(substitute),
                launch_wrapper_sha256: Some(nanh.1.sha256.clone()),
                ..args.clone_binding()
            };
            assert!(
                bind_launch_wrapper(&mut same, Some(&nanh))
                    .unwrap_err()
                    .contains("separate")
            );
        }
    }

    #[tokio::test]
    async fn run_and_prepare_refuse_an_unbound_wrapper_before_any_operation() {
        let args = RunArgs {
            apps: vec![DesktopHarnessKind::ChatGpt],
            model: "qwen3.6".into(),
            launch_wrapper: Some("/synthetic/wrapper".into()),
            launch_wrapper_sha256: Some("a".repeat(64)),
            launch_wrapper_facts: Some("/synthetic/facts".into()),
            ..RunArgs::default()
        };
        assert!(run(args).await.unwrap_err().contains("--launch-wrapper"));
        let args = RunArgs {
            launch_wrapper: Some("/synthetic/wrapper".into()),
            ..RunArgs::default()
        };
        let refused = prepare(args).await.unwrap_err();
        if std::env::var_os("NAN_API_KEY").is_none() {
            assert!(refused.contains("--launch-wrapper"));
        }
    }
}
