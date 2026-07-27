use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use ralphx_domain::entities::is_open_automation_run;
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;
use serde_json::{json, Value};

use crate::application::automation::decomposition_verifier::{
    parse_authoring_state, AutomationAuthoringMode, AutomationDecompositionVerificationStatus,
    AutomationDecompositionVerifier, AutomationDecompositionVerifierInvocation,
    AutomationDecompositionVerifierInvocationOutput, AutomationDecompositionVerifierInvoker,
    AutomationGoalReplanState, AutomationGoalReplanStatus,
};
use crate::application::automation::judge::{
    automation_judge_loop_suspected, AutomationGoalItemStatus, AutomationJudgeDecision,
    AutomationJudgeGoalItemProposal, AutomationJudgeItemStatusUpdate,
    AutomationJudgeNextBaseBranch, AutomationJudgeVerdict,
};
use crate::application::automation::plan_gate::{
    AUTOMATION_PLAN_GATE_TRIGGER_RUN_NOW_ERROR_CODE, PLAN_JUDGE_FAILED_PAUSED_REASON_CODE,
};
use crate::application::automation::service::{
    run_status_blocks_trigger_run_now, run_status_is_cancellable, ApplyAutomationJudgeVerdictInput,
    AutomationJudgeApplyNoopReason, AutomationRunNowAction, AutomationService,
    CompleteAutomationJudgeInput, CreateAutomationDraftInput, CreateAutomationRunInput,
    CreateMergedBaseSuccessorRunInput, PendingGoalReplanApplyOutcome, UpdateAutomationConfigInput,
    UpdateAutomationSettingsInput, AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE,
};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, NoopAutomationEventEmitter,
};
use crate::application::{AppState, NotificationService};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactId, Automation,
    AutomationId, AutomationJudgeState, AutomationPlanApprovalMode, AutomationPlanJudgeState,
    AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatConversationId, IdeationAnalysisBaseRefKind,
    ProjectId, DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ArtifactRepository, ArtifactVersionSummary,
    AutomationConfigPatch, AutomationRepository, AutomationRunRepository, AutomationSettingsPatch,
};
use crate::domain::services::GithubServiceTrait;
use crate::error::AppError;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryArtifactRepository,
    MemoryAutomationRepository, MemoryAutomationRunRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn notification_service() -> Arc<NotificationService> {
    AppState::new_test().notification_service()
}

/// Artifact repository fake that delegates storage to an in-memory repo while
/// recording `create_with_previous_version` links so spec-versioning tests can
/// assert the `previous_version_id` chain (memory repo drops that link).
#[derive(Default)]
struct RecordingArtifactRepository {
    inner: MemoryArtifactRepository,
    versioned_from: Mutex<Vec<(String, String)>>,
}

impl RecordingArtifactRepository {
    /// Recorded `(previous_version_id, new_artifact_id)` pairs, in call order.
    fn versioned_from(&self) -> Vec<(String, String)> {
        self.versioned_from.lock().unwrap().clone()
    }
}

#[async_trait]
impl ArtifactRepository for RecordingArtifactRepository {
    async fn create(&self, artifact: Artifact) -> crate::error::AppResult<Artifact> {
        self.inner.create(artifact).await
    }

    async fn get_by_id(&self, id: &ArtifactId) -> crate::error::AppResult<Option<Artifact>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_id_at_version(
        &self,
        id: &ArtifactId,
        version: u32,
    ) -> crate::error::AppResult<Option<Artifact>> {
        self.inner.get_by_id_at_version(id, version).await
    }

    async fn get_by_bucket(
        &self,
        bucket_id: &crate::domain::entities::ArtifactBucketId,
    ) -> crate::error::AppResult<Vec<Artifact>> {
        self.inner.get_by_bucket(bucket_id).await
    }

    async fn get_by_type(
        &self,
        artifact_type: crate::domain::entities::ArtifactType,
    ) -> crate::error::AppResult<Vec<Artifact>> {
        self.inner.get_by_type(artifact_type).await
    }

    async fn get_by_task(
        &self,
        task_id: &crate::domain::entities::TaskId,
    ) -> crate::error::AppResult<Vec<Artifact>> {
        self.inner.get_by_task(task_id).await
    }

    async fn get_by_process(
        &self,
        process_id: &crate::domain::entities::ProcessId,
    ) -> crate::error::AppResult<Vec<Artifact>> {
        self.inner.get_by_process(process_id).await
    }

    async fn update(&self, artifact: &Artifact) -> crate::error::AppResult<()> {
        self.inner.update(artifact).await
    }

    async fn delete(&self, id: &ArtifactId) -> crate::error::AppResult<()> {
        self.inner.delete(id).await
    }

    async fn get_derived_from(
        &self,
        artifact_id: &ArtifactId,
    ) -> crate::error::AppResult<Vec<Artifact>> {
        self.inner.get_derived_from(artifact_id).await
    }

    async fn get_related(
        &self,
        artifact_id: &ArtifactId,
    ) -> crate::error::AppResult<Vec<Artifact>> {
        self.inner.get_related(artifact_id).await
    }

    async fn add_relation(
        &self,
        relation: crate::domain::entities::ArtifactRelation,
    ) -> crate::error::AppResult<crate::domain::entities::ArtifactRelation> {
        self.inner.add_relation(relation).await
    }

