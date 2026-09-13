use super::{
    ComposerErrorCategory, ComposerFailure, ComposerGuardContext, ComposerOperation, GuiFailure,
};
use super::{app_names, map_error};
#[cfg(any(test, windows))]
use crate::native::{FitFailureStage, FitWindowError};
use crate::process::Observation;
use crate::{
    native::{
        DisplayRelation, FailureCategory, ForegroundRelation, GuardFailure, Native, Page, Window,
    },
    report::{GuiStage, Reason},
};
use nan_harness_core::DesktopHarnessKind;
use num_traits::ToPrimitive as _;
use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};
use xa11y::{Point, Rect};

pub(super) type AcquisitionFailure = (
    Reason,
    crate::diagnostics::GuiAcquisitionStage,
    ComposerErrorCategory,
    Option<crate::native::FitForegroundRelation>,
);

fn acquisition_failure(
    reason: Reason,
    stage: crate::diagnostics::GuiAcquisitionStage,
) -> AcquisitionFailure {
    (reason, stage, super::error_category(reason), None)
}

fn timeout_stage(
    inventory_count: usize,
    named_count: usize,
    eligible_count: usize,
) -> crate::diagnostics::GuiAcquisitionStage {
    if eligible_count > 0 {
        crate::diagnostics::GuiAcquisitionStage::WindowStability
    } else if named_count > 0 {
        crate::diagnostics::GuiAcquisitionStage::WindowCandidatesTooSmall
    } else if inventory_count == 0 {
        crate::diagnostics::GuiAcquisitionStage::WindowInventoryEmpty
    } else {
        crate::diagnostics::GuiAcquisitionStage::WindowCandidatesEmpty
    }
}

fn candidate_counts(kind: DesktopHarnessKind, windows: &[Window]) -> (usize, usize) {
    let named_count = windows
        .iter()
        .filter(|window| matches_app(kind, &window.name))
        .count();
    let eligible_count = eligible_windows(kind, windows).count();
    (named_count, eligible_count)
}

#[derive(Default)]
struct CandidateInventory {
    total: usize,
    named: usize,
    eligible: usize,
    owned_name_mismatch: bool,
}

impl CandidateInventory {
    fn observe(&mut self, kind: DesktopHarnessKind, windows: &[Window], owner: u32) {
        let (named, eligible) = candidate_counts(kind, windows);
        self.total += windows.len();
        self.named += named;
        self.eligible += eligible;
        // Read-only Unix group evidence diagnoses an unrecognized owner name.
        // It never makes that window eligible for input or relaxes ownership.
        self.owned_name_mismatch |= cfg!(unix)
            && windows.iter().any(|window| {
                !matches_app(kind, &window.name)
                    && super::process_ownership(window.pid, owner).is_ok()
            });
    }

    fn stage(&self) -> crate::diagnostics::GuiAcquisitionStage {
        if self.named == 0 && self.owned_name_mismatch {
            crate::diagnostics::GuiAcquisitionStage::WindowOwnerNameMismatch
        } else {
            timeout_stage(self.total, self.named, self.eligible)
        }
    }
}

fn eligible_windows(kind: DesktopHarnessKind, windows: &[Window]) -> impl Iterator<Item = &Window> {
    windows.iter().filter(move |window| {
        matches_app(kind, &window.name) && window.bounds.width >= 300 && window.bounds.height >= 200
    })
}

pub(super) struct Visual {
    native: Native,
    window: RefCell<Window>,
    scale: Cell<Option<f32>>,
}

impl Visual {
    pub(super) fn ensure_absent(kind: DesktopHarnessKind) -> Result<(), Reason> {
        let native = Native::new()?;
        confirm_absence(
            || {
                let windows = native.windows_for_absence()?;
                if windows.iter().any(|window| matches_app(kind, &window.name)) {
                    return Err(Reason::AlreadyRunning);
                }
                Ok(())
            },
            Instant::now() + Duration::from_secs(5),
        )
    }

