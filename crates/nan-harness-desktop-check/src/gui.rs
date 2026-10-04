//! Native controls are resolved inside one app; native errors never enter public reports.

mod accessibility_probe;
#[cfg(any(target_os = "macos", test))]
mod claude_chat_navigation;
#[cfg(target_os = "macos")]
mod claude_native_chat;
mod claude_native_probe;
#[cfg(windows)]
mod claude_windows_chat;
#[cfg(any(windows, test))]
mod claude_windows_fit;
#[cfg(any(windows, test))]
mod claude_windows_ready;
mod clipboard;
mod codex_dom_probe;
mod dom_probe;
mod native_copy_probe;
mod native_icon_probe;
mod process_absence;
mod qualification_directory;
mod stability;
mod visual;
mod zed_zoom_probe;

#[cfg(target_os = "macos")]
pub(crate) use claude_native_chat::ClaudeNativeChatSession;
#[cfg(windows)]
pub(crate) use claude_windows_chat::ClaudeWindowsChatSession;
pub(crate) use codex_dom_probe::CodexDomSession;
pub(crate) use dom_probe::{DomAction, DomPurpose, DomTurn, RendererSession};
pub(crate) use native_copy_probe::NativeClipboardSession;

use crate::process::Observation;
use crate::report::{GuiStage, InputMode, Reason, ResponseVerification};
use nan_harness_core::DesktopHarnessKind;
use std::cell::Cell;
use std::time::{Duration, Instant};
use xa11y::{App, AppExt as _, Locator};

// One rejected native snapshot supplies both the verdict and closed category.
// A later foreground query could describe recovery rather than the rejection.
fn observe_startup_guard(
    guard: impl FnOnce() -> Result<(), (Reason, ComposerErrorCategory)>,
    observations: &mut Vec<ComposerFailure>,
) -> Result<(), Reason> {
    guard().map_err(|(reason, error_category)| {
        observations.push(ComposerFailure {
            operation: ComposerOperation::Guard,
            error_category,
            guard_context: None,
            geometry_relation: None,
            input_observation: None,
        });
        reason
    })
}

const WAIT: Duration = Duration::from_secs(10);
fn attachment_budget(deadline: Option<Instant>) -> Result<Duration, visual::AcquisitionFailure> {
    deadline.map_or(Ok(WAIT), |deadline| {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err((
                Reason::DesktopUnavailable,
                crate::diagnostics::GuiAcquisitionStage::WindowStability,
                ComposerErrorCategory::Other,
                None,
                None,
            ))
        } else {
            Ok(remaining.min(WAIT))
        }
    })
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AbsenceStage {
    AccessibilityProvider,
    AccessibilityEnumeration,
    NativeWindows,
    ProcessEnumeration,
}

pub(crate) struct AbsenceFailure {
    pub(crate) stage: AbsenceStage,
    pub(crate) reason: Reason,
}

pub(crate) struct GuiFailure {
    pub(crate) stage: GuiStage,
    pub(crate) reason: Reason,
    pub(crate) composer: Option<ComposerFailure>,
}

