use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

use super::judge::SPEC_ATTACHMENT_MAX_BYTES;
use super::plan_gate::{
    clear_plan_phase_publication_metadata, AutomationPlanVerificationStartOutcome,
    AutomationPlanVerificationStartRequest, AutomationPlanVerificationStarter,
    AutomationRunResumer, NoopAutomationPlanVerificationStarter, AUTOMATION_PLAN_REMINDER_PROMPT,
    PLAN_JUDGE_FAILED_PAUSED_REASON_CODE, PLAN_RESUME_FAILED_ERROR_CODE,
    PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE,
};
use super::provisioning::{
    AutomationRunStartOutcome, AutomationRunStartRequest, AutomationRunStarter,
};
use super::scheduler::{
    load_spec_attachment, AutomationJudgeInvocation, AutomationJudgeInvocationOutput,
    AutomationJudgeInvoker, AutomationPlanJudgeInvocation, AutomationPlanJudgeInvocationOutput,
    AutomationPlanJudgeInvoker, AutomationScheduler, AutomationSchedulerConfig,
    AutomationSchedulerRegistry, AutomationSignalChecker,
};
use super::transition::NoopAutomationEventEmitter;
use crate::application::plan_artifact_approval::PlanArtifactApprovalWriter;
use crate::application::services::pr_auto_merge_status::{
    auto_merge_enable_failure_summary, AUTO_MERGE_ENABLE_WARNING_CODE,
    AUTO_MERGE_SUPERVISION_STATUS_WAITING,
};
use crate::application::services::pr_merge_poller::sync_agent_workspace_auto_merge_preference_for_workspace;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunStatus, Artifact,
    ArtifactId, ArtifactType, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow,
    ProjectId, VerificationGap, VerificationRunSnapshot, VerificationStatus,
    DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, ArtifactRepository,
    AutomationRepository, AutomationRunRepository, AutomationSettingsPatch,
    IdeationSessionRepository, PlanApprovalActor, PlanArtifactApproval,
    PlanArtifactApprovalRepository,
};
use crate::domain::services::github_service::{PrHealth, PrMergeableState, PrStatus, PrSyncState};
use crate::domain::services::GithubServiceTrait;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::AutomationsRuntimeConfig;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository, MemoryArtifactRepository,
    MemoryAutomationRepository, MemoryAutomationRunRepository, MemoryChatConversationRepository,
    MemoryIdeationSessionRepository, MemoryPlanArtifactApprovalRepository,
};
use crate::tests::mock_github_service::MockGithubService;

#[derive(Default)]
struct RecordingStarter;

#[async_trait]
impl AutomationRunStarter for RecordingStarter {
    async fn start_run(
        &self,
        _request: AutomationRunStartRequest,
    ) -> AppResult<AutomationRunStartOutcome> {
        Ok(AutomationRunStartOutcome {
            branch_name: Some("ralphx/automation-run-1".to_string()),
        })
    }
}

#[derive(Default)]
struct RecordingPlanVerificationStarter {
    calls: Mutex<Vec<AutomationPlanVerificationStartRequest>>,
    responses: Mutex<VecDeque<AppResult<AutomationPlanVerificationStartOutcome>>>,
    reviewing_session_repo: Option<Arc<MemoryIdeationSessionRepository>>,
}

impl RecordingPlanVerificationStarter {
    fn with_outcomes(outcomes: Vec<AutomationPlanVerificationStartOutcome>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from(
                outcomes.into_iter().map(Ok).collect::<Vec<_>>(),
            )),
            reviewing_session_repo: None,
        }
    }

    fn with_errors(errors: Vec<&str>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from(
                errors
                    .into_iter()
                    .map(|error| Err(AppError::Infrastructure(error.to_string())))
                    .collect::<Vec<_>>(),
            )),
            reviewing_session_repo: None,
        }
    }

    fn with_reviewing_side_effect(session_repo: Arc<MemoryIdeationSessionRepository>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::new()),
            reviewing_session_repo: Some(session_repo),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn calls(&self) -> Vec<AutomationPlanVerificationStartRequest> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl AutomationPlanVerificationStarter for RecordingPlanVerificationStarter {
    async fn start_verification(
        &self,
        request: AutomationPlanVerificationStartRequest,
    ) -> AppResult<AutomationPlanVerificationStartOutcome> {
        self.calls.lock().unwrap().push(request.clone());
        let response = self.responses.lock().unwrap().pop_front().unwrap_or(Ok(
            AutomationPlanVerificationStartOutcome::Started { generation: 1 },
        ));
        if matches!(
            &response,
            Ok(AutomationPlanVerificationStartOutcome::Started { .. }
                | AutomationPlanVerificationStartOutcome::AlreadyInProgress { .. })
        ) {
            if let Some(repo) = self.reviewing_session_repo.as_ref() {
                repo.update_verification_state(
                    &request.session_id,
                    VerificationStatus::Reviewing,
                    true,
                )
                .await?;
            }
        }
        response
    }
}

#[derive(Default)]
struct RecordingResumer {
    running: Mutex<bool>,
    running_responses: Mutex<VecDeque<bool>>,
    launches_paused: Mutex<bool>,
    prompts: Mutex<Vec<(ChatConversationId, String)>>,
    switches: Mutex<Vec<ChatConversationId>>,
    fail_next_send: Mutex<bool>,
    queue_next_send: Mutex<bool>,
    purged_queued_messages: Mutex<usize>,
}

impl RecordingResumer {
    fn set_running(&self, running: bool) {
        *self.running.lock().unwrap() = running;
    }

    fn set_running_responses(&self, responses: Vec<bool>) {
        *self.running_responses.lock().unwrap() = VecDeque::from(responses);
    }

    fn set_launches_paused(&self, paused: bool) {
        *self.launches_paused.lock().unwrap() = paused;
    }

    fn fail_next_send(&self) {
        *self.fail_next_send.lock().unwrap() = true;
    }

    fn queue_next_send(&self) {
        *self.queue_next_send.lock().unwrap() = true;
    }

    fn prompts(&self) -> Vec<(ChatConversationId, String)> {
        self.prompts.lock().unwrap().clone()
    }

    fn switches(&self) -> Vec<ChatConversationId> {
        self.switches.lock().unwrap().clone()
    }

    fn purged_queued_messages(&self) -> usize {
        *self.purged_queued_messages.lock().unwrap()
    }
}

#[async_trait]
impl AutomationRunResumer for RecordingResumer {
    async fn is_agent_running(&self, _conversation_id: &ChatConversationId) -> AppResult<bool> {
        if let Some(response) = self.running_responses.lock().unwrap().pop_front() {
            return Ok(response);
        }
        Ok(*self.running.lock().unwrap())
    }

    async fn launches_paused(&self) -> AppResult<bool> {
        Ok(*self.launches_paused.lock().unwrap())
    }

    async fn switch_to_edit(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.switches.lock().unwrap().push(conversation_id.clone());
        Ok(())
    }

    async fn resume_with_prompt(
        &self,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<()> {
        self.prompts
            .lock()
            .unwrap()
            .push((conversation_id.clone(), prompt.to_string()));
        if std::mem::take(&mut *self.fail_next_send.lock().unwrap()) {
            return Err(AppError::Infrastructure("send failed".to_string()));
        }
        if std::mem::take(&mut *self.queue_next_send.lock().unwrap()) {
            *self.purged_queued_messages.lock().unwrap() += 1;
            return Err(AppError::Infrastructure(
                "send was queued instead of spawning".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSignalChecker {
    calls: Mutex<Vec<(String, i64)>>,
    responses: Mutex<VecDeque<Result<PrStatus, String>>>,
}

impl RecordingSignalChecker {
    fn with_responses(responses: Vec<Result<PrStatus, String>>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[derive(Default)]
struct RecordingJudgeInvoker {
    calls: Mutex<Vec<AutomationRunId>>,
    responses: Mutex<VecDeque<Result<String, String>>>,
}

impl RecordingJudgeInvoker {
    fn with_outputs(outputs: Vec<String>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from(
                outputs.into_iter().map(Ok).collect::<Vec<_>>(),
            )),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl AutomationJudgeInvoker for RecordingJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationJudgeInvocation,
    ) -> AppResult<AutomationJudgeInvocationOutput> {
        self.calls
            .lock()
            .unwrap()
            .push(input.previous_run.id.clone());
        match self.responses.lock().unwrap().pop_front() {
            Some(Ok(raw_output)) => Ok(AutomationJudgeInvocationOutput {
                raw_output,
                model_id: Some("haiku".to_string()),
            }),
            Some(Err(error)) => Err(AppError::Validation(error)),
            None => Ok(AutomationJudgeInvocationOutput {
                raw_output: valid_continue_verdict(),
                model_id: Some("haiku".to_string()),
            }),
        }
    }
}

struct BlockingJudgeInvoker {
    calls: Mutex<Vec<AutomationRunId>>,
    release: Notify,
    output: String,
}

impl BlockingJudgeInvoker {
    fn new(output: String) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            release: Notify::new(),
            output,
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn release(&self) {
        self.release.notify_one();
    }
}

#[async_trait]
impl AutomationJudgeInvoker for BlockingJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationJudgeInvocation,
    ) -> AppResult<AutomationJudgeInvocationOutput> {
        self.calls
            .lock()
            .unwrap()
            .push(input.previous_run.id.clone());
        self.release.notified().await;
        Ok(AutomationJudgeInvocationOutput {
            raw_output: self.output.clone(),
            model_id: Some("haiku".to_string()),
        })
    }
}

#[derive(Default)]
struct RecordingPlanJudgeInvoker {
    calls: Mutex<Vec<AutomationPlanJudgeInvocation>>,
    responses: Mutex<VecDeque<Result<String, String>>>,
}

impl RecordingPlanJudgeInvoker {
    fn with_outputs(outputs: Vec<String>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from(
                outputs.into_iter().map(Ok).collect::<Vec<_>>(),
            )),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn calls(&self) -> Vec<AutomationPlanJudgeInvocation> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl AutomationPlanJudgeInvoker for RecordingPlanJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeInvocationOutput> {
        self.calls.lock().unwrap().push(input);
        match self.responses.lock().unwrap().pop_front() {
            Some(Ok(raw_output)) => Ok(AutomationPlanJudgeInvocationOutput {
                raw_output,
                model_id: Some("plan-judge-model".to_string()),
            }),
            Some(Err(error)) => Err(AppError::Validation(error)),
            None => Ok(AutomationPlanJudgeInvocationOutput {
                raw_output: valid_plan_approve_verdict("plan-artifact-1"),
                model_id: Some("plan-judge-model".to_string()),
            }),
        }
    }
}

struct MutatingPlanJudgeInvoker {
    session_repo: Arc<MemoryIdeationSessionRepository>,
    session_id: crate::domain::entities::IdeationSessionId,
    replacement_artifact_id: ArtifactId,
    output: String,
}

#[async_trait]
impl AutomationPlanJudgeInvoker for MutatingPlanJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeInvocationOutput> {
        self.session_repo
            .update_plan_artifact_id(
                &self.session_id,
                Some(self.replacement_artifact_id.as_str().to_string()),
            )
            .await?;
        Ok(AutomationPlanJudgeInvocationOutput {
            raw_output: self
                .output
                .replace("plan-artifact-1", &input.plan_artifact_id),
            model_id: Some("plan-judge-model".to_string()),
        })
    }
}

#[async_trait]
impl AutomationSignalChecker for RecordingSignalChecker {
    async fn check_pr_status(
        &self,
        workspace: &AgentConversationWorkspace,
        pr_number: i64,
    ) -> AppResult<PrStatus> {
        self.calls
            .lock()
            .unwrap()
            .push((workspace.conversation_id.as_str().to_string(), pr_number));
        match self.responses.lock().unwrap().pop_front() {
            Some(Ok(status)) => Ok(status),
            Some(Err(error)) => Err(AppError::Validation(error)),
            None => Ok(PrStatus::Open),
        }
    }
}

struct ApprovingPlanJudgeInvoker {
    approval_repo: Arc<MemoryPlanArtifactApprovalRepository>,
    session_id: crate::domain::entities::IdeationSessionId,
    artifact_id: ArtifactId,
    artifact_version: u32,
    output: String,
}

#[async_trait]
impl AutomationPlanJudgeInvoker for ApprovingPlanJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeInvocationOutput> {
        self.approval_repo.approve(
            self.session_id.clone(),
            self.artifact_id.clone(),
            self.artifact_version,
            PlanApprovalActor::User,
        );
        Ok(AutomationPlanJudgeInvocationOutput {
            raw_output: self
                .output
                .replace("plan-artifact-1", &input.plan_artifact_id),
            model_id: Some("plan-judge-model".to_string()),
        })
    }
}

struct SupersedingPlanJudgeInvoker {
    run_repo: Arc<MemoryAutomationRunRepository>,
    superseded_state: AutomationPlanJudgeState,
    output: String,
}

#[async_trait]
impl AutomationPlanJudgeInvoker for SupersedingPlanJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeInvocationOutput> {
        self.run_repo
            .compare_and_swap_plan_judge_state(
                &input.run.id,
                AutomationPlanJudgeState::InProgress,
                self.superseded_state,
                None,
                None,
            )
            .await?;
        Ok(AutomationPlanJudgeInvocationOutput {
            raw_output: self
                .output
                .replace("plan-artifact-1", &input.plan_artifact_id),
            model_id: Some("plan-judge-model".to_string()),
        })
    }
}

struct ResettingFailingPlanJudgeInvoker {
    run_repo: Arc<MemoryAutomationRunRepository>,
    session_repo: Arc<MemoryIdeationSessionRepository>,
    session_id: crate::domain::entities::IdeationSessionId,
    replacement_artifact_id: ArtifactId,
    calls: Mutex<usize>,
}

impl ResettingFailingPlanJudgeInvoker {
    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

#[async_trait]
impl AutomationPlanJudgeInvoker for ResettingFailingPlanJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeInvocationOutput> {
        *self.calls.lock().unwrap() += 1;
        self.session_repo
            .update_plan_artifact_id(
                &self.session_id,
                Some(self.replacement_artifact_id.as_str().to_string()),
            )
            .await?;
        self.run_repo
            .compare_and_swap_plan_judge_state(
                &input.run.id,
                AutomationPlanJudgeState::InProgress,
                AutomationPlanJudgeState::None,
                None,
                None,
            )
            .await?;
        Err(AppError::Validation("stale judge failure".to_string()))
    }
}

struct MemoryPlanArtifactApprovalWriter {
    approval_repo: Arc<MemoryPlanArtifactApprovalRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
}

#[async_trait]
impl PlanArtifactApprovalWriter for MemoryPlanArtifactApprovalWriter {
    async fn approve_current_plan_artifact(
        &self,
        session_id: crate::domain::entities::IdeationSessionId,
        requested_artifact_id: Option<String>,
        approved_by: PlanApprovalActor,
    ) -> AppResult<PlanArtifactApproval> {
        let artifact_id = requested_artifact_id
            .map(ArtifactId::from_string)
            .ok_or_else(|| {
                AppError::Validation("test approval requires artifact id".to_string())
            })?;
        let artifact = self
            .artifact_repo
            .get_by_id(&artifact_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("artifact {} not found", artifact_id)))?;
        self.approval_repo.approve(
            session_id.clone(),
            artifact_id.clone(),
            artifact.metadata.version,
            approved_by,
        );
        Ok(self
            .approval_repo
            .get_by_session(&session_id)
            .await?
            .expect("approval inserted"))
    }
}

struct ConflictingPlanArtifactApprovalWriter;

#[async_trait]
impl PlanArtifactApprovalWriter for ConflictingPlanArtifactApprovalWriter {
    async fn approve_current_plan_artifact(
        &self,
        _session_id: crate::domain::entities::IdeationSessionId,
        _requested_artifact_id: Option<String>,
        _approved_by: PlanApprovalActor,
    ) -> AppResult<PlanArtifactApproval> {
        Err(AppError::Conflict(
            "Plan changed before approval. Refresh the current plan and approve again.".to_string(),
        ))
    }
}

fn automation(id: &str, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: format!("Automation {id}"),
        status,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Goal".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: None,
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: String::new(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn automation_with_goal_items(id: &str, status: AutomationStatus) -> Automation {
    let mut automation = automation(id, status);
    automation.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "pending" }
        ])
        .to_string(),
    );
    automation
}

