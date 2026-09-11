use super::GuiFailure;
use super::{app_names, map_error, owned_process};
use crate::{
    native::{Native, Page, Window},
    report::{GuiStage, Reason},
};
use nan_harness_core::DesktopHarnessKind;
use num_traits::ToPrimitive as _;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};
use xa11y::{Point, Rect};

pub(super) struct Visual {
    native: Native,
    window: Window,
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

    pub(super) fn wait(
        kind: DesktopHarnessKind,
        process: &mut tokio::process::Child,
    ) -> Result<Self, Reason> {
        require_running(process)?;
        let owner = process.id().ok_or(Reason::ApplicationExited)?;
        let native = Native::new()?;
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut previous = None;
        #[cfg(windows)]
        let mut fitted = false;
        loop {
            require_running(process)?;
            // ensure_absent succeeded before launch. On X11, a foreground
            // query can still hit the previous probe's stale active window
            // before this app owns a window; retry it until the deadline.
            let snapshot = match native.windows() {
                Err(Reason::ActionUnsupported) if previous.is_none() => {
                    if Instant::now() >= deadline {
                        return Err(Reason::DesktopUnavailable);
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                snapshot => snapshot?,
            };
            let windows = snapshot
                .windows
                .iter()
                .filter(|window| {
                    matches_app(kind, &window.name)
                        && window.bounds.width >= 300
                        && window.bounds.height >= 200
                })
                .collect::<Vec<_>>();
            if windows.len() > 1 {
                return Err(Reason::InstallationAmbiguous);
            }
            if let Some(window) = windows.first() {
                if !owned_process(window.pid, owner) {
                    return Err(Reason::IsolationUnavailable);
                }
                #[cfg(windows)]
                if !fitted {
                    // Fresh hosted Windows sessions can have a smaller work area
                    // than the app's default size. Fit only the verified owner,
                    // then acquire stable geometry again before any input/capture.
                    native.fit_owned_window(window)?;
                    fitted = true;
                    continue;
                }
                if previous.as_ref() == Some(*window) {
                    return Ok(Self {
                        window: (*window).clone(),
                        native,
                        scale: Cell::new(None),
                    });
                }
                previous = Some((*window).clone());
            }
            if Instant::now() >= deadline {
                return Err(Reason::DesktopUnavailable);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub(super) const fn pid(&self) -> u32 {
        self.window.pid
    }

    pub(super) fn guard(&self) -> Result<(), Reason> {
        let snapshot = self.native.windows()?;
        let verdict = snapshot.require_clear(&self.window);
        if verdict == Err(Reason::WindowOccluded) {
            // Transient wave10 diagnostic: record closed occluder classification
            // at the exact rejected snapshot. Never changes the guard verdict
            // and never emits process names, titles, or raw inventory.
            if let Some(diagnostic) = snapshot.occluders(&self.window) {
                crate::occlusion::emit(&diagnostic);
            }
        }
        verdict
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
            x: self.window.bounds.x + 8,
            y: self.window.bounds.y + 8,
            width: self.window.bounds.width - 16,
            height: self.window.bounds.height - 16,
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
        let input_stage = |reason| GuiFailure {
            stage: GuiStage::ComposerInput,
            reason,
        };
        let send_stage = |reason| GuiFailure {
            stage: GuiStage::ComposerSend,
            reason,
        };
        let (bounds, scale) = self
            .find(|page| input_bounds(kind, page))
            .map_err(input_stage)?
            .ok_or(Reason::SelectorNotMatched)
            .map_err(input_stage)?;
        self.click(bounds, scale).map_err(input_stage)?;
        let input = xa11y::input_sim().map_err(map_error).map_err(input_stage)?;
        self.guard().map_err(input_stage)?;
        input
            .keyboard()
            .chord(xa11y::Key::Char('a'), &[super::primary_modifier()])
            .map_err(map_error)
            .map_err(input_stage)?;
        self.guard().map_err(input_stage)?;
        input
            .keyboard()
            .type_text(prompt)
            .map_err(map_error)
            .map_err(input_stage)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self
                .find(|page| page.find_phrase(prompt))
                .map_err(input_stage)?
                .is_some()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(input_stage(Reason::InputMismatch));
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        self.guard().map_err(send_stage)?;
        input
            .keyboard()
            .press(xa11y::Key::Enter)
            .map_err(map_error)
            .map_err(send_stage)
    }

    pub(super) fn contains_response(
        &self,
        kind: DesktopHarnessKind,
        marker: &str,
    ) -> Result<bool, Reason> {
        let mut input_seen = false;
        // Only the transcript above the current empty composer can certify a response.
        // A marker in the editable prompt never counts.
        // Response text and composer placeholders need not share an indentation.
        let response = self.find(|page| {
            let input = input_bounds(kind, page)?;
            input_seen = true;
            page.contains_marker_above(marker, input.y).then_some(())
        })?;
        if !input_seen {
            return Err(Reason::SelectorNotMatched);
        }
        Ok(response.is_some())
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

fn require_running(process: &mut tokio::process::Child) -> Result<(), Reason> {
    match process.try_wait() {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err(Reason::ApplicationExited),
        Err(_) => Err(Reason::IsolationUnavailable),
    }
}

fn input_bounds(kind: DesktopHarnessKind, page: &Page) -> Option<Rect> {
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
    let candidates = labels
        .iter()
        .filter_map(|label| page.find_phrase(label))
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [bounds] => Some(*bounds),
        _ => None,
    }
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
            Err(Reason::ApplicationExited)
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