struct InputFailure {
    operation: ComposerOperation,
    reason: Reason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnershipFailure {
    OwnerGroupLookupUnavailable,
    CandidateGroupLookupUnavailable,
    DifferentGroup,
}

impl OwnershipFailure {
    const fn category(self) -> ComposerErrorCategory {
        match self {
            Self::OwnerGroupLookupUnavailable => {
                ComposerErrorCategory::OwnershipOwnerGroupLookupUnavailable
            }
            Self::CandidateGroupLookupUnavailable => {
                ComposerErrorCategory::OwnershipCandidateGroupLookupUnavailable
            }
            Self::DifferentGroup => ComposerErrorCategory::OwnershipDifferentGroup,
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ComposerOperation {
    LocateAccessible,
    AccessibleLoginCheck,
    AccessibleNamedCount,
    AccessibleEditableCount,
    AccessibleEditableVisible,
    LocateVisual,
    VisualClick,
    Guard,
    SetValue,
    Focus,
    WaitFocused,
    InputSim,
    SelectAll,
    TypeText,
    VerifyInputAccessibility,
    VerifyInputVisual,
    VerifyResponse,
    VerifyResponseGuard,
    VerifyResponseAccessibility,
    VerifyResponseVisual,
    Send,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ComposerErrorCategory {
    ActionUnsupported,
    SelectorNotMatched,
    PermissionRequired,
    Timeout,
    WindowChanged,
    FocusChanged,
    WindowIdentityMissing,
    WindowBoundsChanged,
    ForegroundChanged,
    SameProcessWindow,
    WindowOffDisplay,
    WindowOccluded,
    NativeHelperSpawn,
    NativeHelperPipe,
    NativeHelperTimeout,
    NativeHelperNonzeroExit,
    NativeHelperWindowChanged,
    NativeHelperQueryRejected,
    NativeHelperSessionUnavailable,
    OwnershipOwnerGroupLookupUnavailable,
    OwnershipCandidateGroupLookupUnavailable,
    OwnershipDifferentGroup,
    NativeHelperOutput,
    NativeHelperFitRequest,
    NativeHelperFitIdentityRead,
    NativeHelperFitIdentityMismatch,
    NativeHelperFitForegroundRead,
    NativeHelperFitForegroundMismatch,
    NativeHelperFitMonitorRead,
    NativeHelperFitWorkareaRead,
    NativeHelperFitWindowRead,
    NativeHelperFitWorkareaInvalid,
    NativeHelperFitIdentityChanged,
    NativeHelperFitForegroundChanged,
    NativeHelperFitResize,
    NativeHelperFitPostconditionIdentityRead,
    NativeHelperFitPostconditionIdentityMismatch,
    NativeHelperFitPostconditionForegroundRead,
    NativeHelperFitPostconditionForegroundMismatch,
    NativeHelperFitPostconditionWindowRead,
    NativeHelperFitPostconditionGeometry,
    ForegroundProcessDifferent,
    ForegroundIdentityUnavailable,
    ForegroundWindowDifferent,
    InputMismatch,
    EmptyOcrPage,
    MissingComposerAnchor,
    MarkerWithoutComposerAnchor,
    AmbiguousComposerAnchor,
    Other,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum InputAccessibilityObservation {
    NoMatchingControl,
    ReadableEmptyValue,
    ReadableNonmatchingValue,
    ValueReadUnavailable,
    QueryFailed,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ComposerFailure {
    pub(crate) operation: ComposerOperation,
    pub(crate) error_category: ComposerErrorCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) guard_context: Option<ComposerGuardContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) geometry_relation: Option<crate::diagnostics::DisplayGeometryRelation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input_observation: Option<InputAccessibilityObservation>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ComposerGuardContext {
    Reacquisition,
    BeforeInput,
    BeforeSelectAll,
    BeforeType,
    BeforeSend,
    BeforeResponse,
}

fn initial_claude_policy() -> bool {
    #[cfg(windows)]
    {
        claude_windows_ready::policy()
    }
    #[cfg(not(windows))]
    {
        crate::native::claude_focus_policy()
    }
}

pub(crate) struct Gui {
    app: Option<App>,
    app_error: Option<Reason>,
    kind: DesktopHarnessKind,
    visual: visual::Visual,
    #[cfg(any(target_os = "macos", windows))]
    initial_deadline: Cell<Option<Instant>>,
    #[cfg(windows)]
    initial_observation_deadline: Option<Instant>,
}

fn guard_then<T, Guard, Continuation>(
    guard: Guard,
    continuation: Continuation,
) -> Result<T, (Reason, ComposerErrorCategory)>
where
    Guard: FnOnce() -> Result<(), (Reason, ComposerErrorCategory)>,
    Continuation: FnOnce() -> Result<T, (Reason, ComposerErrorCategory)>,
{
    guard()?;
    continuation()
}

#[cfg(any(windows, test))]
fn settle_absence(
    mut query: impl FnMut(Instant) -> Result<(), AbsenceFailure>,
    mut now: impl FnMut() -> Instant,
    mut pause: impl FnMut(Duration),
    deadline: Instant,
) -> Result<(), AbsenceFailure> {
    let mut last = AbsenceFailure {
        stage: AbsenceStage::AccessibilityEnumeration,
        reason: Reason::AlreadyRunning,
    };
    loop {
        if now() >= deadline {
            return Err(last);
        }
        let result = query(deadline);
        if now() >= deadline {
            return Err(match result {
                Err(failure) => failure,
                Ok(()) => last,
            });
        }
        match result {
            Ok(()) => return Ok(()),
            Err(failure) if failure.reason == Reason::AlreadyRunning => last = failure,
            Err(failure) => return Err(failure),
        }
        pause(Duration::from_millis(50).min(deadline.saturating_duration_since(now())));
    }
}

impl Gui {
    pub(crate) fn ensure_absent(kind: DesktopHarnessKind) -> Result<(), AbsenceFailure> {
        Self::absence_snapshot(kind, None, None, false, None)
    }

    pub(crate) fn ensure_absent_before_launch(
        kind: DesktopHarnessKind,
    ) -> Result<(), AbsenceFailure> {
        #[cfg(windows)]
        if kind == DesktopHarnessKind::Claude {
            let native = crate::native::Native::new().map_err(|reason| AbsenceFailure {
                stage: AbsenceStage::NativeWindows,
                reason,
            })?;
            return Self::absence_snapshot(kind, None, Some(&native), true, None);
        }
        Self::ensure_absent(kind)
    }

    #[cfg(windows)]
    pub(crate) fn process_diagnostic_enabled(kind: DesktopHarnessKind) -> bool {
        !(kind != DesktopHarnessKind::Claude
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
            || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref()
                != Ok("startup-baseline")
            || std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() != Ok("private-env"))
    }

    #[cfg(windows)]
    pub(crate) fn capture_owned_cleanup(
        &self,
        launcher: u32,
        expected: &std::path::Path,
        digest: &str,
        deadline: Instant,
    ) -> Option<crate::native::OwnedCleanupHolder> {
        if !Self::process_diagnostic_enabled(self.kind) {
            return None;
        }
        Self::capture_owned_cleanup_native(
            self.visual.absence_native(),
            launcher,
            expected,
            digest,
            deadline,
        )
    }

    #[cfg(windows)]
    pub(crate) fn capture_owned_cleanup_native(
        native: &crate::native::Native,
        launcher: u32,
        expected: &std::path::Path,
        digest: &str,
        deadline: Instant,
    ) -> Option<crate::native::OwnedCleanupHolder> {
        native
            .start_owned_cleanup_holder(launcher, expected, digest, deadline)
            .map_err(|error| {
                let stage = match error {
                    crate::native::FailureCategory::Timeout => "deadline",
                    crate::native::FailureCategory::InvalidInput => "request",
                    crate::native::FailureCategory::Output => return error,
                    _ => "transport",
                };
                crate::process::windows_correlation::record_preflight(&serde_json::json!({
                    "schemaVersion":1,"mechanism":"windows-owned-cleanup-preflight",
                    "diagnosticsOnly":true,"stage":stage}));
                error
            })
            .ok()
    }

    #[cfg(windows)]
    pub(crate) fn capture_process_correlation(
        &self,
        launcher: u32,
        deadline: Instant,
    ) -> Option<crate::process::windows_correlation::Snapshot> {
        if !Self::process_diagnostic_enabled(self.kind) {
            return None;
        }
        let request = zeroize::Zeroizing::new(format!("{} {launcher}\n", std::process::id()));
        let wire = self
            .visual
            .absence_native()
            .process_correlation_until(true, request.as_bytes(), deadline)
            .ok()?;
        crate::process::windows_correlation::Snapshot::parse(wire, launcher)
    }

    pub(crate) fn ensure_absent_after_stop(
        kind: DesktopHarnessKind,
        gui: Option<&Self>,
        #[cfg(windows)] correlation: Option<crate::process::windows_correlation::Snapshot>,
        #[cfg(windows)] holder: Option<crate::native::OwnedCleanupHolder>,
    ) -> Result<(), AbsenceFailure> {
        #[cfg(windows)]
        if kind == DesktopHarnessKind::Claude {
            let deadline = Instant::now() + Duration::from_secs(5);
            let prepared = if gui.is_none() {
                Some(
                    crate::native::Native::new().map_err(|reason| AbsenceFailure {
                        stage: AbsenceStage::NativeWindows,
                        reason,
                    })?,
                )
            } else {
                None
            };
            let native = gui
                .map(|held| held.visual.absence_native())
                .or(prepared.as_ref());
            let wire = correlation.and_then(|snapshot| {
                native?
                    .process_correlation_until(
                        false,
                        snapshot.request().as_bytes(),
                        deadline.min(Instant::now() + Duration::from_secs(1)),
                    )
                    .ok()
            });
            crate::process::windows_correlation::record(
                wire.as_deref().map(String::as_str),
                Instant::now() >= deadline,
            );
            let cleanup = holder.map_or_else(
                || crate::native::owned_cleanup_unavailable(),
                |held| held.cleanup(deadline),
            );
            crate::process::windows_correlation::record_cleanup(&cleanup);
            let mut observed = false;
            let mut settlement = process_absence::ProcessSettlement::default();
            let outcome = settle_absence(
                |bound| {
                    let result = Self::absence_snapshot(
                        kind,
                        Some(bound),
                        native,
                        false,
                        Some(&mut settlement),
                    );
                    let ax_presence = result.as_ref().err().is_some_and(|failure| {
                        failure.stage == AbsenceStage::AccessibilityEnumeration
                            && failure.reason == Reason::AlreadyRunning
                    });
                    if process_absence::mark_rejection_observation(
                        &mut observed,
                        ax_presence,
                        Instant::now(),
                        bound,
                    ) {
                        process_absence::observe_after_accessibility_rejection(
                            bound,
                            native,
                            Some(&mut settlement),
                        );
                    }
                    // Independent evidence never overrides the original absence verdict.
                    result
                },
                Instant::now,
                std::thread::sleep,
                deadline,
            );
            settlement.record();
            return outcome;
        }
        #[cfg(not(windows))]
        let _ = gui;
        Self::ensure_absent(kind)
    }

    fn absence_snapshot(
        kind: DesktopHarnessKind,
        deadline: Option<Instant>,
        retained_native: Option<&crate::native::Native>,
        before_launch: bool,
        settlement: Option<&mut process_absence::ProcessSettlement>,
    ) -> Result<(), AbsenceFailure> {
        let require_budget = |stage| {
            if deadline.is_some_and(|bound| Instant::now() >= bound) {
                Err(AbsenceFailure {
                    stage,
                    reason: Reason::AlreadyRunning,
                })
            } else {
                Ok(())
            }
        };
        require_budget(AbsenceStage::AccessibilityProvider)?;
        // App::list also queries focus. On AT-SPI, closing the final window can
        // make that unrelated query unsupported even when enumeration succeeds.
        let apps = xa11y::provider()
            .map_err(map_error)
            .map_err(|reason| AbsenceFailure {
                stage: AbsenceStage::AccessibilityProvider,
                reason,
            })?
            .list_apps()
            .map_err(map_error)
            .map_err(|reason| AbsenceFailure {
                stage: AbsenceStage::AccessibilityEnumeration,
                reason,
            })?;
        require_names_absent(kind, apps.iter().filter_map(|app| app.name.as_deref())).map_err(
            |reason| AbsenceFailure {
                stage: AbsenceStage::AccessibilityEnumeration,
                reason,
            },
        )?;
        require_budget(AbsenceStage::NativeWindows)?;
        #[cfg(windows)]
        let native_absence = match (retained_native, deadline) {
            (Some(native), Some(bound)) => visual::Visual::ensure_absent_using(native, kind, bound),
            (Some(native), None) => visual::Visual::ensure_absent_with_native(native, kind),
            _ => visual::Visual::ensure_absent(kind),
        };
        #[cfg(not(windows))]
        let native_absence = {
            let _ = (retained_native, before_launch, settlement);
            visual::Visual::ensure_absent(kind)
        };
        native_absence.map_err(|reason| AbsenceFailure {
            stage: AbsenceStage::NativeWindows,
            reason,
        })?;
        require_budget(AbsenceStage::ProcessEnumeration)?;
        #[cfg(windows)]
        match deadline {
            Some(bound) => {
                process_absence::inspect_absent(kind, bound, retained_native, settlement)
            }
            None => process_absence::ensure_absent(kind, retained_native, before_launch),
        }
        .map_err(|reason| AbsenceFailure {
            stage: AbsenceStage::ProcessEnumeration,
            reason,
        })?;
        Ok(())
    }

    pub(crate) fn wait<P: Observation>(
        kind: DesktopHarnessKind,
        process: &mut P,
    ) -> Result<Self, visual::AcquisitionFailure> {
        let deadline = (kind == DesktopHarnessKind::Claude && initial_claude_policy())
            .then(|| Instant::now() + Duration::from_secs(45));
        let visual = visual::Visual::wait(kind, process, deadline)?;
        // The window can become stable before the accessibility bridge
        // registers the process, especially on Linux CI.
        let (app, app_error) = match App::by_pid(visual.pid(), attachment_budget(deadline)?) {
            Ok(app) => (Some(app), None),
            Err(error) => (None, Some(map_error(error))),
        };
        Ok(Self {
            app,
            app_error,
            kind,
            visual,
            #[cfg(any(target_os = "macos", windows))]
            initial_deadline: Cell::new(deadline),
            #[cfg(windows)]
            initial_observation_deadline: deadline,
        })
    }

    #[cfg(any(target_os = "macos", windows))]
    pub(crate) fn finish_initial_ready<P: Observation>(
        &self,
        process: &mut P,
    ) -> Result<(), visual::AcquisitionFailure> {
        #[cfg(target_os = "macos")]
        if let Some(deadline) = self.initial_deadline.take() {
            // Bind after launch diagnostics, immediately before the first
            // conversation guard. The original launch budget is never renewed.
            self.wait_initial_claude_composer(deadline)?;
            self.visual.finish_initial_acquisition(process, deadline)?;
        }
        #[cfg(windows)]
        if let Some(deadline) = self.initial_deadline.take() {
            self.wait_initial_windows_claude_composer(process, deadline)?;
        }
        Ok(())
    }

    pub(crate) fn observe_hosted_startup(
        &self,
        observations: &mut Vec<ComposerFailure>,
    ) -> Result<(), Reason> {
        observe_startup_guard(|| self.visual.guard_composer(), observations)
    }

    pub(crate) fn prepare_conversation(&self) -> Result<(), GuiFailure> {
        if self.kind != DesktopHarnessKind::Zed {
            return Ok(());
        }
        // This app was launched with a fresh private profile and our own workspace.
        // Do not select the broader "trust all projects" checkbox.
        let trust_discovery = |reason| GuiFailure {
            stage: GuiStage::TrustDialogDiscovery,
            reason,
            composer: None,
        };
        let trust_action = |reason| GuiFailure {
            stage: GuiStage::TrustDialogAction,
            reason,
            composer: None,
        };
        let trust_dismissal = |reason| GuiFailure {
            stage: GuiStage::TrustDialogDismissal,
            reason,
            composer: None,
        };
        let panel_stage = |reason| GuiFailure {
            stage: GuiStage::AgentPanel,
            reason,
            composer: None,
        };
        let trust = self
            .available_control("button[name=\"Trust and Continue\"]")
            .map_err(trust_discovery)?;
        if let Some(trust) = trust {
            self.guard_stage(GuiStage::TrustDialogAction)?;
            trust.press().map_err(map_error).map_err(trust_action)?;
            // A successful press may already have begun dismissing the modal.
            // Never dispatch another action; observe the pressed control again.
            trust
                .wait_hidden(WAIT)
                .map_err(map_error)
                .map_err(trust_dismissal)?;
        } else {
            let deadline = Instant::now() + WAIT;
            if let Some((bounds, scale)) = self
                .visual
                .find_phrase("Trust and Continue", deadline)
                .map_err(trust_discovery)?
            {
                self.visual.click(bounds, scale).map_err(trust_action)?;
                // OCR cannot observe the native control's detachment, but this
                // is still a fresh absence observation after the one click.
                let absence_deadline = Instant::now() + WAIT;
                self.visual
                    .wait_phrase_absent("Trust and Continue", absence_deadline)
                    .map_err(trust_dismissal)?;
            } else {
                // The pinned macOS keymap binds Enter to menu::Confirm in this
                // modal. Do not generalize an unproven keyboard path to other
                // platform keymaps.
                if !cfg!(target_os = "macos") {
                    return Err(trust_action(Reason::ActionUnsupported));
                }
                let evidence_deadline = Instant::now() + WAIT;
                keyboard_confirm_once(
                    || {
                        if self.visual.wait_modal_evidence(evidence_deadline)? {
                            Ok(())
                        } else {
                            Err(Reason::SelectorNotMatched)
                        }
                    },
                    || self.require_owned_foreground(),
                    || self.visual.press_confirm(),
                    || self.visual.wait_modal_absent(Instant::now() + WAIT),
                )
                .map_err(|failure| GuiFailure {
                    stage: failure.stage.gui_stage(),
                    reason: failure.reason,
                    composer: None,
                })?;
            }
        }
        if let Some(panel) = self
            .available_control("*[name=\"Agent Panel\"]")
            .map_err(panel_stage)?
        {
            self.guard_stage(GuiStage::AgentPanel)?;
            panel.press().map_err(map_error).map_err(panel_stage)
        } else {
            self.guard_stage(GuiStage::AgentPanel)?;
            xa11y::input_sim()
                .map_err(map_error)
                .map_err(panel_stage)?
                .keyboard()
                .chord(
                    xa11y::Key::Char('/'),
                    &[primary_modifier(), xa11y::Key::Shift],
                )
                .map_err(map_error)
                .map_err(panel_stage)
        }
    }

    fn guard_stage(&self, stage: GuiStage) -> Result<(), GuiFailure> {
        self.visual.guard().map_err(|reason| GuiFailure {
            stage,
            reason,
            composer: None,
        })
    }

    fn require_owned_foreground(&self) -> Result<(), Reason> {
        // A direct foreground query is a second focus proof after the visual
        // guard. It must be owned before the source-visible modal key action.
        let foreground = App::foreground(WAIT)
            .map_err(map_error)
            .map(|app| if app.is_foreground() { app.pid } else { None });
        require_foreground_pid(foreground, self.visual.pid())
    }

    fn available_control(&self, selector: &str) -> Result<Option<Locator>, Reason> {
        let Some(app) = &self.app else {
            return Ok(None);
        };
        let control = app.locator(selector);
        if !unique_accessible_match(control.count().map_err(map_error))? {
            return Ok(None);
        }
        match control.wait_visible(WAIT).map_err(map_error) {
            Ok(_) => Ok(Some(control)),
            Err(Reason::SelectorNotMatched | Reason::ActionUnsupported) => Ok(None),
            Err(reason) => Err(reason),
        }
    }

    pub(crate) fn submit(&self, prompt: &str) -> Result<InputMode, GuiFailure> {
        let input_stage = |operation, reason| GuiFailure {
            stage: GuiStage::ComposerInput,
            reason,
            composer: Some(ComposerFailure {
                operation,
                error_category: error_category(reason),
                guard_context: None,
                geometry_relation: None,
                input_observation: None,
            }),
        };
        let field = match self.input() {
            Ok(field) => field,
            Err(InputFailure {
                reason: Reason::SelectorNotMatched,
                ..
            }) => {
                self.visual.submit(self.kind, prompt)?;
                return Ok(InputMode::VisualAndKeyboard);
            }
            Err(InputFailure { operation, reason }) => return Err(input_stage(operation, reason)),
        };
        self.submit_accessibility(&field, prompt, input_stage)
    }

    fn submit_accessibility(
        &self,
        field: &Locator,
        prompt: &str,
        input_stage: impl Fn(ComposerOperation, Reason) -> GuiFailure,
    ) -> Result<InputMode, GuiFailure> {
        self.visual.reacquire_owned_window().map_err(
            |(reason, error_category, geometry_relation)| GuiFailure {
                stage: GuiStage::ComposerInput,
                reason,
                composer: Some(ComposerFailure {
                    operation: ComposerOperation::Guard,
                    error_category,
                    guard_context: Some(ComposerGuardContext::Reacquisition),
                    geometry_relation,
                    input_observation: None,
                }),
            },
        )?;
        let mode = match guard_then(
            || self.visual.guard_composer(),
            || Ok(field.set_value(prompt)),
        ) {
            Err((reason, category)) => {
                return Err(GuiFailure {
                    stage: GuiStage::ComposerInput,
                    reason,
                    composer: Some(ComposerFailure {
                        operation: ComposerOperation::Guard,
                        error_category: category,
                        guard_context: Some(ComposerGuardContext::BeforeInput),
                        geometry_relation: None,
                        input_observation: None,
                    }),
                });
            }
            Ok(Ok(())) => InputMode::Accessibility,
            Ok(Err(
                xa11y::Error::TextValueNotSupported | xa11y::Error::ActionNotSupported { .. },
            )) => {
                self.keyboard_fill(field, prompt).map_err(
                    |(operation, reason, guard_context)| GuiFailure {
                        stage: GuiStage::ComposerInput,
                        reason,
                        composer: Some(ComposerFailure {
                            operation,
                            error_category: error_category(reason),
                            guard_context,
                            geometry_relation: None,
                            input_observation: None,
                        }),
                    },
                )?;
                InputMode::AccessibilityAndKeyboard
            }
            Ok(Err(error)) => {
                return Err(input_stage(ComposerOperation::SetValue, map_error(error)));
            }
        };
        self.verify_and_send(field, prompt, mode)
    }

    fn verify_and_send(
        &self,
        field: &Locator,
        prompt: &str,
        mode: InputMode,
    ) -> Result<InputMode, GuiFailure> {
        let input_observation = Cell::new(InputAccessibilityObservation::QueryFailed);
        field
            .wait_until(
                |element| match input_readback_observation(element, prompt) {
                    Ok(()) => true,
                    Err(observation) => {
                        input_observation.set(observation);
                        false
                    }
                },
                WAIT,
            )
            .map_err(map_error)
            .map_err(|reason| GuiFailure {
                stage: GuiStage::ComposerInput,
                reason,
                composer: Some(ComposerFailure {
                    operation: ComposerOperation::VerifyInputAccessibility,
                    error_category: error_category(reason),
                    guard_context: None,
                    geometry_relation: None,
                    input_observation: diagnostic_input_observation(
                        self.kind,
                        cfg!(target_os = "macos"),
                        if reason == Reason::Timeout {
                            input_observation.get()
                        } else {
                            InputAccessibilityObservation::QueryFailed
                        },
                    ),
                }),
            })?;
        self.visual.guard().map_err(|reason| GuiFailure {
            stage: GuiStage::ComposerInput,
            reason,
            composer: Some(ComposerFailure {
                operation: ComposerOperation::Guard,
                error_category: error_category(reason),
                guard_context: Some(ComposerGuardContext::BeforeSend),
                geometry_relation: None,
                input_observation: None,
            }),
        })?;
        self.send(field)
            .map_err(|(operation, reason, guard_context)| GuiFailure {
                stage: GuiStage::ComposerSend,
                reason,
                composer: Some(ComposerFailure {
                    operation,
                    error_category: error_category(reason),
                    guard_context,
                    geometry_relation: None,
                    input_observation: None,
                }),
            })?;
        Ok(mode)
    }

    fn keyboard_fill(
        &self,
        field: &Locator,
        prompt: &str,
    ) -> Result<(), (ComposerOperation, Reason, Option<ComposerGuardContext>)> {
        field
            .focus()
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::Focus, reason, None))?;
        field
            .wait_focused(WAIT)
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::WaitFocused, reason, None))?;
        self.visual.guard().map_err(|reason| {
            (
                ComposerOperation::Guard,
                reason,
                Some(ComposerGuardContext::BeforeSelectAll),
            )
        })?;
        let input = xa11y::input_sim()
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::InputSim, reason, None))?;
        input
            .keyboard()
            .chord(
                xa11y::Key::Char('a'),
                &[if cfg!(target_os = "macos") {
                    xa11y::Key::Meta
                } else {
                    xa11y::Key::Ctrl
                }],
            )
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::SelectAll, reason, None))?;
        self.visual.guard().map_err(|reason| {
            (
                ComposerOperation::Guard,
                reason,
                Some(ComposerGuardContext::BeforeType),
            )
        })?;
        input
            .keyboard()
            .type_text(prompt)
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::TypeText, reason, None))
    }

    fn send(
        &self,
        field: &Locator,
    ) -> Result<(), (ComposerOperation, Reason, Option<ComposerGuardContext>)> {
        let Some(app) = self.app.as_ref() else {
            return Err((ComposerOperation::Send, Reason::SelectorNotMatched, None));
        };
        let send = app.locator("button[name=\"Send\"], button[name=\"Send message\"], button[description=\"Send\"], button[description=\"Send message\"]");
        if send
            .count()
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::Send, reason, None))?
            == 1
        {
            return send
                .press()
                .map_err(map_error)
                .map_err(|reason| (ComposerOperation::Send, reason, None));
        }
        field
            .focus()
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::Focus, reason, None))?;
        field
            .wait_focused(WAIT)
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::WaitFocused, reason, None))?;
        self.visual.guard().map_err(|reason| {
            (
                ComposerOperation::Guard,
                reason,
                Some(ComposerGuardContext::BeforeSend),
            )
        })?;
        xa11y::input_sim()
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::InputSim, reason, None))?
            .keyboard()
            .press(xa11y::Key::Enter)
            .map_err(map_error)
            .map_err(|reason| (ComposerOperation::Send, reason, None))
    }

    fn input(&self) -> Result<Locator, InputFailure> {
        let app = self.app.as_ref().ok_or(InputFailure {
            operation: ComposerOperation::LocateAccessible,
            reason: Reason::SelectorNotMatched,
        })?;
        if self.kind != DesktopHarnessKind::Zed
            && app
                .locator("button[name=\"Sign in\"], button[name=\"Log in\"], button[name=\"Continue with Google\"], button[name=\"Continue with email\"]")
                .count()
                .map_err(map_error)
                .map_err(|reason| InputFailure {
                    operation: ComposerOperation::AccessibleLoginCheck,
                    reason,
                })?
                > 0
        {
            return Err(InputFailure {
                operation: ComposerOperation::AccessibleLoginCheck,
                reason: Reason::LoginRequired,
            });
        }
        // App-specific accessible placeholders, followed by a unique editable control.
        let labels = match self.kind {
            DesktopHarnessKind::ChatGpt => {
                &["Message", "Ask anything", "Ask for follow-up changes"][..]
            }
            DesktopHarnessKind::Claude => &[
                "Reply to Claude",
                "How can I help you today?",
                "Message Claude",
            ][..],
            DesktopHarnessKind::Hermes => &["Message Hermes", "Message", "Type a message"][..],
            DesktopHarnessKind::Pen => {
                &["Message", "Ask Pen", "Describe what you want to build"][..]
            }
            DesktopHarnessKind::Zed => &["Message Editor", "Message", "Ask anything"][..],
        };
        let selectors = labels
            .iter()
            .flat_map(|label| {
                [
                    format!("text_area[name=\"{label}\"]"),
                    format!("text_field[name=\"{label}\"]"),
                    format!("text_area[description=\"{label}\"]"),
                    format!("text_field[description=\"{label}\"]"),
                ]
            })
            .collect::<Vec<_>>()
            .join(", ");
        let named = app.locator(&selectors);
        if named
            .count()
            .map_err(map_error)
            .map_err(|reason| InputFailure {
                operation: ComposerOperation::AccessibleNamedCount,
                reason,
            })?
            == 1
        {
            return Ok(named);
        }
        let editable = app.locator("text_area[editable=\"true\"], text_field[editable=\"true\"]");
        if self.kind == DesktopHarnessKind::Zed
            && editable
                .count()
                .map_err(map_error)
                .map_err(|reason| InputFailure {
                    operation: ComposerOperation::AccessibleEditableCount,
                    reason,
                })?
                == 0
        {
            return Err(InputFailure {
                operation: ComposerOperation::AccessibleEditableCount,
                reason: Reason::SelectorNotMatched,
            });
        }
        editable
            .wait_visible(WAIT)
            .map_err(map_error)
            .map_err(|reason| InputFailure {
                operation: ComposerOperation::AccessibleEditableVisible,
                reason,
            })?;
        if editable
            .count()
            .map_err(map_error)
            .map_err(|reason| InputFailure {
                operation: ComposerOperation::AccessibleEditableCount,
                reason,
            })?
            != 1
        {
            return Err(InputFailure {
                operation: ComposerOperation::AccessibleEditableCount,
                reason: Reason::SelectorNotMatched,
            });
        }
        Ok(editable)
    }

    pub(crate) fn wait_text(
        &self,
        marker: &str,
        budget: Duration,
        composer_observations: &mut Vec<ComposerFailure>,
    ) -> Result<ResponseVerification, Reason> {
        let selector = response_selector(marker)?;
        let deadline = Instant::now() + budget;
        loop {
            self.visual.guard_composer().map_err(|(reason, category)| {
                composer_observations.push(ComposerFailure {
                    operation: ComposerOperation::VerifyResponseGuard,
                    error_category: category,
                    guard_context: Some(ComposerGuardContext::BeforeResponse),
                    geometry_relation: None,
                    input_observation: None,
                });
                reason
            })?;
            if let Some(app) = &self.app
                && app
                    .locator(&selector)
                    .count()
                    .map_err(map_error)
                    .inspect_err(|&reason| {
                        composer_observations.push(ComposerFailure {
                            operation: ComposerOperation::VerifyResponseAccessibility,
                            error_category: error_category(reason),
                            guard_context: None,
                            geometry_relation: None,
                            input_observation: None,
                        });
                    })?
                    > 0
            {
                return Ok(ResponseVerification::Accessibility);
            }
            let (pending_reason, category) = match self.visual.contains_response(self.kind, marker)
            {
                Ok(true) => return Ok(ResponseVerification::LocalOcr),
                Ok(false) => (
                    Reason::ResponseMismatch,
                    error_category(Reason::ResponseMismatch),
                ),
                // The composer can disappear during a response/layout transition.
                // Keep polling, but do not misreport a missing region as wrong text.
                Err((Reason::SelectorNotMatched, category)) => {
                    (Reason::SelectorNotMatched, category)
                }
                Err((reason, category)) => {
                    composer_observations.push(ComposerFailure {
                        operation: ComposerOperation::VerifyResponseVisual,
                        error_category: category,
                        guard_context: None,
                        geometry_relation: None,
                        input_observation: None,
                    });
                    return Err(reason);
                }
            };
            if Instant::now() >= deadline {
                composer_observations.push(ComposerFailure {
                    operation: ComposerOperation::VerifyResponseVisual,
                    error_category: category,
                    guard_context: None,
                    geometry_relation: None,
                    input_observation: None,
                });
                return Err(pending_reason);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub(crate) fn quit(&self) -> Result<(), Reason> {
        if let Some(app) = &self.app {
            let quit = app.locator("menu_item[name^=\"Quit\"], menu_item[name=\"Exit\"]");
            if quit.count().map_err(map_error)? == 1 {
                self.visual.guard()?;
                return quit.press().map_err(map_error);
            }
        }
        self.visual.guard()?;
        let input = xa11y::input_sim().map_err(map_error)?;
        if cfg!(target_os = "macos") {
            input
                .keyboard()
                .chord(xa11y::Key::Char('q'), &[xa11y::Key::Meta])
                .map_err(map_error)
        } else {
            input
                .keyboard()
                .chord(xa11y::Key::F(4), &[xa11y::Key::Alt])
                .map_err(map_error)
        }
    }
}

fn input_readback_observation(
    element: Option<&xa11y::ElementData>,
    expected: &str,
) -> Result<(), InputAccessibilityObservation> {
    let Some(element) = element else {
        return input_value_observation(false, None, expected);
    };
    input_value_observation(true, element.value.as_deref(), expected)
}

fn input_value_observation(
    control_present: bool,
    value: Option<&str>,
    expected: &str,
) -> Result<(), InputAccessibilityObservation> {
    if !control_present {
        return Err(InputAccessibilityObservation::NoMatchingControl);
    }
    let Some(value) = value else {
        return Err(InputAccessibilityObservation::ValueReadUnavailable);
    };
    if value == expected {
        Ok(())
    } else if value.is_empty() {
        Err(InputAccessibilityObservation::ReadableEmptyValue)
    } else {
        Err(InputAccessibilityObservation::ReadableNonmatchingValue)
    }
}

fn diagnostic_input_observation(
    kind: DesktopHarnessKind,
    is_macos: bool,
    observation: InputAccessibilityObservation,
) -> Option<InputAccessibilityObservation> {
    (is_macos && kind == DesktopHarnessKind::Pen).then_some(observation)
}

fn require_names_absent<'a>(
    kind: DesktopHarnessKind,
    mut names: impl Iterator<Item = &'a str>,
) -> Result<(), Reason> {
    if names.any(|name| app_names(kind).contains(&name)) {
        Err(Reason::AlreadyRunning)
    } else {
        Ok(())
    }
}

const fn primary_modifier() -> xa11y::Key {
    if cfg!(target_os = "macos") {
        xa11y::Key::Meta
    } else {
        xa11y::Key::Ctrl
    }
}

#[cfg(unix)]
fn process_ownership(pid: u32, owner: u32) -> Result<(), OwnershipFailure> {
    use nix::unistd::{Pid, getpgid};
    let owner = i32::try_from(owner)
        .ok()
        .filter(|id| *id > 0)
        .ok_or(OwnershipFailure::OwnerGroupLookupUnavailable)?;
    let pid = i32::try_from(pid)
        .ok()
        .filter(|id| *id > 0)
        .ok_or(OwnershipFailure::CandidateGroupLookupUnavailable)?;
    classify_process_groups(
        getpgid(Some(Pid::from_raw(owner)))
            .map(Pid::as_raw)
            .map_err(|_| ()),
        getpgid(Some(Pid::from_raw(pid)))
            .map(Pid::as_raw)
            .map_err(|_| ()),
    )
}

#[cfg(windows)]
fn qualification_process_ownership(pid: u32, owner: u32) -> Option<Result<(), OwnershipFailure>> {
    use std::io::Read as _;
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return None;
    }
    let python = std::env::var_os("FEASIBILITY_WINDOWS_PROOF_PYTHON")?;
    let script = std::env::var_os("FEASIBILITY_WINDOWS_PROOF_SCRIPT")?;
    let outcome = (|| {
        let python = std::path::PathBuf::from(python);
        let script = std::path::PathBuf::from(script);
        if !python.is_absolute()
            || !python.is_file()
            || python.is_symlink()
            || !script.is_absolute()
            || !script.is_file()
            || script.is_symlink()
        {
            return Err(OwnershipFailure::CandidateGroupLookupUnavailable);
        }
        let mut command = Command::new(python);
        command
            .env_clear()
            .arg(script)
            .arg("descendant")
            .arg(pid.to_string())
            .arg(owner.to_string())
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        let mut child = command
            .spawn()
            .map_err(|_| OwnershipFailure::CandidateGroupLookupUnavailable)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(OwnershipFailure::CandidateGroupLookupUnavailable);
                }
            }
        }
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or(OwnershipFailure::CandidateGroupLookupUnavailable)?
            .take(64)
            .read_to_end(&mut output)
            .map_err(|_| OwnershipFailure::CandidateGroupLookupUnavailable)?;
        if output == b"true" {
            Ok(())
        } else {
            Err(OwnershipFailure::CandidateGroupLookupUnavailable)
        }
    })();
    Some(outcome)
}

