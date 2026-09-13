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
        for (wire, expected) in [
            (
                "no-matching-bundle-process",
                ClaudeIdentityObservation::NoMatchingBundleProcess,
            ),
            (
                "matching-process-no-visible-window",
                ClaudeIdentityObservation::MatchingProcessNoVisibleWindow,
            ),
            (
                "window-name-mismatch",
                ClaudeIdentityObservation::WindowNameMismatch,
            ),
            (
                "window-not-eligible",
                ClaudeIdentityObservation::WindowNotEligible,
            ),
            ("window-eligible", ClaudeIdentityObservation::WindowEligible),
            (
                "ambiguous-identity",
                ClaudeIdentityObservation::AmbiguousIdentity,
            ),
            (
                "query-unavailable",
                ClaudeIdentityObservation::QueryUnavailable,
            ),
            ("overflow", ClaudeIdentityObservation::Overflow),
        ] {
            assert_eq!(
                ClaudeIdentityObservation::parse(&format!("OBS {wire}\n")).unwrap(),
                expected
            );
        }
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
}