    async fn get_relations(
        &self,
        artifact_id: &ArtifactId,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::ArtifactRelation>> {
        self.inner.get_relations(artifact_id).await
    }

    async fn get_relations_by_type(
        &self,
        artifact_id: &ArtifactId,
        relation_type: crate::domain::entities::ArtifactRelationType,
    ) -> crate::error::AppResult<Vec<crate::domain::entities::ArtifactRelation>> {
        self.inner
            .get_relations_by_type(artifact_id, relation_type)
            .await
    }

    async fn delete_relation(
        &self,
        from_id: &ArtifactId,
        to_id: &ArtifactId,
    ) -> crate::error::AppResult<()> {
        self.inner.delete_relation(from_id, to_id).await
    }

    async fn create_with_previous_version(
        &self,
        artifact: Artifact,
        previous_version_id: ArtifactId,
    ) -> crate::error::AppResult<Artifact> {
        self.versioned_from.lock().unwrap().push((
            previous_version_id.as_str().to_string(),
            artifact.id.as_str().to_string(),
        ));
        self.inner.create(artifact).await
    }

    async fn get_version_history(
        &self,
        id: &ArtifactId,
    ) -> crate::error::AppResult<Vec<ArtifactVersionSummary>> {
        self.inner.get_version_history(id).await
    }

    async fn resolve_latest_artifact_id(
        &self,
        id: &ArtifactId,
    ) -> crate::error::AppResult<ArtifactId> {
        self.inner.resolve_latest_artifact_id(id).await
    }

    async fn archive(&self, id: &ArtifactId) -> crate::error::AppResult<Artifact> {
        self.inner.archive(id).await
    }
}

#[derive(Default)]
struct RecordingEmitter {
    events: Mutex<Vec<AutomationEvent>>,
}

impl RecordingEmitter {
    fn events(&self) -> Vec<AutomationEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl AutomationEventEmitter for RecordingEmitter {
    fn emit(&self, event: AutomationEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn service_with_emitter(
    event_emitter: Arc<dyn AutomationEventEmitter>,
) -> (
    AutomationService,
    Arc<MemoryAutomationRepository>,
    Arc<MemoryAutomationRunRepository>,
) {
    let (service, automation_repo, run_repo, _artifact_repo) =
        service_with_emitter_and_artifacts(event_emitter);
    (service, automation_repo, run_repo)
}

fn service_with_emitter_and_artifacts(
    event_emitter: Arc<dyn AutomationEventEmitter>,
) -> (
    AutomationService,
    Arc<MemoryAutomationRepository>,
    Arc<MemoryAutomationRunRepository>,
    Arc<RecordingArtifactRepository>,
) {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let artifact_repo = Arc::new(RecordingArtifactRepository::default());
    let service = AutomationService::new(
        automation_repo.clone(),
        run_repo.clone(),
        event_emitter,
        artifact_repo.clone(),
        notification_service(),
    );
    (service, automation_repo, run_repo, artifact_repo)
}

fn service_with_auto_merge_controls(
    event_emitter: Arc<dyn AutomationEventEmitter>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    github: Arc<MockGithubService>,
) -> (
    AutomationService,
    Arc<MemoryAutomationRepository>,
    Arc<MemoryAutomationRunRepository>,
) {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let artifact_repo = Arc::new(RecordingArtifactRepository::default());
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> = workspace_repo;
    let github_trait: Arc<dyn GithubServiceTrait> = github;
    let service = AutomationService::new(
        automation_repo.clone(),
        run_repo.clone(),
        event_emitter,
        artifact_repo,
        notification_service(),
    )
    .with_pr_auto_merge_controls(workspace_repo_trait, Some(github_trait));
    (service, automation_repo, run_repo)
}

struct StaticDecompositionVerifierInvoker {
    raw_output: String,
    mutate_repo: Option<Arc<MemoryAutomationRepository>>,
}

#[async_trait]
impl AutomationDecompositionVerifierInvoker for StaticDecompositionVerifierInvoker {
    async fn invoke(
        &self,
        input: AutomationDecompositionVerifierInvocation,
    ) -> crate::error::AppResult<AutomationDecompositionVerifierInvocationOutput> {
        if let Some(repo) = self.mutate_repo.as_ref() {
            repo.update_goal_items_json(
                &input.automation.id,
                Some(
                    r#"[{"id":"phase-new","title":"Changed while verifying","status":"pending"}]"#
                        .to_string(),
                ),
            )
            .await?;
        }
        Ok(AutomationDecompositionVerifierInvocationOutput {
            raw_output: self.raw_output.clone(),
            model_id: Some("verifier-model".to_string()),
        })
    }
}

struct SequenceDecompositionVerifierInvoker {
    raw_outputs: Mutex<VecDeque<String>>,
    retry_flags: Mutex<Vec<bool>>,
}

#[async_trait]
impl AutomationDecompositionVerifierInvoker for SequenceDecompositionVerifierInvoker {
    async fn invoke(
        &self,
        input: AutomationDecompositionVerifierInvocation,
    ) -> crate::error::AppResult<AutomationDecompositionVerifierInvocationOutput> {
        self.retry_flags.lock().unwrap().push(input.retry_reminder);
        let raw_output = self
            .raw_outputs
            .lock()
            .unwrap()
            .pop_front()
            .expect("test verifier output");
        Ok(AutomationDecompositionVerifierInvocationOutput {
            raw_output,
            model_id: Some("verifier-model".to_string()),
        })
    }
}

fn approved_decomposition_output() -> String {
    json!({
        "decision": "approve",
        "reason": "The phases cover the complete spec in dependency-safe order.",
        "confidence": "high",
        "findings": []
    })
    .to_string()
}

fn revision_decomposition_output() -> String {
    json!({
        "decision": "revise",
        "reason": "The plan needs a clearer frontend follow-up phase.",
        "confidence": "medium",
        "findings": [{
            "severity": "medium",
            "category": "phase_boundaries",
            "description": "The frontend work is not split into its own phase.",
            "goalItemIds": ["phase-2"]
        }]
    })
    .to_string()
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
        goal_items_json: Some(
            r#"[{"id":"phase-1","title":"Run 1","status":"pending"}]"#.to_string(),
        ),
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
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn automation_run(
    id: &str,
    automation_id: &AutomationId,
    run_index: i64,
    status: AutomationRunStatus,
    judge_state: AutomationJudgeState,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: automation_id.clone(),
        run_index,
        status,
        judge_state,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: Some(ChatConversationId::from_string(format!(
            "conversation-{run_index}"
        ))),
        run_prompt: format!("Run {run_index} prompt"),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: Some(format!("ralphx/run-{run_index}")),
        pr_number: Some(100 + run_index),
        pr_url: Some(format!(
            "https://github.com/acme/project/pull/{}",
            100 + run_index
        )),
        pr_title: Some(format!("Run {run_index} PR")),
        pr_head_ref_name: Some(format!("ralphx/run-{run_index}")),
        pr_base_ref_name: Some("main".to_string()),
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: None,
        judge_verdict_json: (judge_state == AutomationJudgeState::Done).then(|| {
            r#"{"decision":"stop","goalMet":false,"reason":"stored fixture verdict","confidence":1,"goalProgress":null,"updatedItemStatuses":null,"nextRunPrompt":null,"nextBaseBranch":null}"#
                .to_string()
        }),
        judge_model_id: None,
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: Some(now),
        finished_at: Some(now),
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
        "ralphx/automation-run".to_string(),
        "/tmp/ralphx-automation-run".to_string(),
    )
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

struct LostStatusAutomationRepository {
    automation: Mutex<Automation>,
    winning_status: AutomationStatus,
}

impl LostStatusAutomationRepository {
    fn new(initial_status: AutomationStatus, winning_status: AutomationStatus) -> Self {
        Self {
            automation: Mutex::new(automation("automation-1", initial_status)),
            winning_status,
        }
    }

    fn status(&self) -> AutomationStatus {
        self.automation.lock().unwrap().status
    }
}

#[async_trait]
impl AutomationRepository for LostStatusAutomationRepository {
    async fn create(&self, automation: Automation) -> crate::error::AppResult<Automation> {
        *self.automation.lock().unwrap() = automation.clone();
        Ok(automation)
    }

    async fn get_by_id(&self, id: &AutomationId) -> crate::error::AppResult<Option<Automation>> {
        let automation = self.automation.lock().unwrap();
        if automation.id == *id {
            Ok(Some(automation.clone()))
        } else {
            Ok(None)
        }
    }

    async fn list(
        &self,
        project_id: Option<ProjectId>,
    ) -> crate::error::AppResult<Vec<Automation>> {
        let automation = self.automation.lock().unwrap();
        if project_id
            .as_ref()
            .is_none_or(|project_id| automation.project_id == *project_id)
        {
            Ok(vec![automation.clone()])
        } else {
            Ok(Vec::new())
        }
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
    ) -> crate::error::AppResult<Vec<Automation>> {
        self.list(Some(project_id.clone())).await
    }

    async fn update_settings(
        &self,
        id: &AutomationId,
        patch: AutomationSettingsPatch,
    ) -> crate::error::AppResult<Option<Automation>> {
        let mut automation = self.automation.lock().unwrap();
        if automation.id != *id {
            return Ok(None);
        }
        if let Some(name) = patch.name {
            automation.name = name;
        }
        if let Some(max_runs) = patch.max_runs {
            automation.max_runs = max_runs;
        }
        if let Some(max_consecutive_failures) = patch.max_consecutive_failures {
            automation.max_consecutive_failures = max_consecutive_failures;
        }
        if let Some(plan_approval_mode) = patch.plan_approval_mode {
            automation.plan_approval_mode = plan_approval_mode;
        }
        if let Some(pr_merge_mode) = patch.pr_merge_mode {
            automation.pr_merge_mode = pr_merge_mode;
        }
        if let Some(plan_deep_verification) = patch.plan_deep_verification {
            automation.plan_deep_verification = plan_deep_verification;
        }
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_config(
        &self,
        id: &AutomationId,
        patch: AutomationConfigPatch,
    ) -> crate::error::AppResult<Option<Automation>> {
        let mut automation = self.automation.lock().unwrap();
        if automation.id != *id {
            return Ok(None);
        }
        if let Some(goal_prompt) = patch.goal_prompt {
            automation.goal_prompt = goal_prompt;
        }
        if let Some(first_run_prompt) = patch.first_run_prompt {
            automation.first_run_prompt = Some(first_run_prompt);
        }
        if let Some(provider_harness) = patch.provider_harness {
            automation.provider_harness = provider_harness;
        }
        if let Some(model_id) = patch.model_id {
            automation.model_id = model_id;
        }
        if let Some(run_mode) = patch.run_mode {
            automation.run_mode = run_mode;
        }
        if let Some(base_ref_kind) = patch.base_ref_kind {
            automation.base_ref_kind = base_ref_kind;
        }
        if let Some(base_ref) = patch.base_ref {
            automation.base_ref = base_ref;
        }
        if let Some(spec_artifact_id) = patch.spec_artifact_id {
            automation.spec_artifact_id = Some(spec_artifact_id);
        }
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_goal_items_json(
        &self,
        id: &AutomationId,
        goal_items_json: Option<String>,
    ) -> crate::error::AppResult<Option<Automation>> {
        let mut automation = self.automation.lock().unwrap();
        if automation.id != *id {
            return Ok(None);
        }
        automation.goal_items_json = goal_items_json;
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_goal_items_json_if_unchanged(
        &self,
        id: &AutomationId,
        expected_goal_items_json: Option<String>,
        goal_items_json: Option<String>,
    ) -> crate::error::AppResult<Option<Automation>> {
        let mut automation = self.automation.lock().unwrap();
        if automation.id != *id || automation.goal_items_json != expected_goal_items_json {
            return Ok(None);
        }
        automation.goal_items_json = goal_items_json;
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_authoring_state_if_unchanged(
        &self,
        id: &AutomationId,
        expected_updated_at: chrono::DateTime<Utc>,
        authoring_state_json: Option<String>,
    ) -> crate::error::AppResult<bool> {
        let mut automation = self.automation.lock().unwrap();
        if automation.id != *id || automation.updated_at != expected_updated_at {
            return Ok(false);
        }
        automation.authoring_state_json = authoring_state_json;
        automation.updated_at = Utc::now();
        Ok(true)
    }

    async fn compare_and_swap_status(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        _to: AutomationStatus,
        _paused_reason_code: Option<String>,
        _paused_reason_detail: Option<String>,
    ) -> crate::error::AppResult<bool> {
        let mut automation = self.automation.lock().unwrap();
        if automation.id == *id && automation.status == from {
            automation.status = self.winning_status;
            automation.updated_at = Utc::now();
        }
        Ok(false)
    }

    async fn delete_terminal(&self, _id: &AutomationId) -> crate::error::AppResult<bool> {
        Ok(false)
    }

    async fn delete_attachments_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> crate::error::AppResult<usize> {
        Ok(0)
    }

    async fn delete_context_refs_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> crate::error::AppResult<usize> {
        Ok(0)
    }
}

struct SkipJudgeLosesRunRepository {
    runs: Mutex<Vec<AutomationRun>>,
}

impl SkipJudgeLosesRunRepository {
    fn new(runs: Vec<AutomationRun>) -> Self {
        Self {
            runs: Mutex::new(runs),
        }
    }
}

#[async_trait]
impl AutomationRunRepository for SkipJudgeLosesRunRepository {
    async fn create_run(&self, run: AutomationRun) -> crate::error::AppResult<AutomationRun> {
        self.runs.lock().unwrap().push(run.clone());
        Ok(run)
    }

    async fn get_by_id(
        &self,
        id: &AutomationRunId,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .iter()
            .find(|run| run.id == *id)
            .cloned())
    }

    async fn delete_run_if_deletable(
        &self,
        _automation_id: &AutomationId,
        _run_id: &AutomationRunId,
    ) -> crate::error::AppResult<usize> {
        Ok(0)
    }

    async fn list_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> crate::error::AppResult<Vec<AutomationRun>> {
        let mut runs: Vec<_> = self
            .runs
            .lock()
            .unwrap()
            .iter()
            .filter(|run| run.automation_id == *automation_id)
            .cloned()
            .collect();
        runs.sort_by_key(|run| run.run_index);
        Ok(runs)
    }

    async fn latest_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .iter()
            .filter(|run| run.automation_id == *automation_id)
            .max_by_key(|run| run.run_index)
            .cloned())
    }

    async fn find_run_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .iter()
            .filter(|run| run.conversation_id.as_ref() == Some(conversation_id))
            .max_by_key(|run| run.run_index)
            .cloned())
    }

    async fn compare_and_swap_status(
        &self,
        _id: &AutomationRunId,
        _from: AutomationRunStatus,
        _to: AutomationRunStatus,
        _error_code: Option<String>,
        _error_detail: Option<String>,
    ) -> crate::error::AppResult<bool> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn compare_and_swap_status_with_agent_phase_started_at(
        &self,
        _id: &AutomationRunId,
        _from: AutomationRunStatus,
        _to: AutomationRunStatus,
        _agent_phase_started_at: chrono::DateTime<Utc>,
        _error_code: Option<String>,
        _error_detail: Option<String>,
    ) -> crate::error::AppResult<bool> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn compare_and_swap_status_clearing_plan_pending_instructions(
        &self,
        _id: &AutomationRunId,
        _from: AutomationRunStatus,
        _to: AutomationRunStatus,
        _error_code: Option<String>,
        _error_detail: Option<String>,
    ) -> crate::error::AppResult<bool> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn update_start_metadata(
        &self,
        _id: &AutomationRunId,
        _conversation_id: &ChatConversationId,
        _branch_name: Option<String>,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn update_publication_metadata(
        &self,
        _id: &AutomationRunId,
        _metadata: crate::domain::repositories::AutomationRunPublicationMetadata,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn update_merge_metadata(
        &self,
        _id: &AutomationRunId,
        _merge_commit_sha: Option<String>,
        _pr_merged_at: Option<chrono::DateTime<Utc>>,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn increment_signal_check_failures(
        &self,
        _id: &AutomationRunId,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn reset_signal_check_failures(
        &self,
        _id: &AutomationRunId,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn update_published_run_error(
        &self,
        _id: &AutomationRunId,
        _error_code: Option<String>,
        _error_detail: Option<String>,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn compare_and_swap_judge_state(
        &self,
        _id: &AutomationRunId,
        _from: AutomationJudgeState,
        _to: AutomationJudgeState,
        _guard: AutomationJudgeTransitionGuard,
        _judge_verdict_json: Option<String>,
        _judge_model_id: Option<String>,
        _judge_lease_expires_at: Option<chrono::DateTime<Utc>>,
        _error_detail: Option<String>,
    ) -> crate::error::AppResult<bool> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn compare_and_swap_plan_judge_state(
        &self,
        _id: &AutomationRunId,
        _from: AutomationPlanJudgeState,
        _to: AutomationPlanJudgeState,
        _plan_judge_verdict_json: Option<String>,
        _plan_judge_lease_expires_at: Option<chrono::DateTime<Utc>>,
    ) -> crate::error::AppResult<bool> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn clear_judge_state(&self, _id: &AutomationRunId) -> crate::error::AppResult<()> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn clear_plan_judge_state(&self, _id: &AutomationRunId) -> crate::error::AppResult<bool> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn set_plan_pending_instructions(
        &self,
        _id: &AutomationRunId,
        _plan_pending_instructions: Option<String>,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn set_plan_revision_round(
        &self,
        _id: &AutomationRunId,
        _plan_revision_round: i64,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn set_plan_last_parked_artifact_id(
        &self,
        _id: &AutomationRunId,
        _plan_last_parked_artifact_id: Option<String>,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn set_plan_last_parked_artifact_ids(
        &self,
        _id: &AutomationRunId,
        _plan_last_parked_artifact_id: Option<String>,
        _plan_last_parked_blueprint_artifact_id: Option<String>,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn set_plan_reminder_count(
        &self,
        _id: &AutomationRunId,
        _plan_reminder_count: i64,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn set_agent_phase_started_at(
        &self,
        _id: &AutomationRunId,
        _agent_phase_started_at: Option<chrono::DateTime<Utc>>,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn clear_finished_at(&self, _id: &AutomationRunId) -> crate::error::AppResult<()> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn create_judge_successor_run(
        &self,
        _automation_id: &AutomationId,
        _previous_run_id: &AutomationRunId,
        _successor: AutomationRun,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }

    async fn skip_judge_and_create_successor_run(
        &self,
        _automation_id: &AutomationId,
        _previous_run_id: &AutomationRunId,
        _successor: AutomationRun,
    ) -> crate::error::AppResult<Option<AutomationRun>> {
        Ok(None)
    }

    async fn delete_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> crate::error::AppResult<usize> {
        Err(AppError::Validation(
            "unused test repository method".to_string(),
        ))
    }
}

#[tokio::test]
async fn service_creates_lists_gets_and_updates_mechanical_settings() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let project_id = ProjectId::from_string("project-1".to_string());

    let draft = service
        .create_draft(CreateAutomationDraftInput {
            id: None,
            project_id: project_id.clone(),
            name: Some("  Large migration  ".to_string()),
            setup_conversation_id: None,
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            authoring_mode: None,
        })
        .await
        .unwrap();

    assert_eq!(draft.name, "Large migration");
    assert_eq!(draft.status, AutomationStatus::Draft);
    assert_eq!(draft.run_mode, "edit");
    assert_eq!(draft.completion_signal, "pr_merged");
    assert_eq!(draft.max_runs, 25);

    let listed = service.list_automations(Some(project_id)).await.unwrap();
    assert_eq!(listed, vec![draft.clone()]);

    let detail = service.get_automation_detail(&draft.id).await.unwrap();
    assert_eq!(detail.automation, draft.clone());
    assert!(detail.runs.is_empty());

    let updated = service
        .update_settings(UpdateAutomationSettingsInput {
            id: draft.id.clone(),
            name: Some("Renamed automation".to_string()),
            max_runs: Some(7),
            max_consecutive_failures: Some(2),
            plan_approval_mode: None,
            pr_merge_mode: None,
            plan_deep_verification: None,
        })
        .await
        .unwrap();

    assert_eq!(updated.name, "Renamed automation");
    assert_eq!(updated.max_runs, 7);
    assert_eq!(updated.max_consecutive_failures, 2);
    assert_eq!(updated.status, AutomationStatus::Draft);
    assert_eq!(
        automation_repo
            .get_by_id(&draft.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Draft
    );
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationUpdated {
            automation_id: draft.id
        }]
    );
}

#[tokio::test]
async fn service_update_settings_writes_plan_gate_settings_on_active_automation() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let updated = service
        .update_settings(UpdateAutomationSettingsInput {
            id: active.id.clone(),
            name: None,
            max_runs: None,
            max_consecutive_failures: None,
            plan_approval_mode: Some(AutomationPlanApprovalMode::Automatic),
            pr_merge_mode: Some(AutomationPrMergeMode::Automatic),
            plan_deep_verification: Some(true),
        })
        .await
        .unwrap();

    assert_eq!(
        updated.plan_approval_mode,
        AutomationPlanApprovalMode::Automatic
    );
    assert_eq!(updated.pr_merge_mode, AutomationPrMergeMode::Automatic);
    assert!(updated.plan_deep_verification);
    assert_eq!(updated.status, AutomationStatus::Active);
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationUpdated {
            automation_id: active.id
        }]
    );
}

#[tokio::test]
async fn service_update_settings_rejects_automatic_merge_for_stacked_chain() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.chain_mode = "pr_head_stacked".to_string();
    automation_repo.create(active.clone()).await.unwrap();

    let error = service
        .update_settings(UpdateAutomationSettingsInput {
            id: active.id.clone(),
            name: None,
            max_runs: None,
            max_consecutive_failures: None,
            plan_approval_mode: None,
            pr_merge_mode: Some(AutomationPrMergeMode::Automatic),
            plan_deep_verification: None,
        })
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains(AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE))
    );
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.pr_merge_mode, AutomationPrMergeMode::Manual);
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_update_settings_allows_unrelated_edits_for_existing_stacked_auto_merge() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.chain_mode = "pr_head_stacked".to_string();
    active.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(active.clone()).await.unwrap();

    let updated = service
        .update_settings(UpdateAutomationSettingsInput {
            id: active.id.clone(),
            name: Some("Renamed while repairing".to_string()),
            max_runs: Some(9),
            max_consecutive_failures: None,
            plan_approval_mode: None,
            pr_merge_mode: None,
            plan_deep_verification: None,
        })
        .await
        .unwrap();

    assert_eq!(updated.name, "Renamed while repairing");
    assert_eq!(updated.max_runs, 9);
    assert_eq!(updated.chain_mode, "pr_head_stacked");
    assert_eq!(updated.pr_merge_mode, AutomationPrMergeMode::Automatic);
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationUpdated {
            automation_id: active.id
        }]
    );
}

#[tokio::test]
async fn service_update_config_rejects_stacked_chain_when_auto_merge_is_enabled() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let mut draft = automation("automation-1", AutomationStatus::Draft);
    draft.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(draft.clone()).await.unwrap();

    let error = service
        .update_config(UpdateAutomationConfigInput {
            chain_mode: Some("pr_head_stacked".to_string()),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains(AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE))
    );
    let stored = automation_repo.get_by_id(&draft.id).await.unwrap().unwrap();
    assert_eq!(stored.chain_mode, "merged_base");
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_finalize_rejects_stacked_chain_when_auto_merge_is_enabled() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let mut draft = automation("automation-1", AutomationStatus::Draft);
    draft.chain_mode = "pr_head_stacked".to_string();
    draft.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(draft.clone()).await.unwrap();

    let error = service.finalize(&draft.id).await.unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains(AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE))
    );
    assert_eq!(
        automation_repo
            .get_by_id(&draft.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Draft
    );
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_update_config_writes_provided_fields_on_draft_and_emits_event() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let mut draft = automation("automation-1", AutomationStatus::Draft);
    draft.goal_prompt = String::new();
    draft.first_run_prompt = None;
    draft.base_ref = String::new();
    automation_repo.create(draft.clone()).await.unwrap();

    let updated = service
        .update_config(UpdateAutomationConfigInput {
            id: draft.id.clone(),
            goal_prompt: Some("Migrate the payments module".to_string()),
            first_run_prompt: Some("Implement item 1 in a scoped PR and publish it.".to_string()),
            provider_harness: Some("codex".to_string()),
            model_id: Some("gpt-5.4".to_string()),
            logical_effort: Some("high".to_string()),
            run_mode: Some("edit".to_string()),
            base_ref_kind: Some("local_branch".to_string()),
            base_ref: Some("main".to_string()),
            base_display_name: Some("main".to_string()),
            goal_items_json: Some(
                r#"[{"id":"phase-1","title":"Build shared context model","status":"pending"}]"#
                    .to_string(),
            ),
            chain_mode: None,
            completion_signal: None,
            setup_analysis_summary: Some("Setup summary".to_string()),
            spec_artifact_id: None,
            spec_content: None,
        })
        .await
        .unwrap();

    assert_eq!(updated.goal_prompt, "Migrate the payments module");
    assert_eq!(
        updated.first_run_prompt.as_deref(),
        Some("Implement item 1 in a scoped PR and publish it.")
    );
    assert_eq!(updated.provider_harness, "codex");
    assert_eq!(updated.model_id, "gpt-5.4");
    assert_eq!(updated.base_ref_kind, "local_branch");
    assert_eq!(updated.base_ref, "main");
    assert_eq!(
        updated.goal_items_json.as_deref(),
        Some(r#"[{"id":"phase-1","title":"Build shared context model","status":"pending"}]"#),
    );
    // Fields left None keep their pre-existing values.
    assert_eq!(updated.chain_mode, "merged_base");
    assert_eq!(updated.completion_signal, "pr_merged");
    assert_eq!(updated.status, AutomationStatus::Draft);

    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationUpdated {
            automation_id: draft.id
        }]
    );
}

#[tokio::test]
async fn service_update_config_preserves_integration_base_from_project_default_downgrade() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter);
    let mut draft = automation("automation-1", AutomationStatus::Draft);
    draft.base_ref_kind = "local_branch".to_string();
    draft.base_ref = "ralphx/ralphx/automation-abc".to_string();
    draft.base_display_name = Some("ralphx/ralphx/automation-abc".to_string());
    automation_repo.create(draft.clone()).await.unwrap();

    let updated = service
        .update_config(UpdateAutomationConfigInput {
            base_ref_kind: Some("project_default".to_string()),
            base_ref: Some("main".to_string()),
            base_display_name: Some("Project default (main)".to_string()),
            ..empty_config_input(draft.id)
        })
        .await
        .unwrap();

    assert_eq!(updated.base_ref_kind, "local_branch");
    assert_eq!(updated.base_ref, "ralphx/ralphx/automation-abc");
    assert_eq!(
        updated.base_display_name.as_deref(),
        Some("ralphx/ralphx/automation-abc")
    );
    assert_ne!(updated.base_ref_kind, "project_default");
    assert_ne!(updated.base_ref, "main");
}

#[tokio::test]
async fn service_update_config_allows_project_default_to_local_branch_change() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter);
    let draft = automation("automation-1", AutomationStatus::Draft);
    automation_repo.create(draft.clone()).await.unwrap();

    let updated = service
        .update_config(UpdateAutomationConfigInput {
            base_ref_kind: Some("local_branch".to_string()),
            base_ref: Some("some-branch".to_string()),
            base_display_name: Some("some-branch".to_string()),
            ..empty_config_input(draft.id)
        })
        .await
        .unwrap();

    assert_eq!(updated.base_ref_kind, "local_branch");
    assert_eq!(updated.base_ref, "some-branch");
    assert_eq!(updated.base_display_name.as_deref(), Some("some-branch"));
    assert_ne!(updated.base_ref_kind, "project_default");
}

