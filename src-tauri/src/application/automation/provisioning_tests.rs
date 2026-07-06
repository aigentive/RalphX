use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;

use super::provisioning::{
    AutomationRunProvisioner, AutomationRunStartOutcome, AutomationRunStartRequest,
    AutomationRunStarter,
};
use super::transition::NoopAutomationEventEmitter;
use crate::domain::agents::LogicalEffort;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Automation, AutomationId,
    AutomationJudgeState, AutomationPromptAuthor, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatContextType, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AutomationRepository, AutomationRunRepository,
    ChatConversationRepository,
};
use crate::domain::services::{
    ComposerArtifactReference, ComposerIntegrationReference, ComposerProjectReference,
    ComposerProjectReferenceKind,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAutomationRepository,
    MemoryAutomationRunRepository, MemoryChatConversationRepository,
};

fn automation(id: &str) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: "Large migration".to_string(),
        status: AutomationStatus::Active,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Implement all items".to_string(),
        setup_conversation_id: None,
        provider_harness: "codex".to_string(),
        model_id: "gpt-5.4".to_string(),
        logical_effort: Some("high".to_string()),
        run_mode: "edit".to_string(),
        base_ref_kind: "local_branch".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("main".to_string()),
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Build the first PR".to_string()),
        setup_analysis_summary: None,
        created_at: now,
        updated_at: now,
    }
}

fn run(automation_id: AutomationId) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string("run-1"),
        automation_id,
        run_index: 1,
        status: AutomationRunStatus::Pending,
        judge_state: AutomationJudgeState::None,
        judge_lease_expires_at: None,
        conversation_id: None,
        run_prompt: "Build the first PR".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "feature/base".to_string(),
        base_from_run_id: None,
        branch_name: None,
        pr_number: None,
        pr_url: None,
        pr_title: None,
        pr_head_ref_name: None,
        pr_base_ref_name: None,
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: None,
        judge_verdict_json: None,
        judge_model_id: None,
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: None,
        finished_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[derive(Clone)]
struct RecordingStarter {
    requests: Arc<Mutex<Vec<AutomationRunStartRequest>>>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
}