fn automation_run(
    id: &str,
    automation_id: &AutomationId,
    status: AutomationRunStatus,
    conversation_id: Option<ChatConversationId>,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: automation_id.clone(),
        run_index: 1,
        status,
        judge_state: AutomationJudgeState::None,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id,
        run_prompt: "Run prompt".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        branch_name: Some("ralphx/automation-run-1".to_string()),
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
        started_at: Some(now),
        finished_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn workspace(conversation_id: &ChatConversationId) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/automation-run-1".to_string(),
        "/tmp/ralphx-automation-run-1".to_string(),
    )
}

fn open_pr_health(head: &str) -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: None,
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "feature/pr".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some(head.to_string()),
            base_ref_oid: Some("base".to_string()),
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }
}

fn plan_workspace_with_session(
    conversation_id: &ChatConversationId,
    plan_artifact_id: Option<&str>,
) -> (AgentConversationWorkspace, IdeationSession) {
    let session = {
        let builder = IdeationSession::builder()
            .project_id(ProjectId::from_string("project-1".to_string()))
            .session_flow(IdeationSessionFlow::Planning);
        match plan_artifact_id {
            Some(artifact_id) => {
                builder.plan_artifact_id(ArtifactId::from_string(artifact_id.to_string()))
            }
            None => builder,
        }
        .build()
    };
    let mut workspace = workspace(conversation_id);
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    workspace.linked_ideation_session_id = Some(session.id.clone());
    (workspace, session)
}

struct ParkedPlanGateScenario {
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<MemoryAgentRunRepository>,
    session_repo: Arc<MemoryIdeationSessionRepository>,
    approval_repo: Arc<MemoryPlanArtifactApprovalRepository>,
    artifact_repo: Arc<MemoryArtifactRepository>,
    resumer: Arc<RecordingResumer>,
    automation_id: AutomationId,
    conversation_id: ChatConversationId,
    session_id: crate::domain::entities::IdeationSessionId,
}

impl ParkedPlanGateScenario {
    async fn new(
        automation_status: AutomationStatus,
        paused_reason_code: Option<&str>,
        plan_artifact_id: &str,
    ) -> Self {
        let automation_repo = Arc::new(MemoryAutomationRepository::new());
        let run_repo = Arc::new(MemoryAutomationRunRepository::new(
            automation_repo.shared_state(),
        ));
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
        let approval_repo = Arc::new(MemoryPlanArtifactApprovalRepository::new());
        let artifact_repo = Arc::new(MemoryArtifactRepository::new());
        let resumer = Arc::new(RecordingResumer::default());
        let automation_id = AutomationId::from_string("automation-1");
        let mut automation = automation(automation_id.as_str(), automation_status);
        automation.paused_reason_code = paused_reason_code.map(str::to_string);
        automation_repo.create(automation).await.unwrap();
        let conversation_id = ChatConversationId::from_string("conversation-1");
        let mut run = automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::AwaitingPlanApproval,
            Some(conversation_id.clone()),
        );
        run.plan_last_parked_artifact_id = Some(plan_artifact_id.to_string());
        run.plan_revision_round = 1;
        run_repo.create_run(run).await.unwrap();
        let (workspace, session) =
            plan_workspace_with_session(&conversation_id, Some(plan_artifact_id));
        let session_id = session.id.clone();
        session_repo.create(session).await.unwrap();
        workspace_repo.create_or_update(workspace).await.unwrap();

