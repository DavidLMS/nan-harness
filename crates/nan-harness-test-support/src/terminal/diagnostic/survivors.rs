//! Bounded survivor attribution before and after owned-process cleanup.
//!
//! Windows counts all active members of the retained kernel Job, including nested Jobs and a root
//! whose exit is unconfirmed. It publishes no executable names: a secondary naming query must not
//! discard authoritative membership or add a global PID ancestry assumption. Unix correlates the
//! process table's parent chains and process groups and publishes bounded executable base names.
//! Neither path publishes process identifiers, paths, command lines, or environment data.

#[cfg(any(not(windows), test))]
use std::collections::HashMap;
use std::time::Duration;

#[cfg(any(not(windows), test))]
const MAX_PUBLISHED_NAMES: usize = 8;
#[cfg(any(not(windows), test))]
const MAX_NAME_LENGTH: usize = 48;
#[cfg(any(not(windows), test))]
const ANCESTRY_HOPS: usize = 64;
#[cfg(not(windows))]
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether a survivor scan was requested, ran, or could not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanState {
    /// No survivor attribution was needed for this observation.
    NotNeeded,
    /// The platform ownership query succeeded, including a successfully empty result.
    Available,
    /// The platform query failed or timed out; the scan carries no information.
    Unavailable,
}

/// Live owned processes; Windows counts every active Job member without publishing names.
#[derive(Debug, Clone)]
pub struct SurvivorScan {
    pub state: ScanState,
    pub names: Vec<String>,
    pub count: u32,
}

impl SurvivorScan {
    #[must_use]
    pub const fn not_needed() -> Self {
        Self {
            state: ScanState::NotNeeded,
            names: Vec::new(),
            count: 0,
        }
    }

    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            state: ScanState::Unavailable,
            names: Vec::new(),
            count: 0,
        }
    }
}

#[cfg(any(not(windows), test))]
#[derive(Debug)]
struct ProcessEntry {
    parent: u32,
    /// Process group identifier where the process table reports one.
    group: Option<u32>,
    name: String,
}

/// Process identifiers mapped to their recorded parent, process group, and executable base name.
#[cfg(any(not(windows), test))]
#[derive(Debug, Default)]
pub struct ProcessTable {
    entries: HashMap<u32, ProcessEntry>,
}

#[cfg(any(not(windows), test))]
impl ProcessTable {
    /// Parses `pid<separator>ppid<separator>pgid<separator>name` lines; malformed lines are
    /// skipped and an empty group field means the platform did not report one.
    pub fn parse(separator: char, text: &str) -> Self {
        let mut entries = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let fields = line.split(separator).collect::<Vec<_>>();
            let [pid, parent, group, name @ ..] = fields.as_slice() else {
                continue;
            };
            let (Ok(pid), Ok(parent)) = (pid.trim().parse::<u32>(), parent.trim().parse::<u32>())
            else {
                continue;
            };
            let group = group.trim().parse::<u32>().ok();
            entries.insert(
                pid,
                ProcessEntry {
                    parent,
                    group,
                    name: sanitize(&name.join(&separator.to_string())),
                },
            );
        }
        Self { entries }
    }

    /// Names and count of live processes owned by `root`, excluding the root itself.
    ///
    /// A process is owned when its recorded parent chain reaches the root, or when it still shares
    /// the root's process group, which survives reparenting on platforms that reparent orphans.
    pub fn descendants(&self, root: u32) -> SurvivorScan {
        let mut names = Vec::new();
        let mut count = 0_u32;
        for pid in self.entries.keys().copied() {
            if pid == root || !self.is_owned(root, pid) {
                continue;
            }
            count = count.saturating_add(1);
            if let Some(entry) = self.entries.get(&pid) {
                names.push(entry.name.clone());
            }
        }
        names.sort();
        names.dedup();
        names.truncate(MAX_PUBLISHED_NAMES);
        SurvivorScan {
            state: ScanState::Available,
            names,
            count,
        }
    }

    fn is_owned(&self, root: u32, pid: u32) -> bool {
        if self
            .entries
            .get(&pid)
            .is_some_and(|entry| entry.group == Some(root))
        {
            return true;
        }
        let mut current = pid;
        for _ in 0..ANCESTRY_HOPS {
            let Some(entry) = self.entries.get(&current) else {
                return false;
            };
            if entry.parent == root {
                return true;
            }
            if entry.parent == 0 || entry.parent == current {
                return false;
            }
            current = entry.parent;
        }
        false
    }
}

/// Reduces a raw executable name to a bounded, printable base name.
#[cfg(any(not(windows), test))]
pub fn sanitize(raw: &str) -> String {
    let base = raw
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '+')
        })
        .take(MAX_NAME_LENGTH)
        .collect::<String>();
    if base.is_empty() {
        "<unknown>".to_owned()
    } else {
        base
    }
}

/// Inventories the live descendants of `root` using the platform's own process listing.
#[cfg(not(windows))]
pub async fn scan(root: u32) -> SurvivorScan {
    match platform_table().await {
        Some(table) => table.descendants(root),
        None => SurvivorScan::unavailable(),
    }
}

