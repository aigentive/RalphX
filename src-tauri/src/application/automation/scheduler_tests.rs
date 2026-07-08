use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

use super::judge::SPEC_ATTACHMENT_MAX_BYTES;
use super::plan_gate::{AutomationRunResumer, AUTOMATION_PLAN_REMINDER_PROMPT};
use super::provisioning::{
    AutomationRunStartOutcome, AutomationRunStartRequest, AutomationRunStarter,
};
use super::scheduler::{
    load_spec_attachment, AutomationJudgeInvocation, AutomationJudgeInvocationOutput,
    AutomationJudgeInvoker, AutomationScheduler, AutomationSchedulerConfig,
    AutomationSchedulerRegistry, AutomationSignalChecker,
};
use super::transition::NoopAutomationEventEmitter;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunStatus, Artifact,
    ArtifactId, ArtifactType, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow,
    ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, ArtifactRepository,
    AutomationRepository, AutomationRunRepository, IdeationSessionRepository,
};
use crate::domain::services::github_service::PrStatus;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::AutomationsRuntimeConfig;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository, MemoryArtifactRepository,
    MemoryAutomationRepository, MemoryAutomationRunRepository, MemoryChatConversationRepository,
    MemoryIdeationSessionRepository,
};

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
struct RecordingResumer {
    running: Mutex<bool>,
    launches_paused: Mutex<bool>,
    prompts: Mutex<Vec<(ChatConversationId, String)>>,
    fail_next_send: Mutex<bool>,
}

impl RecordingResumer {
    fn set_running(&self, running: bool) {
        *self.running.lock().unwrap() = running;
    }

    fn set_launches_paused(&self, paused: bool) {
        *self.launches_paused.lock().unwrap() = paused;
    }

    fn fail_next_send(&self) {
        *self.fail_next_send.lock().unwrap() = true;
    }

    fn prompts(&self) -> Vec<(ChatConversationId, String)> {
        self.prompts.lock().unwrap().clone()
    }
}

#[async_trait]
impl AutomationRunResumer for RecordingResumer {
    async fn is_agent_running(&self, _conversation_id: &ChatConversationId) -> AppResult<bool> {
        Ok(*self.running.lock().unwrap())
    }

    async fn launches_paused(&self) -> AppResult<bool> {
        Ok(*self.launches_paused.lock().unwrap())
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
    resumer: Arc<dyn AutomationRunResumer>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    AutomationScheduler::new(
        automation_repo,
        run_repo,
        agent_run_repo,
        Arc::new(MemoryChatConversationRepository::new()),
        workspace_repo,
        ideation_session_repo,
        Arc::new(RecordingStarter),
        resumer,
        signal_checker,
        judge_invoker,
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(MemoryArtifactRepository::new()),
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let scheduler = AutomationScheduler::new(
        automation_repo,
        run_repo.clone(),
        Arc::new(MemoryAgentRunRepository::new()),
        conversation_repo,
        workspace_repo,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(RecordingStarter),
        Arc::new(RecordingResumer::default()),
        Arc::new(RecordingSignalChecker::default()),
        Arc::new(RecordingJudgeInvoker::default()),
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(MemoryArtifactRepository::new()),
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
async fn automation_scheduler_provisions_pending_successor_runs() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
async fn automation_scheduler_marks_published_run_closed_from_github_signal() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
async fn automation_scheduler_holds_signals_while_automation_is_paused() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
async fn automation_scheduler_parks_plan_run_before_agent_completed_terminalization() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
async fn automation_scheduler_waits_without_reminder_while_launches_are_paused() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
async fn automation_scheduler_refreshes_plan_baseline_for_parked_revision_without_live_agent() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
}

#[tokio::test]
async fn automation_scheduler_marks_plan_phase_agent_failed_when_agent_run_failed() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
}

#[tokio::test]
async fn automation_scheduler_running_timeout_prefers_agent_phase_started_at() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
async fn automation_scheduler_detaches_judge_without_blocking_other_signal_checks() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
}

#[tokio::test]
async fn automation_scheduler_marks_stale_in_progress_judge_failed() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
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
