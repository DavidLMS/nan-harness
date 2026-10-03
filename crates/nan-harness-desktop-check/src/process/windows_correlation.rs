//! Historical ancestry correlation is diagnostic, never job membership or cleanup proof.
use zeroize::Zeroizing;

pub(crate) struct Snapshot(Zeroizing<String>, u32);

impl Snapshot {
    pub(crate) fn parse(wire: Zeroizing<String>, launcher: u32) -> Option<Self> {
        if wire.len() > 8192 {
            return None;
        }
        let mut lines = wire.lines();
        let count = lines
            .next()?
            .strip_prefix("snapshot ")?
            .parse::<usize>()
            .ok()?;
        if !(1..=65).contains(&count) {
            return None;
        }
        let mut ids = Vec::new();
        for index in 0..count {
            let words = lines.next()?.split_whitespace().collect::<Vec<_>>();
            if words.len() != 3 {
                return None;
            }
            let pid = words[0].parse::<u32>().ok()?;
            let time = words[1].parse::<u64>().ok()?;
            let descendant = words[2];
            if pid == 0
                || time == 0
                || ids.contains(&pid)
                || (index == 0 && (pid != launcher || descendant != "0"))
                || (index > 0 && descendant != "1")
            {
                return None;
            }
            ids.push(pid);
        }
        if lines.next().is_some() {
            return None;
        }
        Some(Self(wire, launcher))
    }

    #[cfg(windows)]
    pub(crate) fn request(&self) -> Zeroizing<String> {
        Zeroizing::new(format!(
            "{} {}\n{}",
            std::process::id(),
            self.1,
            self.0.strip_prefix("snapshot ").unwrap_or("")
        ))
    }
}

pub(crate) fn observed(wire: &str) -> Option<serde_json::Value> {
    let words = wire.split_whitespace().collect::<Vec<_>>();
    if words.len() != 5 || words[0] != "observed" {
        return None;
    }
    let launcher = match words[1] {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let counts = words[2..]
        .iter()
        .map(|word| word.parse::<usize>().ok())
        .collect::<Option<Vec<_>>>()?;
    if counts.iter().any(|count| *count > 64) || counts[1] + counts[2] != counts[0] {
        return None;
    }
    Some(
        serde_json::json!({"schemaVersion":1,"mechanism":"windows-process-correlation","diagnosticsOnly":true,
        "status":"observed","sameLauncherSurvives":launcher,"verifiedDescendantsPresent":counts[1]>0,
        "unlinkedMatchesPresent":counts[2]>0,"matchedCount":counts[0],"verifiedDescendantCount":counts[1],"unlinkedCount":counts[2]}),
    )
}

#[cfg(windows)]
pub(crate) fn record(wire: Option<&str>, deadline: bool) {
    let value = wire.and_then(observed).unwrap_or_else(|| {
        serde_json::json!({
        "schemaVersion":1,"mechanism":"windows-process-correlation","diagnosticsOnly":true,
        "status":if deadline {"deadline"} else {"unavailable"},"sameLauncherSurvives":null,
        "verifiedDescendantsPresent":null,"unlinkedMatchesPresent":null,"matchedCount":null,
        "verifiedDescendantCount":null,"unlinkedCount":null})
    });
    super::windows_observation::record_value(&value, "windows-process-correlation");
}

#[cfg(windows)]
pub(crate) fn record_cleanup(value: &serde_json::Value) {
    super::windows_observation::record_value(value, "windows-owned-descendant-cleanup");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_snapshot_rejects_replacement_duplicate_and_extra_wire() {
        let snapshot = Snapshot::parse(
            Zeroizing::new("snapshot 2\n12 100 0\n13 101 1\n".into()),
            12,
        )
        .unwrap();
        assert!(snapshot.0.contains("13 101"));
        assert_eq!(snapshot.1, 12);
        for wire in [
            "snapshot 1\n11 100 0\n",
            "snapshot 2\n12 100 0\n12 101 1\n",
            "snapshot 1\n12 0 0\n",
            "snapshot 1\n12 100 0\nprivate\n",
        ] {
            assert!(Snapshot::parse(Zeroizing::new(wire.into()), 12).is_none());
        }
    }
    #[test]
    fn closed_receipt_partitions_counts_without_private_identity() {
        let value = observed("observed 1 3 2 1\n").unwrap();
        assert_eq!(value["verifiedDescendantsPresent"], true);
        assert_eq!(value["unlinkedMatchesPresent"], true);
        assert_eq!(value.as_object().unwrap().len(), 10);
        for wire in [
            "observed 2 0 0 0",
            "observed 0 3 1 1",
            "observed 0 65 65 0",
            "observed 0 0 0 0 private",
        ] {
            assert!(observed(wire).is_none());
        }
        assert!(!value.to_string().contains("created"));
    }
}