impl RecordingStarter {
    fn new(workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            workspace_repo,
        }
    }

    fn requests(&self) -> Vec<AutomationRunStartRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl AutomationRunStarter for RecordingStarter {
    async fn start_run(
        &self,
        request: AutomationRunStartRequest,
    ) -> AppResult<AutomationRunStartOutcome> {
        self.requests.lock().unwrap().push(request.clone());
        let workspace = AgentConversationWorkspace::new(
            request.conversation_id.clone(),
            ProjectId::from_string(request.project_id.clone()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::LocalBranch,
            request.base_ref.clone(),
            request.base_display_name.clone(),
            None,
            "ralphx/automation-run-1".to_string(),
            "/tmp/ralphx/automation-run-1".to_string(),
        );
        self.workspace_repo.create_or_update(workspace).await?;
        Ok(AutomationRunStartOutcome {
            branch_name: Some("ralphx/automation-run-1".to_string()),
        })
    }
}

struct FailingStarter;

#[async_trait]
impl AutomationRunStarter for FailingStarter {
    async fn start_run(
        &self,
        _request: AutomationRunStartRequest,
    ) -> AppResult<AutomationRunStartOutcome> {
        Err(AppError::Validation("starter failed".to_string()))
    }
}

#[test]
fn automation_run_start_request_maps_to_manual_start_input() {
    let mut automation = automation("automation-1");
    automation.base_source_pull_request_json = Some(
        r#"{"number":42,"url":"https://github.test/pull/42","title":"Base PR","headRefName":"feature/base","baseRefName":"main","headRefOid":"abc123"}"#
            .to_string(),
    );
    let run = run(automation.id.clone());
    let conversation_id = "11111111-1111-4111-8111-111111111111";
    let mut request = AutomationRunStartRequest::from_automation_run(
        &automation,
        &run,
        crate::domain::entities::ChatConversationId::from_string(conversation_id),
    );
    request.composer_project_references = vec![ComposerProjectReference {
        path: "docs/spec.md".to_string(),
        kind: Some(ComposerProjectReferenceKind::File),
    }];
    request.composer_integration_references = vec![ComposerIntegrationReference {
        provider: "linear".to_string(),
        kind: "linear".to_string(),
        id: "LIN-1".to_string(),
        key: Some("LIN-1".to_string()),
        title: Some("Migration task".to_string()),
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }];
    request.composer_artifact_references = vec![ComposerArtifactReference {
        artifact_id: "artifact-1".to_string(),
        kind: "plan".to_string(),
        title: Some("Plan".to_string()),
        session_id: None,
        version: Some(1),
        status: None,
    }];

    let input = request.into_start_input().unwrap();

    assert_eq!(input.project_id, "project-1");
    assert_eq!(input.content, "Build the first PR");
    assert_eq!(input.conversation_id.as_deref(), Some(conversation_id));
    assert_eq!(input.provider_harness.as_deref(), Some("codex"));
    assert_eq!(input.model_override.as_deref(), Some("gpt-5.4"));
    assert_eq!(input.logical_effort, Some(LogicalEffort::High));
    assert_eq!(input.mode.as_deref(), Some("edit"));
    assert_eq!(input.base_ref_kind.as_deref(), Some("local_branch"));
    assert_eq!(input.base_branch_mode.as_deref(), Some("isolated"));
    assert_eq!(input.base_ref.as_deref(), Some("feature/base"));
    assert_eq!(input.base_display_name.as_deref(), Some("main"));
    assert_eq!(
        input
            .base_source_pull_request
            .as_ref()
            .map(|source| source.number),
        Some(42)
    );
    assert_eq!(input.composer_project_references.len(), 1);
    assert_eq!(input.composer_integration_references.len(), 1);
    assert_eq!(input.composer_artifact_references.len(), 1);
}

#[test]
fn automation_run_start_request_trims_optional_fields_and_rejects_invalid_values() {
    let automation = automation("automation-1");
    let run = run(automation.id.clone());
    let mut request = AutomationRunStartRequest::from_automation_run(
        &automation,
        &run,
        crate::domain::entities::ChatConversationId::from_string(
            "33333333-3333-4333-8333-333333333333",
        ),
    );
    request.provider_harness = "  ".to_string();
    request.model_id = "  ".to_string();
    request.logical_effort = Some("  ".to_string());
    request.run_mode = "  ".to_string();
    request.base_ref_kind = "  ".to_string();
    request.base_ref = "  ".to_string();
    request.base_display_name = Some("  ".to_string());

    let input = request.clone().into_start_input().unwrap();

    assert!(input.provider_harness.is_none());
    assert!(input.model_override.is_none());
    assert!(input.logical_effort.is_none());
    assert!(input.mode.is_none());
    assert!(input.base_ref_kind.is_none());
    assert!(input.base_ref.is_none());
    assert!(input.base_display_name.is_none());

    request.logical_effort = Some("impossible".to_string());
    assert!(matches!(
        request.clone().into_start_input().unwrap_err(),
        AppError::Validation(_)
    ));

    request.logical_effort = None;
    request.base_source_pull_request_json = Some("{not-json".to_string());
    assert!(matches!(
        request.into_start_input().unwrap_err(),
        AppError::Validation(_)
    ));
}

#[test]
fn automation_run_start_request_drops_source_pr_after_run_one() {
    let mut automation = automation("automation-1");
    automation.base_display_name = Some("Source PR #42".to_string());
    automation.base_source_pull_request_json = Some(
        r#"{"number":42,"url":"https://github.test/pull/42","title":"Base PR","headRefName":"feature/base","baseRefName":"main","headRefOid":"abc123"}"#
            .to_string(),
    );
    let mut run = run(automation.id.clone());
    run.run_index = 2;
    run.base_ref_kind = "local_branch".to_string();
    run.base_ref_used = "release/2026".to_string();
    let request = AutomationRunStartRequest::from_automation_run(
        &automation,
        &run,
        crate::domain::entities::ChatConversationId::from_string(
            "22222222-2222-4222-8222-222222222222",
        ),
    );

    assert_eq!(request.base_ref_kind, "local_branch");
    assert_eq!(request.base_ref, "release/2026");
    assert_eq!(request.base_display_name.as_deref(), Some("release/2026"));
    assert!(request.base_source_pull_request_json.is_none());

    let input = request.into_start_input().unwrap();
    assert_eq!(input.base_ref_kind.as_deref(), Some("local_branch"));
    assert_eq!(input.base_ref.as_deref(), Some("release/2026"));
    assert_eq!(input.base_display_name.as_deref(), Some("release/2026"));
    assert!(input.base_source_pull_request.is_none());
}

#[tokio::test]
async fn provision_first_run_noops_or_rejects_when_not_ready() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let existing_automation = automation("automation-1");
    automation_repo
        .create(existing_automation.clone())
        .await
        .unwrap();
    run_repo
        .create_run(run(existing_automation.id.clone()))
        .await
        .unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo.clone(),
        conversation_repo.clone(),
        workspace_repo.clone(),
        Arc::new(starter.clone()),
        Arc::new(NoopAutomationEventEmitter),
    );

    assert!(provisioner
        .provision_first_run(&existing_automation)
        .await
        .unwrap()
        .is_none());
    assert!(starter.requests().is_empty());

    let mut missing_prompt = automation("automation-2");
    missing_prompt.first_run_prompt = Some("   ".to_string());
    automation_repo
        .create(missing_prompt.clone())
        .await
        .unwrap();
    let error = provisioner
        .provision_first_run(&missing_prompt)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));
}

