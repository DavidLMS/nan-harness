//! Native controls are resolved inside one app; native errors never enter public reports.

mod visual;

use crate::report::{InputMode, Reason, ResponseVerification};
use nan_harness_core::DesktopHarnessKind;
use std::time::{Duration, Instant};
use xa11y::{App, AppExt as _, Locator};

const WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AbsenceStage {
    AccessibilityProvider,
    AccessibilityEnumeration,
    NativeWindows,
}

pub(crate) struct AbsenceFailure {
    pub(crate) stage: AbsenceStage,
    pub(crate) reason: Reason,
}

pub(crate) struct Gui {
    app: Option<App>,
    kind: DesktopHarnessKind,
    visual: visual::Visual,
}

impl Gui {
    pub(crate) fn ensure_absent(kind: DesktopHarnessKind) -> Result<(), AbsenceFailure> {
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
        visual::Visual::ensure_absent(kind).map_err(|reason| AbsenceFailure {
            stage: AbsenceStage::NativeWindows,
            reason,
        })
    }

    pub(crate) fn wait(
        kind: DesktopHarnessKind,
        process: &mut tokio::process::Child,
    ) -> Result<Self, Reason> {
        let visual = visual::Visual::wait(kind, process)?;
        let app = App::by_pid(visual.pid(), Duration::ZERO).ok();
        Ok(Self { app, kind, visual })
    }

    pub(crate) fn prepare_conversation(&self) -> Result<(), Reason> {
        if self.kind != DesktopHarnessKind::Zed {
            return Ok(());
        }
        // This app was launched with a fresh private profile and our own workspace.
        // Do not select the broader "trust all projects" checkbox.
        if let Some(app) = &self.app {
            let trust = app.locator("button[name=\"Trust and Continue\"]");
            trust.wait_visible(WAIT).map_err(map_error)?;
            if trust.count().map_err(map_error)? != 1 {
                return Err(Reason::SelectorNotMatched);
            }
            self.visual.guard()?;
            trust.press().map_err(map_error)?;
            trust.wait_hidden(WAIT).map_err(map_error)?;
            let panel = app.locator("*[name=\"Agent Panel\"]");
            panel.wait_visible(WAIT).map_err(map_error)?;
            if panel.count().map_err(map_error)? != 1 {
                return Err(Reason::SelectorNotMatched);
            }
            self.visual.guard()?;
            panel.press().map_err(map_error)
        } else {
            self.visual.click_phrase("Trust and Continue")?;
            self.visual.guard()?;
            xa11y::input_sim()
                .map_err(map_error)?
                .keyboard()
                .chord(
                    xa11y::Key::Char('/'),
                    &[primary_modifier(), xa11y::Key::Shift],
                )
                .map_err(map_error)
        }
    }

    pub(crate) fn submit(&self, prompt: &str) -> Result<InputMode, Reason> {
        let field = match self.input() {
            Ok(field) => field,
            Err(Reason::SelectorNotMatched) => {
                self.visual.submit(self.kind, prompt)?;
                return Ok(InputMode::VisualAndKeyboard);
            }
            Err(reason) => return Err(reason),
        };
        self.visual.guard()?;
        let mode = match field.set_value(prompt) {
            Ok(()) => InputMode::Accessibility,
            Err(xa11y::Error::TextValueNotSupported | xa11y::Error::ActionNotSupported { .. }) => {
                field.focus().map_err(map_error)?;
                field.wait_focused(WAIT).map_err(map_error)?;
                self.visual.guard()?;
                let input = xa11y::input_sim().map_err(map_error)?;
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
                    .map_err(map_error)?;
                self.visual.guard()?;
                input.keyboard().type_text(prompt).map_err(map_error)?;
                InputMode::AccessibilityAndKeyboard
            }
            Err(error) => return Err(map_error(error)),
        };
        field
            .wait_until(
                |element| element.is_some_and(|element| element.value.as_deref() == Some(prompt)),
                WAIT,
            )
            .map_err(|_| Reason::InputMismatch)?;
        self.visual.guard()?;
        let app = self.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let send = app.locator("button[name=\"Send\"], button[name=\"Send message\"], button[description=\"Send\"], button[description=\"Send message\"]");
        if send.count().map_err(map_error)? == 1 {
            send.press().map_err(map_error)?;
        } else {
            field.focus().map_err(map_error)?;
            field.wait_focused(WAIT).map_err(map_error)?;
            self.visual.guard()?;
            xa11y::input_sim()
                .map_err(map_error)?
                .keyboard()
                .press(xa11y::Key::Enter)
                .map_err(map_error)?;
        }
        Ok(mode)
    }