#[tokio::test]
async fn service_update_config_patches_goal_while_preserving_integration_base() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter);
    let mut draft = automation("automation-1", AutomationStatus::Draft);
    draft.base_ref_kind = "local_branch".to_string();
    draft.base_ref = "ralphx/ralphx/automation-abc".to_string();
    automation_repo.create(draft.clone()).await.unwrap();

    let updated = service
        .update_config(UpdateAutomationConfigInput {
            goal_prompt: Some("Updated automation goal".to_string()),
            base_ref_kind: Some("project_default".to_string()),
            base_ref: Some("main".to_string()),
            base_display_name: Some("Project default (main)".to_string()),
            ..empty_config_input(draft.id)
        })
        .await
        .unwrap();

    assert_eq!(updated.goal_prompt, "Updated automation goal");
    assert_eq!(updated.base_ref_kind, "local_branch");
    assert_eq!(updated.base_ref, "ralphx/ralphx/automation-abc");
    assert_ne!(updated.base_ref_kind, "project_default");
    assert_ne!(updated.base_ref, "main");
}

fn empty_config_input(id: AutomationId) -> UpdateAutomationConfigInput {
    UpdateAutomationConfigInput {
        id,
        goal_prompt: None,
        first_run_prompt: None,
        provider_harness: None,
        model_id: None,
        logical_effort: None,
        run_mode: None,
        base_ref_kind: None,
        base_ref: None,
        base_display_name: None,
        goal_items_json: None,
        chain_mode: None,
        completion_signal: None,
        setup_analysis_summary: None,
        spec_artifact_id: None,
        spec_content: None,
    }
}

#[tokio::test]
async fn service_update_config_links_existing_spec_artifact_id() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo, artifact_repo) =
        service_with_emitter_and_artifacts(emitter.clone());
    let draft = automation("automation-1", AutomationStatus::Draft);
    automation_repo.create(draft.clone()).await.unwrap();

    let seeded = artifact_repo
        .create(Artifact::new_inline(
            "Existing spec",
            crate::domain::entities::ArtifactType::Specification,
            "# Existing spec",
            "user",
        ))
        .await
        .unwrap();

    let updated = service
        .update_config(UpdateAutomationConfigInput {
            spec_artifact_id: Some(seeded.id.as_str().to_string()),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap();

    assert_eq!(
        updated.spec_artifact_id.as_deref(),
        Some(seeded.id.as_str())
    );
    // No artifact was materialized; the existing id was linked directly.
    assert!(artifact_repo.versioned_from().is_empty());
}

#[tokio::test]
async fn service_update_config_rejects_missing_spec_artifact_id() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo, _artifact_repo) =
        service_with_emitter_and_artifacts(emitter.clone());
    let draft = automation("automation-1", AutomationStatus::Draft);
    automation_repo.create(draft.clone()).await.unwrap();

    let error = service
        .update_config(UpdateAutomationConfigInput {
            spec_artifact_id: Some("missing-artifact".to_string()),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("does not reference an existing artifact"))
    );
    // Fail closed: nothing persisted, no id linked, no event emitted.
    let stored = automation_repo.get_by_id(&draft.id).await.unwrap().unwrap();
    assert_eq!(stored.spec_artifact_id, None);
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_update_config_materializes_spec_content_and_versions_on_reauthor() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo, artifact_repo) =
        service_with_emitter_and_artifacts(emitter.clone());
    let draft = automation("automation-1", AutomationStatus::Draft);
    automation_repo.create(draft.clone()).await.unwrap();

    let first = service
        .update_config(UpdateAutomationConfigInput {
            spec_content: Some("# Automation spec\n\nPhase 1: build it.".to_string()),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap();

    let first_spec_id = first
        .spec_artifact_id
        .clone()
        .expect("spec artifact linked");
    let first_artifact = artifact_repo
        .get_by_id(&ArtifactId::from_string(first_spec_id.clone()))
        .await
        .unwrap()
        .expect("spec artifact exists");
    assert_eq!(
        first_artifact.artifact_type,
        crate::domain::entities::ArtifactType::Specification
    );
    assert_eq!(first_artifact.metadata.version, 1);
    match &first_artifact.content {
        crate::domain::entities::ArtifactContent::Inline { text } => {
            assert!(text.contains("Phase 1: build it."))
        }
        other => panic!("expected inline spec content, got {other:?}"),
    }
    // First authoring is a fresh create, not a version chain.
    assert!(artifact_repo.versioned_from().is_empty());

    // Re-authoring replaces via a NEW artifact id chained off the previous one.
    let second = service
        .update_config(UpdateAutomationConfigInput {
            spec_content: Some("# Automation spec v2\n\nPhase 1: build it better.".to_string()),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap();

    let second_spec_id = second
        .spec_artifact_id
        .clone()
        .expect("versioned spec artifact linked");
    assert_ne!(
        second_spec_id, first_spec_id,
        "re-author mints a new artifact id"
    );

    let versioned = artifact_repo.versioned_from();
    assert_eq!(
        versioned,
        vec![(first_spec_id.clone(), second_spec_id.clone())],
        "second write chains previous_version_id -> new id"
    );
    let second_artifact = artifact_repo
        .get_by_id(&ArtifactId::from_string(second_spec_id))
        .await
        .unwrap()
        .expect("versioned spec artifact exists");
    assert_eq!(second_artifact.metadata.version, 2);
    // Versioning is NOT expressed through derived_from (relations concept).
    assert!(second_artifact.derived_from.is_empty());
}

#[tokio::test]
async fn service_update_config_rejects_active_automation_and_leaves_row_intact() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let error = service
        .update_config(UpdateAutomationConfigInput {
            id: active.id.clone(),
            goal_prompt: Some("Should not persist".to_string()),
            first_run_prompt: None,
            provider_harness: None,
            model_id: None,
            logical_effort: None,
            run_mode: None,
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            goal_items_json: None,
            chain_mode: None,
            completion_signal: None,
            setup_analysis_summary: None,
            spec_artifact_id: None,
            spec_content: None,
        })
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Validation(message) if message.contains("draft or paused")));
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.goal_prompt, "Goal");
    assert_eq!(stored.status, AutomationStatus::Active);
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_create_draft_then_config_then_finalize_activates_automation() {
    // KEY acceptance test: an empty draft can be populated via update_config
    // and then finalized to Active — proving the config-write path unblocks
    // the previously stuck draft->active transition.
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let project_id = ProjectId::from_string("project-1".to_string());

    let draft = service
        .create_draft(CreateAutomationDraftInput {
            id: None,
            project_id,
            name: Some("Payments automation".to_string()),
            setup_conversation_id: None,
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            authoring_mode: None,
        })
        .await
        .unwrap();
    // Freshly created drafts start with empty goal/prompt/base — not finalizable.
    assert_eq!(draft.status, AutomationStatus::Draft);
    assert!(draft.goal_prompt.is_empty());
    assert!(draft.first_run_prompt.is_none());
    let premature = service.finalize(&draft.id).await.unwrap_err();
    assert!(matches!(premature, AppError::Validation(_)));

    service
        .update_config(UpdateAutomationConfigInput {
            id: draft.id.clone(),
            goal_prompt: Some("Ship the migration in a serial PR chain".to_string()),
            first_run_prompt: Some(
                "Implement the first slice in a scoped PR and publish it.".to_string(),
            ),
            provider_harness: None,
            model_id: None,
            logical_effort: None,
            run_mode: Some("edit".to_string()),
            base_ref_kind: Some("local_branch".to_string()),
            base_ref: Some("main".to_string()),
            base_display_name: Some("main".to_string()),
            goal_items_json: Some(
                r#"[{"id":"phase-1","title":"Implement first slice","status":"pending"}]"#
                    .to_string(),
            ),
            chain_mode: None,
            completion_signal: None,
            setup_analysis_summary: None,
            spec_artifact_id: None,
            spec_content: None,
        })
        .await
        .unwrap();

    let finalized = service.finalize(&draft.id).await.unwrap();
    assert_eq!(finalized.status, AutomationStatus::Active);
    assert_eq!(
        automation_repo
            .get_by_id(&draft.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
}

#[tokio::test]
async fn ideation_bridge_finalization_requires_verified_plan_delivery_contract() {
    let (service, automation_repo, _run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut draft = automation("automation-bridge", AutomationStatus::Draft);
    draft.run_mode = "ideation".to_string();
    draft.completion_signal = "ideation_finalized".to_string();
    draft.plan_deep_verification = false;
    automation_repo.create(draft.clone()).await.unwrap();

    let unverified = service.finalize(&draft.id).await.unwrap_err();
    assert!(matches!(
        unverified,
        AppError::Validation(message) if message.contains("deep plan verification")
    ));

    automation_repo
        .update_settings(
            &draft.id,
            AutomationSettingsPatch {
                plan_deep_verification: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let finalized = service.finalize(&draft.id).await.unwrap();
    assert_eq!(finalized.status, AutomationStatus::Active);
    assert_eq!(finalized.run_mode, "ideation");
    assert_eq!(finalized.completion_signal, "ideation_finalized");
}

#[tokio::test]
async fn trusted_decomposition_accepts_the_verified_ideation_task_graph_policy() {
    let (service, automation_repo, _run_repo, artifact_repo) =
        service_with_emitter_and_artifacts(Arc::new(NoopAutomationEventEmitter));
    let spec = artifact_repo
        .create(Artifact::new_inline(
            "Ideation bridge spec",
            crate::domain::entities::ArtifactType::Specification,
            "# Plan\n\nCreate a dependency-safe task graph.",
            "user",
        ))
        .await
        .unwrap();
    let mut draft = automation("automation-bridge-trusted", AutomationStatus::Draft);
    draft.run_mode = "ideation".to_string();
    draft.completion_signal = "ideation_finalized".to_string();
    draft.plan_approval_mode = AutomationPlanApprovalMode::Automatic;
    draft.pr_merge_mode = AutomationPrMergeMode::Manual;
    draft.plan_deep_verification = true;
    draft.spec_artifact_id = Some(spec.id.to_string());
    automation_repo.create(draft.clone()).await.unwrap();

    let input = service.load_decomposition_input(&draft).await.unwrap();

    assert_eq!(input.run_mode, "ideation");
    assert_eq!(input.completion_signal, "ideation_finalized");
    assert!(input.plan_deep_verification);
}

#[tokio::test]
async fn service_status_controls_fail_when_compare_and_swap_loses() {
    let emitter = Arc::new(RecordingEmitter::default());

    let pause_repo = Arc::new(LostStatusAutomationRepository::new(
        AutomationStatus::Active,
        AutomationStatus::Stopped,
    ));
    let pause_service = AutomationService::new(
        pause_repo.clone(),
        Arc::new(MemoryAutomationRunRepository::new(
            MemoryAutomationRepository::new_shared_state(),
        )),
        emitter.clone(),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );
    let pause_id = AutomationId::from_string("automation-1");
    let pause_error = pause_service
        .pause(&pause_id, "user", Some("pause requested"))
        .await
        .unwrap_err();
    assert!(matches!(pause_error, AppError::Conflict(_)));
    assert_eq!(pause_repo.status(), AutomationStatus::Stopped);

    let resume_repo = Arc::new(LostStatusAutomationRepository::new(
        AutomationStatus::Paused,
        AutomationStatus::Stopped,
    ));
    let resume_service = AutomationService::new(
        resume_repo.clone(),
        Arc::new(MemoryAutomationRunRepository::new(
            MemoryAutomationRepository::new_shared_state(),
        )),
        emitter.clone(),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );
    let resume_error = resume_service.resume(&pause_id).await.unwrap_err();
    assert!(matches!(resume_error, AppError::Conflict(_)));
    assert_eq!(resume_repo.status(), AutomationStatus::Stopped);

    let stop_repo = Arc::new(LostStatusAutomationRepository::new(
        AutomationStatus::Active,
        AutomationStatus::Paused,
    ));
    let stop_service = AutomationService::new(
        stop_repo.clone(),
        Arc::new(MemoryAutomationRunRepository::new(
            MemoryAutomationRepository::new_shared_state(),
        )),
        emitter.clone(),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );
    let stop_error = stop_service.stop(&pause_id).await.unwrap_err();
    assert!(matches!(stop_error, AppError::Conflict(_)));
    assert_eq!(stop_repo.status(), AutomationStatus::Paused);
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_status_controls_use_transition_service_and_fail_closed() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let paused = service
        .pause(&active.id, "user", Some("Taking a break"))
        .await
        .unwrap();
    assert_eq!(paused.status, AutomationStatus::Paused);
    assert_eq!(paused.paused_reason_code.as_deref(), Some("user"));

    let resumed = service.resume(&active.id).await.unwrap();
    assert_eq!(resumed.status, AutomationStatus::Active);

    let stopped = service.stop(&active.id).await.unwrap();
    assert_eq!(stopped.status, AutomationStatus::Stopped);

    let error = service.resume(&active.id).await.unwrap_err();
    assert!(matches!(error, AppError::InvalidTransition { .. }));

    assert_eq!(
        emitter.events(),
        vec![
            AutomationEvent::AutomationUpdated {
                automation_id: active.id.clone()
            },
            AutomationEvent::AutomationUpdated {
                automation_id: active.id.clone()
            },
            AutomationEvent::AutomationUpdated {
                automation_id: active.id
            },
        ]
    );
}

#[tokio::test]
async fn service_resume_judge_stopped_unmet_creates_fresh_run_from_failed_attempt() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let mut paused = automation("automation-resume-unmet", AutomationStatus::Paused);
    paused.paused_reason_code = Some("judge_stopped_unmet".to_string());
    paused.paused_reason_detail = Some("Repair the external blocker, then retry.".to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let mut failed = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    failed.run_prompt = "Implement Spec §PR F0".to_string();
    failed.base_ref_kind = "local_branch".to_string();
    failed.base_ref_used = "ralphx/base".to_string();
    run_repo.create_run(failed.clone()).await.unwrap();

    let resumed = service.resume(&paused.id).await.unwrap();

    assert_eq!(resumed.status, AutomationStatus::Active);
    assert!(resumed.paused_reason_code.is_none());
    assert!(resumed.paused_reason_detail.is_none());
    let runs = run_repo.list_for_automation(&paused.id).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
    assert_eq!(runs[1].run_prompt, "Implement Spec §PR F0");
    assert_eq!(runs[1].prompt_author, AutomationPromptAuthor::SetupAgent);
    assert_eq!(runs[1].base_ref_kind, "local_branch");
    assert_eq!(runs[1].base_ref_used, "ralphx/base");
    assert_eq!(runs[1].base_from_run_id, Some(failed.id));
    assert!(emitter
        .events()
        .contains(&AutomationEvent::AutomationUpdated {
            automation_id: paused.id.clone(),
        }));
    assert!(emitter
        .events()
        .iter()
        .any(|event| matches!(event, AutomationEvent::AutomationRunUpdated { automation_id, run_id } if automation_id == &paused.id && run_id == &runs[1].id)));
}

#[tokio::test]
async fn service_resume_judge_stopped_unmet_fails_closed_without_current_unmet_stop_verdict() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let mut paused = automation("automation-resume-stale", AutomationStatus::Paused);
    paused.paused_reason_code = Some("judge_stopped_unmet".to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let mut stale = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    stale.judge_verdict_json = Some(serde_json::to_string(&stop_verdict(true, "Done")).unwrap());
    run_repo.create_run(stale).await.unwrap();

    let error = service.resume(&paused.id).await.unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("current unmet stop verdict"))
    );
    let stored = automation_repo
        .get_by_id(&paused.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Paused);
    assert_eq!(
        stored.paused_reason_code.as_deref(),
        Some("judge_stopped_unmet")
    );
    assert_eq!(
        run_repo
            .list_for_automation(&paused.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_resume_judge_stopped_unmet_rejects_each_stale_or_invalid_authority() {
    #[derive(Clone, Copy)]
    enum Case {
        NoRuns,
        NonTerminal,
        JudgeNotDone,
        MissingVerdict,
        InvalidVerdict,
        ContinueVerdict,
        EmptyPrompt,
        MaxRuns,
        ConsecutiveFailures,
    }

    for (case, expected_detail) in [
        (Case::NoRuns, "automation has no runs"),
        (
            Case::NonTerminal,
            "latest run is not terminal with a completed judge",
        ),
        (
            Case::JudgeNotDone,
            "latest run is not terminal with a completed judge",
        ),
        (
            Case::MissingVerdict,
            "latest run has no stored judge verdict",
        ),
        (Case::InvalidVerdict, "latest verdict is invalid"),
        (
            Case::ContinueVerdict,
            "latest verdict is not stop with goalMet=false",
        ),
        (Case::EmptyPrompt, "latest run prompt is empty"),
        (Case::MaxRuns, "reached the configured limit"),
        (Case::ConsecutiveFailures, "consecutive runs failed"),
    ] {
        let emitter = Arc::new(RecordingEmitter::default());
        let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
        let mut paused = automation("automation-resume-invalid", AutomationStatus::Paused);
        paused.paused_reason_code = Some("judge_stopped_unmet".to_string());
        if matches!(case, Case::MaxRuns) {
            paused.max_runs = 1;
        }
        if matches!(case, Case::ConsecutiveFailures) {
            paused.max_consecutive_failures = 1;
        }
        automation_repo.create(paused.clone()).await.unwrap();

        if !matches!(case, Case::NoRuns) {
            let status = if matches!(case, Case::NonTerminal) {
                AutomationRunStatus::Running
            } else {
                AutomationRunStatus::AgentFailed
            };
            let judge_state = if matches!(case, Case::JudgeNotDone) {
                AutomationJudgeState::Failed
            } else {
                AutomationJudgeState::Done
            };
            let mut latest = automation_run("run-invalid", &paused.id, 1, status, judge_state);
            match case {
                Case::MissingVerdict => latest.judge_verdict_json = None,
                Case::InvalidVerdict => latest.judge_verdict_json = Some("{".to_string()),
                Case::ContinueVerdict => {
                    latest.judge_verdict_json = Some(continue_verdict(
                        "Continue the unfinished goal with focused tests and publish the result.",
                    ))
                }
                Case::EmptyPrompt => latest.run_prompt = "   ".to_string(),
                _ => {}
            }
            run_repo.create_run(latest).await.unwrap();
        }

        let error = service.resume(&paused.id).await.unwrap_err();

        assert!(
            matches!(error, AppError::Validation(ref detail) if detail.contains(expected_detail)),
            "unexpected rejection for {expected_detail}: {error:?}"
        );
        let stored = automation_repo
            .get_by_id(&paused.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, AutomationStatus::Paused);
        assert_eq!(
            stored.paused_reason_code.as_deref(),
            Some("judge_stopped_unmet")
        );
        assert!(emitter.events().is_empty());
    }
}

#[tokio::test]
async fn service_resume_judge_stopped_unmet_rolls_back_when_retry_creation_fails() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter);
    let mut paused = automation("automation-resume-rollback", AutomationStatus::Paused);
    paused.paused_reason_code = Some("judge_stopped_unmet".to_string());
    paused.paused_reason_detail = Some("Preserve this recovery guidance".to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let stale_open = automation_run(
        "run-stale-open",
        &paused.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(stale_open.clone()).await.unwrap();
    let failed = automation_run(
        "run-current-failed",
        &paused.id,
        2,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(failed.clone()).await.unwrap();

    let error = service.resume(&paused.id).await.unwrap_err();

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "automation already has an open run")
    );
    let rolled_back = automation_repo
        .get_by_id(&paused.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rolled_back.status, AutomationStatus::Paused);
    assert_eq!(
        rolled_back.paused_reason_code.as_deref(),
        Some("judge_stopped_unmet")
    );
    assert_eq!(
        rolled_back.paused_reason_detail.as_deref(),
        Some("Preserve this recovery guidance")
    );
    assert_eq!(
        run_repo.list_for_automation(&paused.id).await.unwrap(),
        vec![stale_open, failed]
    );
}

#[tokio::test]
async fn service_restart_stopped_automation_creates_fresh_run_and_preserves_cancelled_history() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let active = automation("automation-restart", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let first = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Continue the durable automation goal".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
            base_ref_kind: "local_branch".to_string(),
            base_ref_used: "feature/automation".to_string(),
            base_from_run_id: Some(AutomationRunId::from_string("source-run")),
        })
        .await
        .unwrap();
    service.stop(&active.id).await.unwrap();

    let outcome = service.restart(&active.id).await.unwrap();

    assert!(outcome.scheduled);
    assert_eq!(outcome.reason, None);
    assert_eq!(
        automation_repo
            .get_by_id(&active.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
    let runs = run_repo.list_for_automation(&active.id).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].id, first.id);
    assert_eq!(runs[0].status, AutomationRunStatus::Cancelled);
    assert_eq!(runs[1].run_index, 2);
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
    assert_eq!(runs[1].run_prompt, first.run_prompt);
    assert_eq!(runs[1].prompt_author, first.prompt_author);
    assert_eq!(runs[1].base_ref_kind, first.base_ref_kind);
    assert_eq!(runs[1].base_ref_used, first.base_ref_used);
    assert_eq!(runs[1].base_from_run_id, first.base_from_run_id);
    assert!(emitter
        .events()
        .contains(&AutomationEvent::AutomationUpdated {
            automation_id: active.id.clone(),
        }));
    assert!(emitter
        .events()
        .contains(&AutomationEvent::AutomationRunUpdated {
            automation_id: active.id,
            run_id: runs[1].id.clone(),
        }));
}

#[tokio::test]
async fn service_restart_rejects_non_stopped_automation_without_creating_run() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-restart", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let error = service.restart(&active.id).await.unwrap_err();

    assert!(matches!(
        error,
        AppError::InvalidTransition { from, to }
            if from == AutomationStatus::Active.as_str()
                && to == AutomationStatus::Active.as_str()
    ));
    assert!(run_repo
        .list_for_automation(&active.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn service_restart_rejects_stopped_automation_with_work_in_flight() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let stopped = automation("automation-restart", AutomationStatus::Stopped);
    automation_repo.create(stopped.clone()).await.unwrap();
    let run = automation_run(
        "run-running",
        &stopped.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    let error = service.restart(&stopped.id).await.unwrap_err();

    assert!(matches!(error, AppError::Conflict(message) if message.contains("work in flight")));
    assert_eq!(
        automation_repo
            .get_by_id(&stopped.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Stopped
    );
    assert_eq!(
        run_repo.get_by_id(&run.id).await.unwrap().unwrap().status,
        AutomationRunStatus::Running
    );
}

#[tokio::test]
async fn service_restart_without_prior_runs_uses_first_run_prompt_and_base() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut stopped = automation("automation-restart", AutomationStatus::Stopped);
    stopped.first_run_prompt = Some("Start the automation again".to_string());
    stopped.base_ref_kind = "local_branch".to_string();
    stopped.base_ref = "release/base".to_string();
    automation_repo.create(stopped.clone()).await.unwrap();

    let outcome = service.restart(&stopped.id).await.unwrap();

    assert!(outcome.scheduled);
    assert_eq!(
        automation_repo
            .get_by_id(&stopped.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
    let runs = run_repo.list_for_automation(&stopped.id).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_prompt, "Start the automation again");
    assert_eq!(runs[0].prompt_author, AutomationPromptAuthor::SetupAgent);
    assert_eq!(runs[0].base_ref_kind, "local_branch");
    assert_eq!(runs[0].base_ref_used, "release/base");
    assert_eq!(runs[0].base_from_run_id, None);
}

#[tokio::test]
async fn service_restart_fails_closed_when_stopped_to_active_cas_loses() {
    let automation_repo = Arc::new(LostStatusAutomationRepository::new(
        AutomationStatus::Stopped,
        AutomationStatus::Completed,
    ));
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        MemoryAutomationRepository::new_shared_state(),
    ));
    let cancelled = automation_run(
        "run-cancelled",
        &AutomationId::from_string("automation-1"),
        1,
        AutomationRunStatus::Cancelled,
        AutomationJudgeState::None,
    );
    run_repo.create_run(cancelled).await.unwrap();
    let service = AutomationService::new(
        automation_repo.clone(),
        run_repo.clone(),
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );

    let error = service
        .restart(&AutomationId::from_string("automation-1"))
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Conflict(_)));
    assert_eq!(automation_repo.status(), AutomationStatus::Completed);
    assert_eq!(
        run_repo
            .list_for_automation(&AutomationId::from_string("automation-1"))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn service_restart_rolls_back_to_stopped_when_fresh_run_creation_fails() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let stopped = automation("automation-restart", AutomationStatus::Stopped);
    automation_repo.create(stopped.clone()).await.unwrap();
    let mut cancelled = automation_run(
        "run-cancelled",
        &stopped.id,
        1,
        AutomationRunStatus::Cancelled,
        AutomationJudgeState::None,
    );
    cancelled.run_prompt = "   ".to_string();
    run_repo.create_run(cancelled.clone()).await.unwrap();

    let error = service.restart(&stopped.id).await.unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("prompt cannot be empty"))
    );
    assert_eq!(
        automation_repo
            .get_by_id(&stopped.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Stopped
    );
    let runs = run_repo.list_for_automation(&stopped.id).await.unwrap();
    assert_eq!(runs, vec![cancelled]);
}

#[tokio::test]
async fn service_resume_does_not_reactivate_stopped_automation() {
    let (service, automation_repo, _) = service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let stopped = automation("automation-stopped", AutomationStatus::Stopped);
    automation_repo.create(stopped.clone()).await.unwrap();

    let error = service.resume(&stopped.id).await.unwrap_err();

    assert!(matches!(error, AppError::InvalidTransition { .. }));
    assert_eq!(
        automation_repo
            .get_by_id(&stopped.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Stopped
    );
}

#[tokio::test]
async fn service_finalizes_complete_draft_through_transition_service() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let draft = automation("automation-1", AutomationStatus::Draft);
    automation_repo.create(draft.clone()).await.unwrap();

    let finalized = service.finalize(&draft.id).await.unwrap();

    assert_eq!(finalized.status, AutomationStatus::Active);
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationUpdated {
            automation_id: draft.id
        }]
    );
}