#[tokio::test]
async fn provision_first_run_creates_owned_draft_and_marks_workspace_for_initial_publish() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let automation = automation("automation-1");
    automation_repo.create(automation.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo,
        run_repo.clone(),
        conversation_repo.clone(),
        workspace_repo.clone(),
        Arc::new(starter.clone()),
        Arc::new(NoopAutomationEventEmitter),
    );

    let started = provisioner
        .provision_first_run(&automation)
        .await
        .unwrap()
        .expect("first run should be provisioned");

    assert_eq!(started.status, AutomationRunStatus::Running);
    assert_eq!(started.run_index, 1);
    assert_eq!(
        started.branch_name.as_deref(),
        Some("ralphx/automation-run-1")
    );
    assert!(started.started_at.is_some());
    let conversation_id = started
        .conversation_id
        .as_ref()
        .expect("run should be linked to a conversation");
    let conversation = conversation_repo
        .get_by_id(conversation_id)
        .await
        .unwrap()
        .expect("draft conversation should exist");
    assert_eq!(conversation.context_type, ChatContextType::Project);
    assert_eq!(conversation.context_id, "project-1");
    assert_eq!(conversation.automation_id, Some(automation.id.clone()));
    assert_eq!(conversation.automation_run_id, Some(started.id.clone()));
    assert_eq!(
        conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Edit)
    );
    assert_eq!(conversation.title.as_deref(), Some("Large migration run 1"));

    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .unwrap()
        .expect("fake starter should create workspace");
    assert!(workspace.auto_publish_initial_pr_enabled);

    let requests = starter.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].conversation_id, *conversation_id);
    assert_eq!(requests[0].run_prompt, "Build the first PR");
    assert_eq!(requests[0].provider_harness, "codex");
    assert_eq!(requests[0].model_id, "gpt-5.4");
    assert_eq!(requests[0].base_ref_kind, "local_branch");
    assert_eq!(requests[0].base_ref, "main");

    let latest = run_repo
        .latest_for_automation(&automation.id)
        .await
        .unwrap()
        .expect("latest run should exist");
    assert_eq!(latest.id, started.id);
    assert_eq!(latest.status, AutomationRunStatus::Running);
}

#[tokio::test]
async fn provision_pending_run_noops_for_non_pending_and_conflicts_on_stale_status() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let automation = automation("automation-1");
    automation_repo.create(automation.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo,
        run_repo.clone(),
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        Arc::new(NoopAutomationEventEmitter),
    );

    let mut running = run(automation.id.clone());
    running.status = AutomationRunStatus::Running;
    assert!(provisioner
        .provision_pending_run(&automation, &running)
        .await
        .unwrap()
        .is_none());

    let mut stored = running.clone();
    stored.id = AutomationRunId::from_string("run-stale");
    run_repo.create_run(stored.clone()).await.unwrap();
    let mut stale_input = stored;
    stale_input.status = AutomationRunStatus::Pending;
    let error = provisioner
        .provision_pending_run(&automation, &stale_input)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(_)));
}

#[tokio::test]
async fn provision_pending_run_marks_agent_failed_when_starter_errors() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation = automation("automation-1");
    automation_repo.create(automation.clone()).await.unwrap();
    let pending = run(automation.id.clone());
    run_repo.create_run(pending.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo,
        run_repo.clone(),
        conversation_repo,
        workspace_repo,
        Arc::new(FailingStarter),
        Arc::new(NoopAutomationEventEmitter),
    );

    let error = provisioner
        .provision_pending_run(&automation, &pending)
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
    let failed = run_repo
        .get_by_id(&pending.id)
        .await
        .unwrap()
        .expect("run should remain persisted");
    assert_eq!(failed.status, AutomationRunStatus::AgentFailed);
    assert_eq!(failed.error_code.as_deref(), Some("start_failed"));
    assert!(failed
        .error_detail
        .as_deref()
        .is_some_and(|detail| detail.contains("starter failed")));
}
