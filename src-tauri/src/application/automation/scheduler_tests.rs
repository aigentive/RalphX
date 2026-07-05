use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};

use super::provisioning::{
    AutomationRunStartOutcome, AutomationRunStartRequest, AutomationRunStarter,
};
use super::scheduler::{
    AutomationScheduler, AutomationSchedulerConfig, AutomationSchedulerRegistry,
    AutomationSignalChecker,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Automation, AutomationId,
    AutomationJudgeState, AutomationPromptAuthor, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatConversationId, IdeationAnalysisBaseRefKind,
    ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AutomationRepository, AutomationRunRepository,
};
use crate::domain::services::github_service::PrStatus;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::AutomationsRuntimeConfig;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAutomationRepository,
    MemoryAutomationRunRepository, MemoryChatConversationRepository,
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
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1".to_string()),
        setup_analysis_summary: None,
        created_at: now,
        updated_at: now,
    }
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

fn scheduler_with(
    automation_repo: Arc<MemoryAutomationRepository>,
    run_repo: Arc<MemoryAutomationRunRepository>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    config: AutomationSchedulerConfig,
) -> AutomationScheduler {
    AutomationScheduler::new(
        automation_repo,
        run_repo,
        Arc::new(MemoryChatConversationRepository::new()),
        workspace_repo,
        Arc::new(RecordingStarter),
        signal_checker,
        Arc::new(AutomationSchedulerRegistry::default()),
        config,
    )
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
        conversation_repo,
        workspace_repo,
        Arc::new(RecordingStarter),
        Arc::new(RecordingSignalChecker::default()),
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
    let mut config = AutomationSchedulerConfig::default();
    config.signal_failure_pause_threshold = 2;
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
    let mut config = AutomationSchedulerConfig::default();
    config.max_run_duration = Duration::from_secs(60);
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