#[tokio::test]
async fn service_finalize_fails_closed_for_incomplete_or_unresolved_drafts() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());

    let mut missing_prompt = automation("automation-missing", AutomationStatus::Draft);
    missing_prompt.first_run_prompt = None;
    automation_repo
        .create(missing_prompt.clone())
        .await
        .unwrap();
    let missing_error = service.finalize(&missing_prompt.id).await.unwrap_err();
    assert!(matches!(missing_error, AppError::Validation(_)));
    assert_eq!(
        automation_repo
            .get_by_id(&missing_prompt.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Draft
    );

    let mut unresolved_base = automation("automation-current", AutomationStatus::Draft);
    unresolved_base.base_ref_kind = "current_branch".to_string();
    automation_repo
        .create(unresolved_base.clone())
        .await
        .unwrap();
    let base_error = service.finalize(&unresolved_base.id).await.unwrap_err();
    assert!(matches!(base_error, AppError::Validation(_)));
    assert_eq!(
        automation_repo
            .get_by_id(&unresolved_base.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Draft
    );

    let mut missing_phase_spec = automation("automation-no-phases", AutomationStatus::Draft);
    missing_phase_spec.goal_items_json = None;
    automation_repo
        .create(missing_phase_spec.clone())
        .await
        .unwrap();
    let phase_error = service.finalize(&missing_phase_spec.id).await.unwrap_err();
    assert!(matches!(phase_error, AppError::Validation(message) if message.contains("phase spec")));
    assert_eq!(
        automation_repo
            .get_by_id(&missing_phase_spec.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Draft
    );

    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn service_creates_pending_runs_without_bypassing_single_flight() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let run = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Implement item 1".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
        })
        .await
        .unwrap();

    assert_eq!(run.run_index, 1);
    assert_eq!(run.status, AutomationRunStatus::Pending);
    assert_eq!(run.judge_state, AutomationJudgeState::None);

    let duplicate = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Implement item 2".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
        })
        .await;
    assert!(matches!(duplicate, Err(AppError::Conflict(_))));

    assert_eq!(
        run_repo
            .list_for_automation(&active.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn service_creates_merged_base_successor_after_judged_terminal_run() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.base_ref_kind = "local_branch".to_string();
    active.base_ref = "main".to_string();
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();

    let outcome = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: active.id.clone(),
            previous_run_id: previous.id.clone(),
            run_prompt: "Implement the next goal item with the attached spec context.".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap();

    assert!(outcome.scheduled);
    let successor = outcome.run.expect("successor should be returned");
    assert_eq!(successor.run_index, 2);
    assert_eq!(successor.status, AutomationRunStatus::Pending);
    assert_eq!(successor.prompt_author, AutomationPromptAuthor::Judge);
    assert_eq!(successor.base_from_run_id, Some(previous.id));
    assert_eq!(successor.base_ref_kind, "local_branch");
    assert_eq!(successor.base_ref_used, "main");
}

#[tokio::test]
async fn service_drops_source_pr_linkage_for_run_two_base() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.base_ref_kind = "local_branch".to_string();
    active.base_ref = "feature/source-pr".to_string();
    active.base_source_pull_request_json = Some(
        r#"{"number":42,"url":"https://github.test/pull/42","title":"Source PR","headRefName":"feature/source-pr","baseRefName":"release/2026","headRefOid":"abc123"}"#
            .to_string(),
    );
    automation_repo.create(active.clone()).await.unwrap();
    let mut previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    previous.pr_base_ref_name = Some("release/2026".to_string());
    run_repo.create_run(previous.clone()).await.unwrap();

    let successor = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: active.id.clone(),
            previous_run_id: previous.id.clone(),
            run_prompt: "Continue from the merged source PR base branch.".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap()
        .run
        .expect("successor should be created");

    assert_eq!(successor.run_index, 2);
    assert_eq!(successor.base_ref_kind, "local_branch");
    assert_eq!(successor.base_ref_used, "release/2026");
    assert_eq!(successor.base_from_run_id, Some(previous.id));
}

