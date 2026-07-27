use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use ralphx_events::RecordingEventSink;

use crate::application::automation::reopen::{
    reopen_automation_run_with_redriver, AutomationRunRedriver, AUTOMATION_RUN_CONTINUATION_PROMPT,
};
use crate::application::automation::transition::{
    AUTOMATION_RUN_UPDATED_EVENT, AUTOMATION_UPDATED_EVENT,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunStatus,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
    ChatContextType, ChatConversation, ChatConversationId, ChatMessage,
    IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::SqliteAutomationRunRepository;

#[derive(Default)]
pub(super) struct RecordingRedriver {
    redrives: Mutex<Vec<(ChatConversationId, String)>>,
}

impl RecordingRedriver {
    pub(super) fn redrives(&self) -> Vec<(ChatConversationId, String)> {
        self.redrives.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl AutomationRunRedriver for RecordingRedriver {
    async fn redrive(
        &self,
        _state: &AppState,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<()> {
        self.redrives
            .lock()
            .unwrap()
            .push((conversation_id.clone(), prompt.to_string()));
        Ok(())
    }
}

struct PanickingRedriver;

#[async_trait::async_trait]
impl AutomationRunRedriver for PanickingRedriver {
    async fn redrive(
        &self,
        _state: &AppState,
        _conversation_id: &ChatConversationId,
        _prompt: &str,
    ) -> AppResult<()> {
        panic!("reject path must not redrive the automation run")
    }
}

pub(super) struct ReopenFixture {
    _temp: tempfile::TempDir,
    pub(super) state: AppState,
    pub(super) automation: Automation,
    pub(super) run: AutomationRun,
    pub(super) conversation_id: ChatConversationId,
    events: Arc<RecordingEventSink>,
}

fn automation(project_id: &ProjectId, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    let (paused_reason_code, paused_reason_detail) = match status {
        AutomationStatus::Paused => (
            Some("workspace_review_blocked".to_string()),
            Some("workspace review must be retried".to_string()),
        ),
        _ => (None, None),
    };
    Automation {
        id: AutomationId::from_string("automation-reopen"),
        project_id: project_id.clone(),
        name: "Resume in place".to_string(),
        status,
        paused_reason_code,
        paused_reason_detail,
        goal_prompt: "Finish the existing implementation".to_string(),
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
        first_run_prompt: Some("Implement the goal".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

pub(super) fn run(
    id: &str,
    automation_id: &AutomationId,
    run_index: i64,
    status: AutomationRunStatus,
    conversation_id: Option<ChatConversationId>,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: automation_id.clone(),
        run_index,
        status,
        judge_state: AutomationJudgeState::Done,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 2,
        plan_reminder_count: 3,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: Some(now - Duration::hours(2)),
        conversation_id,
        run_prompt: "Continue the existing implementation".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: Some("ralphx/automation-reopen".to_string()),
        pr_number: Some(42),
        pr_url: Some("https://example.invalid/pull/42".to_string()),
        pr_title: Some("Stale publication".to_string()),
        pr_head_ref_name: Some("ralphx/automation-reopen".to_string()),
        pr_base_ref_name: Some("main".to_string()),
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: Some("Interrupted after partial implementation".to_string()),
        judge_verdict_json: Some(r#"{"verdict":"done"}"#.to_string()),
        judge_model_id: Some("judge-model".to_string()),
        error_code: Some("agent_failed".to_string()),
        error_detail: Some("agent process exited".to_string()),
        signal_check_failures: 0,
        started_at: Some(now - Duration::hours(3)),
        finished_at: Some(now - Duration::hours(1)),
        created_at: now - Duration::hours(3),
        updated_at: now - Duration::hours(1),
    }
}

pub(super) fn stopped_unmet_verdict() -> String {
    serde_json::json!({
        "decision": "stop",
        "goalMet": false,
        "reason": "The goal remains unmet after infrastructure failures.",
        "confidence": 0.9,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": null,
        "nextBaseBranch": null
    })
    .to_string()
}

async fn setup(status: AutomationRunStatus) -> ReopenFixture {
    setup_with_automation_status(status, AutomationStatus::Paused).await
}

async fn setup_with_automation_status(
    run_status: AutomationRunStatus,
    automation_status: AutomationStatus,
) -> ReopenFixture {
    let paused_reason_code =
        (automation_status == AutomationStatus::Paused).then_some("workspace_review_blocked");
    setup_smart_resume_fixture(run_status, automation_status, paused_reason_code, true, 1).await
}

pub(super) async fn setup_smart_resume_fixture(
    run_status: AutomationRunStatus,
    automation_status: AutomationStatus,
    paused_reason_code: Option<&str>,
    attach_run_conversation: bool,
    run_index: i64,
) -> ReopenFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let worktree = temp.path().join("existing-worktree");
    std::fs::create_dir_all(worktree.join(".git")).expect("existing worktree");
    let project = Project::new(
        "Reopen automation project".to_string(),
        temp.path().to_string_lossy().into_owned(),
    );
    let project_id = project.id.clone();
    let mut state = AppState::new_sqlite_test();
    let events = Arc::new(RecordingEventSink::new());
    state.events = events.clone();
    state.project_repo.create(project).await.expect("project");

    let mut automation = automation(&project_id, automation_status);
    automation.paused_reason_code = paused_reason_code.map(str::to_string);
    state
        .automation_repo
        .create(automation.clone())
        .await
        .expect("automation");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation.automation_id = Some(automation.id.clone());
    let conversation_id = conversation.id;
    let run_id = AutomationRunId::from_string("run-reopen");
    conversation.automation_run_id = Some(run_id.clone());
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("existing conversation");

    let mut prior_message =
        ChatMessage::user_in_project(project_id.clone(), "Prior attempt context");
    prior_message.conversation_id = Some(conversation_id);
    state
        .chat_message_repo
        .create(prior_message)
        .await
        .expect("prior history");
    let mut prior_agent_run = AgentRun::new(conversation_id);
    prior_agent_run.status = AgentRunStatus::Failed;
    prior_agent_run.started_at = Utc::now() - Duration::hours(2);
    prior_agent_run.completed_at = Some(Utc::now() - Duration::hours(1));
    prior_agent_run.error_message = Some("interrupted prior attempt".to_string());
    state
        .agent_run_repo
        .create(prior_agent_run)
        .await
        .expect("prior agent run history");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/automation-reopen".to_string(),
        worktree.to_string_lossy().into_owned(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_url = Some("https://example.invalid/pull/42".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("existing workspace");

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
    monitor.last_error = Some("review interrupted with the agent".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("review monitor");

    let mut run = run(
        run_id.as_str(),
        &automation.id,
        run_index,
        run_status,
        attach_run_conversation.then_some(conversation_id.clone()),
    );
    if paused_reason_code == Some("judge_stopped_unmet") {
        run.judge_verdict_json = Some(stopped_unmet_verdict());
    }
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .expect("failed run");

    ReopenFixture {
        _temp: temp,
        state,
        automation,
        run,
        conversation_id,
        events,
    }
}

async fn assert_not_redriven(fixture: &ReopenFixture) {
    let automation = fixture
        .state
        .automation_repo
        .get_by_id(&fixture.automation.id)
        .await
        .expect("automation read")
        .expect("automation");
    assert_eq!(automation.status, fixture.automation.status);
    assert_eq!(
        automation.paused_reason_code,
        fixture.automation.paused_reason_code
    );
    assert_eq!(
        automation.paused_reason_detail,
        fixture.automation.paused_reason_detail
    );
    let messages = fixture
        .state
        .chat_message_repo
        .get_by_conversation(&fixture.conversation_id)
        .await
        .expect("conversation history");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "Prior attempt context");
    let agent_runs = fixture
        .state
        .agent_run_repo
        .get_by_conversation(&fixture.conversation_id)
        .await
        .expect("agent runs");
    assert_eq!(agent_runs.len(), 1);
    assert_eq!(agent_runs[0].status, AgentRunStatus::Failed);
}

#[tokio::test]
async fn reopen_automation_run_reuses_failed_run_conversation_and_resets_stale_state() {
    let fixture = setup(AutomationRunStatus::AgentFailed).await;
    let redriver = RecordingRedriver::default();
    let before = Utc::now();

    reopen_automation_run_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &fixture.run.id,
        &redriver,
    )
    .await
    .expect("latest failed run should reopen");
    let after = Utc::now();

    let reopened = fixture
        .state
        .automation_run_repo
        .get_by_id(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, AutomationRunStatus::Running);
    assert_eq!(reopened.judge_state, AutomationJudgeState::None);
    assert!(reopened.judge_verdict_json.is_none());
    assert!(reopened.finished_at.is_none());
    assert!(reopened.error_code.is_none());
    assert!(reopened.error_detail.is_none());
    assert_eq!(reopened.plan_reminder_count, 0);
    assert!(reopened.pr_number.is_none());
    assert!(reopened.pr_url.is_none());
    assert!(reopened.pr_title.is_none());
    assert!(reopened.pr_head_ref_name.is_none());
    assert!(reopened.pr_base_ref_name.is_none());
    assert!(reopened
        .agent_phase_started_at
        .is_some_and(|started_at| started_at >= before && started_at <= after));

    let reactivated = fixture
        .state
        .automation_repo
        .get_by_id(&fixture.automation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reactivated.status, AutomationStatus::Active);
    assert!(reactivated.paused_reason_code.is_none());
    assert!(reactivated.paused_reason_detail.is_none());

    let workspace = fixture
        .state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&fixture.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(workspace.publication_pr_number.is_none());
    assert!(workspace.publication_pr_url.is_none());
    assert!(workspace.publication_pr_status.is_none());
    assert!(workspace.publication_push_status.is_none());
    let monitor = fixture
        .state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&fixture.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(monitor.status, AgentWorkspaceReviewMonitorStatus::Idle);
    assert_eq!(monitor.review_outcome, AgentWorkspaceReviewOutcome::None);
    assert_eq!(
        monitor.review_gate_status,
        AgentWorkspaceReviewGateStatus::NotRequired
    );
    assert!(monitor.last_error.is_none());

    assert_eq!(
        redriver.redrives(),
        vec![(
            fixture.conversation_id.clone(),
            AUTOMATION_RUN_CONTINUATION_PROMPT.to_string()
        )]
    );
    let conversations = fixture
        .state
        .chat_conversation_repo
        .list_by_automation_id(&fixture.automation.id)
        .await
        .expect("automation conversations");
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].id, fixture.conversation_id);
    let automation_runs = fixture
        .state
        .automation_run_repo
        .list_for_automation(&fixture.automation.id)
        .await
        .expect("automation runs");
    assert_eq!(automation_runs.len(), 1);
    assert_eq!(automation_runs[0].id, fixture.run.id);

    let event_names: Vec<_> = fixture
        .events
        .events()
        .into_iter()
        .filter_map(|event| {
            matches!(
                event.event.as_str(),
                AUTOMATION_RUN_UPDATED_EVENT | AUTOMATION_UPDATED_EVENT
            )
            .then_some(event.event)
        })
        .collect();
    assert_eq!(
        event_names,
        vec![
            AUTOMATION_RUN_UPDATED_EVENT.to_string(),
            AUTOMATION_UPDATED_EVENT.to_string()
        ]
    );
}

#[tokio::test]
async fn reopen_automation_run_keeps_active_automation_active() {
    let fixture =
        setup_with_automation_status(AutomationRunStatus::AgentFailed, AutomationStatus::Active)
            .await;
    let redriver = RecordingRedriver::default();

    reopen_automation_run_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &fixture.run.id,
        &redriver,
    )
    .await
    .expect("active automation should reopen without a status transition");

    let automation = fixture
        .state
        .automation_repo
        .get_by_id(&fixture.automation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    assert!(automation.paused_reason_code.is_none());
    assert!(automation.paused_reason_detail.is_none());
    assert_eq!(redriver.redrives().len(), 1);
}

#[tokio::test]
async fn reopen_automation_run_reactivates_stopped_automation_and_clears_terminal_fields() {
    let fixture =
        setup_with_automation_status(AutomationRunStatus::AgentFailed, AutomationStatus::Stopped)
            .await;
    let redriver = RecordingRedriver::default();

    reopen_automation_run_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &fixture.run.id,
        &redriver,
    )
    .await
    .expect("stopped automation should reactivate around the reopened run");

    let automation = fixture
        .state
        .automation_repo
        .get_by_id(&fixture.automation.id)
        .await
        .unwrap()
        .unwrap();
    let reopened = fixture
        .state
        .automation_run_repo
        .get_by_id(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(automation.status, AutomationStatus::Active);
    assert_eq!(reopened.status, AutomationRunStatus::Running);
    assert_eq!(reopened.judge_state, AutomationJudgeState::None);
    assert!(reopened.judge_verdict_json.is_none());
    assert!(reopened.finished_at.is_none());
    assert_eq!(redriver.redrives().len(), 1);
}

#[tokio::test]
async fn reopen_automation_run_corrective_transition_loss_preserves_failed_state() {
    let mut fixture = setup(AutomationRunStatus::AgentFailed).await;
    fixture.state.automation_run_repo = Arc::new(SqliteAutomationRunRepository::from_shared(
        Arc::clone(fixture.state.db.inner()),
    ));
    fixture
        .state
        .automation_run_repo
        .create_run(fixture.run.clone())
        .await
        .unwrap();
    fixture
        .state
        .db
        .run(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER reject_reopen_status_update
                 BEFORE UPDATE OF status ON automation_runs
                 WHEN OLD.id = 'run-reopen' AND OLD.status = 'agent_failed' AND NEW.status = 'running'
                 BEGIN
                     SELECT RAISE(IGNORE);
                 END;",
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let error = reopen_automation_run_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &fixture.run.id,
        &PanickingRedriver,
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "failed run changed before it could be resumed")
    );
    let unchanged = fixture
        .state
        .automation_run_repo
        .get_by_id(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.status, AutomationRunStatus::AgentFailed);
    assert_eq!(unchanged.judge_state, AutomationJudgeState::Done);
    assert_eq!(unchanged.judge_verdict_json, fixture.run.judge_verdict_json);
    assert_eq!(unchanged.finished_at, fixture.run.finished_at);
    assert_eq!(
        fixture
            .state
            .automation_repo
            .get_by_id(&fixture.automation.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Paused
    );
    assert!(fixture.events.events().is_empty());
}

#[tokio::test]
async fn reopen_automation_run_rejects_non_latest_without_transition_or_redrive() {
    let fixture = setup(AutomationRunStatus::AgentFailed).await;
    let redriver = PanickingRedriver;
    let latest = run(
        "run-latest",
        &fixture.automation.id,
        2,
        AutomationRunStatus::Completed,
        None,
    );
    fixture
        .state
        .automation_run_repo
        .create_run(latest)
        .await
        .expect("latest run");

    let error = reopen_automation_run_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &fixture.run.id,
        &redriver,
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "only the latest run can be resumed")
    );
    let unchanged = fixture
        .state
        .automation_run_repo
        .get_by_id(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.status, AutomationRunStatus::AgentFailed);
    assert_eq!(unchanged.judge_state, AutomationJudgeState::Done);
    assert_not_redriven(&fixture).await;
}

#[tokio::test]
async fn reopen_automation_run_rejects_non_failed_latest_statuses_without_redrive() {
    for status in [
        AutomationRunStatus::Completed,
        AutomationRunStatus::Running,
        AutomationRunStatus::Published,
    ] {
        let fixture = setup(status).await;
        let redriver = PanickingRedriver;

        let error = reopen_automation_run_with_redriver(
            &fixture.state,
            &fixture.automation.id,
            &fixture.run.id,
            &redriver,
        )
        .await
        .unwrap_err();

        assert!(
            matches!(error, AppError::Conflict(ref detail) if detail == "only a failed run can be resumed")
        );
        assert_eq!(
            fixture
                .state
                .automation_run_repo
                .get_by_id(&fixture.run.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            status
        );
        assert_not_redriven(&fixture).await;
    }
}

#[tokio::test]
async fn reopen_automation_run_rejects_when_existing_conversation_agent_is_running() {
    let fixture = setup(AutomationRunStatus::AgentFailed).await;
    let redriver = PanickingRedriver;
    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        fixture.conversation_id.as_str(),
    );
    fixture
        .state
        .running_agent_registry
        .register(
            running_key,
            0,
            fixture.conversation_id.as_str().to_string(),
            "already-running-agent".to_string(),
            None,
            None,
        )
        .await;

    let error = reopen_automation_run_with_redriver(
        &fixture.state,
        &fixture.automation.id,
        &fixture.run.id,
        &redriver,
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "the run agent is already running")
    );
    let unchanged = fixture
        .state
        .automation_run_repo
        .get_by_id(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.status, AutomationRunStatus::AgentFailed);
    assert_eq!(unchanged.judge_state, AutomationJudgeState::Done);
    assert_not_redriven(&fixture).await;
}
