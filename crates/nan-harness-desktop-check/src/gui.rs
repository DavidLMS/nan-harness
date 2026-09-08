//! Native controls are resolved inside one app; native errors never enter public reports.

use crate::report::{InputMode, Reason};
use nan_harness_core::DesktopHarnessKind;
use std::time::Duration;
use xa11y::{App, AppExt as _, Locator};

const WAIT: Duration = Duration::from_secs(10);

pub(crate) struct Gui {
    app: App,
    kind: DesktopHarnessKind,
}

impl Gui {
    pub(crate) fn ensure_absent(kind: DesktopHarnessKind) -> Result<(), Reason> {
        let apps = App::list().map_err(map_error)?;
        if apps
            .iter()
            .any(|app| app_names(kind).contains(&app.name.as_str()))
        {
            return Err(Reason::AlreadyRunning);
        }
        Ok(())
    }

    pub(crate) fn wait(kind: DesktopHarnessKind, owner: u32) -> Result<Self, Reason> {
        let app = App::find(Duration::from_secs(45), |element| {
            element
                .name
                .as_deref()
                .is_some_and(|name| app_names(kind).contains(&name))
        })
        .map_err(map_error)?;
        let candidates = App::list()
            .map_err(map_error)?
            .into_iter()
            .filter(|candidate| app_names(kind).contains(&candidate.name.as_str()))
            .count();
        if candidates != 1 {
            return Err(Reason::InstallationAmbiguous);
        }
        if !app.pid.is_some_and(|pid| owned_process(pid, owner)) {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(Self { app, kind })
    }

    pub(crate) fn prepare_conversation(&self) -> Result<(), Reason> {
        if self.kind != DesktopHarnessKind::Zed {
            return Ok(());
        }
        // This app was launched with a fresh private profile and our own workspace.
        // Do not select the broader "trust all projects" checkbox.
        let trust = self.app.locator("button[name=\"Trust and Continue\"]");
        trust.wait_visible(WAIT).map_err(map_error)?;
        if trust.count().map_err(map_error)? != 1 {
            return Err(Reason::SelectorNotMatched);
        }
        trust.press().map_err(map_error)?;
        trust.wait_hidden(WAIT).map_err(map_error)?;
        let panel = self.app.locator("*[name=\"Agent Panel\"]");
        panel.wait_visible(WAIT).map_err(map_error)?;
        if panel.count().map_err(map_error)? != 1 {
            return Err(Reason::SelectorNotMatched);
        }
        panel.press().map_err(map_error)
    }

    pub(crate) fn submit(&self, prompt: &str) -> Result<InputMode, Reason> {
        let field = self.input()?;
        let mode = match field.set_value(prompt) {
            Ok(()) => InputMode::Accessibility,
            Err(xa11y::Error::TextValueNotSupported | xa11y::Error::ActionNotSupported { .. }) => {
                field.focus().map_err(map_error)?;
                field.wait_focused(WAIT).map_err(map_error)?;
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
        let send = self.app.locator("button[name=\"Send\"], button[name=\"Send message\"], button[description=\"Send\"], button[description=\"Send message\"]");
        if send.count().map_err(map_error)? == 1 {
            send.press().map_err(map_error)?;
        } else {
            field.focus().map_err(map_error)?;
            field.wait_focused(WAIT).map_err(map_error)?;
            xa11y::input_sim()
                .map_err(map_error)?
                .keyboard()
                .press(xa11y::Key::Enter)
                .map_err(map_error)?;
        }
        Ok(mode)
    }

    fn input(&self) -> Result<Locator, Reason> {
        if self.app.locator("button[name=\"Sign in\"], button[name=\"Log in\"], button[name=\"Continue with Google\"], button[name=\"Continue with email\"]").count().map_err(map_error)? > 0 {
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
        let named = self.app.locator(&selectors);
        if named.count().map_err(map_error)? == 1 {
            return Ok(named);
        }
        let editable = self
            .app
            .locator("text_area[editable=\"true\"], text_field[editable=\"true\"]");
        editable.wait_visible(WAIT).map_err(map_error)?;
        if editable.count().map_err(map_error)? != 1 {
            return Err(Reason::SelectorNotMatched);
        }
        Ok(editable)
    }

    pub(crate) fn wait_text(&self, marker: &str, budget: Duration) -> Result<(), Reason> {
        let selector = response_selector(marker)?;
        self.app
            .locator(&selector)
            .first()
            .wait_visible(budget)
            .map(|_| ())
            .map_err(|_| Reason::ResponseMismatch)
    }

    pub(crate) fn quit(&self) -> Result<(), Reason> {
        let quit = self
            .app
            .locator("menu_item[name^=\"Quit\"], menu_item[name=\"Exit\"]");
        if quit.count().map_err(map_error)? == 1 {
            return quit.press().map_err(map_error);
        }
        let field = self.input()?;
        field.focus().map_err(map_error)?;
        field.wait_focused(WAIT).map_err(map_error)?;
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
        DesktopHarnessKind::Claude => &["Claude"],
        DesktopHarnessKind::Hermes => &["Hermes", "Hermes Desktop"],
        DesktopHarnessKind::Pen => &["Pen", "Pencil"],
        DesktopHarnessKind::Zed => &["Zed", "zed", "zed-editor"],
    }
}

fn response_selector(marker: &str) -> Result<String, Reason> {
    if marker.is_empty()
        || marker.len() > 128
        || !marker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_:".contains(&byte))
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
    fn response_selectors_exclude_input_and_hidden_text() {
        let selector = response_selector("NAN_CHECK_FINAL:NAN_CHECK_READ_abc").unwrap();
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