#[tokio::test]
async fn service_applies_stacked_judge_verdict_from_previous_pr_head() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.base_ref_kind = "local_branch".to_string();
    active.base_ref = "main".to_string();
    active.chain_mode = "pr_head_stacked".to_string();
    automation_repo.create(active.clone()).await.unwrap();
    let mut previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    previous.pr_head_ref_name = Some("ralphx/automation-run-1".to_string());
    run_repo.create_run(previous.clone()).await.unwrap();

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous.clone(),
            verdict: AutomationJudgeVerdict {
                decision: AutomationJudgeDecision::Continue,
                goal_met: false,
                reason: "The next item should stack on the previous PR branch.".to_string(),
                confidence: 0.84,
                goal_progress: None,
                updated_item_statuses: None,
                goal_items_proposal: None,
                next_run_prompt: Some(
                    "Implement the next automation item on top of the previous PR head branch."
                        .to_string(),
                ),
                next_base_branch: Some(AutomationJudgeNextBaseBranch::PreviousPrHead),
            },
        })
        .await
        .unwrap();

    let successor = outcome.successor_run.expect("successor should be created");
    assert_eq!(successor.run_index, 2);
    assert_eq!(successor.status, AutomationRunStatus::Pending);
    assert_eq!(successor.prompt_author, AutomationPromptAuthor::Judge);
    assert_eq!(successor.base_ref_kind, "local_branch");
    assert_eq!(successor.base_ref_used, "ralphx/automation-run-1");
    assert_eq!(successor.base_from_run_id, Some(previous.id));
    assert_eq!(
        run_repo
            .list_for_automation(&active.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn service_rejects_stacked_judge_verdict_without_previous_pr_head() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.chain_mode = "pr_head_stacked".to_string();
    automation_repo.create(active.clone()).await.unwrap();
    let mut previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    previous.pr_head_ref_name = None;
    run_repo.create_run(previous.clone()).await.unwrap();

    let error = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous,
            verdict: AutomationJudgeVerdict {
                decision: AutomationJudgeDecision::Continue,
                goal_met: false,
                reason: "The next item should stack on the previous PR branch.".to_string(),
                confidence: 0.84,
                goal_progress: None,
                updated_item_statuses: None,
                goal_items_proposal: None,
                next_run_prompt: Some(
                    "Implement the next automation item on top of the previous PR head branch."
                        .to_string(),
                ),
                next_base_branch: Some(AutomationJudgeNextBaseBranch::PreviousPrHead),
            },
        })
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
    assert_eq!(
        run_repo
            .list_for_automation(&active.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn service_pauses_before_successor_when_max_runs_exhausted() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.max_runs = 1;
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();

    let outcome = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: active.id.clone(),
            previous_run_id: previous.id,
            run_prompt: "Try to continue beyond max runs.".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(outcome.reason.as_deref(), Some("max_runs_exhausted"));
    assert_eq!(
        automation_repo
            .get_by_id(&active.id)
            .await
            .unwrap()
            .unwrap()
            .paused_reason_code
            .as_deref(),
        Some("max_runs_exhausted")
    );
    assert_eq!(
        run_repo
            .list_for_automation(&active.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn service_pauses_before_successor_when_failure_guardrail_exhausted() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.max_consecutive_failures = 2;
    automation_repo.create(active.clone()).await.unwrap();
    let failure_one = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::PrClosed,
        AutomationJudgeState::Done,
    );
    let failure_two = automation_run(
        "run-2",
        &active.id,
        2,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(failure_one).await.unwrap();
    run_repo.create_run(failure_two.clone()).await.unwrap();

    let outcome = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: active.id.clone(),
            previous_run_id: failure_two.id,
            run_prompt: "Try to continue after repeated failures.".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(outcome.reason.as_deref(), Some("max_consecutive_failures"));
    assert_eq!(
        automation_repo
            .get_by_id(&active.id)
            .await
            .unwrap()
            .unwrap()
            .paused_reason_code
            .as_deref(),
        Some("max_consecutive_failures")
    );
    assert_eq!(
        run_repo
            .list_for_automation(&active.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn service_excludes_workspace_review_blocked_run_from_failure_guardrail() {
    // A workspace-review-gate block terminalizes the run as AgentFailed with the blocked error
    // code, but it is user-actionable — it must NOT count toward max_consecutive_failures.
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.max_consecutive_failures = 1;
    active.base_ref_kind = "local_branch".to_string();
    active.base_ref = "main".to_string();
    automation_repo.create(active.clone()).await.unwrap();

    let mut blocked = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    blocked.error_code = Some("workspace_review_blocked".to_string());
    run_repo.create_run(blocked.clone()).await.unwrap();

    let outcome = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: active.id.clone(),
            previous_run_id: blocked.id.clone(),
            run_prompt: "Continue after the review gate is resolved.".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap();

    assert!(
        outcome.scheduled,
        "review-gate block must not trip the failure guardrail"
    );
    assert_ne!(outcome.reason.as_deref(), Some("max_consecutive_failures"));
    assert_ne!(
        automation_repo
            .get_by_id(&active.id)
            .await
            .unwrap()
            .unwrap()
            .paused_reason_code
            .as_deref(),
        Some("max_consecutive_failures")
    );

    // Control: a genuine AgentFailed run at the same threshold DOES trip the guardrail.
    let (control_service, control_repo, control_runs) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut control = automation("automation-control", AutomationStatus::Active);
    control.max_consecutive_failures = 1;
    control.base_ref_kind = "local_branch".to_string();
    control.base_ref = "main".to_string();
    control_repo.create(control.clone()).await.unwrap();
    let genuine = automation_run(
        "run-1",
        &control.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    control_runs.create_run(genuine.clone()).await.unwrap();
    let control_outcome = control_service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: control.id.clone(),
            previous_run_id: genuine.id.clone(),
            run_prompt: "Continue after a genuine failure.".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap();
    assert!(!control_outcome.scheduled);
    assert_eq!(
        control_outcome.reason.as_deref(),
        Some("max_consecutive_failures")
    );
}

#[tokio::test]
async fn service_terminalizes_review_blocked_run_with_event_and_goal_sync() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.goal_items_json =
        Some(json!([{ "id": "phase-1", "title": "Run 1", "status": "in_progress" }]).to_string());
    automation_repo.create(active.clone()).await.unwrap();
    let running = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(running.clone()).await.unwrap();

    let changed = service
        .terminalize_blocked_run(
            &active.id,
            &running,
            "workspace_review_blocked",
            Some("Review blocked publication".to_string()),
        )
        .await
        .unwrap();

    assert!(changed);
    let stored_run = run_repo.get_by_id(&running.id).await.unwrap().unwrap();
    assert_eq!(stored_run.status, AutomationRunStatus::AgentFailed);
    assert_eq!(
        stored_run.error_code.as_deref(),
        Some("workspace_review_blocked")
    );
    let stored_automation = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(
            stored_automation.goal_items_json.as_deref().unwrap(),
            "phase-1",
        ),
        "pending"
    );
    let events = emitter.events();
    assert!(events.contains(&AutomationEvent::AutomationRunUpdated {
        automation_id: active.id.clone(),
        run_id: running.id.clone()
    }));
    assert!(events.contains(&AutomationEvent::AutomationUpdated {
        automation_id: active.id.clone()
    }));

    let already_terminal = run_repo.get_by_id(&running.id).await.unwrap().unwrap();
    let changed_again = service
        .terminalize_blocked_run(
            &active.id,
            &already_terminal,
            "workspace_review_blocked",
            Some("Review blocked publication".to_string()),
        )
        .await
        .unwrap();
    assert!(!changed_again);
}

#[tokio::test]
async fn service_cancel_run_and_stop_use_run_transition_service() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let run = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Implement item 1".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
        })
        .await
        .unwrap();

    let cancelled = service.cancel_run(&active.id, &run.id).await.unwrap();
    assert_eq!(cancelled.status, AutomationRunStatus::Cancelled);

    let second = automation("automation-2", AutomationStatus::Active);
    automation_repo.create(second.clone()).await.unwrap();
    let second_run = service
        .create_run(CreateAutomationRunInput {
            automation_id: second.id.clone(),
            run_prompt: "Implement item 2".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
        })
        .await
        .unwrap();

    let stopped = service.stop(&second.id).await.unwrap();
    assert_eq!(stopped.status, AutomationStatus::Stopped);
    assert_eq!(
        run_repo
            .get_by_id(&second_run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationRunStatus::Cancelled
    );
    assert_eq!(
        emitter.events(),
        vec![
            AutomationEvent::AutomationRunUpdated {
                automation_id: active.id.clone(),
                run_id: run.id.clone()
            },
            AutomationEvent::AutomationRunUpdated {
                automation_id: active.id,
                run_id: run.id,
            },
            AutomationEvent::AutomationRunUpdated {
                automation_id: second.id.clone(),
                run_id: second_run.id.clone()
            },
            AutomationEvent::AutomationRunUpdated {
                automation_id: second.id.clone(),
                run_id: second_run.id
            },
            AutomationEvent::AutomationUpdated {
                automation_id: second.id
            },
        ]
    );
}

#[tokio::test]
async fn service_cancel_run_reverts_in_progress_goal_items_and_emits_update() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "in_progress" },
            { "id": "item-2", "title": "Second", "status": "done" },
            { "id": "item-3", "title": "Third", "status": "in_progress" }
        ])
        .to_string(),
    );
    automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    let cancelled = service.cancel_run(&active.id, &run.id).await.unwrap();

    assert_eq!(cancelled.status, AutomationRunStatus::Cancelled);
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    let goal_items_json = stored.goal_items_json.as_deref().unwrap();
    assert_eq!(item_status(goal_items_json, "item-1"), "pending");
    assert_eq!(item_status(goal_items_json, "item-2"), "done");
    assert_eq!(item_status(goal_items_json, "item-3"), "pending");
    assert_eq!(
        emitter.events(),
        vec![
            AutomationEvent::AutomationRunUpdated {
                automation_id: active.id.clone(),
                run_id: run.id,
            },
            AutomationEvent::AutomationUpdated {
                automation_id: active.id
            },
        ]
    );
}

#[tokio::test]
async fn service_cancel_run_keeps_close_path_successful_for_malformed_goal_items() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.goal_items_json = Some("not-json".to_string());
    automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();

    let cancelled = service.cancel_run(&active.id, &run.id).await.unwrap();

    assert_eq!(cancelled.status, AutomationRunStatus::Cancelled);
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.goal_items_json.as_deref(), Some("not-json"));
    assert_eq!(
        emitter.events(),
        vec![AutomationEvent::AutomationRunUpdated {
            automation_id: active.id,
            run_id: run.id,
        }]
    );
}

#[tokio::test]
async fn service_stop_sweep_cancels_open_run_and_reverts_in_progress_goal_items() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "in_progress" }
        ])
        .to_string(),
    );
    automation_repo.create(active.clone()).await.unwrap();
    let mut parked = automation_run(
        "run-parked",
        &active.id,
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    parked.plan_judge_state = AutomationPlanJudgeState::InProgress;
    parked.plan_judge_lease_expires_at = Some(Utc::now() + chrono::Duration::minutes(5));
    parked.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    run_repo.create_run(parked.clone()).await.unwrap();

    let stopped = service.stop(&active.id).await.unwrap();

    assert_eq!(stopped.status, AutomationStatus::Stopped);
    let cancelled = run_repo.get_by_id(&parked.id).await.unwrap().unwrap();
    assert_eq!(cancelled.status, AutomationRunStatus::Cancelled);
    assert_eq!(cancelled.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(cancelled.plan_judge_lease_expires_at.is_none());
    assert!(cancelled.plan_judge_verdict_json.is_none());
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "item-2"),
        "pending"
    );
}

#[tokio::test]
async fn service_stop_keeps_automation_active_when_run_cancellation_fails() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    let run_repo = Arc::new(SkipJudgeLosesRunRepository::new(vec![run.clone()]));
    let service = AutomationService::new(
        automation_repo.clone(),
        run_repo.clone(),
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );

    let error = service.stop(&active.id).await.unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
    assert_eq!(
        automation_repo
            .get_by_id(&active.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
    assert_eq!(
        run_repo.get_by_id(&run.id).await.unwrap().unwrap().status,
        AutomationRunStatus::Running
    );
}

#[tokio::test]
async fn service_stop_reverts_in_progress_goal_items_when_no_run_is_cancellable() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.goal_items_json = Some(
        json!([
            { "id": "item-1", "title": "Closed run work", "status": "in_progress" },
            { "id": "item-2", "title": "Done", "status": "done" }
        ])
        .to_string(),
    );
    automation_repo.create(active.clone()).await.unwrap();
    let closed = automation_run(
        "run-closed",
        &active.id,
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(closed.clone()).await.unwrap();

    let stopped = service.stop(&active.id).await.unwrap();

    assert_eq!(stopped.status, AutomationStatus::Stopped);
    assert_eq!(
        run_repo
            .get_by_id(&closed.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationRunStatus::Completed
    );
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "item-1"),
        "pending"
    );
    assert!(emitter
        .events()
        .contains(&AutomationEvent::AutomationUpdated {
            automation_id: active.id
        }));
}

#[tokio::test]
async fn service_cancel_run_clears_parked_plan_judge_state() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::InProgress;
    run.plan_judge_lease_expires_at = Some(Utc::now() + chrono::Duration::minutes(5));
    run.plan_judge_verdict_json = Some(r#"{"decision":"revise"}"#.to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    let cancelled = service.cancel_run(&active.id, &run.id).await.unwrap();

    assert_eq!(cancelled.status, AutomationRunStatus::Cancelled);
    let updated = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(updated.status, AutomationRunStatus::Cancelled);
    assert_eq!(updated.plan_judge_state, AutomationPlanJudgeState::None);
    assert!(updated.plan_judge_lease_expires_at.is_none());
    assert!(updated.plan_judge_verdict_json.is_none());
}

#[tokio::test]
async fn service_cancel_running_automatic_pr_run_disarms_auto_merge() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let github = Arc::new(MockGithubService::new());
    let (service, automation_repo, run_repo) = service_with_auto_merge_controls(
        Arc::new(NoopAutomationEventEmitter),
        Arc::clone(&workspace_repo),
        Arc::clone(&github),
    );
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();
    let conversation_id = run.conversation_id.clone().unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(101);
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_auto_merge_method = DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string();
    workspace_repo.create_or_update(workspace).await.unwrap();

    let cancelled = service.cancel_run(&active.id, &run.id).await.unwrap();

    assert_eq!(cancelled.status, AutomationRunStatus::Cancelled);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!workspace.pr_auto_merge_desired);
    assert_eq!(workspace.pr_auto_merge_current, Some(false));
    let github_state = github.state();
    assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
    assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(101));
}

#[tokio::test]
async fn service_stop_running_automatic_pr_run_disarms_auto_merge() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let github = Arc::new(MockGithubService::new());
    let (service, automation_repo, run_repo) = service_with_auto_merge_controls(
        Arc::new(NoopAutomationEventEmitter),
        Arc::clone(&workspace_repo),
        Arc::clone(&github),
    );
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();
    let conversation_id = run.conversation_id.clone().unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(101);
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_auto_merge_method = DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string();
    workspace_repo.create_or_update(workspace).await.unwrap();

    let stopped = service.stop(&active.id).await.unwrap();

    assert_eq!(stopped.status, AutomationStatus::Stopped);
    assert_eq!(
        run_repo.get_by_id(&run.id).await.unwrap().unwrap().status,
        AutomationRunStatus::Cancelled
    );
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!workspace.pr_auto_merge_desired);
    assert_eq!(workspace.pr_auto_merge_current, Some(false));
    let github_state = github.state();
    assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
    assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(101));
}

#[tokio::test]
async fn service_cancel_published_automatic_pr_run_disarms_auto_merge() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let github = Arc::new(MockGithubService::new());
    let (service, automation_repo, run_repo) = service_with_auto_merge_controls(
        Arc::new(NoopAutomationEventEmitter),
        Arc::clone(&workspace_repo),
        Arc::clone(&github),
    );
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.pr_merge_mode = AutomationPrMergeMode::Automatic;
    automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Published,
        AutomationJudgeState::None,
    );
    run_repo.create_run(run.clone()).await.unwrap();
    let conversation_id = run.conversation_id.clone().unwrap();
    let mut workspace = workspace(&conversation_id);
    workspace.publication_pr_number = Some(101);
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_auto_merge_method = DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string();
    workspace_repo.create_or_update(workspace).await.unwrap();

    let cancelled = service.cancel_run(&active.id, &run.id).await.unwrap();

    assert_eq!(cancelled.status, AutomationRunStatus::Cancelled);
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!workspace.pr_auto_merge_desired);
    assert_eq!(workspace.pr_auto_merge_current, Some(false));
    let github_state = github.state();
    assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
    assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(101));
}

#[tokio::test]
async fn service_delete_is_terminal_only_and_removes_runs() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let run = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Implement item 1".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
        })
        .await
        .unwrap();

    let active_delete = service.delete(&active.id).await.unwrap_err();
    assert!(matches!(active_delete, AppError::Validation(_)));

    service.cancel_run(&active.id, &run.id).await.unwrap();
    service.stop(&active.id).await.unwrap();
    service.delete(&active.id).await.unwrap();

    assert!(automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .is_none());
    assert!(run_repo
        .list_for_automation(&active.id)
        .await
        .unwrap()
        .is_empty());
    // The row-delete core emits AutomationDeleted (not AutomationUpdated) with the
    // project id captured before the row was removed.
    assert!(emitter
        .events()
        .contains(&AutomationEvent::AutomationDeleted {
            automation_id: active.id,
            project_id: ProjectId::from_string("project-1".to_string()),
        }));
}

#[tokio::test]
async fn service_run_now_applies_stored_continue_verdict_after_resume() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let paused = automation("automation-1", AutomationStatus::Paused);
    automation_repo.create(paused.clone()).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run.judge_verdict_json = Some(continue_verdict(
        "Implement the next automation item with focused tests and publish the follow-up PR.",
    ));
    run_repo.create_run(run).await.unwrap();

    let outcome = service.trigger_run_now(&paused.id).await.unwrap();

    assert!(outcome.scheduled);
    let automation = automation_repo
        .get_by_id(&paused.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    let runs = run_repo.list_for_automation(&paused.id).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
    assert_eq!(runs[1].prompt_author, AutomationPromptAuthor::Judge);
    assert_eq!(
        runs[1].run_prompt,
        "Implement the next automation item with focused tests and publish the follow-up PR."
    );
    assert_eq!(runs[1].base_from_run_id, Some(runs[0].id.clone()));
}

#[tokio::test]
async fn service_run_now_refuses_plan_gate_paused_automation_without_unpausing() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut paused = automation("automation-1", AutomationStatus::Paused);
    paused.paused_reason_code = Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &paused.id,
            1,
            AutomationRunStatus::AwaitingPlanApproval,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();

    let error = service.trigger_run_now(&paused.id).await.unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains(AUTOMATION_PLAN_GATE_TRIGGER_RUN_NOW_ERROR_CODE))
    );
    let automation = automation_repo
        .get_by_id(&paused.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Paused);
    assert_eq!(
        automation.paused_reason_code.as_deref(),
        Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE)
    );
}

#[tokio::test]
async fn service_run_now_refuses_judge_in_progress() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &active.id,
            1,
            AutomationRunStatus::Merged,
            AutomationJudgeState::InProgress,
        ))
        .await
        .unwrap();

    let outcome = service.trigger_run_now(&active.id).await.unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(outcome.reason.as_deref(), Some("run in flight"));
}

#[tokio::test]
async fn service_retry_judge_requires_latest_failed_terminal_judge() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &active.id,
            1,
            AutomationRunStatus::Merged,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();

    let action = service.retry_judge_action(&active.id).await.unwrap();

    assert_eq!(
        action.into_schedule_outcome().reason.as_deref(),
        Some("latest judge is not failed")
    );
}