        Self {
            automation_repo,
            run_repo,
            workspace_repo,
            agent_run_repo,
            session_repo,
            approval_repo,
            artifact_repo,
            resumer,
            automation_id,
            conversation_id,
            session_id,
        }
    }

    async fn new_running(automation_status: AutomationStatus, plan_artifact_id: &str) -> Self {
        let automation_repo = Arc::new(MemoryAutomationRepository::new());
        let run_repo = Arc::new(MemoryAutomationRunRepository::new(
            automation_repo.shared_state(),
        ));
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
        let approval_repo = Arc::new(MemoryPlanArtifactApprovalRepository::new());
        let artifact_repo = Arc::new(MemoryArtifactRepository::new());
        let resumer = Arc::new(RecordingResumer::default());
        let automation_id = AutomationId::from_string("automation-1");
        automation_repo
            .create(automation(automation_id.as_str(), automation_status))
            .await
            .unwrap();
        let conversation_id = ChatConversationId::from_string("conversation-1");
        run_repo
            .create_run(automation_run(
                "run-1",
                &automation_id,
                AutomationRunStatus::Running,
                Some(conversation_id.clone()),
            ))
            .await
            .unwrap();
        let (workspace, session) =
            plan_workspace_with_session(&conversation_id, Some(plan_artifact_id));
        let session_id = session.id.clone();
        session_repo.create(session).await.unwrap();
        workspace_repo.create_or_update(workspace).await.unwrap();

        Self {
            automation_repo,
            run_repo,
            workspace_repo,
            agent_run_repo,
            session_repo,
            approval_repo,
            artifact_repo,
            resumer,
            automation_id,
            conversation_id,
            session_id,
        }
    }

    async fn complete_planning_agent_run(&self) {
        self.agent_run_repo
            .create(agent_run_with_status(
                self.conversation_id.clone(),
                AgentRunStatus::Completed,
            ))
            .await
            .unwrap();
    }

    fn approve(&self, artifact_id: &str, artifact_version: u32) {
        self.approval_repo.approve(
            self.session_id.clone(),
            ArtifactId::from_string(artifact_id.to_string()),
            artifact_version,
            PlanApprovalActor::User,
        );
    }

    async fn use_automatic_plan_approval(&self, provider_harness: &str) {
        self.automation_repo
            .update_config(
                &self.automation_id,
                crate::domain::repositories::AutomationConfigPatch {
                    provider_harness: Some(provider_harness.to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        self.automation_repo
            .update_settings(
                &self.automation_id,
                AutomationSettingsPatch {
                    plan_approval_mode: Some(AutomationPlanApprovalMode::Automatic),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    async fn enable_plan_deep_verification(&self) {
        self.automation_repo
            .update_settings(
                &self.automation_id,
                AutomationSettingsPatch {
                    plan_deep_verification: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    fn scheduler(&self) -> AutomationScheduler {
        self.scheduler_with_plan_judge(Arc::new(RecordingPlanJudgeInvoker::default()))
    }

    fn scheduler_with_plan_judge(
        &self,
        plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
    ) -> AutomationScheduler {
        self.scheduler_with_plan_judge_verification_and_config(
            plan_judge_invoker,
            Arc::new(NoopAutomationPlanVerificationStarter),
            AutomationSchedulerConfig::default(),
        )
    }

    fn scheduler_with_plan_judge_and_verification(
        &self,
        plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
        plan_verification_starter: Arc<dyn AutomationPlanVerificationStarter>,
    ) -> AutomationScheduler {
        self.scheduler_with_plan_judge_verification_and_config(
            plan_judge_invoker,
            plan_verification_starter,
            AutomationSchedulerConfig::default(),
        )
    }

    fn scheduler_with_plan_judge_verification_and_config(
        &self,
        plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
        plan_verification_starter: Arc<dyn AutomationPlanVerificationStarter>,
        config: AutomationSchedulerConfig,
    ) -> AutomationScheduler {
        let session_repo: Arc<dyn IdeationSessionRepository> = self.session_repo.clone();
        let resumer: Arc<dyn AutomationRunResumer> = self.resumer.clone();
        let artifact_repo: Arc<dyn ArtifactRepository> = self.artifact_repo.clone();
        let plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter> =
            Arc::new(MemoryPlanArtifactApprovalWriter {
                approval_repo: Arc::clone(&self.approval_repo),
                artifact_repo: Arc::clone(&artifact_repo),
            });
        scheduler_with_judge_agent_runs_plan_deps_artifacts_writer_and_verification(
            Arc::clone(&self.automation_repo),
            Arc::clone(&self.run_repo),
            Arc::clone(&self.workspace_repo),
            self.agent_run_repo.clone(),
            session_repo,
            self.approval_repo.clone(),
            plan_approval_writer,
            resumer,
            Arc::new(RecordingSignalChecker::default()),
            Arc::new(RecordingJudgeInvoker::default()),
            plan_judge_invoker,
            plan_verification_starter,
            artifact_repo,
            config,
        )
    }

    async fn seed_plan_artifact(&self, artifact_id: &str, text: &str, version: u32) {
        let mut artifact = Artifact::new_inline(
            "Run Plan",
            ArtifactType::Specification,
            text.to_string(),
            "assistant",
        );
        artifact.id = ArtifactId::from_string(artifact_id.to_string());
        artifact.metadata.version = version;
        self.artifact_repo.create(artifact).await.unwrap();
    }

    async fn update_plan_artifact_id(&self, artifact_id: &str) {
        self.session_repo
            .update_plan_artifact_id(&self.session_id, Some(artifact_id.to_string()))
            .await
            .unwrap();
    }

    async fn seed_verification_snapshot(&self, snapshot: VerificationRunSnapshot) {
        self.session_repo
            .save_verification_run_snapshot(&self.session_id, &snapshot)
            .await
            .unwrap();
    }
}

fn agent_run_with_status(conversation_id: ChatConversationId, status: AgentRunStatus) -> AgentRun {
    let mut agent_run = AgentRun::new(conversation_id);
    agent_run.status = status;
    agent_run
}

fn scheduler_with(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    scheduler_with_judge(
        automation_repo,
        run_repo,
        workspace_repo,
        signal_checker,
        Arc::new(RecordingJudgeInvoker::default()),
        config,
    )
}

fn scheduler_with_judge(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    scheduler_with_judge_and_agent_runs(
        automation_repo,
        run_repo,
        workspace_repo,
        Arc::new(MemoryAgentRunRepository::new()),
        signal_checker,
        judge_invoker,
        config,
    )
}

fn scheduler_with_judge_and_agent_runs(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    scheduler_with_judge_agent_runs_and_plan_deps(
        automation_repo,
        run_repo,
        workspace_repo,
        agent_run_repo,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        Arc::new(RecordingResumer::default()),
        signal_checker,
        judge_invoker,
        config,
    )
}

fn scheduler_with_judge_agent_runs_and_plan_deps(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<MemoryPlanArtifactApprovalRepository>,
    resumer: Arc<dyn AutomationRunResumer>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    scheduler_with_judge_agent_runs_plan_deps_and_artifacts(
        automation_repo,
        run_repo,
        workspace_repo,
        agent_run_repo,
        ideation_session_repo,
        plan_approval_repo,
        resumer,
        signal_checker,
        judge_invoker,
        Arc::new(RecordingPlanJudgeInvoker::default()),
        Arc::new(MemoryArtifactRepository::new()),
        config,
    )
}

fn scheduler_with_judge_agent_runs_plan_deps_and_artifacts(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<MemoryPlanArtifactApprovalRepository>,
    resumer: Arc<dyn AutomationRunResumer>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    let plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter> =
        Arc::new(MemoryPlanArtifactApprovalWriter {
            approval_repo: Arc::clone(&plan_approval_repo),
            artifact_repo: Arc::clone(&artifact_repo),
        });
    scheduler_with_judge_agent_runs_plan_deps_artifacts_and_writer(
        automation_repo,
        run_repo,
        workspace_repo,
        agent_run_repo,
        ideation_session_repo,
        plan_approval_repo,
        plan_approval_writer,
        resumer,
        signal_checker,
        judge_invoker,
        plan_judge_invoker,
        artifact_repo,
        config,
    )
}

fn scheduler_with_judge_agent_runs_plan_deps_artifacts_and_writer(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<MemoryPlanArtifactApprovalRepository>,
    plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter>,
    resumer: Arc<dyn AutomationRunResumer>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    scheduler_with_judge_agent_runs_plan_deps_artifacts_writer_and_verification(
        automation_repo,
        run_repo,
        workspace_repo,
        agent_run_repo,
        ideation_session_repo,
        plan_approval_repo,
        plan_approval_writer,
        resumer,
        signal_checker,
        judge_invoker,
        plan_judge_invoker,
        Arc::new(NoopAutomationPlanVerificationStarter),
        artifact_repo,
        config,
    )
}

fn scheduler_with_judge_agent_runs_plan_deps_artifacts_writer_and_verification(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<MemoryPlanArtifactApprovalRepository>,
    plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter>,
    resumer: Arc<dyn AutomationRunResumer>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
    plan_verification_starter: Arc<dyn AutomationPlanVerificationStarter>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    let plan_approval_repo_trait: Arc<dyn PlanArtifactApprovalRepository> =
        plan_approval_repo.clone();
    AutomationScheduler::new(
        automation_repo,
        run_repo,
        agent_run_repo,
        Arc::new(MemoryChatConversationRepository::new()),
        workspace_repo,
        ideation_session_repo,
        plan_approval_repo_trait,
        plan_approval_writer,
        Arc::new(RecordingStarter),
        resumer,
        signal_checker,
        judge_invoker,
        plan_judge_invoker,
        plan_verification_starter,
        Arc::new(NoopAutomationEventEmitter),
        artifact_repo,
        Arc::new(AutomationSchedulerRegistry::default()),
        config,
    )
}

fn valid_continue_verdict() -> String {
    json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "The next item remains and should be implemented in a scoped PR.",
        "confidence": 0.87,
        "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One item complete." },
        "updatedItemStatuses": [{ "id": "item-2", "status": "done" }],
        "nextRunPrompt": "Implement item 2 from the automation goal. Keep the change scoped, include targeted tests, and publish the PR.",
        "nextBaseBranch": "automation_base"
    })
    .to_string()
}

fn valid_stop_verdict(goal_met: bool) -> String {
    json!({
        "decision": "stop",
        "goalMet": goal_met,
        "reason": "The automation should stop based on the latest run evidence.",
        "confidence": 0.9,
        "goalProgress": { "completedItems": 2, "totalItems": 2, "summary": "No remaining runnable work." },
        "updatedItemStatuses": null,
        "nextRunPrompt": null,
        "nextBaseBranch": null
    })
    .to_string()
}

fn valid_plan_approve_verdict(artifact_id: &str) -> String {
    json!({
        "decision": "approve",
        "reason": "The plan is aligned with the automation goal and current phase.",
        "confidence": "high",
        "evaluatedArtifactId": artifact_id
    })
    .to_string()
}

fn valid_plan_revise_verdict(artifact_id: &str, instructions: &str) -> String {
    json!({
        "decision": "revise",
        "reason": "The plan needs a narrower recovery and validation section.",
        "confidence": "medium",
        "revisionInstructions": instructions,
        "evaluatedArtifactId": artifact_id
    })
    .to_string()
}

fn item_status(goal_items_json: &str, id: &str) -> String {
    let value: Value = serde_json::from_str(goal_items_json).unwrap();
    value
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
        .and_then(|item| item.get("status"))
        .and_then(Value::as_str)
        .unwrap()
        .to_string()
}

async fn wait_for_run_count(
    run_repo: &MemoryAutomationRunRepository,
    automation_id: &AutomationId,
    expected: usize,
) -> Vec<AutomationRun> {
    for _ in 0..100 {
        let runs = run_repo.list_for_automation(automation_id).await.unwrap();
        if runs.len() == expected {
            return runs;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {expected} automation runs");
}

async fn wait_for_latest_judge_state(
    run_repo: &MemoryAutomationRunRepository,
    automation_id: &AutomationId,
    expected: AutomationJudgeState,
) -> AutomationRun {
    for _ in 0..100 {
        let latest = run_repo
            .latest_for_automation(automation_id)
            .await
            .unwrap()
            .unwrap();
        if latest.judge_state == expected {
            return latest;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for judge state {expected:?}");
}

async fn wait_for_latest_plan_judge_state(
    run_repo: &MemoryAutomationRunRepository,
    automation_id: &AutomationId,
    expected: AutomationPlanJudgeState,
) -> AutomationRun {
    let mut last = None;
    for _ in 0..100 {
        let latest = run_repo
            .latest_for_automation(automation_id)
            .await
            .unwrap()
            .unwrap();
        if latest.plan_judge_state == expected {
            return latest;
        }
        last = Some(latest.plan_judge_state);
        sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "timed out waiting for plan judge state {expected:?}; last observed state: {:?}",
        last
    );
}

async fn wait_for_plan_judge_call_count(plan_judge: &RecordingPlanJudgeInvoker, expected: usize) {
    for _ in 0..100 {
        if plan_judge.call_count() == expected {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {expected} plan judge calls");
}

fn reviewing_verification_snapshot(generation: i32) -> VerificationRunSnapshot {
    VerificationRunSnapshot {
        generation,
        status: VerificationStatus::Reviewing,
        in_progress: true,
        current_round: 1,
        max_rounds: 3,
        best_round_index: None,
        convergence_reason: None,
        current_gaps: Vec::new(),
        rounds: Vec::new(),
    }
}

fn needs_revision_verification_snapshot(generation: i32) -> VerificationRunSnapshot {
    VerificationRunSnapshot {
        generation,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 2,
        max_rounds: 3,
        best_round_index: Some(1),
        convergence_reason: Some("gaps_remaining".to_string()),
        current_gaps: vec![VerificationGap {
            severity: "critical".to_string(),
            category: "state_machine".to_string(),
            description: "Plan omits stale-cache falsification.".to_string(),
            why_it_matters: Some("The judge could approve a false success.".to_string()),
            source: Some("implementation_feasibility".to_string()),
        }],
        rounds: Vec::new(),
    }
}

async fn wait_for_resetting_plan_judge_call_count(
    plan_judge: &ResettingFailingPlanJudgeInvoker,
    expected: usize,
) {
    for _ in 0..100 {
        if plan_judge.call_count() == expected {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {expected} resetting plan judge calls");
}

async fn wait_for_automation_status(
    automation_repo: &MemoryAutomationRepository,
    automation_id: &AutomationId,
    expected: AutomationStatus,
) -> Automation {
    for _ in 0..100 {
        let automation = automation_repo
            .get_by_id(automation_id)
            .await
            .unwrap()
            .unwrap();
        if automation.status == expected {
            return automation;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for automation status {expected:?}");
}

#[test]
fn automation_scheduler_config_maps_runtime_values() {
    let config = AutomationsRuntimeConfig {
        scheduler_poll_secs: 45,
        signal_failure_pause_threshold: 7,
        judge_timeout_secs: 240,
        publish_grace_secs: 90,
        max_run_duration_secs: 7_200,
        plan_judge_model: crate::domain::agents::standard_harness_map(
            "claude-plan-model".to_string(),
            "codex-plan-model".to_string(),
        ),
        plan_max_revision_rounds: 4,
    };

    let scheduler_config = AutomationSchedulerConfig::from_runtime(&config);

    assert_eq!(scheduler_config.poll_interval, Duration::from_secs(45));
    assert_eq!(scheduler_config.signal_failure_pause_threshold, 7);
    assert_eq!(scheduler_config.judge_timeout, Duration::from_secs(240));
    assert_eq!(scheduler_config.publish_grace, Duration::from_secs(90));
    assert_eq!(
        scheduler_config.max_run_duration,
        Duration::from_secs(7_200)
    );
    assert_eq!(scheduler_config.plan_max_revision_rounds, 4);
    assert_eq!(
        scheduler_config
            .plan_judge_models
            .get(&AgentHarnessKind::Claude)
            .map(String::as_str),
        Some("claude-plan-model")
    );
    assert_eq!(
        scheduler_config
            .plan_judge_models
            .get(&AgentHarnessKind::Codex)
            .map(String::as_str),
        Some("codex-plan-model")
    );
}

#[test]
fn automation_scheduler_registry_rejects_duplicate_loop_start() {
    let registry = AutomationSchedulerRegistry::default();

    assert!(registry.try_start_loop());
    assert!(registry.has_started_loop());
    assert!(!registry.try_start_loop());
}

#[test]
fn automation_scheduler_registry_enforces_per_automation_lease() {
    let registry = AutomationSchedulerRegistry::default();
    let automation_id = AutomationId::from_string("automation-1");
    let now = Instant::now();

    let first = registry
        .try_acquire_automation(&automation_id, now, Duration::from_secs(30))
        .expect("first lease should acquire");
    assert!(
        registry
            .try_acquire_automation(&automation_id, now, Duration::from_secs(30))
            .is_none(),
        "overlapping lease should be refused"
    );

    drop(first);
    assert!(
        registry
            .try_acquire_automation(&automation_id, now, Duration::from_secs(30))
            .is_some(),
        "released lease should be acquirable"
    );
}

#[tokio::test]
async fn automation_scheduler_tick_only_leases_active_automations() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    automation_repo
        .create(automation("active-1", AutomationStatus::Active))
        .await
        .unwrap();
    automation_repo
        .create(automation("paused-1", AutomationStatus::Paused))
        .await
        .unwrap();
    let registry = Arc::new(AutomationSchedulerRegistry::default());
    let plan_approval_repo = Arc::new(MemoryPlanArtifactApprovalRepository::new());
    let artifact_repo: Arc<dyn ArtifactRepository> = Arc::new(MemoryArtifactRepository::new());
    let plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter> =
        Arc::new(MemoryPlanArtifactApprovalWriter {
            approval_repo: Arc::clone(&plan_approval_repo),
            artifact_repo: Arc::clone(&artifact_repo),
        });
    let plan_approval_repo_trait: Arc<dyn PlanArtifactApprovalRepository> =
        plan_approval_repo.clone();
    let scheduler = AutomationScheduler::new(
        automation_repo,
        run_repo.clone(),
        Arc::new(MemoryAgentRunRepository::new()),
        conversation_repo,
        workspace_repo,
        Arc::new(MemoryIdeationSessionRepository::new()),
        plan_approval_repo_trait,
        plan_approval_writer,
        Arc::new(RecordingStarter),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        Arc::new(RecordingPlanJudgeInvoker::default()),
        Arc::new(NoopAutomationPlanVerificationStarter),
        Arc::new(NoopAutomationEventEmitter),
        artifact_repo,
        registry,
        AutomationSchedulerConfig::from_runtime(&AutomationsRuntimeConfig::default()),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.total_automations, 2);
    assert_eq!(summary.active_automations, 1);
    assert_eq!(summary.leased_automations, 1);
    assert_eq!(summary.active_without_runs, 1);
    assert_eq!(summary.active_with_runs, 0);
    assert_eq!(summary.provisioned_runs, 1);
    assert_eq!(summary.provisioning_errors, 0);
    assert_eq!(summary.automation_errors, 0);

    let latest = run_repo
        .latest_for_automation(&AutomationId::from_string("active-1"))
        .await
        .unwrap()
        .expect("run should be created");
    assert_eq!(
        latest.status,
        crate::domain::entities::AutomationRunStatus::Running
    );
    assert_eq!(
        latest.branch_name.as_deref(),
        Some("ralphx/automation-run-1")
    );
}

#[tokio::test]
async fn automation_scheduler_marks_running_run_published_from_workspace_pr() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.active_with_runs, 1);
    assert_eq!(summary.published_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Published);
    assert_eq!(latest.pr_number, Some(77));
    assert_eq!(
        latest.pr_url.as_deref(),
        Some("https://github.com/acme/project/pull/77")
    );
    assert_eq!(
        latest.pr_head_ref_name.as_deref(),
        Some("ralphx/automation-run-1")
    );
    assert_eq!(latest.pr_base_ref_name.as_deref(), Some("main"));
}

#[tokio::test]
async fn automation_scheduler_enables_workspace_auto_merge_preference_for_automatic_pr_merge() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.pr_autofix_enabled = true;
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.published_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Published);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(workspace.pr_autofix_enabled);
    assert!(workspace.pr_auto_merge_desired);
    assert_eq!(
        workspace.pr_auto_merge_method,
        DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD
    );
    assert_eq!(
        workspace.pr_supervision_status.as_deref(),
        Some("monitoring")
    );
}

#[tokio::test]
async fn automation_scheduler_keeps_autofix_disabled_when_automatic_pr_merge_publishes() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.pr_autofix_enabled = false;
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.published_runs, 1);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!workspace.pr_autofix_enabled);
    assert!(workspace.pr_auto_merge_desired);
}

#[tokio::test]
async fn automation_scheduler_does_not_arm_auto_merge_when_publication_cas_loses() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    run_repo.lose_next_running_to_published_cas();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.published_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!workspace.pr_auto_merge_desired);
}

#[tokio::test]
async fn automation_scheduler_publishes_when_auto_merge_preference_write_fails() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo.fail_next_pr_supervision_preference_update("workspace repo unavailable");
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.published_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Published);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!workspace.pr_auto_merge_desired);
}

#[tokio::test]
async fn automation_scheduler_rearms_published_automatic_run_after_crash_window() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(77);
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.pr_auto_merge_desired = false;
    workspace_repo.create_or_update(workspace).await.unwrap();
    let checker = Arc::new(RecordingSignalChecker::default());
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        checker.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.signal_check_errors, 0);
    assert_eq!(checker.call_count(), 1);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(workspace.pr_auto_merge_desired);
}

#[tokio::test]
async fn automation_scheduler_leaves_auto_merge_preference_untouched_for_manual_pr_merge() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.pr_autofix_enabled = true;
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.published_runs, 1);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(workspace.pr_autofix_enabled);
    assert!(!workspace.pr_auto_merge_desired);
    assert_eq!(
        workspace.pr_auto_merge_method,
        DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD
    );
    assert!(workspace.pr_supervision_status.is_none());
}

#[tokio::test]
async fn automation_scheduler_provisions_pending_successor_runs() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let mut run = automation_run("run-2", &automation_id, AutomationRunStatus::Pending, None);
    run.run_index = 2;
    run.base_from_run_id = Some(AutomationRunId::from_string("run-1"));
    run_repo.create_run(run).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.active_with_runs, 1);
    assert_eq!(summary.provisioned_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.run_index, 2);
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.conversation_id.is_some());
    assert_eq!(
        latest.branch_name.as_deref(),
        Some("ralphx/automation-run-1")
    );
}

#[tokio::test]
async fn automation_scheduler_marks_published_run_merged_from_github_signal() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(77);
    run.pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let checker = Arc::new(RecordingSignalChecker::with_responses(vec![Ok(
        PrStatus::Merged {
            merge_commit_sha: Some("abc123".to_string()),
            merged_at: Some("2026-07-05T12:00:00Z".to_string()),
        },
    )]));
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        checker,
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.merged_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Merged);
    assert_eq!(latest.merge_commit_sha.as_deref(), Some("abc123"));
    assert_eq!(
        latest.pr_merged_at,
        Some(Utc.with_ymd_and_hms(2026, 7, 5, 12, 0, 0).unwrap())
    );
    assert!(latest.finished_at.is_some());
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("merged"));
}

#[tokio::test]
async fn automation_scheduler_does_not_write_merge_metadata_when_merged_status_cas_loses() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(77);
    run.pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/77".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    run_repo.lose_next_published_to_merged_cas();
    let checker = Arc::new(RecordingSignalChecker::with_responses(vec![Ok(
        PrStatus::Merged {
            merge_commit_sha: Some("abc123".to_string()),
            merged_at: Some("2026-07-05T12:00:00Z".to_string()),
        },
    )]));
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        checker,
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.merged_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Published);
    assert!(latest.merge_commit_sha.is_none());
    assert!(latest.pr_merged_at.is_none());
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("merged"));
}

#[tokio::test]
async fn automation_scheduler_marks_published_run_closed_from_github_signal() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(78);
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(78);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::with_responses(vec![Ok(
            PrStatus::Closed,
        )])),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.closed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::PrClosed);
    assert_eq!(latest.error_code.as_deref(), Some("pr_closed"));
    assert!(latest.finished_at.is_some());
}

#[tokio::test]
async fn automation_scheduler_pauses_after_bounded_signal_check_errors() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(79);
    run.signal_check_failures = 1;
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(79);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let config = AutomationSchedulerConfig {
        signal_failure_pause_threshold: 2,
        ..AutomationSchedulerConfig::default()
    };
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::with_responses(vec![Err(
            "gh unavailable".to_string(),
        )])),
        config,
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.signal_check_errors, 1);
    assert_eq!(summary.paused_automations, 1);
    let automation = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Paused);
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some("signal_verification_failed")
    );
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Published);
    assert_eq!(latest.signal_check_failures, 2);
}

