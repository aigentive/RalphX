use std::sync::Arc;

use chrono::Utc;
use serde_json::{json, Value};

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, ProjectId, TaskOutcome, TaskOutcomeId,
    TaskOutcomeStatus,
};
pub use crate::domain::repositories::AGENT_WORKSPACE_PR_OUTCOME_SOURCE;
use crate::domain::repositories::{
    canonical_terminal_pr_source_ref_id, terminal_pr_status_for_class, TaskOutcomeRepository,
    UpsertTaskOutcomeInput, TERMINAL_PR_SOURCE_REF_KIND, WORKSPACE_PR_CLOSED_CLASS,
    WORKSPACE_PR_FAILED_CLASS, WORKSPACE_PR_MERGED_CLASS, WORKSPACE_PR_MERGED_CLEAN_CLASS,
    WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS, WORKSPACE_PR_TERMINAL_CLASS,
};
use crate::error::AppResult;

pub const AGENT_WORKSPACE_OUTCOME_SOURCE: &str = "agent_workspace";
pub const GITHUB_PR_REVIEW_OUTCOME_SOURCE: &str = "github_pr_review";

pub struct AgentWorkspaceOutcomeAdapter {
    outcome_repo: Arc<dyn TaskOutcomeRepository>,
}

impl AgentWorkspaceOutcomeAdapter {
    pub fn new(outcome_repo: Arc<dyn TaskOutcomeRepository>) -> Self {
        Self { outcome_repo }
    }

    pub async fn record_turn_with_code_changes(
        &self,
        workspace: &AgentConversationWorkspace,
        agent_run: Option<&AgentRun>,
    ) -> AppResult<TaskOutcome> {
        let mut evidence = json!({
            "conversation_id": workspace.conversation_id.as_str(),
            "workspace_mode": workspace.mode.to_string(),
            "branch_name": workspace.branch_name,
            "base_ref_kind": workspace.base_ref_kind.to_string(),
            "base_ref": workspace.base_ref,
            "has_uncommitted_changes": true,
        });
        add_agent_run_evidence(&mut evidence, agent_run);

        self.record(WorkspaceOutcomeRecord {
            project_id: workspace.project_id.clone(),
            source: AGENT_WORKSPACE_OUTCOME_SOURCE,
            source_ref_kind: "conversation",
            source_ref_id: workspace.conversation_id.as_str().to_string(),
            conversation_id: Some(workspace.conversation_id.as_str().to_string()),
            agent_run_id: agent_run.map(|run| run.id.as_str().to_string()),
            pull_request_id: None,
            outcome_class: "workspace_code_changes",
            status: TaskOutcomeStatus::Eligible,
            evidence_json: evidence,
            provider_harness: agent_run.and_then(|run| run.harness).map(|h| h.to_string()),
            provider_session_id: agent_run.and_then(|run| run.provider_session_id.clone()),
        })
        .await
    }

    pub async fn record_publish_succeeded(
        &self,
        workspace: &AgentConversationWorkspace,
        event: Option<&AgentConversationWorkspacePublicationEvent>,
        summary: &str,
    ) -> AppResult<TaskOutcome> {
        let (source_ref_kind, source_ref_id) =
            publication_source_ref(workspace, event, "published");
        self.record(WorkspaceOutcomeRecord {
            project_id: workspace.project_id.clone(),
            source: AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            source_ref_kind,
            source_ref_id,
            conversation_id: Some(workspace.conversation_id.as_str().to_string()),
            agent_run_id: None,
            pull_request_id: workspace.publication_pr_number.map(|n| n.to_string()),
            outcome_class: "workspace_pr_published",
            status: TaskOutcomeStatus::Succeeded,
            evidence_json: workspace_publication_evidence(workspace, event, summary),
            provider_harness: None,
            provider_session_id: None,
        })
        .await
    }

    pub async fn record_pr_review_requested_changes(
        &self,
        workspace: &AgentConversationWorkspace,
        event: Option<&AgentConversationWorkspacePublicationEvent>,
        pr_number: i64,
        author: Option<&str>,
        summary: &str,
        classification: Option<&str>,
    ) -> AppResult<TaskOutcome> {
        let (source_ref_kind, source_ref_id) =
            publication_source_ref(workspace, event, "github_review");
        let mut evidence = workspace_publication_evidence(workspace, event, summary);
        merge_object(
            &mut evidence,
            json!({
                "pull_request_number": pr_number,
                "review_author": author,
                "classification": classification,
            }),
        );

        self.record(WorkspaceOutcomeRecord {
            project_id: workspace.project_id.clone(),
            source: GITHUB_PR_REVIEW_OUTCOME_SOURCE,
            source_ref_kind,
            source_ref_id,
            conversation_id: Some(workspace.conversation_id.as_str().to_string()),
            agent_run_id: None,
            pull_request_id: Some(pr_number.to_string()),
            outcome_class: "workspace_pr_changes_requested",
            status: TaskOutcomeStatus::Eligible,
            evidence_json: evidence,
            provider_harness: None,
            provider_session_id: None,
        })
        .await
    }

