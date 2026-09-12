use std::io::{BufRead, Write};
use thiserror::Error;

pub const ERROR_REPORT_PROMPT: &str = "Send an anonymous error report? [y/N] ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptDecision {
    Send,
    Decline,
}

/// Reads one explicit error-report decision from an interactive terminal.
///
/// # Errors
///
/// Returns [`PromptError`] when the prompt cannot be written or the answer cannot be read.
pub fn ask_to_send_error_report<R, W>(
    input: &mut R,
    output: &mut W,
) -> Result<PromptDecision, PromptError>
where
    R: BufRead,
    W: Write,
{
    output
        .write_all(
            nan_harness_i18n::messages::prompt_error_report(nan_harness_i18n::locale()).as_bytes(),
        )
        .map_err(PromptError::Write)?;
    output.flush().map_err(PromptError::Write)?;
    let mut answer = String::new();
    input.read_line(&mut answer).map_err(PromptError::Read)?;
    if answer.trim().eq_ignore_ascii_case("y")
        || (nan_harness_i18n::locale() == nan_harness_i18n::Locale::Es
            && nan_harness_i18n::yes_no(nan_harness_i18n::Locale::Es, &answer) == Some(true))
    {
        Ok(PromptDecision::Send)
    } else {
        Ok(PromptDecision::Decline)
    }
}

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("could not show the error-report prompt: {0}")]
    Write(std::io::Error),
    #[error("could not read the error-report decision: {0}")]
    Read(std::io::Error),
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for PromptError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::Write(field_0) => m::error_prompt_write(locale, &(field_0)),
            Self::Read(field_0) => m::error_prompt_read(locale, &(field_0)),
        }
    }
}