    // Keep acquisition as one bounded state machine so process-liveness,
    // candidate ownership, and stability transitions cannot be reordered.
    pub(super) fn wait<P: Observation>(
        kind: DesktopHarnessKind,
        process: &mut P,
    ) -> Result<Self, AcquisitionFailure> {
        require_running(process).map_err(|reason| {
            acquisition_failure(reason, crate::diagnostics::GuiAcquisitionStage::ProcessLive)
        })?;
        let owner = process.id().ok_or(acquisition_failure(
            Reason::ApplicationExited,
            crate::diagnostics::GuiAcquisitionStage::ProcessLive,
        ))?;
        let native = Native::new().map_err(|reason| {
            acquisition_failure(
                reason,
                crate::diagnostics::GuiAcquisitionStage::NativeHelper,
            )
        })?;
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut previous = None;
        // Keep candidate discovery separate from geometry filtering. A named
        // window that is too small is actionable geometry evidence, while an
        // empty named set means the app has not exposed a usable window yet.
        let mut inventory = CandidateInventory::default();
        #[cfg(windows)]
        let mut fitted = false;
        loop {
            require_running(process).map_err(|reason| {
                acquisition_failure(reason, crate::diagnostics::GuiAcquisitionStage::ProcessLive)
            })?;
            // ensure_absent succeeded before launch. On X11, a foreground
            // query can still hit the previous probe's stale active window
            // before this app owns a window; retry it until the deadline.
            let snapshot = match native.windows() {
                Err(Reason::ActionUnsupported) if previous.is_none() => {
                    if Instant::now() >= deadline {
                        return Err((
                            Reason::DesktopUnavailable,
                            crate::diagnostics::GuiAcquisitionStage::NativeHelper,
                            ComposerErrorCategory::Other,
                            None,
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                snapshot => snapshot.map_err(|reason| {
                    acquisition_failure(
                        reason,
                        crate::diagnostics::GuiAcquisitionStage::NativeHelper,
                    )
                })?,
            };
            inventory.observe(kind, &snapshot.windows, owner);
            let windows = eligible_windows(kind, &snapshot.windows).collect::<Vec<_>>();
            if windows.len() > 1 {
                return Err((
                    Reason::InstallationAmbiguous,
                    crate::diagnostics::GuiAcquisitionStage::WindowCandidates,
                    ComposerErrorCategory::Other,
                    None,
                ));
            }
            if let Some(window) = windows.first() {
                if let Err(failure) = super::process_ownership(window.pid, owner) {
                    return Err((
                        Reason::IsolationUnavailable,
                        crate::diagnostics::GuiAcquisitionStage::WindowOwnership,
                        failure.category(),
                        None,
                    ));
                }
                #[cfg(windows)]
                if !fitted {
                    fit_owned_window(&native, window)?;
                    fitted = true;
                    continue;
                }
                #[cfg(windows)]
                if fitted && !snapshot.contains_display(window) {
                    return Err((
                        Reason::ActionUnsupported,
                        crate::diagnostics::GuiAcquisitionStage::WindowStability,
                        ComposerErrorCategory::NativeHelperFitPostconditionGeometry,
                        None,
                    ));
                }
                if previous.as_ref() == Some(*window) {
                    #[cfg(target_os = "macos")]
                    initial_readiness(&native, &snapshot, window, owner)?;
                    return Ok(Self {
                        window: RefCell::new((*window).clone()),
                        native,
                        scale: Cell::new(None),
                    });
                }
                previous = Some((*window).clone());
            }
            if Instant::now() >= deadline {
                return Err((
                    Reason::DesktopUnavailable,
                    inventory.stage(),
                    ComposerErrorCategory::Other,
                    None,
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub(super) fn pid(&self) -> u32 {
        self.window.borrow().pid
    }

    pub(super) fn guard(&self) -> Result<(), Reason> {
        let snapshot = self.native.windows()?;
        let expected = self.window.borrow().clone();
        let verdict = snapshot.guard_failure(&expected);
        let reason = verdict.map_err(GuardFailure::reason);
        if reason == Err(Reason::WindowOccluded) {
            // Transient wave10 diagnostic: record closed occluder classification
            // at the exact rejected snapshot. Never changes the guard verdict
            // and never emits process names, titles, or raw inventory.
            if let Some(diagnostic) = snapshot.occluders(&expected) {
                crate::occlusion::emit(&diagnostic);
            }
        }
        reason
    }

    pub(super) fn guard_composer(&self) -> Result<(), (Reason, ComposerErrorCategory)> {
        let snapshot = self
            .native
            .windows_with_category()
            .map_err(|category| (category.reason(), native_error_category(category)))?;
        let expected = self.window.borrow().clone();
        let verdict = snapshot.guard_failure(&expected);
        if verdict == Err(GuardFailure::Occluded)
            && let Some(diagnostic) = snapshot.occluders(&expected)
        {
            crate::occlusion::emit(&diagnostic);
        }
        verdict.map_err(|failure| {
            let category = match failure {
                GuardFailure::ForegroundChanged => snapshot.foreground_relation(&expected).map_or(
                    ComposerErrorCategory::ForegroundChanged,
                    foreground_category,
                ),
                _ => guard_error_category(failure),
            };
            (failure.reason(), category)
        })
    }

    pub(super) fn reacquire_owned_window(
        &self,
    ) -> Result<
        (),
        (
            Reason,
            ComposerErrorCategory,
            Option<crate::diagnostics::DisplayGeometryRelation>,
        ),
    > {
        let expected = self.window.borrow().clone();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut previous = None;
        loop {
            let snapshot = self
                .native
                .windows_with_category()
                .map_err(|category| (category.reason(), native_error_category(category), None))?;
            let Some(current) = snapshot
                .windows
                .iter()
                .find(|window| window.id == expected.id && window.pid == expected.pid)
                .cloned()
            else {
                return Err((
                    Reason::WindowChanged,
                    ComposerErrorCategory::WindowIdentityMissing,
                    None,
                ));
            };
            if let Err(failure) = snapshot.guard_failure(&current) {
                let category = match failure {
                    GuardFailure::ForegroundChanged => {
                        snapshot.foreground_relation(&current).map_or(
                            ComposerErrorCategory::ForegroundChanged,
                            foreground_category,
                        )
                    }
                    _ => guard_error_category(failure),
                };
                let geometry_relation = (failure == GuardFailure::OffDisplay)
                    .then(|| snapshot.off_display_relation(&current))
                    .flatten()
                    .map(display_geometry_relation);
                return Err((failure.reason(), category, geometry_relation));
            }
            if previous.as_ref() == Some(&current) {
                *self.window.borrow_mut() = current;
                return Ok(());
            }
            previous = Some(current);
            if Instant::now() >= deadline {
                return Err((
                    Reason::WindowChanged,
                    ComposerErrorCategory::WindowBoundsChanged,
                    None,
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn screenshot(&self) -> Result<xa11y::Screenshot, Reason> {
        self.guard()?;
        let bounds = self.capture_bounds();
        let screenshot = xa11y::screenshot_region(bounds).map_err(map_error)?;
        self.guard()?;
        if !screenshot.scale.is_finite()
            || screenshot.scale <= 0.0
            || self
                .scale
                .get()
                .is_some_and(|previous| previous.to_bits() != screenshot.scale.to_bits())
            || (f64::from(bounds.width) * f64::from(screenshot.scale) - f64::from(screenshot.width))
                .abs()
                > 1.0
            || (f64::from(bounds.height) * f64::from(screenshot.scale)
                - f64::from(screenshot.height))
            .abs()
                > 1.0
        {
            return Err(Reason::WindowChanged);
        }
        self.scale.set(Some(screenshot.scale));
        Ok(screenshot)
    }

    fn page(&self, interpolate: bool) -> Result<(Page, f32), Reason> {
        let screenshot = self.screenshot()?;
        let screenshot = if interpolate {
            crate::native::prepare_ocr_image(screenshot)?
        } else {
            screenshot
        };
        Ok((self.native.recognize(&screenshot)?, screenshot.scale))
    }

    fn find<T>(&self, find: impl FnMut(&Page) -> Option<T>) -> Result<Option<(T, f32)>, Reason> {
        find_in_pages(|interpolate| self.page(interpolate), find)
    }

    fn capture_bounds(&self) -> Rect {
        // Keep rounded corners and transparent decoration outside the captured pixels.
        Rect {
            x: self.window.borrow().bounds.x + 8,
            y: self.window.borrow().bounds.y + 8,
            width: self.window.borrow().bounds.width - 16,
            height: self.window.borrow().bounds.height - 16,
        }
    }

    pub(super) fn find_phrase(
        &self,
        phrase: &str,
        deadline: Instant,
    ) -> Result<Option<(Rect, f32)>, Reason> {
        wait_for_phrase(|| self.find(|page| page.find_phrase(phrase)), deadline)
    }

    pub(super) fn wait_phrase_absent(&self, phrase: &str, deadline: Instant) -> Result<(), Reason> {
        // Duplicate matches forbid a click, but still prove the phrase is present.
        wait_absent(
            || self.find(|page| page.contains_phrase(phrase).then_some(())),
            deadline,
        )
    }

    pub(super) fn wait_modal_evidence(&self, deadline: Instant) -> Result<bool, Reason> {
        wait_for_phrase(|| self.find(modal_confirm_anchor), deadline).map(|anchor| anchor.is_some())
    }

    pub(super) fn wait_modal_absent(&self, deadline: Instant) -> Result<(), Reason> {
        wait_absent(|| self.find(modal_confirm_presence), deadline)
    }

    pub(super) fn press_confirm(&self) -> Result<(), Reason> {
        self.guard()?;
        let input = xa11y::input_sim().map_err(map_error)?;
        self.guard()?;
        input
            .keyboard()
            .press(xa11y::Key::Enter)
            .map_err(map_error)?;
        self.guard()
    }

    pub(super) fn click(&self, bounds: Rect, scale: f32) -> Result<(), Reason> {
        let point = point_in_window(self.capture_bounds(), bounds, scale)?;
        self.guard()?;
        xa11y::input_sim()
            .map_err(map_error)?
            .mouse()
            .click(point)
            .map_err(map_error)?;
        self.guard()
    }

    pub(super) fn submit(&self, kind: DesktopHarnessKind, prompt: &str) -> Result<(), GuiFailure> {
        let input_stage = |operation, reason| GuiFailure {
            stage: GuiStage::ComposerInput,
            reason,
            composer: Some(ComposerFailure {
                operation,
                error_category: visual_error_category(reason),
                guard_context: None,
                geometry_relation: None,
            }),
        };
        let send_stage = |operation, reason| GuiFailure {
            stage: GuiStage::ComposerSend,
            reason,
            composer: Some(ComposerFailure {
                operation,
                error_category: visual_error_category(reason),
                guard_context: None,
                geometry_relation: None,
            }),
        };
        let (bounds, scale) = self.locate_composer(kind)?;
        self.click(bounds, scale)
            .map_err(|reason| input_stage(ComposerOperation::VisualClick, reason))?;
        let input = xa11y::input_sim()
            .map_err(map_error)
            .map_err(|reason| input_stage(ComposerOperation::InputSim, reason))?;
        self.guard_composer()
            .map_err(|(reason, error_category)| GuiFailure {
                stage: GuiStage::ComposerInput,
                reason,
                composer: Some(ComposerFailure {
                    operation: ComposerOperation::Guard,
                    error_category,
                    guard_context: Some(ComposerGuardContext::BeforeSelectAll),
                    geometry_relation: None,
                }),
            })?;
        input
            .keyboard()
            .chord(xa11y::Key::Char('a'), &[super::primary_modifier()])
            .map_err(map_error)
            .map_err(|reason| input_stage(ComposerOperation::SelectAll, reason))?;
        self.guard_composer()
            .map_err(|(reason, error_category)| GuiFailure {
                stage: GuiStage::ComposerInput,
                reason,
                composer: Some(ComposerFailure {
                    operation: ComposerOperation::Guard,
                    error_category,
                    guard_context: Some(ComposerGuardContext::BeforeType),
                    geometry_relation: None,
                }),
            })?;
        input
            .keyboard()
            .type_text(prompt)
            .map_err(map_error)
            .map_err(|reason| input_stage(ComposerOperation::TypeText, reason))?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self
                .find(|page| page.find_phrase(prompt))
                .map_err(|reason| input_stage(ComposerOperation::VerifyInputVisual, reason))?
                .is_some()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(input_stage(
                    ComposerOperation::VerifyInputVisual,
                    Reason::InputMismatch,
                ));
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        self.guard_composer()
            .map_err(|(reason, error_category)| GuiFailure {
                stage: GuiStage::ComposerSend,
                reason,
                composer: Some(ComposerFailure {
                    operation: ComposerOperation::Guard,
                    error_category,
                    guard_context: Some(ComposerGuardContext::BeforeSend),
                    geometry_relation: None,
                }),
            })?;
        input
            .keyboard()
            .press(xa11y::Key::Enter)
            .map_err(map_error)
            .map_err(|reason| send_stage(ComposerOperation::Send, reason))
    }

    fn locate_composer(&self, kind: DesktopHarnessKind) -> Result<(Rect, f32), GuiFailure> {
        let mut category = ComposerErrorCategory::MissingComposerAnchor;
        let failure = |reason, error_category| GuiFailure {
            stage: GuiStage::ComposerInput,
            reason,
            composer: Some(ComposerFailure {
                operation: ComposerOperation::LocateVisual,
                error_category,
                guard_context: None,
                geometry_relation: None,
            }),
        };
        self.find(|page| match response_input_bounds(kind, page) {
            Ok(bounds) => Some(bounds),
            Err(observed) => {
                category = observed;
                None
            }
        })
        .map_err(|reason| failure(reason, visual_error_category(reason)))?
        .ok_or_else(|| failure(Reason::SelectorNotMatched, category))
    }

    pub(super) fn contains_response(
        &self,
        kind: DesktopHarnessKind,
        marker: &str,
    ) -> Result<bool, (Reason, ComposerErrorCategory)> {
        let mut visual_failure = None;
        let mut anchor_seen = false;
        // Only the transcript above the current empty composer can certify a response.
        // A marker in the editable prompt never counts. Response text and composer
        // placeholders need not share an indentation.
        for interpolate in [false, true] {
            let (page, _) = self
                .page(interpolate)
                .map_err(|reason| (reason, visual_error_category(reason)))?;
            match response_on_page(kind, &page, marker) {
                Ok(true) => return Ok(true),
                Ok(false) => anchor_seen = true,
                Err(category) => visual_failure = Some(category),
            }
        }
        if anchor_seen {
            return Ok(false);
        }
        let category = visual_failure.unwrap_or(ComposerErrorCategory::MissingComposerAnchor);
        Err((Reason::SelectorNotMatched, category))
    }
}

#[cfg(windows)]
fn fit_owned_window(native: &Native, window: &Window) -> Result<(), AcquisitionFailure> {
    native.fit_owned_window(window).map_err(|failure| {
        let foreground_relation = match failure {
            FitWindowError::Diagnostic(ref diagnostic) => diagnostic.foreground_relation,
            FitWindowError::Transport(_) => None,
        };
        (
            Reason::ActionUnsupported,
            crate::diagnostics::GuiAcquisitionStage::WindowStability,
            fit_error_category(failure),
            foreground_relation,
        )
    })
}

fn response_on_page(
    kind: DesktopHarnessKind,
    page: &Page,
    marker: &str,
) -> Result<bool, ComposerErrorCategory> {
    let input = response_input_bounds(kind, page).map_err(|category| {
        // A marker without a unique empty composer is diagnostic evidence only:
        // it may still be in the editable input and must never certify a response.
        if category == ComposerErrorCategory::MissingComposerAnchor && page.contains_phrase(marker)
        {
            ComposerErrorCategory::MarkerWithoutComposerAnchor
        } else {
            category
        }
    })?;
    Ok(page.contains_marker_above(marker, input.y))
}

fn visual_error_category(reason: Reason) -> ComposerErrorCategory {
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

fn native_error_category(category: FailureCategory) -> ComposerErrorCategory {
    match category {
        FailureCategory::Spawn => ComposerErrorCategory::NativeHelperSpawn,
        FailureCategory::Pipe => ComposerErrorCategory::NativeHelperPipe,
        FailureCategory::Timeout => ComposerErrorCategory::NativeHelperTimeout,
        FailureCategory::NonzeroExit => ComposerErrorCategory::NativeHelperNonzeroExit,
        FailureCategory::WindowChanged => ComposerErrorCategory::NativeHelperWindowChanged,
        FailureCategory::WindowQueryRejected => ComposerErrorCategory::NativeHelperQueryRejected,
        FailureCategory::SessionUnavailable => {
            ComposerErrorCategory::NativeHelperSessionUnavailable
        }
        FailureCategory::Output => ComposerErrorCategory::NativeHelperOutput,
        FailureCategory::InvalidInput => ComposerErrorCategory::Other,
    }
}

#[cfg(any(test, windows))]
fn fit_error_category(failure: FitWindowError) -> ComposerErrorCategory {
    match failure {
        FitWindowError::Transport(category) => native_error_category(category),
        FitWindowError::Diagnostic(failure) => match failure.stage {
            FitFailureStage::Request => ComposerErrorCategory::NativeHelperFitRequest,
            FitFailureStage::IdentityRead => ComposerErrorCategory::NativeHelperFitIdentityRead,
            FitFailureStage::IdentityMismatch => {
                ComposerErrorCategory::NativeHelperFitIdentityMismatch
            }
            FitFailureStage::ForegroundRead => ComposerErrorCategory::NativeHelperFitForegroundRead,
            FitFailureStage::ForegroundMismatch => {
                ComposerErrorCategory::NativeHelperFitForegroundMismatch
            }
            FitFailureStage::MonitorRead => ComposerErrorCategory::NativeHelperFitMonitorRead,
            FitFailureStage::WorkareaRead => ComposerErrorCategory::NativeHelperFitWorkareaRead,
            FitFailureStage::WindowRead => ComposerErrorCategory::NativeHelperFitWindowRead,
            FitFailureStage::WorkareaInvalid => {
                ComposerErrorCategory::NativeHelperFitWorkareaInvalid
            }
            FitFailureStage::IdentityChanged => {
                ComposerErrorCategory::NativeHelperFitIdentityChanged
            }
            FitFailureStage::ForegroundChanged => {
                ComposerErrorCategory::NativeHelperFitForegroundChanged
            }
            FitFailureStage::Resize => ComposerErrorCategory::NativeHelperFitResize,
            FitFailureStage::PostconditionIdentityRead => {
                ComposerErrorCategory::NativeHelperFitPostconditionIdentityRead
            }
            FitFailureStage::PostconditionIdentityMismatch => {
                ComposerErrorCategory::NativeHelperFitPostconditionIdentityMismatch
            }
            FitFailureStage::PostconditionForegroundRead => {
                ComposerErrorCategory::NativeHelperFitPostconditionForegroundRead
            }
            FitFailureStage::PostconditionForegroundMismatch => {
                ComposerErrorCategory::NativeHelperFitPostconditionForegroundMismatch
            }
            FitFailureStage::PostconditionWindowRead => {
                ComposerErrorCategory::NativeHelperFitPostconditionWindowRead
            }
            FitFailureStage::PostconditionGeometry => {
                ComposerErrorCategory::NativeHelperFitPostconditionGeometry
            }
        },
    }
}

fn guard_error_category(failure: GuardFailure) -> ComposerErrorCategory {
    match failure {
        GuardFailure::IdentityMissing => ComposerErrorCategory::WindowIdentityMissing,
        GuardFailure::BoundsChanged => ComposerErrorCategory::WindowBoundsChanged,
        GuardFailure::ForegroundChanged => ComposerErrorCategory::ForegroundChanged,
        GuardFailure::SameProcessWindow => ComposerErrorCategory::SameProcessWindow,
        GuardFailure::OffDisplay => ComposerErrorCategory::WindowOffDisplay,
        GuardFailure::Occluded => ComposerErrorCategory::WindowOccluded,
    }
}

fn foreground_category(relation: ForegroundRelation) -> ComposerErrorCategory {
    match relation {
        ForegroundRelation::IdentityUnavailable => {
            ComposerErrorCategory::ForegroundIdentityUnavailable
        }
        ForegroundRelation::DifferentProcess => ComposerErrorCategory::ForegroundProcessDifferent,
        ForegroundRelation::SameProcessDifferentWindow => {
            ComposerErrorCategory::ForegroundWindowDifferent
        }
    }
}

fn display_geometry_relation(
    relation: DisplayRelation,
) -> crate::diagnostics::DisplayGeometryRelation {
    match relation {
        DisplayRelation::PartialMonitorOverlap => {
            crate::diagnostics::DisplayGeometryRelation::PartialMonitorOverlap
        }
        DisplayRelation::NoMonitorOverlap => {
            crate::diagnostics::DisplayGeometryRelation::NoMonitorOverlap
        }
    }
}

fn confirm_absence(
    mut inventory: impl FnMut() -> Result<(), Reason>,
    deadline: Instant,
) -> Result<(), Reason> {
    loop {
        match inventory() {
            // Window-server teardown can race enumeration after the app exits.
            // Only a later successful inventory certifies absence. Keep input
            // and capture guards immediate, and never retry an observed app.
            Err(Reason::ActionUnsupported) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            outcome => return outcome,
        }
    }
}

fn find_in_pages<T>(
    mut capture: impl FnMut(bool) -> Result<(Page, f32), Reason>,
    mut find: impl FnMut(&Page) -> Option<T>,
) -> Result<Option<(T, f32)>, Reason> {
    // Interpolation can recover small glyphs but can also change OCR of text
    // already readable at native density. Preserve that reading as the first try.
    for interpolate in [false, true] {
        let (page, scale) = capture(interpolate)?;
        if let Some(found) = find(&page) {
            return Ok(Some((found, scale)));
        }
    }
    Ok(None)
}

fn wait_for_phrase<T>(
    mut find: impl FnMut() -> Result<Option<T>, Reason>,
    deadline: Instant,
) -> Result<Option<T>, Reason> {
    loop {
        if let Some(found) = find()? {
            return Ok(Some(found));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_absent<T>(
    mut find: impl FnMut() -> Result<Option<T>, Reason>,
    deadline: Instant,
) -> Result<(), Reason> {
    loop {
        if find()?.is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn require_running<P: Observation>(process: &mut P) -> Result<(), Reason> {
    match process.try_wait() {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err(Reason::ApplicationExited),
        Err(_) => Err(Reason::IsolationUnavailable),
    }
}

#[cfg(target_os = "macos")]
fn initial_readiness(
    native: &Native,
    snapshot: &crate::native::Snapshot,
    window: &Window,
    owner: u32,
) -> Result<(), AcquisitionFailure> {
    if let Err(failure) = snapshot.non_foreground_failure(window) {
        return Err((
            failure.reason(),
            crate::diagnostics::GuiAcquisitionStage::WindowStability,
            guard_error_category(failure),
            None,
        ));
    }
    if snapshot.guard_failure(window) != Err(GuardFailure::ForegroundChanged) {
        return Ok(());
    }
    let activation_deadline = Instant::now() + Duration::from_secs(2);
    activate_and_wait(
        || {
            super::process_ownership(window.pid, owner)
                .map_err(|failure| (Reason::IsolationUnavailable, failure.category()))
        },
        || {
            native
                .activate_owned_window(window)
                .map_err(|failure| (failure.reason(), native_error_category(failure)))
        },
        || {
            native
                .windows_with_category()
                .map_err(|category| (category.reason(), native_error_category(category)))
                .and_then(|fresh| {
                    fresh
                        .guard_failure(window)
                        .map_err(|failure| (failure.reason(), guard_error_category(failure)))
                })
        },
        activation_deadline,
    )
    .map_err(|(reason, category)| {
        (
            reason,
            crate::diagnostics::GuiAcquisitionStage::WindowStability,
            category,
            None,
        )
    })
}

#[cfg(any(test, target_os = "macos"))]
fn activate_and_wait<Own, Activate, Guard>(
    ownership: Own,
    activate: Activate,
    mut guard: Guard,
    deadline: Instant,
) -> Result<(), (Reason, ComposerErrorCategory)>
where
    Own: FnOnce() -> Result<(), (Reason, ComposerErrorCategory)>,
    Activate: FnOnce() -> Result<(), (Reason, ComposerErrorCategory)>,
    Guard: FnMut() -> Result<(), (Reason, ComposerErrorCategory)>,
{
    // The ownership closure is intentionally evaluated before the activation
    // closure. Activation is issued at most once; only a ForegroundChanged
    // postcondition is retryable during this initial two-second wait.
    ownership()?;
    activate()?;
    loop {
        match guard() {
            Ok(()) => return Ok(()),
            Err((Reason::FocusChanged, ComposerErrorCategory::ForegroundChanged))
                if Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
fn input_bounds(kind: DesktopHarnessKind, page: &Page) -> Option<Rect> {
    response_input_bounds(kind, page).ok()
}

fn response_input_bounds(
    kind: DesktopHarnessKind,
    page: &Page,
) -> Result<Rect, ComposerErrorCategory> {
    if page.words.is_empty() {
        return Err(ComposerErrorCategory::EmptyOcrPage);
    }
    let labels = match kind {
        // The blinking insertion caret overlaps the first placeholder glyph.
        // Match the unchanged words after it, without relaxing OCR confidence.
        DesktopHarnessKind::Zed => &["the Zed Agent,", "the Zed Agent"][..],
        DesktopHarnessKind::ChatGpt => {
            &["Ask for follow-up changes", "Ask anything", "Message"][..]
        }
        DesktopHarnessKind::Claude => &[
            "Reply to Claude",
            "How can I help you today?",
            "Message Claude",
        ][..],
        DesktopHarnessKind::Hermes => &["Message Hermes", "Type a message"][..],
        DesktopHarnessKind::Pen => &["Ask Pen", "Describe what you want to build"][..],
    };
    let mut matched_label = None;
    let mut match_count = 0;
    for label in labels {
        let count = phrase_match_count(page, label);
        match_count += count;
        if count == 1 {
            matched_label = Some(label);
        }
    }
    if match_count != 1 {
        return Err(if match_count == 0 {
            ComposerErrorCategory::MissingComposerAnchor
        } else {
            ComposerErrorCategory::AmbiguousComposerAnchor
        });
    }
    let matched_label = matched_label.ok_or(ComposerErrorCategory::MissingComposerAnchor)?;
    page.find_phrase(matched_label)
        .ok_or(ComposerErrorCategory::MissingComposerAnchor)
}

fn phrase_match_count(page: &Page, phrase: &str) -> usize {
    let expected = phrase.split_whitespace().collect::<Vec<_>>();
    if expected.is_empty() {
        return 0;
    }
    page.words
        .windows(expected.len())
        .filter(|words| {
            words
                .iter()
                .zip(&expected)
                .all(|(word, expected)| word.text == *expected)
        })
        .count()
}

fn modal_confirm_anchor(page: &Page) -> Option<()> {
    // Zed 1.18.1 renders these exact labels from the source-backed modal
    // view. Each find must be unique; merely "containing" either phrase is
    // not a safe target for the source-visible Enter confirmation.
    (page.find_phrase("Unrecognized Project").is_some()
        && page.find_phrase("Restricted Mode prevents:").is_some())
    .then_some(())
}

fn modal_confirm_presence(page: &Page) -> Option<()> {
    // A failed confirm can leave one or both anchors duplicated. Duplicates are
    // not unique targets, but they are still modal presence and must not be
    // mistaken for disappearance.
    (page.contains_phrase("Unrecognized Project")
        || page.contains_phrase("Restricted Mode prevents:"))
    .then_some(())
}

fn matches_app(kind: DesktopHarnessKind, name: &str) -> bool {
    let name = name.strip_suffix(".exe").unwrap_or(name);
    app_names(kind)
        .iter()
        .any(|expected| expected.eq_ignore_ascii_case(name))
}

fn point_in_window(window: Rect, pixels: Rect, scale: f32) -> Result<Point, Reason> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(Reason::IsolationUnavailable);
    }
    let x = ((f64::from(pixels.x) + f64::from(pixels.width) / 2.0) / f64::from(scale))
        .round()
        .to_i32()
        .ok_or(Reason::IsolationUnavailable)?;
    let y = ((f64::from(pixels.y) + f64::from(pixels.height) / 2.0) / f64::from(scale))
        .round()
        .to_i32()
        .ok_or(Reason::IsolationUnavailable)?;
    if x < 0
        || y < 0
        || u32::try_from(x).ok().is_none_or(|x| x >= window.width)
        || u32::try_from(y).ok().is_none_or(|y| y >= window.height)
    {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(Point {
        x: window
            .x
            .checked_add(x)
            .ok_or(Reason::IsolationUnavailable)?,
        y: window
            .y
            .checked_add(y)
            .ok_or(Reason::IsolationUnavailable)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::FitFailure;

    #[test]
    fn initial_activation_requires_ownership_and_issues_one_activation() {
        let events = RefCell::new(Vec::new());
        let activations = Cell::new(0);
        let result = activate_and_wait(
            || {
                events.borrow_mut().push("ownership");
                Ok(())
            },
            || {
                events.borrow_mut().push("activation");
                activations.set(activations.get() + 1);
                Ok(())
            },
            || {
                events.borrow_mut().push("guard");
                Ok(())
            },
            Instant::now(),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(activations.get(), 1);
        assert_eq!(*events.borrow(), ["ownership", "activation", "guard"]);
    }

    #[test]
    fn initial_activation_retries_only_foreground_change_after_one_activation() {
        let remaining_foreground_changes = Cell::new(1);
        let activations = Cell::new(0);
        let result = activate_and_wait(
            || Ok(()),
            || {
                activations.set(activations.get() + 1);
                Ok(())
            },
            || {
                if remaining_foreground_changes.get() == 1 {
                    remaining_foreground_changes.set(0);
                    Err((
                        Reason::FocusChanged,
                        ComposerErrorCategory::ForegroundChanged,
                    ))
                } else {
                    Ok(())
                }
            },
            Instant::now() + Duration::from_secs(2),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(activations.get(), 1);
    }

    #[test]
    fn initial_activation_denies_an_ownership_mismatch() {
        let activations = Cell::new(0);
        let result = activate_and_wait(
            || {
                Err((
                    Reason::IsolationUnavailable,
                    ComposerErrorCategory::OwnershipDifferentGroup,
                ))
            },
            || {
                activations.set(activations.get() + 1);
                Ok(())
            },
            || Ok(()),
            Instant::now(),
        );
        assert_eq!(
            result,
            Err((
                Reason::IsolationUnavailable,
                ComposerErrorCategory::OwnershipDifferentGroup
            ))
        );
        assert_eq!(activations.get(), 0);
    }

    #[test]
    fn initial_activation_propagates_non_foreground_postguard_failure() {
        let activations = Cell::new(0);
        let result = activate_and_wait(
            || Ok(()),
            || {
                activations.set(activations.get() + 1);
                Ok(())
            },
            || {
                Err((
                    Reason::WindowOccluded,
                    ComposerErrorCategory::WindowOccluded,
                ))
            },
            Instant::now() + Duration::from_secs(2),
        );
        assert_eq!(
            result,
            Err((
                Reason::WindowOccluded,
                ComposerErrorCategory::WindowOccluded
            ))
        );
        assert_eq!(activations.get(), 1);
    }

    #[test]
    fn acquisition_timeout_distinguishes_empty_candidates_from_geometry() {
        assert_eq!(
            timeout_stage(0, 0, 0),
            crate::diagnostics::GuiAcquisitionStage::WindowInventoryEmpty
        );
        assert_eq!(
            timeout_stage(1, 0, 0),
            crate::diagnostics::GuiAcquisitionStage::WindowCandidatesEmpty
        );
        assert_eq!(
            timeout_stage(1, 1, 0),
            crate::diagnostics::GuiAcquisitionStage::WindowCandidatesTooSmall
        );
        assert_eq!(
            timeout_stage(1, 1, 1),
            crate::diagnostics::GuiAcquisitionStage::WindowStability
        );
    }

    #[cfg(unix)]
    #[test]
    fn unrecognized_owned_windows_are_diagnosed_but_never_eligible() {
        let window = Window {
            pid: std::process::id(),
            id: 1,
            bounds: Rect {
                x: 0,
                y: 0,
                width: 400,
                height: 300,
            },
            name: "unexpected-owner-name".into(),
            layer: 0,
        };
        let mut inventory = CandidateInventory::default();
        inventory.observe(
            DesktopHarnessKind::Claude,
            std::slice::from_ref(&window),
            std::process::id(),
        );
        assert_eq!(
            inventory.stage(),
            crate::diagnostics::GuiAcquisitionStage::WindowOwnerNameMismatch
        );
        assert_eq!(
            eligible_windows(DesktopHarnessKind::Claude, std::slice::from_ref(&window)).count(),
            0
        );
        let mut unowned = window;
        unowned.pid = u32::MAX;
        let mut inventory = CandidateInventory::default();
        inventory.observe(DesktopHarnessKind::Claude, &[unowned], std::process::id());
        assert_eq!(
            inventory.stage(),
            crate::diagnostics::GuiAcquisitionStage::WindowCandidatesEmpty
        );
    }

    #[test]
    fn visual_input_verification_preserves_timeout_mismatch_and_helper_failures() {
        assert_eq!(
            visual_error_category(Reason::Timeout),
            ComposerErrorCategory::Timeout
        );
        assert_eq!(
            visual_error_category(Reason::InputMismatch),
            ComposerErrorCategory::InputMismatch
        );
        assert_eq!(
            visual_error_category(Reason::ActionUnsupported),
            ComposerErrorCategory::ActionUnsupported
        );
        assert_eq!(
            visual_error_category(Reason::PermissionRequired),
            ComposerErrorCategory::PermissionRequired
        );
    }

    #[test]
    fn fit_failures_map_exhaustively_to_closed_categories() {
        let stages = [
            (
                FitFailureStage::Request,
                ComposerErrorCategory::NativeHelperFitRequest,
            ),
            (
                FitFailureStage::IdentityRead,
                ComposerErrorCategory::NativeHelperFitIdentityRead,
            ),
            (
                FitFailureStage::IdentityMismatch,
                ComposerErrorCategory::NativeHelperFitIdentityMismatch,
            ),
            (
                FitFailureStage::ForegroundRead,
                ComposerErrorCategory::NativeHelperFitForegroundRead,
            ),
            (
                FitFailureStage::ForegroundMismatch,
                ComposerErrorCategory::NativeHelperFitForegroundMismatch,
            ),
            (
                FitFailureStage::MonitorRead,
                ComposerErrorCategory::NativeHelperFitMonitorRead,
            ),
            (
                FitFailureStage::WorkareaRead,
                ComposerErrorCategory::NativeHelperFitWorkareaRead,
            ),
            (
                FitFailureStage::WindowRead,
                ComposerErrorCategory::NativeHelperFitWindowRead,
            ),
            (
                FitFailureStage::WorkareaInvalid,
                ComposerErrorCategory::NativeHelperFitWorkareaInvalid,
            ),
            (
                FitFailureStage::IdentityChanged,
                ComposerErrorCategory::NativeHelperFitIdentityChanged,
            ),
            (
                FitFailureStage::ForegroundChanged,
                ComposerErrorCategory::NativeHelperFitForegroundChanged,
            ),
            (
                FitFailureStage::Resize,
                ComposerErrorCategory::NativeHelperFitResize,
            ),
            (
                FitFailureStage::PostconditionIdentityRead,
                ComposerErrorCategory::NativeHelperFitPostconditionIdentityRead,
            ),
            (
                FitFailureStage::PostconditionIdentityMismatch,
                ComposerErrorCategory::NativeHelperFitPostconditionIdentityMismatch,
            ),
            (
                FitFailureStage::PostconditionForegroundRead,
                ComposerErrorCategory::NativeHelperFitPostconditionForegroundRead,
            ),
            (
                FitFailureStage::PostconditionForegroundMismatch,
                ComposerErrorCategory::NativeHelperFitPostconditionForegroundMismatch,
            ),
            (
                FitFailureStage::PostconditionWindowRead,
                ComposerErrorCategory::NativeHelperFitPostconditionWindowRead,
            ),
            (
                FitFailureStage::PostconditionGeometry,
                ComposerErrorCategory::NativeHelperFitPostconditionGeometry,
            ),
        ];
        for (stage, category) in stages {
            assert_eq!(
                fit_error_category(FitWindowError::Diagnostic(FitFailure {
                    stage,
                    foreground_relation: None,
                })),
                category
            );
        }
        assert_eq!(
            fit_error_category(FitWindowError::Transport(FailureCategory::Timeout)),
            ComposerErrorCategory::NativeHelperTimeout
        );
    }

    #[test]
    fn candidate_counts_classify_synthetic_native_windows() {
        let windows = vec![Window {
            id: 1,
            pid: 7,
            bounds: Rect {
                x: 0,
                y: 0,
                width: 120,
                height: 80,
            },
            name: "claude".into(),
            layer: 0,
        }];
        assert_eq!(
            candidate_counts(DesktopHarnessKind::Claude, &windows),
            (1, 0)
        );
        let mut eligible = windows[0].clone();
        eligible.bounds.width = 300;
        eligible.bounds.height = 200;
        assert_eq!(
            candidate_counts(DesktopHarnessKind::Claude, &[eligible]),
            (1, 1)
        );
        assert_eq!(candidate_counts(DesktopHarnessKind::Zed, &windows), (0, 0));
    }
    use std::fmt::Write as _;

    #[test]
    fn phrase_absence_requires_a_fresh_success_and_reports_a_visible_timeout() {
        let rectangle = Rect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        assert_eq!(wait_absent(|| Ok(None::<Rect>), Instant::now()), Ok(()));
        let mut attempts = 0;
        assert_eq!(
            wait_absent(
                || {
                    attempts += 1;
                    Ok((attempts < 3).then_some(rectangle))
                },
                Instant::now() + Duration::from_secs(1)
            ),
            Ok(())
        );
        assert_eq!(attempts, 3);
        assert_eq!(
            wait_absent(|| Ok(Some(rectangle)), Instant::now()),
            Err(Reason::Timeout)
        );
        assert_eq!(
            wait_absent(
                || Err::<Option<Rect>, Reason>(Reason::FocusChanged),
                Instant::now()
            ),
            Err(Reason::FocusChanged)
        );
    }

    #[test]
    fn absence_requires_a_successful_inventory_and_never_retries_an_observed_app() {
        let mut attempts = 0;
        assert_eq!(
            confirm_absence(
                || {
                    attempts += 1;
                    if attempts == 1 {
                        Err(Reason::ActionUnsupported)
                    } else {
                        Ok(())
                    }
                },
                Instant::now() + Duration::from_secs(1)
            ),
            Ok(())
        );
        assert_eq!(attempts, 2);
        assert_eq!(
            confirm_absence(|| Err(Reason::ActionUnsupported), Instant::now()),
            Err(Reason::ActionUnsupported)
        );
        let mut attempts = 0;
        assert_eq!(
            confirm_absence(
                || {
                    attempts += 1;
                    Err(Reason::AlreadyRunning)
                },
                Instant::now() + Duration::from_secs(1)
            ),
            Err(Reason::AlreadyRunning)
        );
        assert_eq!(attempts, 1);
    }

    #[test]
    fn phrase_click_waits_for_readiness_without_repeating_uncertain_input() {
        let rectangle = Rect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        let mut attempts = 0;
        let found = wait_for_phrase(
            || {
                attempts += 1;
                if attempts == 3 {
                    Ok(Some((rectangle, 1.0)))
                } else {
                    Ok(None)
                }
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(attempts, 3);
        assert_eq!(found, Some((rectangle, 1.0)));
        let mut attempts = 0;
        let result: Option<(Rect, f32)> = wait_for_phrase(
            || {
                attempts += 1;
                Ok(None)
            },
            Instant::now(),
        )
        .unwrap();
        assert_eq!(attempts, 1);
        assert_eq!(result, None);
        assert_eq!(
            wait_for_phrase(
                || Err::<Option<(Rect, f32)>, Reason>(Reason::FocusChanged),
                Instant::now(),
            ),
            Err(Reason::FocusChanged)
        );
    }

    #[test]
    fn native_text_is_preferred_and_interpolation_only_recovers_missing_text() {
        let page = |text| {
            Page::parse(
                &format!("5\t1\t1\t1\t1\t1\t10\t10\t80\t10\t95\t{text}\n"),
                200,
                100,
            )
            .unwrap()
        };
        let mut attempts = Vec::new();
        let found = find_in_pages(
            |interpolate| {
                attempts.push(interpolate);
                Ok((page(if interpolate { "WRONG" } else { "TEST" }), 1.0))
            },
            |page| page.find_phrase("TEST"),
        )
        .unwrap();
        assert!(found.is_some());
        assert_eq!(attempts, [false]);
        let found = find_in_pages(
            |interpolate| {
                Ok((
                    page(if interpolate { "TEST" } else { "WRONG" }),
                    if interpolate { 2.0 } else { 1.0 },
                ))
            },
            |page| page.find_phrase("TEST"),
        )
        .unwrap();
        assert_eq!(found.unwrap().1.to_bits(), 2.0_f32.to_bits());
        assert!(
            find_in_pages(
                |_| Ok((page("WRONG"), 1.0)),
                |page| page.find_phrase("TEST")
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            find_in_pages(
                |_| Err(Reason::FocusChanged),
                |page| page.find_phrase("TEST")
            ),
            Err(Reason::FocusChanged)
        );
    }

    #[test]
    fn zed_window_names_include_the_supported_linux_alias() {
        for name in ["Zed", "zed", "zed-editor", "zeditor", "Zed.exe"] {
            assert!(matches_app(DesktopHarnessKind::Zed, name));
        }
        assert!(!matches_app(DesktopHarnessKind::Zed, "unrelated-editor"));
    }

    #[tokio::test]
    async fn an_exited_launcher_fails_before_desktop_observation() {
        let mut command = if cfg!(windows) {
            let mut command = tokio::process::Command::new("cmd.exe");
            command.args(["/C", "exit", "3"]);
            command
        } else {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.args(["-c", "exit 3"]);
            command
        };
        let mut process = command.spawn().unwrap();
        process.wait().await.unwrap();
        assert!(matches!(
            Visual::wait(DesktopHarnessKind::Zed, &mut process),
            Err((
                Reason::ApplicationExited,
                crate::diagnostics::GuiAcquisitionStage::ProcessLive,
                ComposerErrorCategory::Other,
                None
            ))
        ));
    }

    #[test]
    fn zed_composer_ignores_the_caret_but_requires_a_unique_exact_anchor() {
        let text = "5\t1\t1\t1\t1\t1\t10\t40\t80\t10\t0\thtessage\n\
                    5\t1\t1\t1\t1\t2\t100\t40\t30\t10\t96\tthe\n\
                    5\t1\t1\t1\t1\t3\t140\t40\t30\t10\t96\tZed\n\
                    5\t1\t1\t1\t1\t4\t180\t40\t60\t10\t96\tAgent,\n";
        let page = Page::parse(text, 300, 100).unwrap();
        assert_eq!(
            input_bounds(DesktopHarnessKind::Zed, &page),
            Some(Rect {
                x: 100,
                y: 40,
                width: 140,
                height: 10
            })
        );
        let duplicated = Page::parse(&text.repeat(2), 300, 100).unwrap();
        assert!(input_bounds(DesktopHarnessKind::Zed, &duplicated).is_none());
        let changed = Page::parse(&text.replace("Agent,", "Agent?"), 300, 100).unwrap();
        assert!(input_bounds(DesktopHarnessKind::Zed, &changed).is_none());
    }

    #[test]
    fn pen_response_lookup_reports_closed_anchor_diagnostics() {
        let word = |index: usize, x: u32, y: u32, text: &str| {
            format!("5\t1\t1\t1\t1\t{index}\t{x}\t{y}\t80\t10\t95\t{text}\n")
        };
        let empty = Page::parse("", 400, 200).unwrap();
        assert_eq!(
            response_on_page(DesktopHarnessKind::Pen, &empty, "marker"),
            Err(ComposerErrorCategory::EmptyOcrPage)
        );

        let missing = Page::parse(&word(1, 10, 20, "canvas"), 400, 200).unwrap();
        assert_eq!(
            response_on_page(DesktopHarnessKind::Pen, &missing, "marker"),
            Err(ComposerErrorCategory::MissingComposerAnchor)
        );

        let anchor = word(1, 10, 150, "Ask") + &word(2, 100, 150, "Pen");
        for y in [20, 150] {
            let unpositioned = Page::parse(&word(1, 10, y, "marker"), 400, 200).unwrap();
            assert_eq!(
                response_on_page(DesktopHarnessKind::Pen, &unpositioned, "marker"),
                Err(ComposerErrorCategory::MarkerWithoutComposerAnchor)
            );
        }
        let ambiguous = Page::parse(
            &(anchor.clone() + &word(3, 10, 170, "Ask") + &word(4, 100, 170, "Pen")),
            400,
            200,
        )
        .unwrap();
        assert_eq!(
            response_on_page(DesktopHarnessKind::Pen, &ambiguous, "marker"),
            Err(ComposerErrorCategory::AmbiguousComposerAnchor)
        );

        let response = word(1, 10, 30, "marker") + &anchor;
        let page = Page::parse(&response, 400, 200).unwrap();
        assert_eq!(
            response_on_page(DesktopHarnessKind::Pen, &page, "marker"),
            Ok(true)
        );

        let input_only = Page::parse(&(word(1, 10, 150, "marker") + &anchor), 400, 200).unwrap();
        assert_eq!(
            response_on_page(DesktopHarnessKind::Pen, &input_only, "marker"),
            Ok(false)
        );
    }

    #[test]
    fn zed_keyboard_confirm_requires_both_unique_source_visible_anchors() {
        let words = |modal: &[(&str, u32, u32)]| {
            modal
                .iter()
                .enumerate()
                .fold(String::new(), |mut rows, (index, (text, x, y))| {
                    writeln!(
                        rows,
                        "5\t1\t1\t1\t1\t{}\t{}\t{}\t80\t10\t95\t{text}",
                        index + 1,
                        x,
                        y
                    )
                    .unwrap();
                    rows
                })
        };
        let header = [("Unrecognized", 10, 10), ("Project", 110, 10)];
        let body = [
            ("Restricted", 10, 40),
            ("Mode", 100, 40),
            ("prevents:", 150, 40),
        ];
        let modal = words(&header).clone() + &words(&body);
        let page = Page::parse(&modal, 300, 100).unwrap();
        assert!(modal_confirm_anchor(&page).is_some());

        // A source label can appear once as telemetry or in a stale copy while
        // the rendered anchor is duplicated. Presence is never clickability.
        let no_modal = String::new();
        assert!(modal_confirm_anchor(&Page::parse(&no_modal, 300, 100).unwrap()).is_none());
        assert!(modal_confirm_presence(&Page::parse(&no_modal, 300, 100).unwrap()).is_none());
        let body_only = words(&body);
        assert!(modal_confirm_anchor(&Page::parse(&body_only, 300, 100).unwrap()).is_none());
        let duplicated_header = words(&header).clone() + &words(&header) + &words(&body);
        assert!(
            modal_confirm_anchor(&Page::parse(&duplicated_header, 300, 100).unwrap()).is_none()
        );
        assert!(
            modal_confirm_presence(&Page::parse(&duplicated_header, 300, 100).unwrap()).is_some()
        );
        let duplicated_body = words(&header).clone() + &words(&body) + &words(&body);
        assert!(modal_confirm_anchor(&Page::parse(&duplicated_body, 300, 100).unwrap()).is_none());
        assert!(
            modal_confirm_presence(&Page::parse(&duplicated_body, 300, 100).unwrap()).is_some()
        );
        let changed = modal.replace("Unrecognized", "Unrecognized!");
        assert!(modal_confirm_anchor(&Page::parse(&changed, 300, 100).unwrap()).is_none());
    }

    #[test]
    fn screenshot_points_follow_window_origin_and_dpi() {
        let window = Rect {
            x: -1000,
            y: 80,
            width: 800,
            height: 600,
        };
        let pixels = Rect {
            x: 200,
            y: 100,
            width: 80,
            height: 40,
        };
        assert_eq!(
            point_in_window(window, pixels, 2.0).unwrap(),
            Point::new(-880, 140)
        );
        assert!(point_in_window(window, pixels, f32::NAN).is_err());
        assert!(point_in_window(window, Rect { x: 1600, ..pixels }, 2.0).is_err());
    }
}