    fn input(&self) -> Result<Locator, Reason> {
        let app = self.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        if self.kind != DesktopHarnessKind::Zed && app.locator("button[name=\"Sign in\"], button[name=\"Log in\"], button[name=\"Continue with Google\"], button[name=\"Continue with email\"]").count().map_err(map_error)? > 0 {
            return Err(Reason::LoginRequired);
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
        if named.count().map_err(map_error)? == 1 {
            return Ok(named);
        }
        let editable = app.locator("text_area[editable=\"true\"], text_field[editable=\"true\"]");
        if self.kind == DesktopHarnessKind::Zed && editable.count().map_err(map_error)? == 0 {
            return Err(Reason::SelectorNotMatched);
        }
        editable.wait_visible(WAIT).map_err(map_error)?;
        if editable.count().map_err(map_error)? != 1 {
            return Err(Reason::SelectorNotMatched);
        }
        Ok(editable)
    }

    pub(crate) fn wait_text(
        &self,
        marker: &str,
        budget: Duration,
    ) -> Result<ResponseVerification, Reason> {
        let selector = response_selector(marker)?;
        let deadline = Instant::now() + budget;
        loop {
            self.visual.guard()?;
            if let Some(app) = &self.app
                && app.locator(&selector).count().map_err(map_error)? > 0
            {
                return Ok(ResponseVerification::Accessibility);
            }
            let pending_reason = match self.visual.contains_response(self.kind, marker) {
                Ok(true) => return Ok(ResponseVerification::LocalOcr),
                Ok(false) => Reason::ResponseMismatch,
                // The composer can disappear during a response/layout transition.
                // Keep polling, but do not misreport a missing region as wrong text.
                Err(Reason::SelectorNotMatched) => Reason::SelectorNotMatched,
                Err(reason) => return Err(reason),
            };
            if Instant::now() >= deadline {
                if pending_reason == Reason::ResponseMismatch {
                    self.visual.retain_failed_response(self.kind)?;
                }
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
fn owned_process(pid: u32, owner: u32) -> bool {
    use nix::unistd::{Pid, getpgid};
    if pid == 0 || owner == 0 {
        return false;
    }
    let (Ok(pid), Ok(owner)) = (i32::try_from(pid), i32::try_from(owner)) else {
        return false;
    };
    let Ok(group) = getpgid(Some(Pid::from_raw(owner))) else {
        return false;
    };
    getpgid(Some(Pid::from_raw(pid))).is_ok_and(|candidate| candidate == group)
}

#[cfg(windows)]
fn owned_process(pid: u32, owner: u32) -> bool {
    use std::process::{Command, Stdio};
    Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "$candidateId = [uint32]$env:NAN_CHECK_APP_PID; $ownerId = [uint32]$env:NAN_CHECK_OWNER_PID; for ($depth = 0; $depth -lt 32; $depth++) { if ($candidateId -eq $ownerId) { exit 0 }; $candidate = Get-CimInstance Win32_Process -Filter \"ProcessId=$candidateId\" -ErrorAction Stop; if ($null -eq $candidate -or $candidate.ParentProcessId -eq 0) { exit 1 }; $candidateId = $candidate.ParentProcessId }; exit 1"])
        .env("NAN_CHECK_APP_PID", pid.to_string()).env("NAN_CHECK_OWNER_PID", owner.to_string()).env_remove("NAN_API_KEY").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
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

fn map_error(error: xa11y::Error) -> Reason {
    let reason = match &error {
        xa11y::Error::PermissionDenied { .. } => Reason::PermissionRequired,
        xa11y::Error::TextValueNotSupported | xa11y::Error::ActionNotSupported { .. } => {
            Reason::ActionUnsupported
        }
        _ => Reason::SelectorNotMatched,
    };
    drop(error);
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(unix)]
    #[test]
    fn process_ownership_requires_a_known_group() {
        assert!(owned_process(std::process::id(), std::process::id()));
        assert!(!owned_process(u32::MAX, std::process::id()));
        assert!(!owned_process(std::process::id(), u32::MAX));
    }
}