#[tokio::test]
async fn service_retry_judge_redispatches_failed_terminal_latest_run() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let failed = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    run_repo.create_run(failed.clone()).await.unwrap();

    let action = service.retry_judge_action(&active.id).await.unwrap();

    match action {
        AutomationRunNowAction::StartJudge {
            automation, run, ..
        } => {
            assert_eq!(automation.id, active.id);
            assert_eq!(run.id, failed.id);
        }
        AutomationRunNowAction::Outcome(outcome) => {
            panic!("expected judge retry dispatch, got {outcome:?}");
        }
    }
}

#[tokio::test]
async fn service_retry_plan_judge_reactivates_exact_current_failed_parked_run() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut paused = automation("automation-1", AutomationStatus::Paused);
    paused.paused_reason_code = Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::Failed;
    run.plan_last_parked_artifact_id = Some("plan-current".to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    let outcome = service
        .retry_plan_judge(&paused.id, "plan-current")
        .await
        .unwrap();

    assert!(outcome.scheduled);
    assert_eq!(
        automation_repo
            .get_by_id(&paused.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
    assert_eq!(
        run_repo
            .get_by_id(&run.id)
            .await
            .unwrap()
            .unwrap()
            .plan_judge_state,
        AutomationPlanJudgeState::None
    );
}

#[tokio::test]
async fn service_retry_plan_judge_reports_validation_and_readiness_reasons() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-active", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let empty_artifact = service
        .retry_plan_judge(&active.id, "   ")
        .await
        .unwrap_err();
    assert!(matches!(
        empty_artifact,
        AppError::Validation(message)
            if message == "current plan artifact is required to retry plan judge"
    ));

    let stopped = automation("automation-stopped", AutomationStatus::Stopped);
    automation_repo.create(stopped.clone()).await.unwrap();
    let inactive_error = service
        .retry_plan_judge(&stopped.id, "plan-current")
        .await
        .unwrap_err();
    assert!(
        matches!(inactive_error, AppError::Validation(message) if message.contains("active or paused"))
    );

    let completed = automation_run(
        "run-completed",
        &active.id,
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::None,
    );
    run_repo.create_run(completed).await.unwrap();
    let not_awaiting = service
        .retry_plan_judge(&active.id, "plan-current")
        .await
        .unwrap();
    assert!(!not_awaiting.scheduled);
    assert_eq!(
        not_awaiting.reason.as_deref(),
        Some("latest run is not awaiting plan approval")
    );

    let mut waiting = automation_run(
        "run-waiting",
        &active.id,
        2,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    waiting.plan_last_parked_artifact_id = Some("plan-current".to_string());
    waiting.plan_judge_state = AutomationPlanJudgeState::None;
    run_repo.create_run(waiting).await.unwrap();
    let not_failed = service
        .retry_plan_judge(&active.id, "plan-current")
        .await
        .unwrap();
    assert!(!not_failed.scheduled);
    assert_eq!(
        not_failed.reason.as_deref(),
        Some("latest plan judge is not failed")
    );
}

#[tokio::test]
async fn service_retry_plan_judge_rolls_back_pause_when_retry_cas_errors() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let mut paused = automation("automation-1", AutomationStatus::Paused);
    paused.paused_reason_code = Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::Failed;
    run.plan_last_parked_artifact_id = Some("plan-current".to_string());
    let run_repo = Arc::new(SkipJudgeLosesRunRepository::new(vec![run.clone()]));
    let service = AutomationService::new(
        automation_repo.clone(),
        run_repo,
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );

    let error = service
        .retry_plan_judge(&paused.id, "plan-current")
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message == "unused test repository method")
    );
    let stored = automation_repo
        .get_by_id(&paused.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Paused);
    assert_eq!(
        stored.paused_reason_code.as_deref(),
        Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE)
    );
}

#[tokio::test]
async fn service_retry_plan_judge_rejects_stale_artifact_without_reactivating() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut paused = automation("automation-1", AutomationStatus::Paused);
    paused.paused_reason_code = Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::AwaitingPlanApproval,
        AutomationJudgeState::None,
    );
    run.plan_judge_state = AutomationPlanJudgeState::Failed;
    run.plan_last_parked_artifact_id = Some("plan-current".to_string());
    run_repo.create_run(run.clone()).await.unwrap();

    let error = service
        .retry_plan_judge(&paused.id, "plan-stale")
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Conflict(_)));
    assert_eq!(
        automation_repo
            .get_by_id(&paused.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Paused
    );
    assert_eq!(
        run_repo
            .get_by_id(&run.id)
            .await
            .unwrap()
            .unwrap()
            .plan_judge_state,
        AutomationPlanJudgeState::Failed
    );
}

#[tokio::test]
async fn service_run_now_direct_helper_does_not_claim_judge_dispatched() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &active.id,
            1,
            AutomationRunStatus::Merged,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();

    let outcome = service.trigger_run_now(&active.id).await.unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(outcome.reason.as_deref(), Some("judge dispatcher required"));
}

#[tokio::test]
async fn service_skip_judge_creates_template_successor_for_latest_terminal_run() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, run_repo) = service_with_emitter(emitter.clone());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let mut run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    run.pr_number = Some(593);
    run_repo.create_run(run.clone()).await.unwrap();

    let outcome = service.skip_judge(&active.id, &run.id).await.unwrap();

    assert!(outcome.scheduled);
    let runs = run_repo.list_for_automation(&active.id).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].judge_state, AutomationJudgeState::Skipped);
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
    assert_eq!(
        runs[1].prompt_author,
        AutomationPromptAuthor::SkipJudgeTemplate
    );
    assert_eq!(
        runs[1].run_prompt,
        "Continue the goal; previous run merged PR #593."
    );
    assert_eq!(runs[1].base_from_run_id, Some(run.id.clone()));
    assert!(emitter
        .events()
        .contains(&AutomationEvent::AutomationRunUpdated {
            automation_id: active.id,
            run_id: run.id,
        }));
}

#[tokio::test]
async fn service_skip_judge_resumes_paused_failed_judge_and_creates_successor() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut paused = automation("automation-1", AutomationStatus::Paused);
    paused.paused_reason_code = Some("judge_failed".to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let failed_run = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    run_repo.create_run(failed_run.clone()).await.unwrap();

    let outcome = service
        .skip_judge(&paused.id, &failed_run.id)
        .await
        .unwrap();

    assert!(outcome.scheduled);
    let stored = automation_repo
        .get_by_id(&paused.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Active);
    assert!(stored.paused_reason_code.is_none());
    let runs = run_repo.list_for_automation(&paused.id).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].judge_state, AutomationJudgeState::Skipped);
    assert_eq!(runs[1].base_from_run_id, Some(failed_run.id));
}

#[tokio::test]
async fn service_skip_judge_rejects_non_latest_run() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    run_repo
        .create_run(automation_run(
            "run-1",
            &active.id,
            1,
            AutomationRunStatus::Merged,
            AutomationJudgeState::Done,
        ))
        .await
        .unwrap();
    let latest = automation_run(
        "run-2",
        &active.id,
        2,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    run_repo.create_run(latest).await.unwrap();

    let error = service
        .skip_judge(&active.id, &AutomationRunId::from_string("run-1"))
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Validation(message) if message.contains("latest")));
}

#[tokio::test]
async fn service_skip_judge_loses_cleanly_when_scheduler_started_judge() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::None,
    );
    let run_repo = Arc::new(SkipJudgeLosesRunRepository::new(vec![run.clone()]));
    let service = AutomationService::new(
        automation_repo,
        run_repo,
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );

    let outcome = service.skip_judge(&active.id, &run.id).await.unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(outcome.reason.as_deref(), Some("judge already started"));
}

#[tokio::test]
async fn service_skip_judge_loses_cleanly_when_failed_judge_was_redispatched() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let mut paused = automation("automation-1", AutomationStatus::Paused);
    paused.paused_reason_code = Some("judge_failed".to_string());
    automation_repo.create(paused.clone()).await.unwrap();
    let run = automation_run(
        "run-1",
        &paused.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    let run_repo = Arc::new(SkipJudgeLosesRunRepository::new(vec![run.clone()]));
    let service = AutomationService::new(
        automation_repo,
        run_repo,
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );

    let outcome = service.skip_judge(&paused.id, &run.id).await.unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(
        outcome.reason.as_deref(),
        Some("not skipped: judge redispatched")
    );
}

#[tokio::test]
async fn service_run_now_reports_all_unscheduled_readiness_reasons() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));

    let stopped = automation("automation-stopped", AutomationStatus::Stopped);
    automation_repo.create(stopped.clone()).await.unwrap();
    let stopped_outcome = service.trigger_run_now(&stopped.id).await.unwrap();
    assert_eq!(
        stopped_outcome.reason.as_deref(),
        Some("automation is not active")
    );

    let active_without_runs = automation("automation-empty", AutomationStatus::Active);
    automation_repo
        .create(active_without_runs.clone())
        .await
        .unwrap();
    let no_runs = service
        .trigger_run_now(&active_without_runs.id)
        .await
        .unwrap();
    assert_eq!(no_runs.reason.as_deref(), Some("automation has no runs"));

    let active_with_cancelled = automation("automation-cancelled", AutomationStatus::Active);
    automation_repo
        .create(active_with_cancelled.clone())
        .await
        .unwrap();
    run_repo
        .create_run(automation_run(
            "run-cancelled",
            &active_with_cancelled.id,
            1,
            AutomationRunStatus::Cancelled,
            AutomationJudgeState::None,
        ))
        .await
        .unwrap();
    let replacement = service
        .trigger_run_now(&active_with_cancelled.id)
        .await
        .unwrap();
    assert!(replacement.scheduled);
    assert_eq!(replacement.reason, None);
    let replacement_run = run_repo
        .latest_for_automation(&active_with_cancelled.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replacement_run.run_index, 2);
    assert_eq!(replacement_run.status, AutomationRunStatus::Pending);
    assert_eq!(replacement_run.run_prompt, "Run 1 prompt");

    let active_missing_verdict = automation("automation-missing-verdict", AutomationStatus::Active);
    automation_repo
        .create(active_missing_verdict.clone())
        .await
        .unwrap();
    let mut missing_verdict_run = automation_run(
        "run-missing-verdict",
        &active_missing_verdict.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    missing_verdict_run.judge_verdict_json = None;
    run_repo.create_run(missing_verdict_run).await.unwrap();
    let missing_verdict = service
        .trigger_run_now(&active_missing_verdict.id)
        .await
        .unwrap();
    assert_eq!(
        missing_verdict.reason.as_deref(),
        Some("judge verdict is missing")
    );

    let active_skipped = automation("automation-skipped", AutomationStatus::Active);
    automation_repo
        .create(active_skipped.clone())
        .await
        .unwrap();
    run_repo
        .create_run(automation_run(
            "run-skipped",
            &active_skipped.id,
            1,
            AutomationRunStatus::Merged,
            AutomationJudgeState::Skipped,
        ))
        .await
        .unwrap();
    let skipped = service.trigger_run_now(&active_skipped.id).await.unwrap();
    assert_eq!(skipped.reason.as_deref(), Some("judge already skipped"));

    let active_failed = automation("automation-failed-judge", AutomationStatus::Active);
    automation_repo.create(active_failed.clone()).await.unwrap();
    let failed_run = automation_run(
        "run-failed-judge",
        &active_failed.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    run_repo.create_run(failed_run.clone()).await.unwrap();
    let action = service
        .trigger_run_now_action(&active_failed.id)
        .await
        .unwrap();
    match action {
        AutomationRunNowAction::StartJudge {
            automation, run, ..
        } => {
            assert_eq!(automation.id, active_failed.id);
            assert_eq!(run.id, failed_run.id);
        }
        AutomationRunNowAction::Outcome(outcome) => {
            panic!("expected judge start, got {outcome:?}");
        }
    }
}

#[tokio::test]
async fn service_skip_judge_reports_fail_closed_readiness_reasons() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));

    let paused = automation("automation-paused", AutomationStatus::Paused);
    automation_repo.create(paused.clone()).await.unwrap();
    let paused_error = service
        .skip_judge(&paused.id, &AutomationRunId::from_string("missing"))
        .await
        .unwrap_err();
    assert!(matches!(paused_error, AppError::Validation(message) if message.contains("active")));

    let mut wrong_chain = automation("automation-chain", AutomationStatus::Active);
    wrong_chain.chain_mode = "unsupported".to_string();
    automation_repo.create(wrong_chain.clone()).await.unwrap();
    let chain_error = service
        .skip_judge(&wrong_chain.id, &AutomationRunId::from_string("missing"))
        .await
        .unwrap_err();
    assert!(matches!(chain_error, AppError::Validation(message) if message.contains("chain_mode")));

    let active_done = automation("automation-done", AutomationStatus::Active);
    automation_repo.create(active_done.clone()).await.unwrap();
    let done_run = automation_run(
        "run-done",
        &active_done.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(done_run.clone()).await.unwrap();
    let already_started = service
        .skip_judge(&active_done.id, &done_run.id)
        .await
        .unwrap();
    assert_eq!(
        already_started.reason.as_deref(),
        Some("judge already started")
    );

    let active_running = automation("automation-running", AutomationStatus::Active);
    automation_repo
        .create(active_running.clone())
        .await
        .unwrap();
    let running_run = automation_run(
        "run-running",
        &active_running.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    run_repo.create_run(running_run.clone()).await.unwrap();
    let not_ready = service
        .skip_judge(&active_running.id, &running_run.id)
        .await
        .unwrap();
    assert_eq!(
        not_ready.reason.as_deref(),
        Some("run is not ready for judge skipping")
    );
}

#[tokio::test]
async fn service_create_run_and_successor_validate_inputs_before_writes() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let draft = automation("automation-draft", AutomationStatus::Draft);
    automation_repo.create(draft.clone()).await.unwrap();

    let inactive_run = service
        .create_run(CreateAutomationRunInput {
            automation_id: draft.id.clone(),
            run_prompt: "will not run".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(inactive_run, AppError::Validation(message) if message.contains("active")));

    let active = automation("automation-active", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let empty_prompt = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "   ".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(empty_prompt, AppError::Validation(message) if message.contains("prompt")));

    let previous = automation_run(
        "run-previous",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    let latest = automation_run(
        "run-latest",
        &active.id,
        2,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    run_repo.create_run(latest).await.unwrap();
    let stale_previous = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: active.id.clone(),
            previous_run_id: previous.id,
            run_prompt: "Continue".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap_err();
    assert!(matches!(stale_previous, AppError::Validation(message) if message.contains("latest")));

    let mut wrong_chain = automation("automation-wrong-chain", AutomationStatus::Active);
    wrong_chain.chain_mode = "unsupported".to_string();
    automation_repo.create(wrong_chain.clone()).await.unwrap();
    let chain_error = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: wrong_chain.id.clone(),
            previous_run_id: AutomationRunId::from_string("missing"),
            run_prompt: "Continue".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap_err();
    assert!(matches!(chain_error, AppError::Validation(message) if message.contains("chain_mode")));

    let active_without_runs = automation("automation-no-runs", AutomationStatus::Active);
    automation_repo
        .create(active_without_runs.clone())
        .await
        .unwrap();
    let missing_run = service
        .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
            automation_id: active_without_runs.id.clone(),
            previous_run_id: AutomationRunId::from_string("missing"),
            run_prompt: "   ".to_string(),
            prompt_author: AutomationPromptAuthor::Judge,
        })
        .await
        .unwrap_err();
    assert!(matches!(missing_run, AppError::Validation(message) if message.contains("prompt")));
}

#[tokio::test]
async fn service_judge_verdict_stop_and_loop_outcomes_update_automation_state() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));

    let active_complete = automation("automation-complete", AutomationStatus::Active);
    automation_repo
        .create(active_complete.clone())
        .await
        .unwrap();
    let complete_run = automation_run(
        "run-complete",
        &active_complete.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(complete_run.clone()).await.unwrap();
    let complete = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active_complete.clone(),
            previous_run: complete_run,
            verdict: stop_verdict(true, "Goal is complete"),
        })
        .await
        .unwrap();
    assert_eq!(
        complete.terminal_automation_status,
        Some(AutomationStatus::Completed)
    );

    let mut active_unmet = automation("automation-unmet", AutomationStatus::Active);
    active_unmet.goal_items_json =
        Some(json!([{ "id": "phase-1", "title": "Run 1", "status": "in_progress" }]).to_string());
    automation_repo.create(active_unmet.clone()).await.unwrap();
    let unmet_run = automation_run(
        "run-unmet",
        &active_unmet.id,
        1,
        AutomationRunStatus::PrClosed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(unmet_run.clone()).await.unwrap();
    let unmet = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active_unmet.clone(),
            previous_run: unmet_run,
            verdict: stop_verdict(false, "The goal is not met but should not continue"),
        })
        .await
        .unwrap();
    assert_eq!(
        unmet.terminal_automation_status,
        Some(AutomationStatus::Paused)
    );
    assert_eq!(unmet.reason.as_deref(), Some("judge_stopped_unmet"));
    let unmet_stored = automation_repo
        .get_by_id(&active_unmet.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(unmet_stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );

    let mut active_loop = automation("automation-loop", AutomationStatus::Active);
    active_loop.goal_items_json =
        Some(json!([{ "id": "phase-1", "title": "Run 1", "status": "in_progress" }]).to_string());
    automation_repo.create(active_loop.clone()).await.unwrap();
    let mut loop_run = automation_run(
        "run-loop",
        &active_loop.id,
        1,
        AutomationRunStatus::PrClosed,
        AutomationJudgeState::Done,
    );
    loop_run.run_prompt = "repeat me".to_string();
    run_repo.create_run(loop_run.clone()).await.unwrap();
    let suspected = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active_loop.clone(),
            previous_run: loop_run,
            verdict: continue_verdict_struct(
                "repeat me",
                AutomationJudgeNextBaseBranch::AutomationBase,
            ),
        })
        .await
        .unwrap();
    assert_eq!(
        suspected.terminal_automation_status,
        Some(AutomationStatus::Paused)
    );
    assert_eq!(suspected.reason.as_deref(), Some("judge_loop_suspected"));
    let loop_stored = automation_repo
        .get_by_id(&active_loop.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(loop_stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );
}