#[tokio::test]
async fn automation_scheduler_surfaces_auto_merge_enable_warning_without_signal_penalty() {
    let worktree = tempfile::tempdir().expect("worktree path");
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(79);
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.worktree_path = worktree.path().to_string_lossy().to_string();
    workspace.publication_pr_number = Some(79);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.pr_auto_merge_desired = true;
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let github = Arc::new(MockGithubService::new());
    {
        let mut github_state = github.state();
        github_state.fetch_pr_health_result = Some(Ok(open_pr_health("auto-merge-warning-head")));
        github_state.enable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
            "branch protection blocks it".to_string(),
        )));
    }
    let github_trait: Arc<dyn GithubServiceTrait> = github;
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();
    let current = sync_agent_workspace_auto_merge_preference_for_workspace(
        github_trait,
        worktree.path(),
        79,
        &workspace,
        workspace_repo_trait,
    )
    .await
    .unwrap();
    assert!(!current);
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.signal_check_errors, 0);
    assert_eq!(summary.paused_automations, 0);
    let automation = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Published);
    assert_eq!(latest.signal_check_failures, 0);
    assert_eq!(
        latest.error_code.as_deref(),
        Some(AUTO_MERGE_ENABLE_WARNING_CODE)
    );
    assert!(latest
        .error_detail
        .as_deref()
        .unwrap_or_default()
        .contains("branch protection blocks it"));
}

#[tokio::test]
async fn automation_scheduler_does_not_rewrite_identical_auto_merge_warning() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    let warning = auto_merge_enable_failure_summary("branch protection blocks it");
    run.pr_number = Some(79);
    run.error_code = Some(AUTO_MERGE_ENABLE_WARNING_CODE.to_string());
    run.error_detail = Some(warning.clone());
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(79);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.pr_supervision_status = Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING.to_string());
    workspace.pr_supervision_summary = Some(warning);
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.signal_check_errors, 0);
    assert_eq!(run_repo.published_run_error_update_count(), 0);
}

#[tokio::test]
async fn automation_scheduler_does_not_clobber_unrelated_published_run_error() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(79);
    run.error_code = Some("manual_review_note".to_string());
    run.error_detail = Some("Human added a note".to_string());
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(79);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(false);
    workspace.pr_supervision_status = Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING.to_string());
    workspace.pr_supervision_summary = Some(auto_merge_enable_failure_summary(
        "branch protection blocks it",
    ));
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.signal_check_errors, 0);
    assert_eq!(run_repo.published_run_error_update_count(), 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.error_code.as_deref(), Some("manual_review_note"));
    assert_eq!(latest.error_detail.as_deref(), Some("Human added a note"));
}

#[tokio::test]
async fn automation_scheduler_holds_signals_while_automation_is_paused() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Paused))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(80);
    run_repo.create_run(run).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(80);
    workspace.publication_pr_status = Some("open".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let checker = Arc::new(RecordingSignalChecker::default());
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        checker.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.active_automations, 0);
    assert_eq!(checker.call_count(), 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Published);
}

#[tokio::test]
async fn automation_scheduler_times_out_running_run() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Running,
        Some(conversation_id),
    );
    let old = Utc::now() - chrono::Duration::hours(2);
    run.started_at = Some(old);
    run.created_at = old;
    run_repo.create_run(run).await.unwrap();
    let config = AutomationSchedulerConfig {
        max_run_duration: Duration::from_secs(60),
        ..AutomationSchedulerConfig::default()
    };
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        config,
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(latest.error_code.as_deref(), Some("timeout"));
    assert!(latest.finished_at.is_some());
}

#[tokio::test]
async fn automation_scheduler_completes_agent_completed_run_from_agent_run_status() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.run_mode = "plan".to_string();
    automation.completion_signal = "agent_completed".to_string();
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut agent_run = AgentRun::new(conversation_id);
    agent_run.status = crate::domain::entities::AgentRunStatus::Completed;
    agent_run.completed_at = Some(Utc::now());
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_and_agent_runs(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.completed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Completed);
    assert!(latest.finished_at.is_some());
}

#[tokio::test]
async fn automation_scheduler_does_not_enable_auto_merge_for_agent_completed_run_without_pr() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.run_mode = "plan".to_string();
    automation.completion_signal = "agent_completed".to_string();
    automation.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.pr_autofix_enabled = true;
    workspace_repo.create_or_update(workspace).await.unwrap();
    let mut agent_run = AgentRun::new(conversation_id.clone());
    agent_run.status = crate::domain::entities::AgentRunStatus::Completed;
    agent_run.completed_at = Some(Utc::now());
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_and_agent_runs(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        Arc::clone(&workspace_repo),
        agent_run_repo,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.completed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Completed);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(workspace.pr_autofix_enabled);
    assert!(!workspace.pr_auto_merge_desired);
    assert!(workspace.pr_supervision_status.is_none());
}

#[tokio::test]
async fn automation_scheduler_parks_plan_run_before_agent_completed_terminalization() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let judge_invoker = Arc::new(RecordingJudgeInvoker::default());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.completion_signal = "agent_completed".to_string();
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let (workspace, session) =
        plan_workspace_with_session(&conversation_id, Some("plan-artifact-1"));
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    let mut agent_run = agent_run_with_status(conversation_id, AgentRunStatus::Completed);
    agent_run.completed_at = Some(Utc::now());
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        judge_invoker.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.completed_runs, 0);
    assert_eq!(summary.judges_started, 0);
    assert_eq!(judge_invoker.call_count(), 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(
        latest.plan_last_parked_artifact_id.as_deref(),
        Some("plan-artifact-1")
    );
    assert_eq!(latest.plan_revision_round, 1);
}

#[tokio::test]
async fn automation_scheduler_sends_one_plan_reminder_after_terminal_turn_without_artifact() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let resumer = Arc::new(RecordingResumer::default());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Running,
        Some(conversation_id.clone()),
    );
    run_repo.create_run(run).await.unwrap();
    let (workspace, session) = plan_workspace_with_session(&conversation_id, None);
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    let mut agent_run = agent_run_with_status(conversation_id.clone(), AgentRunStatus::Completed);
    agent_run.completed_at = Some(Utc::now());
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert_eq!(latest.plan_reminder_count, 1);
    assert!(latest.agent_phase_started_at.is_some());
    let prompts = resumer.prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].0, conversation_id);
    assert_eq!(prompts[0].1, AUTOMATION_PLAN_REMINDER_PROMPT);
    assert_eq!(latest.run_prompt, "Run prompt");
}

#[tokio::test]
async fn automation_scheduler_redelivers_plan_reminder_after_resume_crash_ignores_stale_terminal_agent_run(
) {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let resumer = Arc::new(RecordingResumer::default());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let phase_started_at = Utc::now();
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Running,
        Some(conversation_id.clone()),
    );
    run.plan_reminder_count = 1;
    run.agent_phase_started_at = Some(phase_started_at);
    run_repo.create_run(run).await.unwrap();
    let (workspace, session) = plan_workspace_with_session(&conversation_id, None);
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    let mut stale_agent_run =
        agent_run_with_status(conversation_id.clone(), AgentRunStatus::Completed);
    stale_agent_run.started_at = phase_started_at - chrono::Duration::minutes(5);
    stale_agent_run.completed_at = Some(phase_started_at - chrono::Duration::minutes(4));
    agent_run_repo.create(stale_agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.finished_at.is_none());
    assert_eq!(latest.plan_reminder_count, 1);
    let prompts = resumer.prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].0, conversation_id);
    assert_eq!(prompts[0].1, AUTOMATION_PLAN_REMINDER_PROMPT);
}

#[tokio::test]
async fn automation_scheduler_waits_without_reminder_while_launches_are_paused() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let resumer = Arc::new(RecordingResumer::default());
    resumer.set_launches_paused(true);
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let (workspace, session) = plan_workspace_with_session(&conversation_id, None);
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    agent_run_repo
        .create(agent_run_with_status(
            conversation_id,
            AgentRunStatus::Completed,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert_eq!(latest.plan_reminder_count, 0);
    assert!(resumer.prompts().is_empty());
}

#[tokio::test]
async fn automation_scheduler_fails_second_terminal_plan_turn_without_artifact() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Running,
        Some(conversation_id.clone()),
    );
    run.plan_reminder_count = 1;
    run_repo.create_run(run).await.unwrap();
    let (workspace, session) = plan_workspace_with_session(&conversation_id, None);
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    agent_run_repo
        .create(agent_run_with_status(
            conversation_id,
            AgentRunStatus::Completed,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(latest.error_code.as_deref(), Some("plan_not_submitted"));
}

#[tokio::test]
async fn automation_scheduler_fails_running_run_when_plan_reminder_send_fails() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let resumer = Arc::new(RecordingResumer::default());
    resumer.fail_next_send();
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let (workspace, session) = plan_workspace_with_session(&conversation_id, None);
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    agent_run_repo
        .create(agent_run_with_status(
            conversation_id,
            AgentRunStatus::Completed,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(latest.error_code.as_deref(), Some("plan_reminder_failed"));
    assert_eq!(latest.plan_reminder_count, 1);
    assert_eq!(resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_suppresses_publication_failures_during_plan_phase() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let (mut workspace, session) = plan_workspace_with_session(&conversation_id, None);
    workspace.publication_pr_number = Some(42);
    workspace.publication_push_status = Some("no_changes".to_string());
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    agent_run_repo
        .create(agent_run_with_status(
            conversation_id,
            AgentRunStatus::Running,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.published_runs, 0);
    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.pr_number.is_none());
}

#[tokio::test]
async fn automation_scheduler_reenters_running_when_awaiting_plan_agent_is_live() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let resumer = Arc::new(RecordingResumer::default());
    resumer.set_running(true);
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::AwaitingPlanApproval,
        Some(conversation_id.clone()),
    );
    run.plan_last_parked_artifact_id = Some("old-plan-artifact".to_string());
    run.plan_revision_round = 2;
    run.plan_judge_state = AutomationPlanJudgeState::Failed;
    run_repo.create_run(run).await.unwrap();
    let (workspace, session) =
        plan_workspace_with_session(&conversation_id, Some("new-plan-artifact"));
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(MemoryAgentRunRepository::new()),
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.agent_phase_started_at.is_some());
    assert_eq!(
        latest.plan_last_parked_artifact_id.as_deref(),
        Some("old-plan-artifact")
    );
    assert_eq!(latest.plan_revision_round, 2);
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::Failed);
    assert!(resumer.prompts().is_empty());
}

#[tokio::test]
async fn automation_scheduler_arm_zero_reentry_preserves_live_agent_phase_basis() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let resumer = Arc::new(RecordingResumer::default());
    resumer.set_running_responses(vec![true, false]);
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::AwaitingPlanApproval,
        Some(conversation_id.clone()),
    );
    run.plan_last_parked_artifact_id = Some("old-plan-artifact".to_string());
    run.plan_revision_round = 2;
    run_repo.create_run(run).await.unwrap();
    let (workspace, session) =
        plan_workspace_with_session(&conversation_id, Some("new-plan-artifact"));
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    let mut agent_run = agent_run_with_status(conversation_id.clone(), AgentRunStatus::Running);
    let agent_run_id = agent_run.id;
    let agent_started_at = Utc::now() - chrono::Duration::seconds(5);
    agent_run.started_at = agent_started_at;
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler_agent_run_repo: Arc<dyn AgentRunRepository> = agent_run_repo.clone();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        scheduler_agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        resumer,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    scheduler.tick_once().await.unwrap();
    agent_run_repo
        .update_status(&agent_run_id, AgentRunStatus::Completed)
        .await
        .unwrap();
    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(latest.agent_phase_started_at, Some(agent_started_at));
    assert_eq!(
        latest.plan_last_parked_artifact_id.as_deref(),
        Some("new-plan-artifact")
    );
    assert_eq!(latest.plan_revision_round, 3);
}

#[tokio::test]
async fn automation_scheduler_refreshes_plan_baseline_for_parked_revision_without_live_agent() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::AwaitingPlanApproval,
        Some(conversation_id.clone()),
    );
    run.plan_last_parked_artifact_id = Some("old-plan-artifact".to_string());
    run.plan_revision_round = 2;
    run.plan_judge_state = AutomationPlanJudgeState::Failed;
    run.plan_pending_instructions = Some("Revise the old plan.".to_string());
    run_repo.create_run(run).await.unwrap();
    let (workspace, session) =
        plan_workspace_with_session(&conversation_id, Some("new-plan-artifact"));
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(MemoryAgentRunRepository::new()),
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(
        latest.plan_last_parked_artifact_id.as_deref(),
        Some("new-plan-artifact")
    );
    assert_eq!(latest.plan_revision_round, 3);
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(latest.plan_pending_instructions.is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_deep_verification_triggers_once_for_first_park_from_running() {
    let scenario =
        ParkedPlanGateScenario::new_running(AutomationStatus::Active, "plan-artifact-1").await;
    scenario
        .automation_repo
        .update_config(
            &scenario.automation_id,
            crate::domain::repositories::AutomationConfigPatch {
                provider_harness: Some("codex".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    scenario.enable_plan_deep_verification().await;
    scenario.complete_planning_agent_run().await;
    let starter = Arc::new(RecordingPlanVerificationStarter::default());
    let scheduler = scenario.scheduler_with_plan_judge_and_verification(
        Arc::new(RecordingPlanJudgeInvoker::default()),
        starter.clone(),
    );

    scheduler.tick_once().await.unwrap();

    assert_eq!(starter.call_count(), 1);
    let calls = starter.calls();
    assert_eq!(calls[0].session_id, scenario.session_id);
    assert_eq!(calls[0].artifact_id, "plan-artifact-1");
    assert_eq!(calls[0].provider_harness, Some(AgentHarnessKind::Codex));
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(
        latest.plan_last_parked_artifact_id.as_deref(),
        Some("plan-artifact-1")
    );
    assert_eq!(latest.plan_revision_round, 1);

    scheduler.tick_once().await.unwrap();

    assert_eq!(starter.call_count(), 1);
}

#[tokio::test]
async fn automation_scheduler_automatic_plan_judge_holds_until_first_verification_terminates() {
    let scenario =
        ParkedPlanGateScenario::new_running(AutomationStatus::Active, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.enable_plan_deep_verification().await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    scenario.complete_planning_agent_run().await;
    let starter = Arc::new(
        RecordingPlanVerificationStarter::with_reviewing_side_effect(scenario.session_repo.clone()),
    );
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let scheduler =
        scenario.scheduler_with_plan_judge_and_verification(plan_judge.clone(), starter.clone());

    let park_summary = scheduler.tick_once().await.unwrap();

    assert_eq!(starter.call_count(), 1);
    assert_eq!(park_summary.judges_started, 0);
    assert_eq!(plan_judge.call_count(), 0);

    let hold_summary = scheduler.tick_once().await.unwrap();

    assert_eq!(starter.call_count(), 1);
    assert_eq!(hold_summary.judges_started, 0);
    assert_eq!(plan_judge.call_count(), 0);

    scenario
        .session_repo
        .update_verification_state(&scenario.session_id, VerificationStatus::Verified, false)
        .await
        .unwrap();

    let terminal_summary = scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    assert_eq!(terminal_summary.judges_started, 1);
    assert_eq!(plan_judge.calls()[0].plan_artifact_id, "plan-artifact-1");
}

#[tokio::test]
async fn automation_scheduler_plan_deep_verification_off_makes_zero_verification_calls() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.update_plan_artifact_id("plan-artifact-2").await;
    scenario
        .seed_plan_artifact("plan-artifact-2", "Updated plan body.", 2)
        .await;
    let starter = Arc::new(RecordingPlanVerificationStarter::default());
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-2"),
    ]));
    let scheduler =
        scenario.scheduler_with_plan_judge_and_verification(plan_judge.clone(), starter.clone());

    let summary = scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    assert_eq!(summary.judges_started, 1);
    assert_eq!(starter.call_count(), 0);
    assert_eq!(plan_judge.calls()[0].plan_artifact_id, "plan-artifact-2");
}

#[tokio::test]
async fn automation_scheduler_plan_judge_holds_while_verification_in_progress() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.enable_plan_deep_verification().await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    scenario
        .seed_verification_snapshot(reviewing_verification_snapshot(0))
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::default());
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 0);
    assert_eq!(plan_judge.call_count(), 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::None);
}