#[cfg(windows)]
fn process_ownership(pid: u32, owner: u32) -> Result<(), OwnershipFailure> {
    use std::process::{Command, Stdio};
    if owner == 0 {
        return Err(OwnershipFailure::OwnerGroupLookupUnavailable);
    }
    if pid == 0 {
        return Err(OwnershipFailure::CandidateGroupLookupUnavailable);
    }
    if let Some(outcome) = qualification_process_ownership(pid, owner) {
        return outcome;
    }
    Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "try { $candidateId = [uint32]$env:NAN_CHECK_APP_PID; $ownerId = [uint32]$env:NAN_CHECK_OWNER_PID; for ($depth = 0; $depth -lt 32; $depth++) { if ($candidateId -eq $ownerId) { exit 0 }; $candidate = Get-CimInstance Win32_Process -Filter \"ProcessId=$candidateId\" -ErrorAction Stop; if ($null -eq $candidate) { exit 43 }; if ($candidate.ParentProcessId -eq 0) { exit 42 }; $candidateId = $candidate.ParentProcessId }; exit 43 } catch { exit 43 }"])
        .env("NAN_CHECK_APP_PID", pid.to_string()).env("NAN_CHECK_OWNER_PID", owner.to_string()).env_remove("NAN_API_KEY").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map_or(Err(OwnershipFailure::CandidateGroupLookupUnavailable), |status| if status.success() { Ok(()) } else if status.code() == Some(42) { Err(OwnershipFailure::DifferentGroup) } else { Err(OwnershipFailure::CandidateGroupLookupUnavailable) })
}

