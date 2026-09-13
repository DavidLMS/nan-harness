use crate::diagnostics::ClaudeIdentityObservation;
use crate::report::Reason;

const MAX_OUTPUT_BYTES: usize = 512;
pub(crate) const MAX_BUNDLE_INPUT_BYTES: usize = 4095;

pub(crate) fn parse_bundle_input(input: &[u8]) -> Result<&[u8], Reason> {
    if input.len() < 2
        || input.len() > MAX_BUNDLE_INPUT_BYTES + 1
        || input.last() != Some(&b'\n')
        || input[..input.len() - 1]
            .iter()
            .any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(Reason::DesktopUnavailable);
    }
    Ok(&input[..input.len() - 1])
}

pub(crate) fn classify_window_state(
    matching_processes: Result<usize, ()>,
    visible_windows: Result<usize, ()>,
    named_windows: Result<usize, ()>,
    eligible_windows: Result<usize, ()>,
    overflow: bool,
) -> ClaudeIdentityObservation {
    if overflow {
        ClaudeIdentityObservation::Overflow
    } else if matching_processes.is_err()
        || visible_windows.is_err()
        || named_windows.is_err()
        || eligible_windows.is_err()
    {
        ClaudeIdentityObservation::QueryUnavailable
    } else if matching_processes == Ok(0) {
        ClaudeIdentityObservation::NoMatchingBundleProcess
    } else if matching_processes != Ok(1) {
        ClaudeIdentityObservation::AmbiguousIdentity
    } else if visible_windows == Ok(0) {
        ClaudeIdentityObservation::MatchingProcessNoVisibleWindow
    } else if named_windows == Ok(0) {
        ClaudeIdentityObservation::WindowNameMismatch
    } else if eligible_windows == Ok(0) {
        ClaudeIdentityObservation::WindowNotEligible
    } else {
        ClaudeIdentityObservation::WindowEligible
    }
}

impl ClaudeIdentityObservation {
    pub(crate) fn parse(output: &str) -> Result<Self, Reason> {
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(Reason::DesktopUnavailable);
        }
        let mut lines = output.lines();
        let state = lines
            .next()
            .and_then(|line| line.strip_prefix("OBS "))
            .ok_or(Reason::DesktopUnavailable)?;
        if lines.next().is_some() {
            return Err(Reason::DesktopUnavailable);
        }
        match state {
            "no-matching-bundle-process" => Ok(Self::NoMatchingBundleProcess),
            "matching-process-no-visible-window" => Ok(Self::MatchingProcessNoVisibleWindow),
            "window-name-mismatch" => Ok(Self::WindowNameMismatch),
            "window-not-eligible" => Ok(Self::WindowNotEligible),
            "window-eligible" => Ok(Self::WindowEligible),
            "ambiguous-identity" => Ok(Self::AmbiguousIdentity),
            "query-unavailable" => Ok(Self::QueryUnavailable),
            "overflow" => Ok(Self::Overflow),
            _ => Err(Reason::DesktopUnavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_one_closed_observation_line() {
        assert_eq!(
            ClaudeIdentityObservation::parse("OBS window-name-mismatch\n").unwrap(),
            ClaudeIdentityObservation::WindowNameMismatch
        );
        for output in [
            "OBS query-unavailable\nextra\n",
            "OBS unknown\n",
            "window-eligible\n",
            "OBS window-eligible\nprivate-name\n",
        ] {
            assert!(ClaudeIdentityObservation::parse(output).is_err());
        }
        assert!(
            ClaudeIdentityObservation::parse(&format!("OBS {}\n", "x".repeat(MAX_OUTPUT_BYTES)))
                .is_err()
        );
    }

    #[test]
    fn bounded_bundle_input_accepts_exact_eof_and_rejects_extra_data() {
        assert_eq!(
            parse_bundle_input(b"/selected/Claude.app\n").unwrap(),
            b"/selected/Claude.app"
        );
        for input in [
            b"/selected/Claude.app\nextra\n".as_slice(),
            b"/selected/Claude.app\0\n".as_slice(),
            b"/selected/Claude.app\r\n".as_slice(),
        ] {
            assert!(parse_bundle_input(input).is_err());
        }
        let oversized = vec![b'x'; MAX_BUNDLE_INPUT_BYTES + 2];
        assert!(parse_bundle_input(&oversized).is_err());
    }

    #[test]
    fn classifier_keeps_query_errors_and_caps_distinct() {
        assert_eq!(
            classify_window_state(Ok(0), Ok(0), Ok(0), Ok(0), false),
            ClaudeIdentityObservation::NoMatchingBundleProcess
        );
        assert_eq!(
            classify_window_state(Ok(2), Ok(0), Ok(0), Ok(0), false),
            ClaudeIdentityObservation::AmbiguousIdentity
        );
        assert_eq!(
            classify_window_state(Ok(1), Ok(0), Ok(0), Ok(0), false),
            ClaudeIdentityObservation::MatchingProcessNoVisibleWindow
        );
        assert_eq!(
            classify_window_state(Ok(1), Ok(1), Ok(0), Ok(0), false),
            ClaudeIdentityObservation::WindowNameMismatch
        );
        assert_eq!(
            classify_window_state(Ok(1), Ok(1), Ok(1), Ok(0), false),
            ClaudeIdentityObservation::WindowNotEligible
        );
        assert_eq!(
            classify_window_state(Ok(1), Ok(1), Ok(1), Ok(1), false),
            ClaudeIdentityObservation::WindowEligible
        );
        assert_eq!(
            classify_window_state(Err(()), Ok(0), Ok(0), Ok(0), false),
            ClaudeIdentityObservation::QueryUnavailable
        );
        assert_eq!(
            classify_window_state(Ok(1), Ok(1), Ok(1), Ok(1), true),
            ClaudeIdentityObservation::Overflow
        );
    }
}
