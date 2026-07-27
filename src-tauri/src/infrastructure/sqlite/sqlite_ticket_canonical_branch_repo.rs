use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};

use crate::domain::entities::{
    ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState,
    TicketCanonicalBranchPolicyKind,
};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::error::{AppError, AppResult};

#[path = "sqlite_ticket_canonical_branch_repo_connection.rs"]
mod connection;
#[path = "sqlite_ticket_canonical_branch_repo_mapping.rs"]
mod mapping;

pub use connection::SqliteTicketCanonicalBranchRepository;
use mapping::{branch_name_conflict, immutable_binding_conflict, row_to_branch};

#[cfg(test)]
#[path = "sqlite_ticket_canonical_branch_strict_repo_tests.rs"]
mod strict_tests;
#[cfg(test)]
#[path = "sqlite_ticket_canonical_branch_repo_tests.rs"]
mod tests;

#[async_trait]
impl TicketCanonicalBranchRepository for SqliteTicketCanonicalBranchRepository {
    async fn get(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<Option<TicketCanonicalBranch>> {
        let project_id = project_id.as_str().to_string();
        let provider = provider.to_string();
        let issue_key = issue_key.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM ticket_canonical_branches
                     WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![project_id, provider, issue_key],
                    row_to_branch,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn get_by_branch_name(
        &self,
        project_id: &ProjectId,
        branch_name: &str,
    ) -> AppResult<Option<TicketCanonicalBranch>> {
        let project_id = project_id.as_str().to_string();
        let branch_name = branch_name.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM ticket_canonical_branches
                     WHERE project_id = ?1 AND branch_name = ?2",
                    params![project_id, branch_name],
                    row_to_branch,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn upsert(&self, branch: TicketCanonicalBranch) -> AppResult<TicketCanonicalBranch> {
        if branch.policy_kind != TicketCanonicalBranchPolicyKind::LegacyCanonicalBase {
            return Err(AppError::Conflict(
                "Strict ticket bindings must be created with create_if_absent".to_string(),
            ));
        }
        branch.validate_policy().map_err(AppError::Validation)?;
        let fetch_project_id = branch.project_id.as_str().to_string();
        let fetch_provider = branch.provider.clone();
        let fetch_issue_key = branch.issue_key.clone();
        self.db
            .run(move |conn| {
                let existing = conn
                    .query_row(
                        "SELECT * FROM ticket_canonical_branches
                         WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                        params![fetch_project_id, fetch_provider, fetch_issue_key],
                        row_to_branch,
                    )
                    .optional()?;
                if let Some(existing) = existing.as_ref().filter(|stored| {
                    stored.policy_kind == TicketCanonicalBranchPolicyKind::StrictGitConvention
                }) {
                    return Err(immutable_binding_conflict(existing));
                }
                let collision = conn
                    .query_row(
                        "SELECT provider, issue_key FROM ticket_canonical_branches
                         WHERE project_id = ?1 AND branch_name = ?2
                           AND NOT (provider = ?3 AND issue_key = ?4)",
                        params![
                            branch.project_id.as_str(),
                            &branch.branch_name,
                            &branch.provider,
                            &branch.issue_key,
                        ],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;
                if let Some((provider, issue_key)) = collision {
                    return Err(branch_name_conflict(&branch, &provider, &issue_key));
                }
                let updated_at = Utc::now().to_rfc3339();
                let insert_or_update = conn.execute(
                    "INSERT INTO ticket_canonical_branches (
                        project_id, provider, issue_key, branch_name, base_branch, base_commit,
                        origin_pushed, terminal, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                    ON CONFLICT(project_id, provider, issue_key) DO UPDATE SET
                        branch_name=excluded.branch_name,
                        base_branch=excluded.base_branch,
                        base_commit=excluded.base_commit,
                        origin_pushed=excluded.origin_pushed,
                        terminal=excluded.terminal,
                        updated_at=excluded.updated_at
                     WHERE ticket_canonical_branches.policy_kind = 'legacy_canonical_base'",
                    params![
                        branch.project_id.as_str(),
                        &branch.provider,
                        &branch.issue_key,
                        &branch.branch_name,
                        &branch.base_branch,
                        branch.base_commit.as_deref(),
                        branch.origin_pushed,
                        branch.terminal,
                        branch.created_at.to_rfc3339(),
                        updated_at,
                    ],
                );
                if let Err(error) = insert_or_update {
                    let collision = conn
                        .query_row(
                            "SELECT provider, issue_key FROM ticket_canonical_branches
                             WHERE project_id = ?1 AND branch_name = ?2",
                            params![branch.project_id.as_str(), &branch.branch_name],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )
                        .optional()?;
                    return match collision {
                        Some((provider, issue_key))
                            if provider != branch.provider || issue_key != branch.issue_key =>
                        {
                            Err(branch_name_conflict(&branch, &provider, &issue_key))
                        }
                        _ => Err(AppError::from(error)),
                    };
                }
                let stored = conn.query_row(
                    "SELECT * FROM ticket_canonical_branches
                         WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![fetch_project_id, fetch_provider, fetch_issue_key],
                    row_to_branch,
                )?;
                if stored.policy_kind == TicketCanonicalBranchPolicyKind::StrictGitConvention {
                    return Err(immutable_binding_conflict(&stored));
                }
                Ok(stored)
            })
            .await
    }

    async fn create_if_absent(
        &self,
        branch: TicketCanonicalBranch,
    ) -> AppResult<TicketCanonicalBranch> {
        branch.validate_policy().map_err(AppError::Validation)?;
        let fetch_project_id = branch.project_id.as_str().to_string();
        let fetch_provider = branch.provider.clone();
        let fetch_issue_key = branch.issue_key.clone();
        self.db
            .run(move |conn| {
                let transaction = conn.unchecked_transaction()?;
                let (policy_version, task_title, username, commit_rule, pr_title) = branch
                    .strict_policy
                    .as_ref()
                    .map(|policy| {
                        (
                            Some(policy.policy_version),
                            Some(policy.task_title.as_str()),
                            policy.username.as_deref(),
                            Some(policy.commit_subject_rule.as_str()),
                            Some(policy.pr_title.as_str()),
                        )
                    })
                    .unwrap_or((None, None, None, None, None));
                let cycle_started_at = branch.cycle.started_at.as_ref().map(DateTime::to_rfc3339);
                let cycle_terminal_at = branch.cycle.terminal_at.as_ref().map(DateTime::to_rfc3339);
                transaction.execute(
                    "INSERT OR IGNORE INTO ticket_canonical_branches (
                        project_id, provider, issue_key, branch_name, base_branch, base_commit,
                        origin_pushed, terminal, policy_kind, policy_version,
                        task_title_snapshot, clickup_username_snapshot, commit_subject_rule,
                        pr_title_snapshot, cycle_generation, cycle_state, cycle_base_commit,
                        cycle_effective_merge_base, cycle_started_at, cycle_terminal_at,
                        created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                        ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22
                    )",
                    params![
                        branch.project_id.as_str(),
                        &branch.provider,
                        &branch.issue_key,
                        &branch.branch_name,
                        &branch.base_branch,
                        branch.base_commit.as_deref(),
                        branch.origin_pushed,
                        branch.terminal,
                        branch.policy_kind.to_string(),
                        policy_version,
                        task_title,
                        username,
                        commit_rule,
                        pr_title,
                        branch.cycle.generation,
                        branch.cycle.state.to_string(),
                        branch.cycle.base_commit.as_deref(),
                        branch.cycle.effective_merge_base.as_deref(),
                        cycle_started_at,
                        cycle_terminal_at,
                        branch.created_at.to_rfc3339(),
                        branch.updated_at.to_rfc3339(),
                    ],
                )?;

                let stored = transaction
                    .query_row(
                        "SELECT * FROM ticket_canonical_branches
                         WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                        params![fetch_project_id, fetch_provider, fetch_issue_key],
                        row_to_branch,
                    )
                    .optional()?;
                if let Some(stored) = stored {
                    transaction.commit()?;
                    return Ok(stored);
                }

                let collision = transaction
                    .query_row(
                        "SELECT provider, issue_key FROM ticket_canonical_branches
                         WHERE project_id = ?1 AND branch_name = ?2",
                        params![branch.project_id.as_str(), &branch.branch_name],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;
                match collision {
                    Some((provider, issue_key)) => {
                        Err(branch_name_conflict(&branch, &provider, &issue_key))
                    }
                    None => Err(AppError::Database(
                        "Ticket binding insert was ignored without a conflicting row".to_string(),
                    )),
                }
            })
            .await
    }

    async fn compare_and_swap_cycle(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
        expected_generation: i64,
        expected_state: TicketCanonicalBranchCycleState,
        replacement: TicketCanonicalBranchCycle,
    ) -> AppResult<bool> {
        replacement
            .validate_replacement(expected_generation)
            .map_err(AppError::Validation)?;
        let project_id = project_id.as_str().to_string();
        let provider = provider.to_string();
        let issue_key = issue_key.to_string();
        self.db
            .run(move |conn| {
                let cycle_started_at = replacement.started_at.as_ref().map(DateTime::to_rfc3339);
                let cycle_terminal_at = replacement.terminal_at.as_ref().map(DateTime::to_rfc3339);
                let affected = conn.execute(
                    "UPDATE ticket_canonical_branches
                     SET cycle_generation = ?1, cycle_state = ?2, cycle_base_commit = ?3,
                         cycle_effective_merge_base = ?4, cycle_started_at = ?5,
                         cycle_terminal_at = ?6, updated_at = ?7
                     WHERE project_id = ?8 AND provider = ?9 AND issue_key = ?10
                       AND policy_kind = 'strict_git_convention'
                       AND cycle_generation = ?11 AND cycle_state = ?12",
                    params![
                        replacement.generation,
                        replacement.state.to_string(),
                        replacement.base_commit,
                        replacement.effective_merge_base,
                        cycle_started_at,
                        cycle_terminal_at,
                        Utc::now().to_rfc3339(),
                        project_id,
                        provider,
                        issue_key,
                        expected_generation,
                        expected_state.to_string(),
                    ],
                )?;
                Ok(affected == 1)
            })
            .await
    }

    async fn mark_origin_pushed(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()> {
        let project_id = project_id.as_str().to_string();
        let provider = provider.to_string();
        let issue_key = issue_key.to_string();
        self.db
            .run(move |conn| {
                let existing = conn
                    .query_row(
                        "SELECT * FROM ticket_canonical_branches
                         WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                        params![project_id, provider, issue_key],
                        row_to_branch,
                    )
                    .optional()?;
                if existing.is_none() {
                    return Ok(());
                }
                let updated_at = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE ticket_canonical_branches
                     SET origin_pushed = 1, updated_at = ?4
                     WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![project_id, provider, issue_key, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn mark_terminal(
        &self,
        project_id: &ProjectId,
        provider: &str,
        issue_key: &str,
    ) -> AppResult<()> {
        let project_id = project_id.as_str().to_string();
        let provider = provider.to_string();
        let issue_key = issue_key.to_string();
        self.db
            .run(move |conn| {
                let existing = conn
                    .query_row(
                        "SELECT * FROM ticket_canonical_branches
                         WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                        params![project_id, provider, issue_key],
                        row_to_branch,
                    )
                    .optional()?;
                match existing.map(|branch| branch.policy_kind) {
                    None => return Ok(()),
                    Some(TicketCanonicalBranchPolicyKind::StrictGitConvention) => {
                        return Err(AppError::Conflict(
                            "Strict ticket bindings use per-cycle state instead of terminal"
                                .to_string(),
                        ))
                    }
                    Some(TicketCanonicalBranchPolicyKind::LegacyCanonicalBase) => {}
                }
                let updated_at = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE ticket_canonical_branches
                     SET terminal = 1, updated_at = ?4
                     WHERE project_id = ?1 AND provider = ?2 AND issue_key = ?3",
                    params![project_id, provider, issue_key, updated_at],
                )?;
                Ok(())
            })
            .await
    }
}