#[cfg(unix)]
fn classify_process_groups(
    owner: Result<i32, ()>,
    candidate: Result<i32, ()>,
) -> Result<(), OwnershipFailure> {
    let owner = owner.map_err(|()| OwnershipFailure::OwnerGroupLookupUnavailable)?;
    let candidate = candidate.map_err(|()| OwnershipFailure::CandidateGroupLookupUnavailable)?;
    if candidate == owner {
        Ok(())
    } else {
        Err(OwnershipFailure::DifferentGroup)
    }
}

fn app_names(kind: DesktopHarnessKind) -> &'static [&'static str] {
    match kind {
        DesktopHarnessKind::ChatGpt => &["ChatGPT", "Codex"],
        DesktopHarnessKind::Claude => &["Claude", "claude-desktop"],
        DesktopHarnessKind::Hermes => &["Hermes", "Hermes Desktop"],
        DesktopHarnessKind::Pen => &["Pen", "Pencil"],
        DesktopHarnessKind::Zed => &["Zed", "zed", "zed-editor", "zeditor"],
    }
}

fn response_selector(marker: &str) -> Result<String, Reason> {
    if marker.trim().is_empty()
        || marker.len() > 256
        || !marker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_: ".contains(&byte))
    {
        return Err(Reason::ResponseMismatch);
    }
    Ok(format!(
        "[visible=\"true\"][editable=\"false\"][name*=\"{marker}\"], [visible=\"true\"][editable=\"false\"][value*=\"{marker}\"]"
    ))
}