#[tokio::test]
async fn service_judge_successor_readiness_pause_cas_loss_is_clean_outcome() {
    let automation_repo = Arc::new(LostStatusAutomationRepository::new(
        AutomationStatus::Active,
        AutomationStatus::Paused,
    ));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.max_runs = 1;
    automation_repo.create(active.clone()).await.unwrap();
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        MemoryAutomationRepository::new_shared_state(),
    ));
    let mut previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    previous.judge_verdict_json = Some(continue_verdict(
        "Continue with the next scoped implementation slice and publish the resulting pull request.",
    ));
    run_repo.create_run(previous.clone()).await.unwrap();
    let service = AutomationService::new(
        automation_repo.clone(),
        run_repo,
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(RecordingArtifactRepository::default()),
        notification_service(),
    );

    let outcome = service
        .apply_stored_judge_verdict(&active.id, &previous.id)
        .await
        .unwrap();

    assert!(outcome.successor_run.is_none());
    assert_eq!(outcome.reason.as_deref(), Some("already settled"));
    assert_eq!(automation_repo.status(), AutomationStatus::Paused);
}

#[tokio::test]
async fn service_judge_continue_successor_keeps_verdict_in_progress_status() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    let mut verdict = continue_verdict_struct(
        "Implement item 1 from the automation goal. Keep the PR scoped and publish it.",
        AutomationJudgeNextBaseBranch::AutomationBase,
    );
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::InProgress,
    }]);

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap();

    assert!(outcome.successor_run.is_some());
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "in_progress"
    );
}

#[tokio::test]
async fn service_judge_stop_goal_met_leaves_exactly_verdict_goal_status() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.goal_items_json =
        Some(json!([{ "id": "phase-1", "title": "Run 1", "status": "in_progress" }]).to_string());
    automation_repo.create(active.clone()).await.unwrap();
    let mut previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    let mut verdict = stop_verdict(true, "Goal is complete");
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::InProgress,
    }]);
    previous.judge_verdict_json = Some(serde_json::to_string(&verdict).unwrap());
    run_repo.create_run(previous.clone()).await.unwrap();

    let outcome = service
        .apply_stored_judge_verdict(&active.id, &previous.id)
        .await
        .unwrap();

    assert_eq!(
        outcome.terminal_automation_status,
        Some(AutomationStatus::Paused)
    );
    assert_eq!(outcome.reason.as_deref(), Some("judge_failed"));
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Paused);
    assert_eq!(stored.paused_reason_code.as_deref(), Some("judge_failed"));
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );
    let stored_run = run_repo.get_by_id(&previous.id).await.unwrap().unwrap();
    assert_eq!(stored_run.judge_state, AutomationJudgeState::Failed);
}

#[tokio::test]
async fn service_judge_verdict_noops_when_latest_run_changed_before_effects() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    let latest = automation_run(
        "run-2",
        &active.id,
        2,
        AutomationRunStatus::Pending,
        AutomationJudgeState::None,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    run_repo.create_run(latest).await.unwrap();
    let mut verdict = stop_verdict(true, "Goal is complete");
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::Done,
    }]);

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap();

    assert_eq!(
        outcome.noop_reason,
        Some(AutomationJudgeApplyNoopReason::NotCurrent)
    );
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Active);
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );
}

#[tokio::test]
async fn service_judge_verdict_noops_when_automation_paused_before_effects() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    automation_repo
        .compare_and_swap_status(
            &active.id,
            AutomationStatus::Active,
            AutomationStatus::Paused,
            None,
            None,
        )
        .await
        .unwrap();
    let mut verdict = stop_verdict(true, "Goal is complete");
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::Done,
    }]);

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap();

    assert_eq!(
        outcome.noop_reason,
        Some(AutomationJudgeApplyNoopReason::NotCurrent)
    );
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Paused);
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );
}

#[tokio::test]
async fn service_complete_judge_verdict_noops_when_dispatch_lease_is_stale() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let lease_expires_at = Utc::now() + chrono::Duration::minutes(3);
    let stale_lease = lease_expires_at + chrono::Duration::minutes(1);
    let mut previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::InProgress,
    );
    previous.judge_lease_expires_at = Some(lease_expires_at);
    run_repo.create_run(previous.clone()).await.unwrap();
    let mut verdict = stop_verdict(true, "Goal is complete");
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::Done,
    }]);
    let verdict_json = serde_json::to_string(&verdict).unwrap();

    let outcome = service
        .complete_judge_verdict(CompleteAutomationJudgeInput {
            automation: active.clone(),
            previous_run: previous.clone(),
            judge_lease_expires_at: stale_lease,
            verdict,
            verdict_json,
            judge_model_id: Some("judge-model".to_string()),
        })
        .await
        .unwrap();

    assert_eq!(
        outcome.noop_reason,
        Some(AutomationJudgeApplyNoopReason::NotCurrent)
    );
    let stored_run = run_repo.get_by_id(&previous.id).await.unwrap().unwrap();
    assert_eq!(stored_run.judge_state, AutomationJudgeState::InProgress);
    assert_eq!(stored_run.judge_verdict_json, None);
    assert_eq!(stored_run.judge_model_id, None);
    assert_eq!(stored_run.judge_lease_expires_at, Some(lease_expires_at));
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Active);
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );
}

#[tokio::test]
async fn service_judge_status_update_recomputes_goal_cas_base_after_refetch() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let stale = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(stale.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &stale.id,
        1,
        AutomationRunStatus::PrClosed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    automation_repo
        .update_goal_items_json_if_unchanged(
            &stale.id,
            stale.goal_items_json.clone(),
            Some(
                json!([
                    { "id": "phase-1", "title": "Run 1", "status": "pending" },
                    { "id": "phase-2", "title": "Concurrent edit", "status": "pending" }
                ])
                .to_string(),
            ),
        )
        .await
        .unwrap()
        .expect("concurrent goal item update should land");
    let mut verdict = stop_verdict(false, "Stop without overwriting concurrent goal items");
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::Done,
    }]);

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: stale.clone(),
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap();

    assert_eq!(
        outcome.terminal_automation_status,
        Some(AutomationStatus::Paused)
    );
    let stored = automation_repo.get_by_id(&stale.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationStatus::Paused);
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "done"
    );
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-2"),
        "pending"
    );
}

#[tokio::test]
async fn service_judge_continue_max_runs_pause_reverts_in_progress_status() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.max_runs = 1;
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    let mut verdict = continue_verdict_struct(
        "Retry item 1 from the automation goal. Keep the PR scoped and publish it.",
        AutomationJudgeNextBaseBranch::AutomationBase,
    );
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::InProgress,
    }]);

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap();

    assert!(outcome.successor_run.is_none());
    assert_eq!(outcome.reason.as_deref(), Some("max_runs_exhausted"));
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Paused);
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );
}

#[tokio::test]
async fn service_judge_continue_consecutive_failure_pause_reverts_in_progress_status() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    active.max_consecutive_failures = 1;
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    let mut verdict = continue_verdict_struct(
        "Retry item 1 from the automation goal. Keep the PR scoped and publish it.",
        AutomationJudgeNextBaseBranch::AutomationBase,
    );
    verdict.updated_item_statuses = Some(vec![AutomationJudgeItemStatusUpdate {
        id: "phase-1".to_string(),
        status: AutomationGoalItemStatus::InProgress,
    }]);

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap();

    assert!(outcome.successor_run.is_none());
    assert_eq!(outcome.reason.as_deref(), Some("max_consecutive_failures"));
    let stored = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, AutomationStatus::Paused);
    assert_eq!(
        item_status(stored.goal_items_json.as_deref().unwrap(), "phase-1"),
        "pending"
    );
}

#[tokio::test]
async fn service_persists_successor_run_prompt_verbatim_for_loop_guard() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-1", AutomationStatus::Active);
    // A spec-linked automation is the case where runs also gain the spawn-time
    // `<automation_context>` prefix; the persisted prompt must still stay clean.
    active.spec_artifact_id = Some("spec-artifact-1".to_string());
    active.base_ref_kind = "local_branch".to_string();
    active.base_ref = "main".to_string();
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-1",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();

    let next_prompt =
        "Implement item 2 from the migration spec. Keep the PR scoped and publish it.";
    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous.clone(),
            verdict: continue_verdict_struct(
                next_prompt,
                AutomationJudgeNextBaseBranch::AutomationBase,
            ),
        })
        .await
        .unwrap();

    let successor = outcome.successor_run.expect("successor should be created");
    // D5: the persisted run_prompt is the judge nextRunPrompt verbatim, with no
    // spawn-time `<automation_context>` prefix, so the loop-guard fingerprint keeps
    // matching when this prompt is repeated.
    assert_eq!(successor.run_prompt, next_prompt);
    assert!(!successor.run_prompt.contains("<automation_context>"));

    // Prove the guard still fires if the judge repeats the same prompt next cycle.
    assert!(automation_judge_loop_suspected(
        &successor,
        &continue_verdict_struct(next_prompt, AutomationJudgeNextBaseBranch::AutomationBase),
    ));
}

#[test]
fn awaiting_plan_approval_service_guards_match_domain_open_predicate() {
    use AutomationJudgeState::{Done, Failed, InProgress, None, Skipped};

    for judge_state in [None, InProgress, Done, Failed, Skipped] {
        assert!(
            is_open_automation_run(AutomationRunStatus::AwaitingPlanApproval, judge_state),
            "awaiting-plan-approval runs should stay open for {judge_state:?}"
        );
    }
    assert!(run_status_is_cancellable(
        AutomationRunStatus::AwaitingPlanApproval
    ));
    assert!(run_status_blocks_trigger_run_now(
        AutomationRunStatus::AwaitingPlanApproval
    ));
}

#[tokio::test]
async fn automation_update_and_finalize_reject_persona_builder_run_mode() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter);
    let draft = automation("automation-persona-builder", AutomationStatus::Draft);
    automation_repo.create(draft.clone()).await.unwrap();

    let update_error = service
        .update_config(UpdateAutomationConfigInput {
            run_mode: Some("persona_builder".to_string()),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap_err();
    assert!(
        matches!(update_error, AppError::Validation(message) if message.contains("PersonaBuilder"))
    );

    let mut persisted_builder = draft.clone();
    persisted_builder.id = AutomationId::from_string("automation-persona-builder-persisted");
    persisted_builder.run_mode = "persona_builder".to_string();
    automation_repo
        .create(persisted_builder.clone())
        .await
        .unwrap();

    let finalize_error = service.finalize(&persisted_builder.id).await.unwrap_err();
    assert!(
        matches!(finalize_error, AppError::Validation(message) if message.contains("PersonaBuilder"))
    );
}

fn continue_verdict(next_prompt: &str) -> String {
    json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "The next item remains and should be implemented in a scoped PR.",
        "confidence": 0.87,
        "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One item complete." },
        "updatedItemStatuses": null,
        "nextRunPrompt": next_prompt,
        "nextBaseBranch": "automation_base"
    })
    .to_string()
}

fn continue_verdict_struct(
    next_prompt: &str,
    next_base_branch: AutomationJudgeNextBaseBranch,
) -> AutomationJudgeVerdict {
    AutomationJudgeVerdict {
        decision: AutomationJudgeDecision::Continue,
        goal_met: false,
        reason: "The next item remains.".to_string(),
        confidence: 0.87,
        goal_progress: None,
        updated_item_statuses: None,
        goal_items_proposal: None,
        next_run_prompt: Some(next_prompt.to_string()),
        next_base_branch: Some(next_base_branch),
    }
}

fn stop_verdict(goal_met: bool, reason: &str) -> AutomationJudgeVerdict {
    AutomationJudgeVerdict {
        decision: AutomationJudgeDecision::Stop,
        goal_met,
        reason: reason.to_string(),
        confidence: 0.91,
        goal_progress: None,
        updated_item_statuses: None,
        goal_items_proposal: None,
        next_run_prompt: None,
        next_base_branch: None,
    }
}

async fn seed_pending_goal_replan(
    service: &AutomationService,
    automation_repo: &Arc<MemoryAutomationRepository>,
    run_repo: &Arc<MemoryAutomationRunRepository>,
) -> (AutomationId, AutomationRun, String) {
    let active = automation("automation-replan", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-replan-source",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    let proposal = vec![
        AutomationJudgeGoalItemProposal {
            id: "phase-2".to_string(),
            title: "Integration follow-up".to_string(),
            status: AutomationGoalItemStatus::Pending,
        },
        AutomationJudgeGoalItemProposal {
            id: "phase-1".to_string(),
            title: "Run 1".to_string(),
            status: AutomationGoalItemStatus::Pending,
        },
    ];
    let proposed_json = serde_json::to_string(&proposal).unwrap();
    let mut verdict = continue_verdict_struct(
        "Plan and implement the integration follow-up with focused tests and publish a scoped pull request.",
        AutomationJudgeNextBaseBranch::AutomationBase,
    );
    verdict.goal_items_proposal = Some(proposal);

    let outcome = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active,
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap();
    (
        AutomationId::from_string("automation-replan"),
        outcome.successor_run.expect("successor run"),
        proposed_json,
    )
}

#[tokio::test]
async fn judge_goal_replan_waits_for_successor_plan_approval_before_applying() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let (automation_id, successor, proposed_json) =
        seed_pending_goal_replan(&service, &automation_repo, &run_repo).await;

    let before_approval = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        before_approval.goal_items_json.as_deref(),
        Some(proposed_json.as_str())
    );
    let pending = parse_authoring_state(before_approval.authoring_state_json.as_deref()).unwrap();
    assert_eq!(
        pending
            .pending_goal_replan
            .as_ref()
            .map(|state| state.status),
        Some(AutomationGoalReplanStatus::Pending)
    );
    assert_eq!(
        successor
            .base_from_run_id
            .as_ref()
            .map(AutomationRunId::as_str),
        Some("run-replan-source")
    );

    assert_eq!(
        service
            .apply_pending_goal_replan_for_run(&automation_id, &successor)
            .await
            .unwrap(),
        PendingGoalReplanApplyOutcome::Applied
    );
    let applied = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        applied.goal_items_json.as_deref(),
        Some(proposed_json.as_str())
    );
    let applied_state = parse_authoring_state(applied.authoring_state_json.as_deref()).unwrap();
    assert_eq!(
        applied_state
            .pending_goal_replan
            .as_ref()
            .map(|state| state.status),
        Some(AutomationGoalReplanStatus::Applied)
    );
}

#[tokio::test]
async fn stale_goal_replan_never_overwrites_newer_goal_items() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let (automation_id, successor, proposed_json) =
        seed_pending_goal_replan(&service, &automation_repo, &run_repo).await;
    let before = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    let newer = json!([
        { "id": "human-phase", "title": "Human override", "status": "pending" }
    ])
    .to_string();
    assert!(service
        .update_goal_items_json_if_unchanged(
            &automation_id,
            before.goal_items_json,
            Some(newer.clone()),
        )
        .await
        .unwrap());

    assert_eq!(
        service
            .apply_pending_goal_replan_for_run(&automation_id, &successor)
            .await
            .unwrap(),
        PendingGoalReplanApplyOutcome::Stale
    );
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.goal_items_json.as_deref(), Some(newer.as_str()));
    assert_ne!(
        stored.goal_items_json.as_deref(),
        Some(proposed_json.as_str())
    );
}