#[tokio::test]
async fn automation_scheduler_plan_approval_delivery_ignores_verification_hold() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.enable_plan_deep_verification().await;
    scenario
        .seed_verification_snapshot(reviewing_verification_snapshot(0))
        .await;
    scenario.approve("plan-artifact-1", 1);
    scenario
        .run_repo
        .set_plan_pending_instructions(
            &AutomationRunId::from_string("run-1"),
            Some("Stale revision instructions.".to_string()),
        )
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.plan_pending_instructions.is_none());
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_skips_verification_start_when_matching_approval_already_exists() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.enable_plan_deep_verification().await;
    scenario.update_plan_artifact_id("plan-artifact-2").await;
    scenario.approve("plan-artifact-2", 2);
    let starter = Arc::new(RecordingPlanVerificationStarter::default());
    let scheduler = scenario.scheduler_with_plan_judge_and_verification(
        Arc::new(RecordingPlanJudgeInvoker::default()),
        starter.clone(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(starter.call_count(), 0);
    assert_eq!(summary.judges_started, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert_eq!(
        latest.plan_last_parked_artifact_id.as_deref(),
        Some("plan-artifact-2")
    );
    assert_eq!(latest.plan_revision_round, 2);
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_plan_judge_receives_terminal_verification_payload() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.enable_plan_deep_verification().await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    scenario
        .seed_verification_snapshot(needs_revision_verification_snapshot(0))
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    let context = plan_judge.calls()[0]
        .verification_context
        .clone()
        .expect("verification context should be included");
    assert_eq!(context.status, "needs_revision");
    assert!(!context.in_progress);
    assert_eq!(context.generation, Some(0));
    assert_eq!(context.current_round, Some(2));
    assert_eq!(context.gap_count, Some(1));
    assert_eq!(context.gap_score, Some(10));
    assert!(context.gaps[0].description.contains("stale-cache"));
}

#[tokio::test]
async fn automation_scheduler_verification_unavailable_proceeds_to_plan_judge() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.enable_plan_deep_verification().await;
    scenario.update_plan_artifact_id("plan-artifact-2").await;
    scenario
        .seed_plan_artifact("plan-artifact-2", "Updated plan body.", 2)
        .await;
    let starter = Arc::new(RecordingPlanVerificationStarter::with_outcomes(vec![
        AutomationPlanVerificationStartOutcome::Unavailable {
            detail: "verification worker unavailable".to_string(),
        },
    ]));
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-2"),
    ]));
    let scheduler =
        scenario.scheduler_with_plan_judge_and_verification(plan_judge.clone(), starter.clone());

    let summary = scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    assert_eq!(summary.judges_started, 1);
    assert_eq!(starter.call_count(), 1);
    let context = plan_judge.calls()[0]
        .verification_context
        .clone()
        .expect("verification context should be included");
    assert_eq!(context.status, "unavailable");
    assert!(context
        .unavailable_reason
        .as_deref()
        .unwrap()
        .contains("verification worker unavailable"));
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
}

#[tokio::test]
async fn automation_scheduler_verification_starter_err_proceeds_to_plan_judge_without_failing_run()
{
    let scenario =
        ParkedPlanGateScenario::new_running(AutomationStatus::Active, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.enable_plan_deep_verification().await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    scenario.complete_planning_agent_run().await;
    let starter = Arc::new(RecordingPlanVerificationStarter::with_errors(vec![
        "starter exploded",
    ]));
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let scheduler =
        scenario.scheduler_with_plan_judge_and_verification(plan_judge.clone(), starter.clone());

    let summary = scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    assert_eq!(starter.call_count(), 1);
    assert_eq!(summary.judges_started, 1);
    let context = plan_judge.calls()[0]
        .verification_context
        .clone()
        .expect("verification context should be included");
    assert_eq!(context.status, "unavailable");
    assert!(context
        .unavailable_reason
        .as_deref()
        .unwrap()
        .contains("starter exploded"));
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(latest.status, AutomationRunStatus::AgentFailed);
    assert!(latest.error_code.is_none());
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
}

#[tokio::test]
async fn automation_scheduler_verification_hold_timeout_proceeds_with_unavailable_payload() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.enable_plan_deep_verification().await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    scenario
        .seed_verification_snapshot(reviewing_verification_snapshot(0))
        .await;
    let mut session = scenario
        .session_repo
        .get_by_id(&scenario.session_id)
        .await
        .unwrap()
        .unwrap();
    session.updated_at = Utc::now() - chrono::Duration::seconds(30);
    scenario.session_repo.create(session).await.unwrap();
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge_verification_and_config(
        plan_judge.clone(),
        Arc::new(NoopAutomationPlanVerificationStarter),
        AutomationSchedulerConfig {
            plan_verification_hold_timeout: Duration::from_secs(1),
            ..AutomationSchedulerConfig::default()
        },
    );

    let summary = scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    assert_eq!(summary.judges_started, 1);
    let context = plan_judge.calls()[0]
        .verification_context
        .clone()
        .expect("verification context should be included");
    assert_eq!(context.status, "unavailable");
    assert!(context
        .unavailable_reason
        .as_deref()
        .unwrap()
        .contains("terminal state"));
}

#[tokio::test]
async fn automation_scheduler_verifier_revision_counts_toward_round_exhaustion() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario.enable_plan_deep_verification().await;
    scenario.update_plan_artifact_id("plan-artifact-2").await;
    let starter = Arc::new(RecordingPlanVerificationStarter::default());
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::default());
    let scheduler = scenario.scheduler_with_plan_judge_verification_and_config(
        plan_judge.clone(),
        starter.clone(),
        AutomationSchedulerConfig {
            plan_max_revision_rounds: 1,
            ..AutomationSchedulerConfig::default()
        },
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.paused_automations, 1);
    assert_eq!(summary.judges_started, 0);
    assert_eq!(starter.call_count(), 1);
    assert_eq!(plan_judge.call_count(), 0);
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE)
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_revision_round, 2);
}

#[tokio::test]
async fn automation_scheduler_does_not_deliver_stale_plan_approval_for_revised_artifact() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-new").await;
    scenario.approve("plan-artifact-old", 1);
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert!(scenario.resumer.prompts().is_empty());
    assert!(scenario.resumer.switches().is_empty());
}

#[tokio::test]
async fn automation_scheduler_delivers_matching_plan_approval_once_and_clears_stale_publication_state(
) {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.approve("plan-artifact-1", 3);
    let mut workspace = scenario
        .workspace_repo
        .get_by_conversation_id(&scenario.conversation_id)
        .await
        .unwrap()
        .unwrap();
    workspace.publication_push_status = Some("no_changes".to_string());
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/42".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    scenario
        .workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.agent_phase_started_at.is_some());
    assert_eq!(latest.run_prompt, "Run prompt");
    assert!(latest.pr_number.is_none());
    let switched = scenario.resumer.switches();
    assert_eq!(switched, vec![scenario.conversation_id.clone()]);
    let prompts = scenario.resumer.prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].0, scenario.conversation_id);
    assert!(prompts[0].1.contains("Run plan v3 approved"));
    assert!(prompts[0].1.contains("publish the run pull request"));
    let cleared_workspace = scenario
        .workspace_repo
        .get_by_conversation_id(&prompts[0].0)
        .await
        .unwrap()
        .unwrap();
    assert!(cleared_workspace.publication_push_status.is_none());
    assert!(cleared_workspace.publication_pr_number.is_none());
    assert!(cleared_workspace.publication_pr_url.is_none());
    assert!(cleared_workspace.publication_pr_status.is_none());

    let second = scheduler.tick_once().await.unwrap();
    assert_eq!(second.failed_runs, 0);
    assert_eq!(second.published_runs, 0);
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn clear_plan_phase_publication_metadata_preserves_concurrent_workspace_preferences() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::AwaitingPlanApproval,
        Some(conversation_id.clone()),
    );
    run.pr_number = Some(42);
    run.pr_url = Some("https://github.com/acme/project/pull/42".to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    let mut stored_workspace = workspace(&conversation_id);
    stored_workspace.publication_push_status = Some("no_changes".to_string());
    stored_workspace.publication_pr_number = Some(42);
    stored_workspace.publication_pr_url =
        Some("https://github.com/acme/project/pull/42".to_string());
    stored_workspace.publication_pr_status = Some("open".to_string());
    stored_workspace.pr_auto_merge_desired = false;
    workspace_repo
        .create_or_update(stored_workspace.clone())
        .await
        .unwrap();

    let mut stale_snapshot = stored_workspace;
    stale_snapshot.pr_auto_merge_desired = true;
    let run_repo_trait: Arc<dyn AutomationRunRepository> = run_repo.clone();
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();

    clear_plan_phase_publication_metadata(
        &run_repo_trait,
        &workspace_repo_trait,
        &run,
        &stale_snapshot,
    )
    .await
    .unwrap();

    let cleared_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(cleared_workspace.publication_push_status.is_none());
    assert!(cleared_workspace.publication_pr_number.is_none());
    assert!(cleared_workspace.publication_pr_url.is_none());
    assert!(cleared_workspace.publication_pr_status.is_none());
    assert!(
        !cleared_workspace.pr_auto_merge_desired,
        "field-scoped publication clear must not replay a stale workspace clone"
    );
}

#[tokio::test]
async fn automation_scheduler_redelivers_plan_approval_after_resume_crash_ignores_stale_terminal_agent_run(
) {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let approval_repo = Arc::new(MemoryPlanArtifactApprovalRepository::new());
    let resumer = Arc::new(RecordingResumer::default());
    let judge_invoker = Arc::new(RecordingJudgeInvoker::default());
    let automation_id = AutomationId::from_string("automation-1");
    let mut automation = automation(automation_id.as_str(), AutomationStatus::Active);
    automation.completion_signal = "agent_completed".to_string();
    automation_repo.create(automation).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let phase_started_at = Utc::now();
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Running,
        Some(conversation_id.clone()),
    );
    run.agent_phase_started_at = Some(phase_started_at);
    run.plan_last_parked_artifact_id = Some("plan-artifact-1".to_string());
    run.plan_revision_round = 1;
    run_repo.create_run(run).await.unwrap();
    let (mut workspace, session) =
        plan_workspace_with_session(&conversation_id, Some("plan-artifact-1"));
    workspace.mode = AgentConversationWorkspaceMode::Edit;
    let session_id = session.id.clone();
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    approval_repo.approve(
        session_id,
        ArtifactId::from_string("plan-artifact-1".to_string()),
        3,
        PlanApprovalActor::User,
    );
    let mut stale_agent_run =
        agent_run_with_status(conversation_id.clone(), AgentRunStatus::Completed);
    stale_agent_run.started_at = phase_started_at - chrono::Duration::minutes(5);
    stale_agent_run.completed_at = Some(phase_started_at - chrono::Duration::minutes(4));
    agent_run_repo.create(stale_agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        approval_repo,
        resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        judge_invoker.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.completed_runs, 0);
    assert_eq!(summary.failed_runs, 0);
    assert_eq!(summary.judges_started, 0);
    assert_eq!(summary.successor_runs, 0);
    assert_eq!(judge_invoker.call_count(), 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.finished_at.is_none());
    let switched = resumer.switches();
    assert_eq!(switched, vec![conversation_id.clone()]);
    let prompts = resumer.prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].0, conversation_id);
    assert!(prompts[0].1.contains("Run plan v3 approved"));
    assert!(prompts[0].1.contains("publish the run pull request"));
}

#[tokio::test]
async fn automation_scheduler_delivers_plan_revision_without_switching_modes_and_clears_instructions(
) {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario
        .run_repo
        .set_plan_pending_instructions(
            &AutomationRunId::from_string("run-1"),
            Some("Tighten the rollout and testing sections.".to_string()),
        )
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert!(latest.plan_pending_instructions.is_none());
    assert!(latest.agent_phase_started_at.is_some());
    assert!(scenario.resumer.switches().is_empty());
    let prompts = scenario.resumer.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0]
        .1
        .contains("Tighten the rollout and testing sections."));
    assert!(prompts[0]
        .1
        .contains("Update the plan artifact and end the turn."));
    let workspace = scenario
        .workspace_repo
        .get_by_conversation_id(&scenario.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
}

#[tokio::test]
async fn automation_scheduler_skips_delivery_when_agent_becomes_live_before_send() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.approve("plan-artifact-1", 1);
    scenario.resumer.set_running_responses(vec![false, true]);
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert!(scenario.resumer.prompts().is_empty());
    assert!(scenario.resumer.switches().is_empty());
}

#[tokio::test]
async fn automation_scheduler_fails_run_when_approval_delivery_send_errors() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.approve("plan-artifact-1", 1);
    scenario.resumer.fail_next_send();
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(
        latest.error_code.as_deref(),
        Some(PLAN_RESUME_FAILED_ERROR_CODE)
    );
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_purges_queued_delivery_prompt_then_fails_run() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.approve("plan-artifact-1", 1);
    scenario.resumer.queue_next_send();
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(
        latest.error_code.as_deref(),
        Some(PLAN_RESUME_FAILED_ERROR_CODE)
    );
    assert_eq!(scenario.resumer.prompts().len(), 1);
    assert_eq!(scenario.resumer.purged_queued_messages(), 1);
}

#[tokio::test]
async fn automation_scheduler_waits_at_plan_gate_while_launches_are_paused_then_retries() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.approve("plan-artifact-1", 1);
    scenario.resumer.set_launches_paused(true);
    let scheduler = scenario.scheduler();

    let paused_summary = scheduler.tick_once().await.unwrap();

    assert_eq!(paused_summary.failed_runs, 0);
    let parked = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parked.status, AutomationRunStatus::AwaitingPlanApproval);
    assert!(scenario.resumer.prompts().is_empty());
    assert!(scenario.resumer.switches().is_empty());

    scenario.resumer.set_launches_paused(false);
    let resumed_summary = scheduler.tick_once().await.unwrap();

    assert_eq!(resumed_summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_recovers_approval_delivery_when_mode_already_edit() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.approve("plan-artifact-1", 1);
    let mut workspace = scenario
        .workspace_repo
        .get_by_conversation_id(&scenario.conversation_id)
        .await
        .unwrap()
        .unwrap();
    workspace.mode = AgentConversationWorkspaceMode::Edit;
    scenario
        .workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_manual_plan_gate_never_dispatches_plan_judge() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Manual mode plan.", 1)
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::default());
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 0);
    assert_eq!(plan_judge.call_count(), 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::None);
}

