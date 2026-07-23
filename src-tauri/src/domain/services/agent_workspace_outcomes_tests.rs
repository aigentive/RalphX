use super::agent_workspace_outcomes::*;
use std::sync::{Arc, RwLock};

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, ProjectId, TaskOutcome, TaskOutcomeId,
    TaskOutcomeStatus,
};
use crate::domain::entities::{AgentRunStatus, ChatConversationId, IdeationAnalysisBaseRefKind};
use crate::domain::repositories::{
    resolve_task_outcome_upsert, TaskOutcomeListOptions, TaskOutcomeRepository,
    UpsertTaskOutcomeInput, WORKSPACE_PUBLISH_FAILED_CLASS, WORKSPACE_SESSION_ABANDONED_CLASS,
};
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::Utc;

#[derive(Default)]
struct TestTaskOutcomeRepository {
    rows: RwLock<Vec<TaskOutcome>>,
}

#[async_trait]
impl TaskOutcomeRepository for TestTaskOutcomeRepository {
    async fn upsert(&self, input: UpsertTaskOutcomeInput) -> AppResult<TaskOutcome> {
        let mut rows = self.rows.write().unwrap();
        let existing_index = rows.iter().position(|row| {
            row.project_id == input.outcome.project_id
                && row.source == input.outcome.source
                && row.source_ref_kind == input.outcome.source_ref_kind
                && row.source_ref_id == input.outcome.source_ref_id
        });
        let existing = existing_index.map(|index| &rows[index]);
        let resolution = resolve_task_outcome_upsert(existing, input.outcome);
        if let Some(index) = existing_index {
            if resolution.should_write {
                let mut outcome = resolution.outcome;
                outcome.updated_at = Utc::now();
                rows[index] = outcome;
            }
            return Ok(rows[index].clone());
        }
        rows.push(resolution.outcome.clone());
        Ok(resolution.outcome)
    }

    async fn get_by_dedupe(
        &self,
        project_id: &ProjectId,
        source: &str,
        source_ref_kind: &str,
        source_ref_id: &str,
    ) -> AppResult<Option<TaskOutcome>> {
        Ok(self
            .rows
            .read()
            .unwrap()
            .iter()
            .find(|row| {
                &row.project_id == project_id
                    && row.source == source
                    && row.source_ref_kind == source_ref_kind
                    && row.source_ref_id == source_ref_id
            })
            .cloned())
    }

    async fn get_by_id(&self, id: &TaskOutcomeId) -> AppResult<Option<TaskOutcome>> {
        Ok(self
            .rows
            .read()
            .unwrap()
            .iter()
            .find(|row| row.id.as_str() == id.as_str())
            .cloned())
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: TaskOutcomeListOptions,
    ) -> AppResult<Vec<TaskOutcome>> {
        Ok(self
            .rows
            .read()
            .unwrap()
            .iter()
            .filter(|row| &row.project_id == project_id)
            .filter(|row| {
                options
                    .source
                    .as_deref()
                    .is_none_or(|source| row.source == source)
            })
            .filter(|row| options.status.is_none_or(|status| row.status == status))
            .cloned()
            .collect())
    }
}

fn workspace() -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("11111111-1111-1111-1111-111111111111".to_string()),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        "agent/conversation-1".to_string(),
        "/tmp/worktree".to_string(),
    )
}

#[tokio::test]
async fn records_direct_workspace_turn_with_conversation_dedupe() {
    let repo = Arc::new(TestTaskOutcomeRepository::default());
    let adapter = AgentWorkspaceOutcomeAdapter::new(repo.clone());
    let workspace = workspace();
    let mut run = AgentRun::new(workspace.conversation_id.clone());
    run.status = AgentRunStatus::Completed;

    let first = adapter
        .record_turn_with_code_changes(&workspace, Some(&run))
        .await
        .expect("record direct workspace outcome");
    let second = adapter
        .record_turn_with_code_changes(&workspace, Some(&run))
        .await
        .expect("upsert direct workspace outcome");
    let outcomes = repo
        .list_by_project(
            &workspace.project_id,
            TaskOutcomeListOptions {
                source: Some(AGENT_WORKSPACE_OUTCOME_SOURCE.to_string()),
                status: Some(TaskOutcomeStatus::Eligible),
            },
        )
        .await
        .expect("list outcomes");

    assert_eq!(first.source_ref_kind, "conversation");
    assert_eq!(first.source_ref_id, "11111111-1111-1111-1111-111111111111");
    assert_eq!(second.source_ref_id, "11111111-1111-1111-1111-111111111111");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].outcome_class.as_deref(),
        Some("workspace_code_changes")
    );
}

#[tokio::test]
async fn records_pr_terminal_with_stable_pull_request_key() {
    let repo = Arc::new(TestTaskOutcomeRepository::default());
    let adapter = AgentWorkspaceOutcomeAdapter::new(repo.clone());
    let mut workspace = workspace();
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_url = Some("https://example.test/pr/42".to_string());
    let event = AgentConversationWorkspacePublicationEvent::new(
        workspace.conversation_id.clone(),
        "pr_terminal",
        "merged",
        "PR merged",
        Some("github_pr_terminal:42:merged".to_string()),
    );

    let outcome = adapter
        .record_pr_terminal(&workspace, Some(&event), 42, "merged", None, "PR merged")
        .await
        .expect("record terminal pr outcome");

    assert_eq!(outcome.source, AGENT_WORKSPACE_PR_OUTCOME_SOURCE);
    assert_eq!(outcome.source_ref_kind, "pull_request");
    assert_eq!(outcome.source_ref_id, "42:terminal");
    assert_eq!(outcome.status, TaskOutcomeStatus::Succeeded);
}

