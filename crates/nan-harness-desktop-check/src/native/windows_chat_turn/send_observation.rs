//! Payload-free passive Send acquisition observations; never action authority.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendObservation {
    phase: &'static str,
    completed_scans: u16,
    pending_scans: u16,
}

impl SendObservation {
    pub(crate) fn parse(line: &str) -> Option<Self> {
        let words: Vec<_> = line
            .strip_prefix("send-observation ")?
            .strip_suffix('\n')?
            .split(' ')
            .collect();
        let [phase, completed, pending] = words.as_slice() else {
            return None;
        };
        let phase = match *phase {
            "paste-check" => "paste-check",
            "paste-checked" => "paste-checked",
            "tree-scan" => "tree-scan",
            "tree-scanned" => "tree-scanned",
            "candidate-check" => "candidate-check",
            "target-check" => "target-check",
            "release" => "release",
            "released" => "released",
            _ => return None,
        };
        let count = |word: &str| {
            if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            word.parse::<u16>().ok().filter(|count| *count <= 1024)
        };
        let completed_scans = count(completed)?;
        let pending_scans = count(pending)?;
        (pending_scans <= completed_scans).then_some(Self {
            phase,
            completed_scans,
            pending_scans,
        })
    }

    pub(crate) fn follows(&self, previous: &Self) -> bool {
        self.completed_scans >= previous.completed_scans
            && self.pending_scans >= previous.pending_scans
    }
}

#[cfg(test)]
mod tests {
    use super::SendObservation;

    #[test]
    fn wire_rejects_unknown_payloads_types_counts_and_trailing_data() {
        let value = SendObservation::parse("send-observation tree-scan 2 1\n").unwrap();
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"phase":"tree-scan","completedScans":2,"pendingScans":1}"#
        );
        for invalid in [
            "send-observation PRIVATE 2 1\n",
            "send-observation {} 2 1\n",
            "send-observation tree-scan true 1\n",
            "send-observation tree-scan -1 0\n",
            "send-observation tree-scan +1 0\n",
            "send-observation tree-scan 0 1\n",
            "send-observation tree-scan 1025 1\n",
            "send-observation tree-scan 1 0 PRIVATE\n",
            "send-observation tree-scan 1 0\nPRIVATE",
            "send-observation tree-scan 1 0",
        ] {
            assert!(SendObservation::parse(invalid).is_none());
        }
        assert!(
            !SendObservation::parse("send-observation released 1 1\n")
                .unwrap()
                .follows(&value)
        );
    }
}