#[tokio::test]
async fn automation_scheduler_automatic_plan_gate_dispatches_single_flight_with_harness_model_and_approves(
) {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("codex").await;
    scenario
        .seed_plan_artifact(
            "plan-artifact-1",
            "Codex-shaped judge should approve this plan.",
            4,
        )
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let mut config = AutomationSchedulerConfig::default();
    config.plan_judge_models.insert(
        crate::domain::agents::AgentHarnessKind::Claude,
        "sonnet".to_string(),
    );
    config.plan_judge_models.insert(
        crate::domain::agents::AgentHarnessKind::Codex,
        "gpt-5.4".to_string(),
    );
    let scheduler = scheduler_with_judge_agent_runs_plan_deps_and_artifacts(
        Arc::clone(&scenario.automation_repo),
        Arc::clone(&scenario.run_repo),
        Arc::clone(&scenario.workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        scenario.session_repo.clone(),
        scenario.approval_repo.clone(),
        scenario.resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        plan_judge.clone(),
        scenario.artifact_repo.clone(),
        config,
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 1);
    let judging = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        judging.plan_judge_state,
        AutomationPlanJudgeState::InProgress
    );
    assert!(judging.plan_judge_lease_expires_at.is_some());
    let second_tick = scheduler.tick_once().await.unwrap();
    assert_eq!(second_tick.judges_started, 0);
    let judged = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;
    assert!(judged
        .plan_judge_verdict_json
        .as_deref()
        .unwrap()
        .contains("evaluatedArtifactId"));
    let calls = plan_judge.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].plan_artifact_id, "plan-artifact-1");
    assert_eq!(calls[0].plan_judge_model.as_deref(), Some("gpt-5.4"));
    assert!(!calls[0]
        .plan_judge_model
        .as_deref()
        .unwrap()
        .contains("sonnet"));
    let approval = scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approval.artifact_id.as_str(), "plan-artifact-1");
    assert_eq!(approval.artifact_version, 4);
    assert_eq!(approval.approved_by, "judge");
}

#[tokio::test]
async fn automation_scheduler_stored_approve_verdict_recovers_missing_approval_row() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let run_id = AutomationRunId::from_string("run-1");
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &run_id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::Done,
            Some(valid_plan_approve_verdict("plan-artifact-1")),
            None,
        )
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    scheduler.tick_once().await.unwrap();

    let approval = scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .expect("stored approve verdict should re-drive approval write");
    assert_eq!(approval.artifact_id.as_str(), "plan-artifact-1");
    assert_eq!(approval.approved_by, "judge");

    scheduler.tick_once().await.unwrap();
    let delivered = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivered.status, AutomationRunStatus::Running);
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_stored_approve_verdict_is_not_revision_fingerprint_baseline() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Approved old plan.", 1)
        .await;
    scenario
        .seed_plan_artifact("plan-artifact-2", "New plan needs revisions.", 2)
        .await;
    let run_id = AutomationRunId::from_string("run-1");
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &run_id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::Done,
            Some(valid_plan_approve_verdict("plan-artifact-1")),
            None,
        )
        .await
        .unwrap();
    scenario.update_plan_artifact_id("plan-artifact-2").await;
    let instructions = "Add a narrow validation matrix before implementation.";
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_revise_verdict("plan-artifact-2", instructions),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    let latest = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;
    assert_eq!(
        latest.plan_pending_instructions.as_deref(),
        Some(instructions)
    );
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    assert!(automation.paused_reason_code.is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_uses_claude_model_override_without_codex_leakage() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact(
            "plan-artifact-1",
            "Claude-shaped judge should approve this plan.",
            1,
        )
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let mut config = AutomationSchedulerConfig::default();
    config
        .plan_judge_models
        .insert(AgentHarnessKind::Claude, "claude-plan-sonnet".to_string());
    config
        .plan_judge_models
        .insert(AgentHarnessKind::Codex, "gpt-5.4".to_string());
    let scheduler = scheduler_with_judge_agent_runs_plan_deps_and_artifacts(
        Arc::clone(&scenario.automation_repo),
        Arc::clone(&scenario.run_repo),
        Arc::clone(&scenario.workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        scenario.session_repo.clone(),
        scenario.approval_repo.clone(),
        scenario.resumer.clone(),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        plan_judge.clone(),
        scenario.artifact_repo.clone(),
        config,
    );

    scheduler.tick_once().await.unwrap();
    wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;

    let calls = plan_judge.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].plan_judge_model.as_deref(),
        Some("claude-plan-sonnet")
    );
    assert!(!calls[0]
        .plan_judge_model
        .as_deref()
        .unwrap()
        .contains("gpt-"));
}

#[tokio::test]
async fn automation_scheduler_plan_judge_retries_invalid_json_once_then_applies_verdict() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact(
            "plan-artifact-1",
            "Retry should recover from malformed JSON.",
            1,
        )
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        "not-json".to_string(),
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    scheduler.tick_once().await.unwrap();
    wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;

    let calls = plan_judge.calls();
    assert_eq!(calls.len(), 2);
    assert!(!calls[0].retry_reminder);
    assert!(calls[1].retry_reminder);
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_revise_sets_pending_instructions_and_baseline() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan needs more detail.", 1)
        .await;
    let instructions =
        "Add the model-resolution falsification, retry behavior, and recovery delivery scenario.";
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_revise_verdict("plan-artifact-1", instructions),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge);

    scheduler.tick_once().await.unwrap();

    let judged = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;
    assert_eq!(
        judged.plan_pending_instructions.as_deref(),
        Some(instructions)
    );
    assert!(judged
        .plan_judge_verdict_json
        .as_deref()
        .unwrap()
        .contains("evaluatedArtifactId"));
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn automation_scheduler_delivered_plan_revision_is_consumed_and_repeat_then_exhausts() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan needs a revision.", 1)
        .await;
    let instructions = "Add explicit recovery coverage before implementing the automation run.";
    let repeated = " Add explicit recovery coverage before implementing the automation run! ";
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_revise_verdict("plan-artifact-1", instructions),
        valid_plan_revise_verdict("plan-artifact-1", repeated),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    scheduler.tick_once().await.unwrap();
    wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;

    scheduler.tick_once().await.unwrap();
    let delivered = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivered.status, AutomationRunStatus::Running);
    assert_eq!(delivered.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(delivered.plan_pending_instructions.is_none());
    assert!(delivered.plan_judge_verdict_json.is_some());
    assert_eq!(scenario.resumer.prompts().len(), 1);

    scenario
        .run_repo
        .compare_and_swap_status(
            &AutomationRunId::from_string("run-1"),
            AutomationRunStatus::Running,
            AutomationRunStatus::AwaitingPlanApproval,
            None,
            None,
        )
        .await
        .unwrap();

    scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 2).await;
    let automation = wait_for_automation_status(
        &scenario.automation_repo,
        &scenario.automation_id,
        AutomationStatus::Paused,
    )
    .await;
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE)
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::Failed);
    assert!(latest.plan_pending_instructions.is_none());
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_plan_judge_repeat_fingerprint_pauses_exhausted_without_overwriting_baseline(
) {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan still repeats.", 1)
        .await;
    let previous_instructions =
        "Add explicit crash recovery coverage for the plan revision delivery path before continuing.";
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &AutomationRunId::from_string("run-1"),
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::InProgress,
            Some(valid_plan_revise_verdict(
                "plan-artifact-1",
                previous_instructions,
            )),
            Some(Utc::now() + chrono::Duration::minutes(1)),
        )
        .await
        .unwrap();
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &AutomationRunId::from_string("run-1"),
            AutomationPlanJudgeState::InProgress,
            AutomationPlanJudgeState::None,
            None,
            None,
        )
        .await
        .unwrap();
    let baseline = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap()
        .plan_judge_verdict_json
        .unwrap();
    let repeated = " Add explicit crash recovery coverage for the plan revision delivery path before continuing! ";
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_revise_verdict("plan-artifact-1", repeated),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge);

    scheduler.tick_once().await.unwrap();

    let automation = wait_for_automation_status(
        &scenario.automation_repo,
        &scenario.automation_id,
        AutomationStatus::Paused,
    )
    .await;
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE)
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::Failed);
    assert_eq!(
        latest.plan_judge_verdict_json.as_deref(),
        Some(baseline.as_str())
    );
    assert!(latest.plan_pending_instructions.is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_round_exhaustion_pauses_without_dispatch() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let run_id = AutomationRunId::from_string("run-1");
    scenario
        .run_repo
        .set_plan_revision_round(&run_id, 4)
        .await
        .unwrap();
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::default());
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.paused_automations, 1);
    assert_eq!(plan_judge.call_count(), 0);
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE)
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(latest.plan_pending_instructions.is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_revision_limit_counts_judge_issued_revisions_timeline() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan v1.", 1)
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_revise_verdict(
            "plan-artifact-1",
            "Add authentication threat-model coverage before implementation.",
        ),
        valid_plan_revise_verdict(
            "plan-artifact-2",
            "Document database migration rollback validation before implementation.",
        ),
        valid_plan_revise_verdict(
            "plan-artifact-3",
            "Specify renderer accessibility checks before implementation.",
        ),
    ]));
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());
    let run_id = AutomationRunId::from_string("run-1");

    for (artifact_id, version, body) in [
        ("plan-artifact-2", 2, "Plan v2."),
        ("plan-artifact-3", 3, "Plan v3."),
        ("plan-artifact-4", 4, "Plan v4."),
    ] {
        scheduler.tick_once().await.unwrap();
        wait_for_latest_plan_judge_state(
            &scenario.run_repo,
            &scenario.automation_id,
            AutomationPlanJudgeState::Done,
        )
        .await;
        scheduler.tick_once().await.unwrap();
        scenario
            .seed_plan_artifact(artifact_id, body, version)
            .await;
        scenario
            .session_repo
            .update_plan_artifact_id(&scenario.session_id, Some(artifact_id.to_string()))
            .await
            .unwrap();
        scenario
            .run_repo
            .compare_and_swap_status(
                &run_id,
                AutomationRunStatus::Running,
                AutomationRunStatus::AwaitingPlanApproval,
                None,
                None,
            )
            .await
            .unwrap();
    }

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.paused_automations, 1);
    assert_eq!(plan_judge.call_count(), 3);
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE)
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_revision_round, 4);
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(latest.plan_pending_instructions.is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_artifact_read_error_pauses_failed_without_approval() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "missing-plan-artifact").await;
    scenario
        .automation_repo
        .update_goal_items_json(
            &scenario.automation_id,
            Some(
                json!([
                    { "id": "item-1", "title": "First", "status": "done" },
                    { "id": "item-2", "title": "Second", "status": "in_progress" }
                ])
                .to_string(),
            ),
        )
        .await
        .unwrap();
    scenario.use_automatic_plan_approval("claude").await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::default());
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    scheduler.tick_once().await.unwrap();

    let automation = wait_for_automation_status(
        &scenario.automation_repo,
        &scenario.automation_id,
        AutomationStatus::Paused,
    )
    .await;
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE)
    );
    assert_eq!(
        item_status(automation.goal_items_json.as_deref().unwrap(), "item-2"),
        "in_progress"
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::Failed);
    assert!(latest.plan_judge_verdict_json.is_none());
    assert_eq!(plan_judge.call_count(), 0);
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn automation_scheduler_stale_plan_judge_failure_after_repark_reset_is_discarded() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Original plan.", 1)
        .await;
    scenario
        .seed_plan_artifact("plan-artifact-2", "Human revised plan.", 2)
        .await;
    let stale_judge = Arc::new(ResettingFailingPlanJudgeInvoker {
        run_repo: scenario.run_repo.clone(),
        session_repo: scenario.session_repo.clone(),
        session_id: scenario.session_id.clone(),
        replacement_artifact_id: ArtifactId::from_string("plan-artifact-2".to_string()),
        calls: Mutex::new(0),
    });
    let scheduler = scenario.scheduler_with_plan_judge(stale_judge.clone());

    scheduler.tick_once().await.unwrap();
    wait_for_resetting_plan_judge_call_count(&stale_judge, 1).await;
    wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::None,
    )
    .await;

    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    assert!(automation.paused_reason_code.is_none());
    let reset = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reset.plan_judge_state, AutomationPlanJudgeState::None);

    let replacement_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-2"),
    ]));
    let replacement_scheduler = scenario.scheduler_with_plan_judge(replacement_judge.clone());
    replacement_scheduler.tick_once().await.unwrap();
    wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;

    let calls = replacement_judge.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].plan_artifact_id, "plan-artifact-2");
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    assert!(automation.paused_reason_code.is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_discarded_when_plan_artifact_changes_before_application() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Original plan.", 1)
        .await;
    scenario
        .seed_plan_artifact("plan-artifact-2", "Revised plan.", 2)
        .await;
    let plan_judge = Arc::new(MutatingPlanJudgeInvoker {
        session_repo: scenario.session_repo.clone(),
        session_id: scenario.session_id.clone(),
        replacement_artifact_id: ArtifactId::from_string("plan-artifact-2".to_string()),
        output: valid_plan_approve_verdict("plan-artifact-1"),
    });
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge);

    scheduler.tick_once().await.unwrap();

    let latest = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;
    assert!(latest.plan_judge_verdict_json.is_none());
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_writer_conflict_discards_without_pause_or_approval() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::with_outputs(vec![
        valid_plan_approve_verdict("plan-artifact-1"),
    ]));
    let session_repo: Arc<dyn IdeationSessionRepository> = scenario.session_repo.clone();
    let resumer: Arc<dyn AutomationRunResumer> = scenario.resumer.clone();
    let artifact_repo: Arc<dyn ArtifactRepository> = scenario.artifact_repo.clone();
    let scheduler = scheduler_with_judge_agent_runs_plan_deps_artifacts_and_writer(
        Arc::clone(&scenario.automation_repo),
        Arc::clone(&scenario.run_repo),
        Arc::clone(&scenario.workspace_repo),
        Arc::new(MemoryAgentRunRepository::new()),
        session_repo,
        scenario.approval_repo.clone(),
        Arc::new(ConflictingPlanArtifactApprovalWriter),
        resumer,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        plan_judge.clone(),
        artifact_repo,
        AutomationSchedulerConfig::default(),
    );

    scheduler.tick_once().await.unwrap();
    wait_for_plan_judge_call_count(&plan_judge, 1).await;

    let latest = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::None,
    )
    .await;
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_none());
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    assert!(automation.paused_reason_code.is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_approve_after_human_approval_keeps_user_attribution() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let plan_judge = Arc::new(ApprovingPlanJudgeInvoker {
        approval_repo: scenario.approval_repo.clone(),
        session_id: scenario.session_id.clone(),
        artifact_id: ArtifactId::from_string("plan-artifact-1".to_string()),
        artifact_version: 1,
        output: valid_plan_approve_verdict("plan-artifact-1"),
    });
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge);

    scheduler.tick_once().await.unwrap();
    let latest = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;

    assert!(latest.plan_judge_verdict_json.is_some());
    let approval = scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approval.artifact_id.as_str(), "plan-artifact-1");
    assert_eq!(approval.artifact_version, 1);
    assert_eq!(approval.approved_by, "user");
}