fn require_foreground_pid(
    foreground: Result<Option<u32>, Reason>,
    owner: u32,
) -> Result<(), Reason> {
    if foreground? == Some(owner) {
        Ok(())
    } else {
        Err(Reason::FocusChanged)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyboardConfirmStage {
    Evidence,
    Focus,
    Confirm,
    Dismissal,
}

impl KeyboardConfirmStage {
    const fn gui_stage(self) -> GuiStage {
        match self {
            Self::Evidence => GuiStage::TrustDialogDiscovery,
            Self::Focus | Self::Confirm => GuiStage::TrustDialogAction,
            Self::Dismissal => GuiStage::TrustDialogDismissal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyboardConfirmFailure {
    stage: KeyboardConfirmStage,
    reason: Reason,
}

fn keyboard_confirm_once(
    mut modal_evidence: impl FnMut() -> Result<(), Reason>,
    mut owned_focus: impl FnMut() -> Result<(), Reason>,
    mut confirm: impl FnMut() -> Result<(), Reason>,
    mut modal_absent: impl FnMut() -> Result<(), Reason>,
) -> Result<(), KeyboardConfirmFailure> {
    modal_evidence().map_err(|reason| KeyboardConfirmFailure {
        stage: KeyboardConfirmStage::Evidence,
        reason,
    })?;
    owned_focus().map_err(|reason| KeyboardConfirmFailure {
        stage: KeyboardConfirmStage::Focus,
        reason,
    })?;
    confirm().map_err(|reason| KeyboardConfirmFailure {
        stage: KeyboardConfirmStage::Confirm,
        reason,
    })?;
    modal_absent().map_err(|reason| KeyboardConfirmFailure {
        stage: KeyboardConfirmStage::Dismissal,
        reason,
    })?;
    Ok(())
}

fn map_error(error: xa11y::Error) -> Reason {
    let reason = match &error {
        xa11y::Error::PermissionDenied { .. } => Reason::PermissionRequired,
        xa11y::Error::TextValueNotSupported
        | xa11y::Error::ActionNotSupported { .. }
        | xa11y::Error::InvalidActionData { .. }
        | xa11y::Error::Unsupported { .. }
        | xa11y::Error::InvalidSelector { .. }
        | xa11y::Error::InvalidConfig { .. }
        | xa11y::Error::AccessibilityNotEnabled { .. } => Reason::ActionUnsupported,
        xa11y::Error::Timeout { .. } => Reason::Timeout,
        xa11y::Error::ElementStale { .. } => Reason::WindowChanged,
        xa11y::Error::NoElementBounds => Reason::IsolationUnavailable,
        xa11y::Error::SelectorNotMatched { .. } => Reason::SelectorNotMatched,
        xa11y::Error::Platform { .. } => Reason::DesktopUnavailable,
    };
    drop(error);
    reason
}

pub(crate) fn error_category(reason: Reason) -> ComposerErrorCategory {
    match reason {
        Reason::ActionUnsupported => ComposerErrorCategory::ActionUnsupported,
        Reason::SelectorNotMatched => ComposerErrorCategory::SelectorNotMatched,
        Reason::PermissionRequired => ComposerErrorCategory::PermissionRequired,
        Reason::Timeout => ComposerErrorCategory::Timeout,
        Reason::InputMismatch => ComposerErrorCategory::InputMismatch,
        Reason::WindowChanged => ComposerErrorCategory::WindowChanged,
        Reason::FocusChanged => ComposerErrorCategory::FocusChanged,
        _ => ComposerErrorCategory::Other,
    }
}

fn unique_accessible_match(count: Result<usize, Reason>) -> Result<bool, Reason> {
    // Fallback is chosen before input. Never retry an uncertain press, choose
    // between duplicate controls, or bypass a denied accessibility permission.
    match count {
        Ok(0) | Err(Reason::SelectorNotMatched | Reason::ActionUnsupported) => Ok(false),
        Ok(1) => Ok(true),
        Ok(_) => Err(Reason::SelectorNotMatched),
        Err(reason) => Err(reason),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn accessibility_attachment_uses_only_remaining_original_acquisition_budget() {
        assert_eq!(attachment_budget(None).unwrap(), WAIT);
        assert!(attachment_budget(Some(Instant::now())).is_err());
        let budget = attachment_budget(Some(Instant::now() + Duration::from_secs(2))).unwrap();
        assert!(budget > Duration::ZERO && budget <= Duration::from_secs(2));
    }

    #[test]
    fn post_stop_settles_only_presence_with_one_shared_deadline() {
        use std::cell::Cell;
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let deadline = start + Duration::from_millis(100);
        let result = settle_absence(
            |bound| {
                assert_eq!(bound, deadline);
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Err(AbsenceFailure {
                        stage: AbsenceStage::AccessibilityEnumeration,
                        reason: Reason::AlreadyRunning,
                    })
                } else {
                    Ok(())
                }
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
            deadline,
        );
        assert!(result.is_ok());
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn post_stop_query_error_and_late_success_never_pass() {
        use std::cell::Cell;
        let start = Instant::now();
        let clock = Cell::new(start);
        let deadline = start + Duration::from_millis(100);
        let calls = Cell::new(0);
        let result = settle_absence(
            |_| {
                calls.set(calls.get() + 1);
                Err(AbsenceFailure {
                    stage: AbsenceStage::ProcessEnumeration,
                    reason: Reason::DesktopUnavailable,
                })
            },
            || clock.get(),
            |_| panic!("query errors must not retry"),
            deadline,
        );
        assert_eq!(result.err().unwrap().reason, Reason::DesktopUnavailable);
        assert_eq!(calls.get(), 1);
        let result = settle_absence(
            |_| {
                clock.set(deadline);
                Ok(())
            },
            || clock.get(),
            |_| panic!("late result must stop"),
            deadline,
        );
        assert_eq!(result.err().unwrap().reason, Reason::AlreadyRunning);
    }

    #[test]
    fn post_stop_persistent_presence_never_queries_after_deadline() {
        use std::cell::Cell;
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let deadline = start + Duration::from_millis(100);
        let result = settle_absence(
            |_| {
                calls.set(calls.get() + 1);
                Err(AbsenceFailure {
                    stage: AbsenceStage::NativeWindows,
                    reason: Reason::AlreadyRunning,
                })
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
            deadline,
        );
        let failure = result.err().unwrap();
        assert_eq!(failure.stage, AbsenceStage::NativeWindows);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn startup_guard_records_only_the_exact_rejected_snapshot_without_requery() {
        use super::*;
        let mut observations = Vec::new();
        let mut calls = 0;
        assert_eq!(
            observe_startup_guard(
                || {
                    calls += 1;
                    Ok(())
                },
                &mut observations
            ),
            Ok(())
        );
        assert_eq!(calls, 1);
        assert!(observations.is_empty());
        for category in [
            ComposerErrorCategory::ForegroundIdentityUnavailable,
            ComposerErrorCategory::ForegroundProcessDifferent,
            ComposerErrorCategory::ForegroundWindowDifferent,
            ComposerErrorCategory::SameProcessWindow,
        ] {
            let count = observations.len();
            assert_eq!(
                observe_startup_guard(
                    || {
                        calls += 1;
                        Err((Reason::FocusChanged, category))
                    },
                    &mut observations
                ),
                Err(Reason::FocusChanged)
            );
            assert_eq!(observations.len(), count + 1);
            assert_eq!(observations[count].error_category, category);
            assert_eq!(observations[count].operation, ComposerOperation::Guard);
            assert_eq!(observations[count].guard_context, None);
        }
        assert_eq!(calls, 5);
        let prefix = observations.clone();
        assert_eq!(
            observe_startup_guard(
                || Err((
                    Reason::WindowChanged,
                    ComposerErrorCategory::WindowIdentityMissing
                )),
                &mut observations
            ),
            Err(Reason::WindowChanged)
        );
        assert_eq!(observations[..prefix.len()], prefix);
        let serialized = serde_json::to_value(&observations).unwrap();
        assert_eq!(
            serialized[0],
            serde_json::json!({"operation":"guard", "errorCategory":"foreground-identity-unavailable"})
        );
        assert_eq!(
            serialized[3],
            serde_json::json!({"operation":"guard", "errorCategory":"same-process-window"})
        );
    }

    use super::*;

    #[test]
    fn timeouts_are_never_reported_as_selector_absence() {
        assert_eq!(
            map_error(xa11y::Error::timeout(Duration::from_secs(1))),
            Reason::Timeout
        );
        assert_eq!(
            map_error(xa11y::Error::selector_not_matched("test selector")),
            Reason::SelectorNotMatched
        );
        assert_eq!(
            map_error(xa11y::Error::AccessibilityNotEnabled {
                app: "test app".into(),
                instructions: "test instructions".into(),
            }),
            Reason::ActionUnsupported
        );
    }

    #[test]
    fn input_verification_keeps_timeout_mismatch_and_provider_failures_distinct() {
        assert_eq!(
            error_category(Reason::Timeout),
            ComposerErrorCategory::Timeout
        );
        assert_eq!(
            error_category(Reason::InputMismatch),
            ComposerErrorCategory::InputMismatch
        );
        assert_eq!(
            error_category(Reason::ActionUnsupported),
            ComposerErrorCategory::ActionUnsupported
        );
        assert_eq!(
            error_category(Reason::PermissionRequired),
            ComposerErrorCategory::PermissionRequired
        );
        assert_ne!(
            ComposerOperation::VerifyInputAccessibility,
            ComposerOperation::VerifyInputVisual
        );
    }

    #[test]
    fn missing_accessible_controls_allow_fallback_but_ambiguity_and_permissions_do_not() {
        for count in [
            Ok(0),
            Err(Reason::SelectorNotMatched),
            Err(Reason::ActionUnsupported),
        ] {
            assert_eq!(unique_accessible_match(count), Ok(false));
        }
        assert_eq!(unique_accessible_match(Ok(1)), Ok(true));
        assert_eq!(
            unique_accessible_match(Ok(2)),
            Err(Reason::SelectorNotMatched)
        );
        assert_eq!(
            unique_accessible_match(Err(Reason::PermissionRequired)),
            Err(Reason::PermissionRequired)
        );
        assert_eq!(
            unique_accessible_match(Err(Reason::ApplicationExited)),
            Err(Reason::ApplicationExited)
        );
    }

    #[test]
    fn input_readback_observation_uses_only_the_current_element_snapshot() {
        assert_eq!(
            input_value_observation(false, None, "expected"),
            Err(InputAccessibilityObservation::NoMatchingControl)
        );
        for (value, expected) in [
            (None, InputAccessibilityObservation::ValueReadUnavailable),
            (Some(""), InputAccessibilityObservation::ReadableEmptyValue),
            (
                Some("different"),
                InputAccessibilityObservation::ReadableNonmatchingValue,
            ),
        ] {
            assert_eq!(
                input_value_observation(true, value, "expected"),
                Err(expected)
            );
        }
        assert_eq!(
            input_value_observation(true, Some("expected"), "expected"),
            Ok(())
        );
        let observation = InputAccessibilityObservation::ReadableEmptyValue;
        assert_eq!(
            diagnostic_input_observation(DesktopHarnessKind::Pen, true, observation),
            Some(observation)
        );
        assert_eq!(
            diagnostic_input_observation(DesktopHarnessKind::Claude, true, observation),
            None
        );
        assert_eq!(
            diagnostic_input_observation(DesktopHarnessKind::Pen, false, observation),
            None
        );
    }

    #[test]
    fn absence_checks_need_app_names_not_a_focused_window() {
        assert_eq!(
            require_names_absent(DesktopHarnessKind::Zed, std::iter::empty()),
            Ok(())
        );
        for name in app_names(DesktopHarnessKind::Zed) {
            assert_eq!(
                require_names_absent(DesktopHarnessKind::Zed, std::iter::once(*name)),
                Err(Reason::AlreadyRunning)
            );
        }
        assert_eq!(
            require_names_absent(
                DesktopHarnessKind::Zed,
                std::iter::once("unrelated application")
            ),
            Ok(())
        );
    }

    #[test]
    fn response_selectors_exclude_input_and_hidden_text() {
        let selector = response_selector("NAN_CHECK_FINAL:NAN_CHECK_READ_abc").unwrap();
        assert!(
            response_selector(&format!(
                "NAN CHECK RESPONSE {}",
                "island ".repeat(32).trim()
            ))
            .is_ok()
        );
        assert!(response_selector(&"a".repeat(257)).is_err());
        assert!(response_selector("   ").is_err());
        assert!(
            selector
                .split(", ")
                .all(|part| part.contains("[visible=\"true\"][editable=\"false\"]"))
        );
        assert!(xa11y::SelectorGroup::parse(&selector).is_ok());
        assert!(response_selector("unsafe\"selector").is_err());
    }

    #[test]
    fn foreground_pid_must_be_the_verified_owner() {
        assert_eq!(require_foreground_pid(Ok(Some(7)), 7), Ok(()));
        assert_eq!(
            require_foreground_pid(Ok(None), 7),
            Err(Reason::FocusChanged)
        );
        assert_eq!(
            require_foreground_pid(Ok(Some(8)), 7),
            Err(Reason::FocusChanged)
        );
        assert_eq!(
            require_foreground_pid(Err(Reason::ActionUnsupported), 7),
            Err(Reason::ActionUnsupported)
        );
    }

    #[test]
    fn guard_then_preserves_guard_failure_and_skips_continuation() {
        let boundaries = [
            ComposerGuardContext::Reacquisition,
            ComposerGuardContext::BeforeInput,
            ComposerGuardContext::BeforeSelectAll,
            ComposerGuardContext::BeforeType,
            ComposerGuardContext::BeforeSend,
            ComposerGuardContext::BeforeResponse,
        ];
        for context in boundaries {
            let mut downstream_input = 0;
            let result = guard_then(
                || {
                    Err((
                        Reason::FocusChanged,
                        ComposerErrorCategory::ForegroundProcessDifferent,
                    ))
                },
                || {
                    downstream_input += 1;
                    Ok::<_, (Reason, ComposerErrorCategory)>(())
                },
            );
            assert_eq!(
                result,
                Err((
                    Reason::FocusChanged,
                    ComposerErrorCategory::ForegroundProcessDifferent,
                ))
            );
            assert_eq!(downstream_input, 0);
            let failure = ComposerFailure {
                operation: ComposerOperation::Guard,
                error_category: ComposerErrorCategory::ForegroundProcessDifferent,
                guard_context: Some(context),
                geometry_relation: None,
                input_observation: None,
            };
            assert_eq!(failure.guard_context, Some(context));
        }
    }

    #[test]
    fn keyboard_confirm_never_repeats_an_uncertain_action() {
        let mut focus_calls = 0;
        let mut confirm_calls = 0;
        assert_eq!(
            keyboard_confirm_once(
                || Err(Reason::ResponseMismatch),
                || {
                    focus_calls += 1;
                    Ok(())
                },
                || {
                    confirm_calls += 1;
                    Ok(())
                },
                || Ok(()),
            ),
            Err(KeyboardConfirmFailure {
                stage: KeyboardConfirmStage::Evidence,
                reason: Reason::ResponseMismatch,
            })
        );
        assert_eq!((focus_calls, confirm_calls), (0, 0));

        for reason in [Reason::SelectorNotMatched, Reason::ResponseMismatch] {
            let mut confirm_calls = 0;
            assert_eq!(
                keyboard_confirm_once(
                    || Err(reason),
                    || Ok(()),
                    || {
                        confirm_calls += 1;
                        Ok(())
                    },
                    || Ok(()),
                )
                .map_err(|failure| failure.reason),
                Err(reason)
            );
            assert_eq!(confirm_calls, 0);
        }

        let mut confirm_calls = 0;
        assert_eq!(
            keyboard_confirm_once(
                || Ok(()),
                || Err(Reason::FocusChanged),
                || {
                    confirm_calls += 1;
                    Ok(())
                },
                || Ok(()),
            ),
            Err(KeyboardConfirmFailure {
                stage: KeyboardConfirmStage::Focus,
                reason: Reason::FocusChanged,
            })
        );
        assert_eq!(confirm_calls, 0);

        let mut confirm_calls = 0;
        assert_eq!(
            keyboard_confirm_once(
                || Ok(()),
                || Ok(()),
                || {
                    confirm_calls += 1;
                    Ok(())
                },
                || Err(Reason::Timeout),
            ),
            Err(KeyboardConfirmFailure {
                stage: KeyboardConfirmStage::Dismissal,
                reason: Reason::Timeout,
            })
        );
        assert_eq!(confirm_calls, 1);

        let mut confirm_calls = 0;
        let mut absence_calls = 0;
        assert_eq!(
            keyboard_confirm_once(
                || Ok(()),
                || Ok(()),
                || {
                    confirm_calls += 1;
                    Err(Reason::ActionUnsupported)
                },
                || {
                    absence_calls += 1;
                    Ok(())
                },
            ),
            Err(KeyboardConfirmFailure {
                stage: KeyboardConfirmStage::Confirm,
                reason: Reason::ActionUnsupported,
            })
        );
        assert_eq!((confirm_calls, absence_calls), (1, 0));
    }

    #[cfg(unix)]
    #[test]
    fn process_ownership_requires_a_known_group() {
        assert_eq!(
            process_ownership(std::process::id(), std::process::id()),
            Ok(())
        );
        assert_eq!(
            process_ownership(u32::MAX, std::process::id()),
            Err(OwnershipFailure::CandidateGroupLookupUnavailable)
        );
        assert_eq!(
            process_ownership(std::process::id(), u32::MAX),
            Err(OwnershipFailure::OwnerGroupLookupUnavailable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_ownership_distinguishes_a_proven_group_mismatch() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let result = process_ownership(child.id(), std::process::id());
        child.kill().unwrap();
        let _ = child.wait();
        assert_eq!(result, Err(OwnershipFailure::DifferentGroup));
    }

    #[cfg(unix)]
    #[test]
    fn process_group_classification_preserves_lookup_boundaries() {
        assert_eq!(
            classify_process_groups(Err(()), Ok(7)),
            Err(OwnershipFailure::OwnerGroupLookupUnavailable)
        );
        assert_eq!(
            classify_process_groups(Ok(7), Err(())),
            Err(OwnershipFailure::CandidateGroupLookupUnavailable)
        );
        assert_eq!(classify_process_groups(Ok(7), Ok(7)), Ok(()));
        assert_eq!(
            classify_process_groups(Ok(7), Ok(8)),
            Err(OwnershipFailure::DifferentGroup)
        );
    }
}

#[cfg(windows)]
impl Gui {
    fn record_claude_windows_uia(&self, directory: &std::path::Path) {
        use std::io::Write as _;
        if self.kind != DesktopHarnessKind::Claude || !claude_windows_ready::policy() {
            return;
        }
        let Some(expected) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
            return;
        };
        let Some(directory) = qualification_directory::canonical_directory(directory) else {
            return;
        };
        if qualification_directory::canonical_directory(std::path::Path::new(&expected)).as_ref()
            != Some(&directory)
        {
            return;
        }
        let Some(deadline) = self.initial_observation_deadline else {
            return;
        };
        let deadline = deadline.min(Instant::now() + Duration::from_secs(3));
        let value = self.visual.claude_uia_inventory_until(deadline);
        let mut nonce = [0; 8];
        if getrandom::fill(&mut nonce).is_err() {
            return;
        }
        if let Ok(bytes) = serde_json::to_vec(&value)
            && let Ok(mut file) = nan_harness_private_fs::open_private_new(
                &directory.join(format!("claude-uia-{}.json", u64::from_le_bytes(nonce))),
            )
        {
            let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
        }
    }
}
