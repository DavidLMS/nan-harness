use super::{app_names, map_error, owned_process};
use crate::{
    native::{Native, Page, Window},
    report::Reason,
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
        let snapshot = Native::new()?.windows()?;
        if snapshot
            .windows
            .iter()
            .any(|window| matches_app(kind, &window.name))
        {
            return Err(Reason::AlreadyRunning);
        }
        Ok(())
    }

    pub(super) fn wait(kind: DesktopHarnessKind, owner: u32) -> Result<Self, Reason> {
        let native = Native::new()?;
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut previous = None;
        loop {
            let snapshot = native.windows()?;
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
        self.native.windows()?.require_clear(&self.window)
    }

    pub(super) fn page(&self) -> Result<(Page, f32), Reason> {
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
        Ok((self.native.recognize(&screenshot)?, screenshot.scale))
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

    pub(super) fn click_phrase(&self, phrase: &str) -> Result<(), Reason> {
        let (page, scale) = self.page()?;
        let bounds = page.find_phrase(phrase).ok_or(Reason::SelectorNotMatched)?;
        self.click(bounds, scale)
    }

    fn click(&self, bounds: Rect, scale: f32) -> Result<(), Reason> {
        let point = point_in_window(self.capture_bounds(), bounds, scale)?;
        self.guard()?;
        xa11y::input_sim()
            .map_err(map_error)?
            .mouse()
            .click(point)
            .map_err(map_error)?;
        self.guard()
    }

    pub(super) fn submit(&self, kind: DesktopHarnessKind, prompt: &str) -> Result<(), Reason> {
        let (page, scale) = self.page()?;
        let bounds = input_bounds(kind, &page).ok_or(Reason::SelectorNotMatched)?;
        self.click(bounds, scale)?;
        let input = xa11y::input_sim().map_err(map_error)?;
        self.guard()?;
        input
            .keyboard()
            .chord(xa11y::Key::Char('a'), &[super::primary_modifier()])
            .map_err(map_error)?;
        self.guard()?;
        input.keyboard().type_text(prompt).map_err(map_error)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (page, _) = self.page()?;
            if page.find_phrase(prompt).is_some() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(Reason::InputMismatch);
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        self.guard()?;
        input.keyboard().press(xa11y::Key::Enter).map_err(map_error)
    }

    pub(super) fn contains_response(
        &self,
        kind: DesktopHarnessKind,
        marker: &str,
    ) -> Result<bool, Reason> {
        let (page, _) = self.page()?;
        let Some(input) = input_bounds(kind, &page) else {
            return Ok(false);
        };
        // Only the transcript above the current empty composer can certify a response.
        // A marker in the editable prompt never counts.
        // Response text and composer placeholders need not share an indentation.
        Ok(page.contains_marker_above(marker, input.y))
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