#[tokio::test]
async fn automation_scheduler_plan_judge_superseded_approve_writes_no_approval() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let plan_judge = Arc::new(SupersedingPlanJudgeInvoker {
        run_repo: scenario.run_repo.clone(),
        superseded_state: AutomationPlanJudgeState::Failed,
        output: valid_plan_approve_verdict("plan-artifact-1"),
    });
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge);

    scheduler.tick_once().await.unwrap();
    let latest = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Failed,
    )
    .await;

    assert!(latest.plan_judge_verdict_json.is_none());
    assert!(latest.plan_pending_instructions.is_none());
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_superseded_revise_writes_no_pending_or_baseline() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let plan_judge = Arc::new(SupersedingPlanJudgeInvoker {
        run_repo: scenario.run_repo.clone(),
        superseded_state: AutomationPlanJudgeState::Failed,
        output: valid_plan_revise_verdict(
            "plan-artifact-1",
            "These instructions arrived after the judge cycle was superseded.",
        ),
    });
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge);

    scheduler.tick_once().await.unwrap();
    let latest = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Failed,
    )
    .await;

    assert!(latest.plan_judge_verdict_json.is_none());
    assert!(latest.plan_pending_instructions.is_none());
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_revise_after_human_approval_discards_revision() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let instructions =
        "Do not persist this revision because the plan was approved while judgment ran.";
    let plan_judge = Arc::new(ApprovingPlanJudgeInvoker {
        approval_repo: scenario.approval_repo.clone(),
        session_id: scenario.session_id.clone(),
        artifact_id: ArtifactId::from_string("plan-artifact-1".to_string()),
        artifact_version: 1,
        output: valid_plan_revise_verdict("plan-artifact-1", instructions),
    });
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge);

    scheduler.tick_once().await.unwrap();
    let latest = wait_for_latest_plan_judge_state(
        &scenario.run_repo,
        &scenario.automation_id,
        AutomationPlanJudgeState::Done,
    )
    .await;

    assert!(latest.plan_judge_verdict_json.is_none());
    assert!(latest.plan_pending_instructions.is_none());
    let approval = scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approval.approved_by, "user");
}

#[tokio::test]
async fn automation_scheduler_plan_judge_revision_recovery_rederives_lost_pending_instructions() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let instructions =
        "Restore the revision instructions if a crash cleared pending delivery before sending.";
    let run_id = AutomationRunId::from_string("run-1");
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &run_id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::Done,
            Some(valid_plan_revise_verdict("plan-artifact-1", instructions)),
            None,
        )
        .await
        .unwrap();
    scenario
        .run_repo
        .set_plan_pending_instructions(&run_id, None)
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    scheduler.tick_once().await.unwrap();

    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AwaitingPlanApproval);
    assert_eq!(
        latest.plan_pending_instructions.as_deref(),
        Some(instructions)
    );

    let second = scheduler.tick_once().await.unwrap();
    assert_eq!(second.failed_runs, 0);
    let resumed = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.status, AutomationRunStatus::Running);
    assert!(resumed.plan_pending_instructions.is_none());
    assert_eq!(scenario.resumer.prompts().len(), 1);
    assert!(scenario.resumer.prompts()[0].1.contains(instructions));
}

#[tokio::test]
async fn automation_scheduler_plan_judge_recovery_ignores_stored_done_for_stale_artifact() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Current plan body.", 1)
        .await;
    let run_id = AutomationRunId::from_string("run-1");
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &run_id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::Done,
            Some(valid_plan_revise_verdict(
                "plan-artifact-old",
                "Stale instructions must not apply to the current plan.",
            )),
            None,
        )
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 0);
    assert_eq!(summary.judge_failures, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(latest.plan_pending_instructions.is_none());
    assert!(scenario
        .approval_repo
        .get_by_session(&scenario.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn automation_scheduler_plan_judge_recovery_corrupt_done_verdict_fails_closed_once() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let run_id = AutomationRunId::from_string("run-1");
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &run_id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::Done,
            Some("not-json".to_string()),
            None,
        )
        .await
        .unwrap();
    let scheduler = scenario.scheduler();

    let first = scheduler.tick_once().await.unwrap();

    assert_eq!(first.judge_failures, 1);
    assert_eq!(first.paused_automations, 1);
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE)
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::Failed);

    let second = scheduler.tick_once().await.unwrap();
    assert_eq!(second.judge_failures, 0);
    assert_eq!(second.paused_automations, 0);
}

#[tokio::test]
async fn automation_scheduler_plan_judge_lease_expiry_pauses_and_failed_is_not_redispatched() {
    let scenario =
        ParkedPlanGateScenario::new(AutomationStatus::Active, None, "plan-artifact-1").await;
    scenario.use_automatic_plan_approval("claude").await;
    scenario
        .seed_plan_artifact("plan-artifact-1", "Plan body.", 1)
        .await;
    let run_id = AutomationRunId::from_string("run-1");
    scenario
        .run_repo
        .compare_and_swap_plan_judge_state(
            &run_id,
            AutomationPlanJudgeState::None,
            AutomationPlanJudgeState::InProgress,
            None,
            Some(Utc::now() - chrono::Duration::minutes(1)),
        )
        .await
        .unwrap();
    let plan_judge = Arc::new(RecordingPlanJudgeInvoker::default());
    let scheduler = scenario.scheduler_with_plan_judge(plan_judge.clone());

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judge_failures, 1);
    assert_eq!(summary.paused_automations, 1);
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE)
    );
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.plan_judge_state, AutomationPlanJudgeState::Failed);
    assert_eq!(plan_judge.call_count(), 0);

    scenario
        .automation_repo
        .compare_and_swap_status(
            &scenario.automation_id,
            AutomationStatus::Paused,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
        .unwrap();
    let resumed = scheduler.tick_once().await.unwrap();
    assert_eq!(resumed.judges_started, 0);
    assert_eq!(plan_judge.call_count(), 0);
}

#[tokio::test]
async fn automation_scheduler_resumes_plan_gate_paused_automation_on_matching_approval_then_delivers_next_tick(
) {
    let scenario = ParkedPlanGateScenario::new(
        AutomationStatus::Paused,
        Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE),
        "plan-artifact-1",
    )
    .await;
    scenario.approve("plan-artifact-1", 1);
    let scheduler = scenario.scheduler();

    let scan_summary = scheduler.tick_once().await.unwrap();

    assert_eq!(scan_summary.failed_runs, 0);
    assert_eq!(scan_summary.paused_automations, 0);
    assert_eq!(scan_summary.resumed_automations, 1);
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    assert!(automation.paused_reason_code.is_none());
    let parked = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parked.status, AutomationRunStatus::AwaitingPlanApproval);
    assert!(scenario.resumer.prompts().is_empty());

    let delivery_summary = scheduler.tick_once().await.unwrap();

    assert_eq!(delivery_summary.failed_runs, 0);
    let latest = scenario
        .run_repo
        .latest_for_automation(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert_eq!(scenario.resumer.prompts().len(), 1);
}

#[tokio::test]
async fn automation_scheduler_does_not_scan_paused_automation_for_other_reasons() {
    let scenario = ParkedPlanGateScenario::new(
        AutomationStatus::Paused,
        Some("user_paused"),
        "plan-artifact-1",
    )
    .await;
    scenario.approve("plan-artifact-1", 1);
    let scheduler = scenario.scheduler();

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.leased_automations, 0);
    let automation = scenario
        .automation_repo
        .get_by_id(&scenario.automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Paused);
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some("user_paused")
    );
    assert!(scenario.resumer.prompts().is_empty());
}

#[tokio::test]
async fn automation_scheduler_marks_plan_phase_agent_failed_when_agent_run_failed() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let (workspace, session) = plan_workspace_with_session(&conversation_id, None);
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    agent_run_repo
        .create(agent_run_with_status(
            conversation_id,
            AgentRunStatus::Failed,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(latest.error_code.as_deref(), Some("agent_failed"));
}

#[tokio::test]
async fn automation_scheduler_marks_plan_phase_cancelled_when_agent_run_cancelled() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut active = automation(automation_id.as_str(), AutomationStatus::Active);
    active.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "in_progress" }
        ])
        .to_string(),
    );
    automation_repo.create(active).await.unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let (workspace, session) = plan_workspace_with_session(&conversation_id, None);
    session_repo.create(session).await.unwrap();
    workspace_repo.create_or_update(workspace).await.unwrap();
    agent_run_repo
        .create(agent_run_with_status(
            conversation_id,
            AgentRunStatus::Cancelled,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with_judge_agent_runs_and_plan_deps(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        session_repo,
        Arc::new(MemoryPlanArtifactApprovalRepository::new()),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Cancelled);
    assert_eq!(latest.error_code.as_deref(), Some("agent_cancelled"));
    let automation = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(automation.goal_items_json.as_deref().unwrap(), "item-2"),
        "pending"
    );
}

#[tokio::test]
async fn automation_scheduler_running_timeout_prefers_agent_phase_started_at() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let old = Utc::now() - chrono::Duration::hours(2);
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::Running,
        Some(conversation_id.clone()),
    );
    run.started_at = Some(old);
    run.created_at = old;
    run.agent_phase_started_at = Some(Utc::now());
    run_repo.create_run(run).await.unwrap();
    workspace_repo
        .create_or_update(workspace(&conversation_id))
        .await
        .unwrap();
    agent_run_repo
        .create(agent_run_with_status(
            conversation_id,
            AgentRunStatus::Running,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with_judge_and_agent_runs(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig {
            max_run_duration: Duration::from_secs(60),
            ..AutomationSchedulerConfig::default()
        },
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
}

#[tokio::test]
async fn automation_scheduler_marks_no_changes_publish_outcome_as_agent_failed() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_push_status = Some("no_changes".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(latest.error_code.as_deref(), Some("no_changes"));
}

#[tokio::test]
async fn automation_scheduler_fails_pr_merged_run_when_agent_process_died_before_publishing() {
    // Regression: a `pr_merged` (auto-publish) run whose agent process was killed and
    // pruned (agent_run -> Cancelled) with no publication started must fail promptly on the
    // next tick, not linger Running until the max_run_duration backstop hours later.
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    // Default automation completion_signal is "pr_merged".
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    // Workspace exists but nothing was ever published (no PR, no push status).
    workspace_repo
        .create_or_update(workspace(&conversation_id))
        .await
        .unwrap();
    // Agent process was pruned as pid_missing -> agent_run Cancelled.
    let mut agent_run = AgentRun::new(conversation_id);
    agent_run.status = crate::domain::entities::AgentRunStatus::Cancelled;
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_and_agent_runs(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(latest.error_code.as_deref(), Some("agent_failed"));
}

#[tokio::test]
async fn automation_scheduler_keeps_running_pr_merged_run_while_agent_is_alive() {
    // A live agent (agent_run Running) with no publication yet must NOT be failed — the run
    // is legitimately in progress.
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    workspace_repo
        .create_or_update(workspace(&conversation_id))
        .await
        .unwrap();
    let agent_run = AgentRun::new(conversation_id); // defaults to Running
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_and_agent_runs(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
}

#[tokio::test]
async fn automation_scheduler_keeps_running_pr_merged_run_when_agent_completed_awaiting_review() {
    // Regression guard for the review -> auto-publish handoff (PR #628): a cleanly-finished
    // `pr_merged` run has agent_run `Completed` with no publication yet while the workspace
    // review runs (auto-publish defers until review passes, so no push status is set). The
    // scheduler is intentionally review-unaware, so it MUST NOT fail such a run — doing so
    // would kill healthy runs mid-review. Only a dead agent (Failed/Cancelled) fails here.
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    // Workspace exists but nothing has been published yet (review still pending).
    workspace_repo
        .create_or_update(workspace(&conversation_id))
        .await
        .unwrap();
    // Agent finished cleanly and is awaiting the review -> publish handoff.
    let mut agent_run = AgentRun::new(conversation_id);
    agent_run.status = crate::domain::entities::AgentRunStatus::Completed;
    agent_run_repo.create(agent_run).await.unwrap();
    let scheduler = scheduler_with_judge_and_agent_runs(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        agent_run_repo,
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::Running);
}

#[tokio::test]
async fn automation_scheduler_marks_publish_failure_as_agent_failed() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            Some(conversation_id.clone()),
        ))
        .await
        .unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_push_status = Some("failed".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.failed_runs, 1);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.status, AutomationRunStatus::AgentFailed);
    assert_eq!(latest.error_code.as_deref(), Some("publish_failed"));
}

#[tokio::test]
async fn automation_scheduler_judges_terminal_run_and_schedules_successor() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut active = automation_with_goal_items(automation_id.as_str(), AutomationStatus::Active);
    active.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "in_progress" }
        ])
        .to_string(),
    );
    automation_repo.create(active).await.unwrap();
    let mut run = automation_run("run-1", &automation_id, AutomationRunStatus::Merged, None);
    run.pr_number = Some(81);
    run.pr_base_ref_name = Some("main".to_string());
    run_repo.create_run(run).await.unwrap();
    let judge = Arc::new(RecordingJudgeInvoker::with_outputs(vec![
        valid_continue_verdict(),
    ]));
    let scheduler = scheduler_with_judge(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        judge.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 1);
    assert_eq!(summary.judges_succeeded, 0);
    assert_eq!(summary.successor_runs, 0);
    let runs = wait_for_run_count(&run_repo, &automation_id, 2).await;
    assert_eq!(judge.call_count(), 1);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].judge_state, AutomationJudgeState::Done);
    assert!(runs[0].judge_lease_expires_at.is_none());
    assert!(runs[0].judge_verdict_json.is_some());
    assert_eq!(runs[0].judge_model_id.as_deref(), Some("haiku"));
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
    assert_eq!(runs[1].prompt_author, AutomationPromptAuthor::Judge);
    assert_eq!(runs[1].base_from_run_id, Some(runs[0].id.clone()));
    assert_eq!(
        runs[1].run_prompt,
        "Implement item 2 from the automation goal. Keep the change scoped, include targeted tests, and publish the PR."
    );
    let automation = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(automation.goal_items_json.as_deref().unwrap(), "item-2"),
        "done"
    );
}