    pub async fn record_pr_terminal(
        &self,
        workspace: &AgentConversationWorkspace,
        event: Option<&AgentConversationWorkspacePublicationEvent>,
        pr_number: i64,
        terminal_status: &str,
        summary: &str,
    ) -> AppResult<TaskOutcome> {
        let outcome_class = match terminal_status {
            "merged" => WORKSPACE_PR_MERGED_CLASS,
            "merged_clean" => WORKSPACE_PR_MERGED_CLEAN_CLASS,
            "merged_with_followups" => WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS,
            "closed" => WORKSPACE_PR_CLOSED_CLASS,
            "failed" => WORKSPACE_PR_FAILED_CLASS,
            _ => WORKSPACE_PR_TERMINAL_CLASS,
        };
        let pull_request_id = pr_number.to_string();
        let source_ref_id = canonical_terminal_pr_source_ref_id(&pull_request_id);
        let mut evidence = workspace_publication_evidence(workspace, event, summary);
        merge_object(
            &mut evidence,
            json!({
                "pull_request_number": pr_number,
                "terminal_status": terminal_status,
            }),
        );

        self.record(WorkspaceOutcomeRecord {
            project_id: workspace.project_id.clone(),
            source: AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            source_ref_kind: TERMINAL_PR_SOURCE_REF_KIND,
            source_ref_id,
            conversation_id: Some(workspace.conversation_id.as_str().to_string()),
            agent_run_id: None,
            pull_request_id: Some(pull_request_id),
            outcome_class,
            status: terminal_pr_status_for_class(Some(outcome_class)),
            evidence_json: evidence,
            provider_harness: None,
            provider_session_id: None,
        })
        .await
    }

    pub async fn record_stale_publish_repair(
        &self,
        workspace: &AgentConversationWorkspace,
        event: Option<&AgentConversationWorkspacePublicationEvent>,
        summary: &str,
    ) -> AppResult<TaskOutcome> {
        let (source_ref_kind, source_ref_id) =
            publication_source_ref(workspace, event, "stale_publish_repair");
        self.record(WorkspaceOutcomeRecord {
            project_id: workspace.project_id.clone(),
            source: AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            source_ref_kind,
            source_ref_id,
            conversation_id: Some(workspace.conversation_id.as_str().to_string()),
            agent_run_id: None,
            pull_request_id: workspace.publication_pr_number.map(|n| n.to_string()),
            outcome_class: "workspace_pr_stale_repair",
            status: TaskOutcomeStatus::Eligible,
            evidence_json: workspace_publication_evidence(workspace, event, summary),
            provider_harness: None,
            provider_session_id: None,
        })
        .await
    }

    async fn record(&self, input: WorkspaceOutcomeRecord<'_>) -> AppResult<TaskOutcome> {
        let now = Utc::now();
        let outcome = TaskOutcome {
            id: TaskOutcomeId::new(),
            project_id: input.project_id,
            source: input.source.to_string(),
            source_ref_kind: input.source_ref_kind.to_string(),
            source_ref_id: input.source_ref_id,
            task_id: None,
            conversation_id: input.conversation_id,
            agent_run_id: input.agent_run_id,
            pull_request_id: input.pull_request_id,
            proposal_id: None,
            verification_id: None,
            review_id: None,
            outcome_class: Some(input.outcome_class.to_string()),
            status: input.status,
            evidence_json: input.evidence_json,
            provider_harness: input.provider_harness,
            provider_session_id: input.provider_session_id,
            created_at: now,
            updated_at: now,
        };
        self.outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await
    }
}

struct WorkspaceOutcomeRecord<'a> {
    project_id: ProjectId,
    source: &'a str,
    source_ref_kind: &'a str,
    source_ref_id: String,
    conversation_id: Option<String>,
    agent_run_id: Option<String>,
    pull_request_id: Option<String>,
    outcome_class: &'a str,
    status: TaskOutcomeStatus,
    evidence_json: Value,
    provider_harness: Option<String>,
    provider_session_id: Option<String>,
}

fn publication_source_ref(
    workspace: &AgentConversationWorkspace,
    event: Option<&AgentConversationWorkspacePublicationEvent>,
    fallback_suffix: &str,
) -> (&'static str, String) {
    if let Some(event) = event {
        return ("publication_event", event.id.clone());
    }
    (
        "conversation",
        format!("{}:{fallback_suffix}", workspace.conversation_id.as_str()),
    )
}

fn workspace_publication_evidence(
    workspace: &AgentConversationWorkspace,
    event: Option<&AgentConversationWorkspacePublicationEvent>,
    summary: &str,
) -> Value {
    json!({
        "conversation_id": workspace.conversation_id.as_str(),
        "workspace_mode": workspace.mode.to_string(),
        "branch_name": workspace.branch_name,
        "base_ref_kind": workspace.base_ref_kind.to_string(),
        "base_ref": workspace.base_ref,
        "publication_pr_number": workspace.publication_pr_number,
        "publication_pr_url": workspace.publication_pr_url,
        "publication_pr_status": workspace.publication_pr_status,
        "publication_push_status": workspace.publication_push_status,
        "summary": summary,
        "event": event.map(|event| json!({
            "id": event.id,
            "step": event.step,
            "status": event.status,
            "summary": event.summary,
            "classification": event.classification,
            "created_at": event.created_at.to_rfc3339(),
        })),
    })
}

fn add_agent_run_evidence(evidence: &mut Value, agent_run: Option<&AgentRun>) {
    let Some(agent_run) = agent_run else {
        return;
    };
    merge_object(
        evidence,
        json!({
            "agent_run_id": agent_run.id.as_str(),
            "agent_run_status": agent_run.status.to_string(),
            "provider_harness": agent_run.harness.map(|h| h.to_string()),
            "provider_session_id": agent_run.provider_session_id,
        }),
    );
}

fn merge_object(target: &mut Value, additional: Value) {
    let (Some(target), Some(additional)) = (target.as_object_mut(), additional.as_object()) else {
        return;
    };
    for (key, value) in additional {
        target.insert(key.clone(), value.clone());
    }
}

pub fn is_direct_edit_workspace(workspace: &AgentConversationWorkspace) -> bool {
    workspace.mode == AgentConversationWorkspaceMode::Edit && !workspace.is_execution_owned()
}
