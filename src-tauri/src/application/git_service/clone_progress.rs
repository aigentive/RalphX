//! Parser for `git clone --progress` stderr.
//!
//! Deliberately tolerant: Git's exact wording varies across versions, and an
//! unrecognized line is simply not a progress update rather than an error. The
//! UI falls back to an indeterminate bar when no percentage is available.

use super::clone::{ClonePhase, CloneProgress};

/// Parse one stderr segment into a progress update, or `None` when the line
/// carries no phase information.
pub(crate) fn parse_clone_progress(line: &str) -> Option<CloneProgress> {
    let phase = detect_phase(line)?;
    let (received, total) = parse_counts(line);

    Some(CloneProgress {
        phase,
        percent: parse_percent(line),
        received,
        total,
        line: line.to_string(),
    })
}

fn detect_phase(line: &str) -> Option<ClonePhase> {
    // `remote: ` prefixes are stripped by matching on substrings rather than
    // prefixes, which also survives Git reordering its own prefixes.
    let normalized = line.to_lowercase();
    if normalized.contains("cloning into") {
        return Some(ClonePhase::Connecting);
    }
    if normalized.contains("enumerating objects") || normalized.contains("counting objects") {
        return Some(ClonePhase::Counting);
    }
    if normalized.contains("compressing objects") {
        return Some(ClonePhase::Compressing);
    }
    if normalized.contains("receiving objects") {
        return Some(ClonePhase::Receiving);
    }
    if normalized.contains("resolving deltas") {
        return Some(ClonePhase::Resolving);
    }
    if normalized.contains("updating files") || normalized.contains("checking out files") {
        return Some(ClonePhase::CheckingOut);
    }
    None
}

/// Read the first `NN%` in the line.
fn parse_percent(line: &str) -> Option<u8> {
    let percent_index = line.find('%')?;
    let digits: String = line[..percent_index]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse::<u16>().ok().map(|value| value.min(100) as u8)
}

/// Read the `(received/total)` pair Git appends to counted phases.
fn parse_counts(line: &str) -> (Option<u64>, Option<u64>) {
    let Some(open) = line.find('(') else {
        return (None, None);
    };
    let Some(close) = line[open..].find(')') else {
        return (None, None);
    };
    let inner = &line[open + 1..open + close];
    let mut parts = inner.split('/');
    let (Some(received), Some(total), None) = (parts.next(), parts.next(), parts.next()) else {
        return (None, None);
    };

    (
        received.trim().parse::<u64>().ok(),
        total.trim().parse::<u64>().ok(),
    )
}
