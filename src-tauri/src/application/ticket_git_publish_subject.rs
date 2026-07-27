use super::{TicketGitPublishFailure, TicketGitPublishFailureKind};

const DYNAMIC_SUMMARY_MARKER: &str = ":summary:";

pub fn frozen_commit_subject_matches(
    rule: &str,
    actual_subject: &str,
) -> Result<bool, TicketGitPublishFailure> {
    validate_frozen_commit_rule(rule)?;
    if actual_subject.trim().is_empty()
        || actual_subject
            .chars()
            .any(|character| character.is_control())
    {
        return Ok(false);
    }
    let Some((prefix, suffix)) = rule.split_once(DYNAMIC_SUMMARY_MARKER) else {
        return Ok(actual_subject == rule);
    };
    if actual_subject.len() < prefix.len() + suffix.len()
        || !actual_subject.starts_with(prefix)
        || !actual_subject.ends_with(suffix)
    {
        return Ok(false);
    }
    let dynamic_end = actual_subject.len() - suffix.len();
    Ok(!actual_subject[prefix.len()..dynamic_end].trim().is_empty())
}

pub fn render_frozen_commit_subject(
    rule: &str,
    summary: &str,
) -> Result<String, TicketGitPublishFailure> {
    validate_frozen_commit_rule(rule)?;
    if !rule.contains(DYNAMIC_SUMMARY_MARKER) {
        return Ok(rule.to_string());
    }
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.is_empty() {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::InvalidFrozenPolicy,
            "Strict ticket commit rule requires a non-empty automatic summary",
        )
        .for_commit_rule(rule));
    }
    Ok(rule.replacen(DYNAMIC_SUMMARY_MARKER, &summary, 1))
}

pub(super) fn validate_frozen_commit_rule(rule: &str) -> Result<(), TicketGitPublishFailure> {
    let invalid = rule.trim().is_empty()
        || rule.chars().any(|character| character.is_control())
        || rule.matches(DYNAMIC_SUMMARY_MARKER).count() > 1;
    if invalid {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::InvalidFrozenPolicy,
            "Frozen strict ticket commit subject rule is invalid",
        )
        .for_commit_rule(rule));
    }
    Ok(())
}