#[tokio::test]
async fn automation_scheduler_sweep_reverts_paused_judge_failed_goal_progress() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut paused = automation_with_goal_items(automation_id.as_str(), AutomationStatus::Paused);
    paused.paused_reason_code = Some("judge_failed".to_string());
    paused.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "in_progress" }
        ])
        .to_string(),
    );
    automation_repo.create(paused).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &automation_id,
        AutomationRunStatus::AgentFailed,
        None,
    );
    run.judge_state = AutomationJudgeState::Failed;
    run_repo.create_run(run).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.active_automations, 0);
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "item-2"),
        "pending"
    );
}

#[tokio::test]
async fn automation_scheduler_sweep_forward_fills_active_running_goal_progress() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation_with_goal_items(
            automation_id.as_str(),
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Running,
            None,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.active_with_runs, 1);
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "item-2"),
        "in_progress"
    );
}

#[tokio::test]
async fn automation_scheduler_sweep_keeps_signal_terminal_judge_done_goal_progress() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut active = automation_with_goal_items(automation_id.as_str(), AutomationStatus::Active);
    active.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "in_progress" }
        ])
        .to_string(),
    );
    automation_repo.create(active).await.unwrap();
    let mut run = automation_run("run-1", &automation_id, AutomationRunStatus::Merged, None);
    run.judge_state = AutomationJudgeState::Done;
    run_repo.create_run(run).await.unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.successor_runs, 0);
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "item-2"),
        "in_progress"
    );
}

#[tokio::test]
async fn automation_scheduler_sweep_does_not_revert_plan_gate_paused_goal_progress() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    let mut paused = automation_with_goal_items(automation_id.as_str(), AutomationStatus::Paused);
    paused.paused_reason_code = Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string());
    paused.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "in_progress" }
        ])
        .to_string(),
    );
    automation_repo.create(paused).await.unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::Cancelled,
            None,
        ))
        .await
        .unwrap();
    let scheduler = scheduler_with(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.resumed_automations, 0);
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "item-2"),
        "in_progress"
    );
}

#[tokio::test]
async fn automation_scheduler_detaches_judge_without_blocking_other_signal_checks() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let judging_id = AutomationId::from_string("automation-judging");
    let signal_id = AutomationId::from_string("automation-signal");
    automation_repo
        .create(automation(judging_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    automation_repo
        .create(automation(signal_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    run_repo
        .create_run(automation_run(
            "run-judging",
            &judging_id,
            AutomationRunStatus::Merged,
            None,
        ))
        .await
        .unwrap();
    let conversation_id = ChatConversationId::from_string("conversation-signal");
    let mut published = automation_run(
        "run-signal",
        &signal_id,
        AutomationRunStatus::Published,
        Some(conversation_id.clone()),
    );
    published.pr_number = Some(91);
    run_repo.create_run(published).await.unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(91);
    workspace.publication_pr_status = Some("open".to_string());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let judge = Arc::new(BlockingJudgeInvoker::new(valid_stop_verdict(true)));
    let scheduler = scheduler_with_judge(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::with_responses(vec![Ok(
            PrStatus::Merged {
                merge_commit_sha: Some("def456".to_string()),
                merged_at: Some("2026-07-05T12:00:00Z".to_string()),
            },
        )])),
        judge.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = timeout(Duration::from_millis(500), scheduler.tick_once())
        .await
        .expect("tick should not wait for blocked judge")
        .unwrap();

    assert_eq!(summary.judges_started, 1);
    assert_eq!(summary.merged_runs, 1);
    let signal_run = run_repo
        .latest_for_automation(&signal_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(signal_run.status, AutomationRunStatus::Merged);
    assert_eq!(signal_run.merge_commit_sha.as_deref(), Some("def456"));

    judge.release();
    let judged =
        wait_for_latest_judge_state(&run_repo, &judging_id, AutomationJudgeState::Done).await;
    assert_eq!(judged.judge_state, AutomationJudgeState::Done);
    assert_eq!(judge.call_count(), 1);
    let automation =
        wait_for_automation_status(&automation_repo, &judging_id, AutomationStatus::Completed)
            .await;
    assert_eq!(automation.status, AutomationStatus::Completed);
}

#[tokio::test]
async fn automation_scheduler_retries_invalid_judge_output_once_then_pauses() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation_with_goal_items(
            automation_id.as_str(),
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &automation_id,
            AutomationRunStatus::AgentFailed,
            None,
        ))
        .await
        .unwrap();
    let judge = Arc::new(RecordingJudgeInvoker::with_outputs(vec![
        "{\"decision\":\"continue\"}".to_string(),
        "still not json".to_string(),
    ]));
    let scheduler = scheduler_with_judge(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        judge.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 1);
    assert_eq!(summary.judge_failures, 0);
    assert_eq!(summary.successor_runs, 0);
    let latest =
        wait_for_latest_judge_state(&run_repo, &automation_id, AutomationJudgeState::Failed).await;
    assert_eq!(judge.call_count(), 2);
    assert!(latest.judge_verdict_json.is_none());
    assert!(latest.judge_lease_expires_at.is_none());
    assert!(latest
        .error_detail
        .as_deref()
        .unwrap()
        .contains("still not json"));
    let automation =
        wait_for_automation_status(&automation_repo, &automation_id, AutomationStatus::Paused)
            .await;
    assert_eq!(automation.status, AutomationStatus::Paused);
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some("judge_failed")
    );
    assert_eq!(
        item_status(automation.goal_items_json.as_deref().unwrap(), "item-2"),
        "pending"
    );
}

#[tokio::test]
async fn automation_scheduler_marks_stale_in_progress_judge_failed() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let mut run = automation_run("run-1", &automation_id, AutomationRunStatus::Merged, None);
    run.judge_state = AutomationJudgeState::InProgress;
    run.judge_lease_expires_at = Some(Utc::now() - chrono::Duration::minutes(1));
    run.updated_at = Utc::now();
    run_repo.create_run(run).await.unwrap();
    let config = AutomationSchedulerConfig {
        judge_timeout: Duration::from_secs(60),
        ..AutomationSchedulerConfig::default()
    };
    let judge = Arc::new(RecordingJudgeInvoker::default());
    let scheduler = scheduler_with_judge(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        judge.clone(),
        config,
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judge_failures, 1);
    assert_eq!(summary.paused_automations, 1);
    assert_eq!(judge.call_count(), 0);
    let latest = run_repo
        .latest_for_automation(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.judge_state, AutomationJudgeState::Failed);
    assert_eq!(
        latest.error_detail.as_deref(),
        Some("Automation judge exceeded judge_timeout_secs")
    );
}

#[tokio::test]
async fn automation_scheduler_redrives_failed_judge_after_resume() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation(automation_id.as_str(), AutomationStatus::Active))
        .await
        .unwrap();
    let mut run = automation_run("run-1", &automation_id, AutomationRunStatus::Merged, None);
    run.judge_state = AutomationJudgeState::Failed;
    run.error_detail = Some("prior invalid output".to_string());
    run_repo.create_run(run).await.unwrap();
    let judge = Arc::new(RecordingJudgeInvoker::with_outputs(vec![
        valid_stop_verdict(true),
    ]));
    let scheduler = scheduler_with_judge(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        judge,
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 1);
    assert_eq!(summary.judges_succeeded, 0);
    let latest =
        wait_for_latest_judge_state(&run_repo, &automation_id, AutomationJudgeState::Done).await;
    assert_eq!(latest.judge_state, AutomationJudgeState::Done);
    assert!(latest.judge_lease_expires_at.is_none());
    let automation = wait_for_automation_status(
        &automation_repo,
        &automation_id,
        AutomationStatus::Completed,
    )
    .await;
    assert_eq!(automation.status, AutomationStatus::Completed);
}

#[tokio::test]
async fn automation_scheduler_consumes_stored_continue_verdict_without_duplicate_judge() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let automation_id = AutomationId::from_string("automation-1");
    automation_repo
        .create(automation_with_goal_items(
            automation_id.as_str(),
            AutomationStatus::Active,
        ))
        .await
        .unwrap();
    let mut run = automation_run("run-1", &automation_id, AutomationRunStatus::Merged, None);
    run.judge_state = AutomationJudgeState::Done;
    run.judge_verdict_json = Some(valid_continue_verdict());
    run_repo.create_run(run).await.unwrap();
    let judge = Arc::new(RecordingJudgeInvoker::default());
    let scheduler = scheduler_with_judge(
        Arc::clone(&automation_repo),
        Arc::clone(&run_repo),
        workspace_repo,
        Arc::new(RecordingSignalChecker::default()),
        judge.clone(),
        AutomationSchedulerConfig::default(),
    );

    let summary = scheduler.tick_once().await.unwrap();

    assert_eq!(summary.judges_started, 0);
    assert_eq!(summary.successor_runs, 1);
    assert_eq!(judge.call_count(), 0);
    let runs = run_repo.list_for_automation(&automation_id).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].prompt_author, AutomationPromptAuthor::Judge);
}

/// Artifact repository that fails every `get_by_id`, delegating all other
/// methods to an inner in-memory repo. Only `get_by_id` is exercised by the
/// `load_spec_attachment` fail-open tests.
struct FailingArtifactRepository {
    inner: MemoryArtifactRepository,
}

impl FailingArtifactRepository {
    fn new() -> Self {
        Self {
            inner: MemoryArtifactRepository::new(),
        }
    }
}

#[async_trait]
impl ArtifactRepository for FailingArtifactRepository {
    async fn create(&self, artifact: Artifact) -> AppResult<Artifact> {
        self.inner.create(artifact).await
    }

    async fn get_by_id(&self, _id: &ArtifactId) -> AppResult<Option<Artifact>> {
        Err(AppError::Infrastructure(
            "artifact lookup failed".to_string(),
        ))
    }

    async fn get_by_id_at_version(
        &self,
        id: &ArtifactId,
        version: u32,
    ) -> AppResult<Option<Artifact>> {
        self.inner.get_by_id_at_version(id, version).await
    }

    async fn get_by_bucket(
        &self,
        bucket_id: &crate::domain::entities::ArtifactBucketId,
    ) -> AppResult<Vec<Artifact>> {
        self.inner.get_by_bucket(bucket_id).await
    }

    async fn get_by_type(&self, artifact_type: ArtifactType) -> AppResult<Vec<Artifact>> {
        self.inner.get_by_type(artifact_type).await
    }

    async fn get_by_task(
        &self,
        task_id: &crate::domain::entities::TaskId,
    ) -> AppResult<Vec<Artifact>> {
        self.inner.get_by_task(task_id).await
    }

    async fn get_by_process(
        &self,
        process_id: &crate::domain::entities::ProcessId,
    ) -> AppResult<Vec<Artifact>> {
        self.inner.get_by_process(process_id).await
    }

    async fn update(&self, artifact: &Artifact) -> AppResult<()> {
        self.inner.update(artifact).await
    }

    async fn delete(&self, id: &ArtifactId) -> AppResult<()> {
        self.inner.delete(id).await
    }

    async fn get_derived_from(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        self.inner.get_derived_from(artifact_id).await
    }

    async fn get_related(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        self.inner.get_related(artifact_id).await
    }

    async fn add_relation(
        &self,
        relation: crate::domain::entities::ArtifactRelation,
    ) -> AppResult<crate::domain::entities::ArtifactRelation> {
        self.inner.add_relation(relation).await
    }

    async fn get_relations(
        &self,
        artifact_id: &ArtifactId,
    ) -> AppResult<Vec<crate::domain::entities::ArtifactRelation>> {
        self.inner.get_relations(artifact_id).await
    }

    async fn get_relations_by_type(
        &self,
        artifact_id: &ArtifactId,
        relation_type: crate::domain::entities::ArtifactRelationType,
    ) -> AppResult<Vec<crate::domain::entities::ArtifactRelation>> {
        self.inner
            .get_relations_by_type(artifact_id, relation_type)
            .await
    }

    async fn delete_relation(&self, from_id: &ArtifactId, to_id: &ArtifactId) -> AppResult<()> {
        self.inner.delete_relation(from_id, to_id).await
    }

    async fn create_with_previous_version(
        &self,
        artifact: Artifact,
        previous_version_id: ArtifactId,
    ) -> AppResult<Artifact> {
        self.inner
            .create_with_previous_version(artifact, previous_version_id)
            .await
    }

    async fn get_version_history(
        &self,
        id: &ArtifactId,
    ) -> AppResult<Vec<crate::domain::repositories::ArtifactVersionSummary>> {
        self.inner.get_version_history(id).await
    }

    async fn resolve_latest_artifact_id(&self, id: &ArtifactId) -> AppResult<ArtifactId> {
        self.inner.resolve_latest_artifact_id(id).await
    }

    async fn archive(&self, id: &ArtifactId) -> AppResult<Artifact> {
        self.inner.archive(id).await
    }
}

#[tokio::test]
async fn load_spec_attachment_returns_truncated_content_when_spec_present() {
    let mem = MemoryArtifactRepository::new();
    // ~30KB of text — larger than SPEC_ATTACHMENT_MAX_BYTES (10KB).
    let spec_text = "spec ".repeat(6_000);
    let artifact = Artifact::new_inline(
        "Automation spec",
        ArtifactType::Specification,
        spec_text.clone(),
        "user",
    );
    let created = mem.create(artifact).await.unwrap();
    let repo: Arc<dyn ArtifactRepository> = Arc::new(mem);

    let mut automation = automation("auto-spec", AutomationStatus::Active);
    automation.spec_artifact_id = Some(created.id.as_str().to_string());

    let attachments = load_spec_attachment(&repo, &automation).await;

    assert_eq!(attachments.len(), 1);
    let attachment = &attachments[0];
    assert_eq!(attachment.file_name, "Automation spec");
    let content = attachment.text_content.as_deref().unwrap();
    assert!(content.len() <= SPEC_ATTACHMENT_MAX_BYTES);
    assert!(content.len() < spec_text.len());
    assert_eq!(attachment.file_size, Some(content.len() as i64));
}

#[tokio::test]
async fn load_spec_attachment_empty_when_no_spec_linked() {
    let repo: Arc<dyn ArtifactRepository> = Arc::new(MemoryArtifactRepository::new());

    let mut automation = automation("auto", AutomationStatus::Active);
    automation.spec_artifact_id = None;
    assert!(load_spec_attachment(&repo, &automation).await.is_empty());

    automation.spec_artifact_id = Some("   ".to_string());
    assert!(load_spec_attachment(&repo, &automation).await.is_empty());
}

#[tokio::test]
async fn load_spec_attachment_empty_when_artifact_missing() {
    let repo: Arc<dyn ArtifactRepository> = Arc::new(MemoryArtifactRepository::new());

    let mut automation = automation("auto", AutomationStatus::Active);
    automation.spec_artifact_id = Some("does-not-exist".to_string());

    assert!(load_spec_attachment(&repo, &automation).await.is_empty());
}

#[tokio::test]
async fn load_spec_attachment_empty_when_repo_errors() {
    let repo: Arc<dyn ArtifactRepository> = Arc::new(FailingArtifactRepository::new());

    let mut automation = automation("auto", AutomationStatus::Active);
    automation.spec_artifact_id = Some("spec-1".to_string());

    assert!(load_spec_attachment(&repo, &automation).await.is_empty());
}