#[tokio::test]
async fn terminal_pr_retries_converge_without_stale_downgrade() {
    let repo = Arc::new(TestTaskOutcomeRepository::default());
    let adapter = AgentWorkspaceOutcomeAdapter::new(repo.clone());
    let mut workspace = workspace();
    workspace.publication_pr_number = Some(7);

    let closed = adapter
        .record_pr_terminal(
            &workspace,
            None,
            42,
            "closed",
            Some(WORKSPACE_TERMINAL_REASON_USER_CLOSED),
            "PR closed",
        )
        .await
        .expect("record closed outcome");
    let merged = adapter
        .record_pr_terminal(&workspace, None, 42, "merged", None, "PR merged")
        .await
        .expect("upgrade merged outcome");
    let stale = adapter
        .record_pr_terminal(
            &workspace,
            None,
            42,
            "closed",
            Some(WORKSPACE_TERMINAL_REASON_ARCHIVE_CLOSED),
            "stale close",
        )
        .await
        .expect("ignore stale close");
    let outcomes = repo
        .list_by_project(&workspace.project_id, TaskOutcomeListOptions::default())
        .await
        .expect("list outcomes");

    assert_eq!(closed.source_ref_id, "42:terminal");
    assert_eq!(merged.id.as_str(), closed.id.as_str());
    assert_eq!(stale.id.as_str(), merged.id.as_str());
    assert_eq!(stale.outcome_class, merged.outcome_class);
    assert_eq!(stale.status, merged.status);
    assert_eq!(stale.evidence_json, merged.evidence_json);
    assert_eq!(stale.updated_at, merged.updated_at);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].pull_request_id.as_deref(), Some("42"));
    assert_eq!(
        outcomes[0].outcome_class.as_deref(),
        Some("workspace_pr_merged")
    );
    assert_eq!(outcomes[0].status, TaskOutcomeStatus::Succeeded);
}

#[tokio::test]
async fn terminal_pr_retry_preserves_exact_close_reason_when_later_observation_is_generic() {
    let repo = Arc::new(TestTaskOutcomeRepository::default());
    let adapter = AgentWorkspaceOutcomeAdapter::new(repo.clone());
    let mut workspace = workspace();
    workspace.publication_pr_number = Some(42);

    let first = adapter
        .record_pr_terminal(
            &workspace,
            None,
            42,
            "closed",
            Some(WORKSPACE_TERMINAL_REASON_USER_CLOSED),
            "PR closed",
        )
        .await
        .expect("record closed outcome");
    let retried = adapter
        .record_pr_terminal(&workspace, None, 42, "closed", None, "PR already closed")
        .await
        .expect("retry closed outcome");
    let outcomes = repo
        .list_by_project(&workspace.project_id, TaskOutcomeListOptions::default())
        .await
        .expect("list outcomes");

    assert_eq!(retried.id, first.id);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].evidence_json["reason"],
        WORKSPACE_TERMINAL_REASON_USER_CLOSED
    );
    assert_eq!(outcomes[0].evidence_json["summary"], "PR already closed");
}

#[tokio::test]
async fn no_pr_terminal_outcomes_replace_the_conversation_row_and_link_the_agent_run() {
    let repo = Arc::new(TestTaskOutcomeRepository::default());
    let adapter = AgentWorkspaceOutcomeAdapter::new(repo.clone());
    let workspace = workspace();
    let mut run = AgentRun::new(workspace.conversation_id.clone());
    run.status = AgentRunStatus::Failed;

    adapter
        .record_turn_with_code_changes(&workspace, Some(&run))
        .await
        .expect("record initial code changes");
    let abandoned = adapter
        .record_no_pr_terminal(&workspace, Some(&run), "no_changes", "Nothing to publish")
        .await
        .expect("record abandonment");
    let publish_failed = adapter
        .record_no_pr_terminal(
            &workspace,
            Some(&run),
            WORKSPACE_TERMINAL_REASON_PUBLISH_FAILED,
            "Publication failed",
        )
        .await
        .expect("upgrade current conversation outcome");
    let outcomes = repo
        .list_by_project(
            &workspace.project_id,
            TaskOutcomeListOptions {
                source: Some(AGENT_WORKSPACE_OUTCOME_SOURCE.to_string()),
                ..TaskOutcomeListOptions::default()
            },
        )
        .await
        .expect("list workspace outcomes");

    assert_eq!(abandoned.source_ref_kind, "conversation");
    assert_eq!(abandoned.status, TaskOutcomeStatus::Failed);
    assert_eq!(
        abandoned.outcome_class.as_deref(),
        Some(WORKSPACE_SESSION_ABANDONED_CLASS)
    );
    assert_eq!(publish_failed.id, abandoned.id);
    assert_eq!(publish_failed.status, TaskOutcomeStatus::Failed);
    assert_eq!(
        publish_failed.outcome_class.as_deref(),
        Some(WORKSPACE_PUBLISH_FAILED_CLASS)
    );
    assert_eq!(publish_failed.agent_run_id, Some(run.id.as_str()));
    assert_eq!(
        publish_failed.evidence_json["reason"],
        WORKSPACE_TERMINAL_REASON_PUBLISH_FAILED
    );
    assert_eq!(outcomes.len(), 1);
}