/// Repeatedly inventories `root`'s descendants until none remain or the attempts are exhausted.
///
/// Owned-tree termination is asynchronous, so a single read can still observe processes that are
/// already dying. The bounded retry keeps a real escapee visible without waiting indefinitely.
#[cfg(not(windows))]
pub async fn scan_after_cleanup(root: u32) -> SurvivorScan {
    let mut last = scan(root).await;
    for _ in 1..3 {
        if last.count == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        last = scan(root).await;
    }
    last
}

#[cfg(not(windows))]
async fn platform_table() -> Option<ProcessTable> {
    let output = tokio::time::timeout(
        QUERY_TIMEOUT,
        tokio::process::Command::new("ps")
            .args(["-eo", "pid=,ppid=,pgid=,comm="])
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut normalized = String::with_capacity(text.len());
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(parent), Some(group)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let name = fields.collect::<Vec<_>>().join(" ");
        normalized.push_str(pid);
        normalized.push('|');
        normalized.push_str(parent);
        normalized.push('|');
        normalized.push_str(group);
        normalized.push('|');
        normalized.push_str(&name);
        normalized.push('\n');
    }
    Some(ProcessTable::parse('|', &normalized))
}

/// Converts only a successful retained-Job query into an available count.
#[cfg(any(windows, test))]
fn job_members(result: &std::io::Result<u32>) -> SurvivorScan {
    match result {
        Ok(count) => SurvivorScan {
            state: ScanState::Available,
            names: Vec::new(),
            count: *count,
        },
        Err(_) => SurvivorScan::unavailable(),
    }
}

#[cfg(windows)]
pub(super) fn scan_job(child: &super::super::OwnedChild) -> SurvivorScan {
    job_members(&child.active_process_count())
}

#[cfg(windows)]
pub(super) async fn scan_job_after_cleanup(child: &super::super::OwnedChild) -> SurvivorScan {
    let mut last = scan_job(child);
    for _ in 1..3 {
        if last.count == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        last = scan_job(child);
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    #[test]
    fn parse_and_descendants_follow_the_recorded_parent_chain() {
        let table = ProcessTable::parse(
            '|',
            "10|1|10|root.exe\n\
             20|10||child.exe\n\
             30|20||leaf.exe\n\
             40|1||unrelated.exe\n\
             malformed\n\
             50|||empty-parent.exe\n\
             60|60||self-parent.exe\n",
        );
        let scanned = table.descendants(10);
        assert_eq!(scanned.state, ScanState::Available);
        assert_eq!(scanned.count, 2);
        assert_eq!(scanned.names, vec!["child.exe", "leaf.exe"]);
        assert_eq!(table.descendants(20).names, vec!["leaf.exe"]);
        assert_eq!(table.descendants(999).count, 0);
    }

    #[test]
    fn published_names_are_bounded_and_deduplicated() {
        let mut text = String::from("1|0|1|root.exe\n");
        for pid in 2..20 {
            let _ = writeln!(text, "{pid}|1||same.exe");
        }
        let scanned = ProcessTable::parse('|', &text).descendants(1);
        assert_eq!(scanned.count, 18);
        assert_eq!(scanned.names, vec!["same.exe"]);
    }

    #[test]
    fn process_group_membership_keeps_orphans_attributable() {
        // The parent exited, so the orphan was reparented; only the group still names its owner.
        let table = ProcessTable::parse('|', "90|1|42|orphan.exe\n42|1|42|launcher.exe\n");
        let scanned = table.descendants(42);
        assert_eq!(scanned.count, 1);
        assert_eq!(scanned.names, vec!["orphan.exe"]);
    }

    #[test]
    fn retained_job_count_does_not_adopt_stale_parent_decoys() {
        // A table cannot distinguish a reused root PID from an old recorded parent PID.
        // This demonstrates that ambiguity; it does not identify any actual CI process.
        let table = ProcessTable::parse(
            '|',
            "42|1||new-root.exe\n90|42||old-service.exe\n91|90||service-child.exe\n",
        );
        assert_eq!(table.descendants(42).count, 2);
        let empty_job = job_members(&Ok(0));
        assert_eq!(empty_job.state, ScanState::Available);
        assert_eq!(empty_job.count, 0);
        assert!(empty_job.names.is_empty());

        // All queried members count, including a still-live root; names are secondary.
        let owned_job = job_members(&Ok(3));
        assert_eq!(owned_job.state, ScanState::Available);
        assert_eq!(owned_job.count, 3);
        assert!(owned_job.names.is_empty());
    }

    #[test]
    fn failed_job_query_cannot_report_an_available_empty_job() {
        let scanned = job_members(&Err(std::io::Error::other("query failed")));
        assert_eq!(scanned.state, ScanState::Unavailable);
        assert_eq!(scanned.count, 0);
        assert!(scanned.names.is_empty());
    }

    #[test]
    fn sanitize_strips_paths_and_unprintable_characters() {
        assert_eq!(sanitize("C:\\Tools\\codex.exe"), "codex.exe");
        assert_eq!(sanitize("/usr/lib/some daemon"), "somedaemon");
        assert_eq!(sanitize(""), "<unknown>");
        assert_eq!(sanitize("  "), "<unknown>");
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn scan_reports_the_current_process_ancestry() {
        let root = std::process::id();
        let scanned = scan(root).await;
        assert_ne!(
            scanned.state,
            ScanState::Unavailable,
            "the platform process listing should be available"
        );
        // The test harness spawns no descendants, so the root must not descend from itself.
        assert!(scanned.names.iter().all(|name| !name.is_empty()));
    }
}
