use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::types::Type;

use crate::domain::entities::{
    ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState,
    TicketCanonicalBranchPolicyKind, TicketGitConventionSnapshot,
};
use crate::error::AppError;

fn invalid_text(value: &str, detail: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid persisted value '{value}': {detail}"),
        )),
    )
}

fn parse_datetime(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&dt));
    }
    Err(invalid_text(
        value,
        "expected an RFC 3339 timestamp".to_string(),
    ))
}

fn parse_policy_kind(value: &str) -> rusqlite::Result<TicketCanonicalBranchPolicyKind> {
    TicketCanonicalBranchPolicyKind::from_str(value).map_err(|error| invalid_text(value, error))
}

fn parse_cycle_state(value: &str) -> rusqlite::Result<TicketCanonicalBranchCycleState> {
    TicketCanonicalBranchCycleState::from_str(value).map_err(|error| invalid_text(value, error))
}

pub(super) fn row_to_branch(row: &rusqlite::Row<'_>) -> rusqlite::Result<TicketCanonicalBranch> {
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let policy_kind = parse_policy_kind(&row.get::<_, String>("policy_kind")?)?;
    let strict_policy = match policy_kind {
        TicketCanonicalBranchPolicyKind::LegacyCanonicalBase => None,
        TicketCanonicalBranchPolicyKind::StrictGitConvention => Some(TicketGitConventionSnapshot {
            policy_version: row.get("policy_version")?,
            task_title: row.get("task_title_snapshot")?,
            username: row.get("clickup_username_snapshot")?,
            commit_subject_rule: row.get("commit_subject_rule")?,
            pr_title: row.get("pr_title_snapshot")?,
        }),
    };
    let cycle_started_at = row
        .get::<_, Option<String>>("cycle_started_at")?
        .map(|value| parse_datetime(&value))
        .transpose()?;
    let cycle_terminal_at = row
        .get::<_, Option<String>>("cycle_terminal_at")?
        .map(|value| parse_datetime(&value))
        .transpose()?;

    let branch = TicketCanonicalBranch {
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        provider: row.get("provider")?,
        issue_key: row.get("issue_key")?,
        branch_name: row.get("branch_name")?,
        base_branch: row.get("base_branch")?,
        base_commit: row.get("base_commit")?,
        origin_pushed: row.get("origin_pushed")?,
        terminal: row.get("terminal")?,
        policy_kind,
        strict_policy,
        cycle: TicketCanonicalBranchCycle {
            generation: row.get("cycle_generation")?,
            state: parse_cycle_state(&row.get::<_, String>("cycle_state")?)?,
            base_commit: row.get("cycle_base_commit")?,
            effective_merge_base: row.get("cycle_effective_merge_base")?,
            started_at: cycle_started_at,
            terminal_at: cycle_terminal_at,
        },
        created_at: parse_datetime(&created_at)?,
        updated_at: parse_datetime(&updated_at)?,
    };
    branch
        .validate_policy()
        .map_err(|error| invalid_text("ticket_canonical_branches row", error))?;
    Ok(branch)
}

pub(super) fn immutable_binding_conflict(branch: &TicketCanonicalBranch) -> AppError {
    AppError::Conflict(format!(
        "Strict ticket binding {}:{} is immutable",
        branch.provider, branch.issue_key
    ))
}

pub(super) fn branch_name_conflict(
    branch: &TicketCanonicalBranch,
    existing_provider: &str,
    existing_issue_key: &str,
) -> AppError {
    AppError::Conflict(format!(
        "Ticket branch '{}' is already bound to {existing_provider}:{existing_issue_key} in project {}",
        branch.branch_name, branch.project_id
    ))
}