#[tokio::test]
async fn pending_goal_replan_without_successor_lineage_is_a_noop() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let (automation_id, mut successor, proposed_json) =
        seed_pending_goal_replan(&service, &automation_repo, &run_repo).await;
    successor.base_from_run_id = None;

    assert_eq!(
        service
            .apply_pending_goal_replan_for_run(&automation_id, &successor)
            .await
            .unwrap(),
        PendingGoalReplanApplyOutcome::None
    );
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        stored.goal_items_json.as_deref(),
        Some(proposed_json.as_str())
    );
    assert_eq!(
        parse_authoring_state(stored.authoring_state_json.as_deref())
            .unwrap()
            .pending_goal_replan
            .as_ref()
            .map(|state| state.status),
        Some(AutomationGoalReplanStatus::Pending)
    );
}

#[tokio::test]
async fn pending_goal_replan_marks_already_applied_goal_items_as_applied() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let (automation_id, successor, proposed_json) =
        seed_pending_goal_replan(&service, &automation_repo, &run_repo).await;
    let before = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    automation_repo
        .update_goal_items_json_if_unchanged(
            &automation_id,
            before.goal_items_json,
            Some(proposed_json.clone()),
        )
        .await
        .unwrap()
        .expect("concurrent proposal application should land");

    assert_eq!(
        service
            .apply_pending_goal_replan_for_run(&automation_id, &successor)
            .await
            .unwrap(),
        PendingGoalReplanApplyOutcome::Applied
    );
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parse_authoring_state(stored.authoring_state_json.as_deref())
            .unwrap()
            .pending_goal_replan
            .as_ref()
            .map(|state| state.status),
        Some(AutomationGoalReplanStatus::Applied)
    );
}

#[tokio::test]
async fn pending_goal_replan_ignores_nonpending_or_unrelated_proposals() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let (automation_id, successor, proposed_json) =
        seed_pending_goal_replan(&service, &automation_repo, &run_repo).await;
    let current = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    let mut state = parse_authoring_state(current.authoring_state_json.as_deref()).unwrap();
    let replan = state.pending_goal_replan.as_mut().unwrap();
    replan.status = AutomationGoalReplanStatus::Rejected;
    automation_repo
        .update_authoring_state_if_unchanged(
            &automation_id,
            current.updated_at,
            Some(serde_json::to_string(&state).unwrap()),
        )
        .await
        .unwrap();

    assert_eq!(
        service
            .apply_pending_goal_replan_for_run(&automation_id, &successor)
            .await
            .unwrap(),
        PendingGoalReplanApplyOutcome::None
    );
    let stored = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        stored.goal_items_json.as_deref(),
        Some(proposed_json.as_str())
    );
}

#[tokio::test]
async fn judge_goal_replan_requires_stored_goal_items_before_successor_creation() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-replan-missing-goals", AutomationStatus::Active);
    active.goal_items_json = None;
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-replan-source",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    let mut verdict = continue_verdict_struct(
        "Plan and implement the next phase with focused tests and a scoped pull request.",
        AutomationJudgeNextBaseBranch::AutomationBase,
    );
    verdict.goal_items_proposal = Some(vec![AutomationJudgeGoalItemProposal {
        id: "phase-2".to_string(),
        title: "Follow-up".to_string(),
        status: AutomationGoalItemStatus::Pending,
    }]);

    let error = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: active.clone(),
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("goalItemsProposal requires stored goal items"))
    );
    assert!(run_repo
        .latest_for_automation(&active.id)
        .await
        .unwrap()
        .is_some_and(|run| run.id.as_str() == "run-replan-source"));
}

#[tokio::test]
async fn judge_goal_replan_rejects_second_pending_source_run() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-replan-conflict", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();
    let previous = automation_run(
        "run-replan-source",
        &active.id,
        1,
        AutomationRunStatus::Merged,
        AutomationJudgeState::Done,
    );
    run_repo.create_run(previous.clone()).await.unwrap();
    let mut state = parse_authoring_state(active.authoring_state_json.as_deref()).unwrap();
    state.pending_goal_replan = Some(AutomationGoalReplanState {
        source_run_id: "older-source-run".to_string(),
        base_goal_items_json: active.goal_items_json.clone().unwrap(),
        proposed_goal_items_json: json!([
            { "id": "older-phase", "title": "Older phase", "status": "pending" }
        ])
        .to_string(),
        reason: "Older proposal remains pending.".to_string(),
        status: AutomationGoalReplanStatus::Pending,
        created_at: Utc::now().to_rfc3339(),
        applied_at: None,
    });
    automation_repo
        .update_authoring_state_if_unchanged(
            &active.id,
            active.updated_at,
            Some(serde_json::to_string(&state).unwrap()),
        )
        .await
        .unwrap();
    let refreshed = automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .unwrap();
    let mut verdict = continue_verdict_struct(
        "Plan and implement the next phase with focused tests and a scoped pull request.",
        AutomationJudgeNextBaseBranch::AutomationBase,
    );
    verdict.goal_items_proposal = Some(vec![AutomationJudgeGoalItemProposal {
        id: "phase-2".to_string(),
        title: "Follow-up".to_string(),
        status: AutomationGoalItemStatus::Pending,
    }]);

    let error = service
        .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
            automation: refreshed,
            previous_run: previous,
            verdict,
        })
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Conflict(message) if message.contains("pending goal re-plan"))
    );
    assert!(run_repo
        .latest_for_automation(&active.id)
        .await
        .unwrap()
        .is_some_and(|run| run.id.as_str() == "run-replan-source"));
}

#[tokio::test]
async fn paused_human_goal_edit_rejects_pending_goal_replan() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let (automation_id, _successor, proposed_json) =
        seed_pending_goal_replan(&service, &automation_repo, &run_repo).await;
    let active = automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .unwrap();
    automation_repo
        .compare_and_swap_status(
            &automation_id,
            AutomationStatus::Active,
            AutomationStatus::Paused,
            Some("user_paused".to_string()),
            None,
        )
        .await
        .unwrap();
    let human_goal_items = json!([
        { "id": "human-phase", "title": "Human replacement", "status": "pending" }
    ])
    .to_string();
    let mut input = empty_config_input(automation_id.clone());
    input.goal_items_json = Some(human_goal_items.clone());

    let updated = service.update_config(input).await.unwrap();

    assert_eq!(
        updated.goal_items_json.as_deref(),
        Some(human_goal_items.as_str())
    );
    assert_ne!(
        updated.goal_items_json.as_deref(),
        Some(proposed_json.as_str())
    );
    assert_eq!(
        parse_authoring_state(updated.authoring_state_json.as_deref())
            .unwrap()
            .pending_goal_replan
            .map(|state| state.status),
        Some(AutomationGoalReplanStatus::Rejected)
    );
    assert_eq!(active.status, AutomationStatus::Active);
}

#[tokio::test]
async fn trusted_decomposition_verification_activates_only_after_current_approval() {
    let (service, automation_repo, _run_repo, _artifact_repo) =
        service_with_emitter_and_artifacts(Arc::new(NoopAutomationEventEmitter));
    let draft = service
        .create_draft(CreateAutomationDraftInput {
            id: None,
            project_id: ProjectId::from_string("project-1".to_string()),
            name: Some("Trusted pipeline".to_string()),
            setup_conversation_id: None,
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            authoring_mode: Some(AutomationAuthoringMode::TrustedAutoFinalize),
        })
        .await
        .unwrap();
    service
        .update_config(UpdateAutomationConfigInput {
            id: draft.id.clone(),
            goal_prompt: Some("Ship the complete trusted pipeline.".to_string()),
            first_run_prompt: Some(
                "Implement the first backend phase with focused tests and publish the PR."
                    .to_string(),
            ),
            goal_items_json: Some(
                json!([
                    { "id": "phase-1", "title": "Backend", "status": "pending" },
                    { "id": "phase-2", "title": "Frontend", "status": "pending" }
                ])
                .to_string(),
            ),
            spec_content: Some(
                "# Trusted pipeline\n\nPhase 1 adds the backend. Phase 2 adds the UI.".to_string(),
            ),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap();
    let manual_policy_error = service.finalize(&draft.id).await.unwrap_err();
    assert!(manual_policy_error
        .to_string()
        .contains("automatic edit/PR-merge policy or the verified ideation/task-graph policy"));
    service
        .update_settings(UpdateAutomationSettingsInput {
            id: draft.id.clone(),
            name: None,
            max_runs: None,
            max_consecutive_failures: None,
            plan_approval_mode: Some(AutomationPlanApprovalMode::Automatic),
            pr_merge_mode: Some(AutomationPrMergeMode::Automatic),
            plan_deep_verification: None,
        })
        .await
        .unwrap();

    let premature = service.finalize(&draft.id).await.unwrap_err();
    assert!(premature
        .to_string()
        .contains("requires a current verified decomposition"));

    let verifier = AutomationDecompositionVerifier::new(
        service,
        Arc::new(StaticDecompositionVerifierInvoker {
            raw_output: approved_decomposition_output(),
            mutate_repo: None,
        }),
        Duration::from_secs(30),
    );
    let outcome = verifier.verify_and_finalize(&draft.id).await.unwrap();

    assert_eq!(outcome.automation.status, AutomationStatus::Active);
    let stored = automation_repo.get_by_id(&draft.id).await.unwrap().unwrap();
    let state = parse_authoring_state(stored.authoring_state_json.as_deref()).unwrap();
    assert_eq!(
        state.verification_status,
        AutomationDecompositionVerificationStatus::Verified
    );
}

#[tokio::test]
async fn trusted_decomposition_verification_retries_invalid_output_then_persists_revision() {
    let (service, automation_repo, _run_repo, _artifact_repo) =
        service_with_emitter_and_artifacts(Arc::new(NoopAutomationEventEmitter));
    let draft = service
        .create_draft(CreateAutomationDraftInput {
            id: None,
            project_id: ProjectId::from_string("project-1".to_string()),
            name: Some("Trusted revision".to_string()),
            setup_conversation_id: None,
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            authoring_mode: Some(AutomationAuthoringMode::TrustedAutoFinalize),
        })
        .await
        .unwrap();
    service
        .update_config(UpdateAutomationConfigInput {
            id: draft.id.clone(),
            goal_prompt: Some("Ship the trusted pipeline after decomposition review.".to_string()),
            first_run_prompt: Some(
                "Implement the backend phase with focused tests and publish the PR.".to_string(),
            ),
            goal_items_json: Some(
                json!([
                    { "id": "phase-1", "title": "Backend", "status": "pending" },
                    { "id": "phase-2", "title": "Frontend", "status": "pending" }
                ])
                .to_string(),
            ),
            spec_content: Some(
                "# Trusted revision\n\nBackend and frontend work must remain separate.".to_string(),
            ),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap();
    service
        .update_settings(UpdateAutomationSettingsInput {
            id: draft.id.clone(),
            name: None,
            max_runs: None,
            max_consecutive_failures: None,
            plan_approval_mode: Some(AutomationPlanApprovalMode::Automatic),
            pr_merge_mode: Some(AutomationPrMergeMode::Automatic),
            plan_deep_verification: None,
        })
        .await
        .unwrap();
    let invoker = Arc::new(SequenceDecompositionVerifierInvoker {
        raw_outputs: Mutex::new(VecDeque::from([
            "not-json".to_string(),
            revision_decomposition_output(),
        ])),
        retry_flags: Mutex::new(Vec::new()),
    });
    let verifier =
        AutomationDecompositionVerifier::new(service, invoker.clone(), Duration::from_secs(30));

    let outcome = verifier.verify_and_finalize(&draft.id).await.unwrap();

    assert_eq!(outcome.automation.status, AutomationStatus::Draft);
    assert_eq!(
        *invoker.retry_flags.lock().unwrap(),
        vec![false, true],
        "invalid verifier output should trigger exactly one retry"
    );
    let stored = automation_repo.get_by_id(&draft.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationStatus::Draft);
    let state = parse_authoring_state(stored.authoring_state_json.as_deref()).unwrap();
    assert_eq!(
        state.verification_status,
        AutomationDecompositionVerificationStatus::NeedsRevision
    );
    assert!(state.verified_at.is_none());
    assert!(state.verdict_json.unwrap().contains("phase_boundaries"));
}

#[tokio::test]
async fn trusted_decomposition_verification_rejects_stale_agent_output() {
    let (service, automation_repo, _run_repo, _artifact_repo) =
        service_with_emitter_and_artifacts(Arc::new(NoopAutomationEventEmitter));
    let draft = service
        .create_draft(CreateAutomationDraftInput {
            id: None,
            project_id: ProjectId::from_string("project-1".to_string()),
            name: Some("Stale verifier".to_string()),
            setup_conversation_id: None,
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            authoring_mode: Some(AutomationAuthoringMode::TrustedAutoFinalize),
        })
        .await
        .unwrap();
    service
        .update_config(UpdateAutomationConfigInput {
            id: draft.id.clone(),
            goal_prompt: Some("Ship without accepting stale verification.".to_string()),
            first_run_prompt: Some(
                "Implement phase one with focused tests and publish the PR.".to_string(),
            ),
            goal_items_json: Some(
                json!([{ "id": "phase-1", "title": "Initial", "status": "pending" }]).to_string(),
            ),
            spec_content: Some("# Spec\n\nImplement the initial phase.".to_string()),
            ..empty_config_input(draft.id.clone())
        })
        .await
        .unwrap();
    service
        .update_settings(UpdateAutomationSettingsInput {
            id: draft.id.clone(),
            name: None,
            max_runs: None,
            max_consecutive_failures: None,
            plan_approval_mode: Some(AutomationPlanApprovalMode::Automatic),
            pr_merge_mode: Some(AutomationPrMergeMode::Automatic),
            plan_deep_verification: None,
        })
        .await
        .unwrap();
    let verifier = AutomationDecompositionVerifier::new(
        service,
        Arc::new(StaticDecompositionVerifierInvoker {
            raw_output: approved_decomposition_output(),
            mutate_repo: Some(automation_repo.clone()),
        }),
        Duration::from_secs(30),
    );

    let error = verifier.verify_and_finalize(&draft.id).await.unwrap_err();

    assert!(error
        .to_string()
        .contains("changed while decomposition verification was running"));
    let stored = automation_repo.get_by_id(&draft.id).await.unwrap().unwrap();
    assert_eq!(stored.status, AutomationStatus::Draft);
    assert_ne!(
        parse_authoring_state(stored.authoring_state_json.as_deref())
            .unwrap()
            .verification_status,
        AutomationDecompositionVerificationStatus::Verified
    );
}

#[tokio::test]
async fn service_create_run_stamps_current_goal_item() {
    let (service, automation_repo, run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-goal-stamp", AutomationStatus::Active);
    active.goal_items_json = Some(
        r#"[
            {"id":"item-1","title":"Done phase","status":"done"},
            {"id":"item-2","title":"Current phase","status":"pending"},
            {"id":"item-3","title":"Later phase","status":"pending"}
        ]"#
        .to_string(),
    );
    automation_repo.create(active.clone()).await.unwrap();

    let run = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Advance the current phase".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: "main".to_string(),
            base_from_run_id: None,
        })
        .await
        .unwrap();

    assert_eq!(run.goal_item_id.as_deref(), Some("item-2"));
    let stored = run_repo.get_by_id(&run.id).await.unwrap().unwrap();
    assert_eq!(stored.goal_item_id.as_deref(), Some("item-2"));
}

#[tokio::test]
async fn service_create_run_stamps_none_without_goal_items() {
    let (service, automation_repo, _run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-goal-stamp-phaseless", AutomationStatus::Active);
    active.goal_items_json = None;
    automation_repo.create(active.clone()).await.unwrap();

    let run = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Run without phases".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: "main".to_string(),
            base_from_run_id: None,
        })
        .await
        .unwrap();

    assert_eq!(run.goal_item_id, None);
}

#[tokio::test]
async fn service_create_run_stamps_none_for_invalid_goal_items_json() {
    let (service, automation_repo, _run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let mut active = automation("automation-goal-stamp-invalid", AutomationStatus::Active);
    active.goal_items_json = Some("not-json".to_string());
    automation_repo.create(active.clone()).await.unwrap();

    let run = service
        .create_run(CreateAutomationRunInput {
            automation_id: active.id.clone(),
            run_prompt: "Run with unparseable phases".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: "main".to_string(),
            base_from_run_id: None,
        })
        .await
        .expect("invalid goal items must not block run creation");

    assert_eq!(run.goal_item_id, None);
}
