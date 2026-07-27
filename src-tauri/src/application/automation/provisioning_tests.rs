use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use super::provisioning::{
    AutomationRunProvisioner, AutomationRunStartOutcome, AutomationRunStartRequest,
    AutomationRunStarter, AUTOMATION_PLAN_PHASE_CONTRACT_BLOCK,
};
use super::transition::{AutomationEvent, AutomationEventEmitter, NoopAutomationEventEmitter};
use crate::application::{AppState, NotificationService};
use crate::domain::agents::LogicalEffort;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Automation, AutomationId,
    AutomationJudgeState, AutomationPlanApprovalMode, AutomationPlanJudgeState,
    AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun, AutomationRunId,
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
    MemoryAgentConversationWorkspaceRepository, MemoryArtifactRepository,
    MemoryAutomationRepository, MemoryAutomationRunRepository, MemoryChatConversationRepository,
};

fn notification_service() -> Arc<NotificationService> {
    AppState::new_test().notification_service()
}

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
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Build the first PR".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
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
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Build the first PR".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "feature/base".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
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
    invoked_at: Arc<Mutex<Vec<chrono::DateTime<chrono::Utc>>>>,
    workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>,
}

impl RecordingStarter {
    fn new(workspace_repo: Arc<MemoryAgentConversationWorkspaceRepository>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            invoked_at: Arc::new(Mutex::new(Vec::new())),
            workspace_repo,
        }
    }

    fn requests(&self) -> Vec<AutomationRunStartRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn invoked_at(&self) -> Vec<chrono::DateTime<chrono::Utc>> {
        self.invoked_at.lock().unwrap().clone()
    }
}

#[async_trait]
impl AutomationRunStarter for RecordingStarter {
    async fn start_run(
        &self,
        request: AutomationRunStartRequest,
    ) -> AppResult<AutomationRunStartOutcome> {
        self.invoked_at.lock().unwrap().push(chrono::Utc::now());
        self.requests.lock().unwrap().push(request.clone());
        let workspace_mode = request
            .run_mode
            .parse::<AgentConversationWorkspaceMode>()
            .unwrap_or(AgentConversationWorkspaceMode::Edit);
        let workspace = AgentConversationWorkspace::new(
            request.conversation_id.clone(),
            ProjectId::from_string(request.project_id.clone()),
            workspace_mode,
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

struct CapturingWarnLayer {
    captured: Arc<Mutex<Vec<String>>>,
}

impl<S: tracing::Subscriber> Layer<S> for CapturingWarnLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if *event.metadata().level() > tracing::Level::WARN {
            return;
        }

        struct MessageVisitor(String);

        impl tracing::field::Visit for MessageVisitor {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "message" {
                    self.0 = value.to_string();
                }
            }

            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }

        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        if !visitor.0.is_empty() {
            self.captured.lock().unwrap().push(visitor.0);
        }
    }
}

fn goal_item_status(goal_items_json: &str, id: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(goal_items_json).ok()?;
    value.as_array()?.iter().find_map(|item| {
        let item = item.as_object()?;
        (item.get("id").and_then(Value::as_str) == Some(id))
            .then(|| {
                item.get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .flatten()
    })
}

fn automation_updated_events(
    events: &[AutomationEvent],
    automation_id: &AutomationId,
) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            AutomationEvent::AutomationUpdated { automation_id: id } if id == automation_id => {
                Some(index)
            }
            _ => None,
        })
        .collect()
}

fn run_updated_events(events: &[AutomationEvent], run_id: &AutomationRunId) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            AutomationEvent::AutomationRunUpdated { run_id: id, .. } if id == run_id => Some(index),
            _ => None,
        })
        .collect()
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

    assert_eq!(input.project_id.as_deref(), Some("project-1"));
    assert_eq!(
        input.content,
        format!("{AUTOMATION_PLAN_PHASE_CONTRACT_BLOCK}\nBuild the first PR")
    );
    assert_eq!(input.conversation_id.as_deref(), Some(conversation_id));
    assert_eq!(input.provider_harness.as_deref(), Some("codex"));
    assert_eq!(input.model_override.as_deref(), Some("gpt-5.4"));
    assert_eq!(input.logical_effort, Some(LogicalEffort::High));
    assert_eq!(input.mode.as_deref(), Some("plan"));
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
fn automation_run_start_request_injects_spec_reference_and_context_when_spec_linked() {
    let mut automation = automation("automation-1");
    automation.spec_artifact_id = Some("spec-artifact-9".to_string());
    automation.goal_prompt = "Migrate every module".to_string();
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"First","status":"done"},{"id":"item-2","title":"Second","status":"pending"}]"#
            .to_string(),
    );
    let run = run(automation.id.clone());
    let request = AutomationRunStartRequest::from_automation_run(
        &automation,
        &run,
        crate::domain::entities::ChatConversationId::from_string(
            "44444444-4444-4444-8444-444444444444",
        ),
    );

    assert_eq!(request.composer_artifact_references.len(), 1);
    let spec_ref = &request.composer_artifact_references[0];
    assert_eq!(spec_ref.artifact_id, "spec-artifact-9");
    assert_eq!(spec_ref.kind, "spec");
    assert_eq!(spec_ref.session_id, None);
    assert_eq!(spec_ref.version, None);
    // The request forwards the raw run prompt; the context prefix is applied only at
    // spawn time, so both the request and the source run stay clean (D5).
    assert_eq!(request.run_prompt, "Build the first PR");
    assert_eq!(run.run_prompt, "Build the first PR");

    let input = request.into_start_input().unwrap();
    assert_eq!(input.composer_artifact_references.len(), 1);
    assert_eq!(input.composer_artifact_references[0].kind, "spec");
    assert!(input
        .content
        .starts_with(AUTOMATION_PLAN_PHASE_CONTRACT_BLOCK));
    assert!(input.content.contains("<automation_context>"));
    assert!(input.content.contains("Migrate every module"));
    assert!(input.content.contains("Build the first PR"));
}

#[test]
fn automation_run_start_request_has_no_spec_reference_or_context_when_unlinked() {
    let automation = automation("automation-1");
    assert_eq!(automation.spec_artifact_id, None);
    let run = run(automation.id.clone());
    let request = AutomationRunStartRequest::from_automation_run(
        &automation,
        &run,
        crate::domain::entities::ChatConversationId::from_string(
            "55555555-5555-4555-8555-555555555555",
        ),
    );

    assert!(request.composer_artifact_references.is_empty());
    assert_eq!(request.automation_context, None);
    assert_eq!(request.run_prompt, "Build the first PR");

    let input = request.into_start_input().unwrap();
    assert!(input.composer_artifact_references.is_empty());
    assert_eq!(
        input.content,
        format!("{AUTOMATION_PLAN_PHASE_CONTRACT_BLOCK}\nBuild the first PR")
    );
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
fn automation_run_mode_rejects_persona_builder() {
    let automation = automation("automation-persona-builder");
    let run = run(automation.id.clone());
    let mut request = AutomationRunStartRequest::from_automation_run(
        &automation,
        &run,
        crate::domain::entities::ChatConversationId::from_string(
            "66666666-6666-4666-8666-666666666666",
        ),
    );
    request.run_mode = "persona_builder".to_string();

    assert!(matches!(
        request.into_start_input().unwrap_err(),
        AppError::Validation(message) if message.contains("PersonaBuilder")
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
async fn provision_first_run_marks_current_goal_item_in_progress_and_emits_automation_update() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"Finished","status":"done"},{"id":"item-2","title":"Active","status":"pending"},{"id":"item-3","title":"Later","status":"pending"}]"#
            .to_string(),
    );
    automation_repo.create(automation.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    let started = provisioner
        .provision_first_run(&automation)
        .await
        .unwrap()
        .expect("first run should be provisioned");

    assert_eq!(started.status, AutomationRunStatus::Running);
    let stored = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist");
    let goal_items_json = stored
        .goal_items_json
        .as_deref()
        .expect("goal items should remain persisted");
    assert_eq!(
        goal_item_status(goal_items_json, "item-1").as_deref(),
        Some("done")
    );
    assert_eq!(
        goal_item_status(goal_items_json, "item-2").as_deref(),
        Some("in_progress")
    );
    assert_eq!(
        goal_item_status(goal_items_json, "item-3").as_deref(),
        Some("pending")
    );

    let events = event_emitter.events();
    let automation_events = automation_updated_events(&events, &automation.id);
    assert_eq!(automation_events.len(), 1);
    let running_event = run_updated_events(&events, &started.id)
        .into_iter()
        .max()
        .expect("running transition should emit a run update");
    assert!(
        automation_events[0] > running_event,
        "goal-item update event should follow the accepted Running transition: {events:?}"
    );
}

#[tokio::test]
async fn provision_pending_successor_run_marks_current_goal_item_in_progress() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"Finished","status":"skipped"},{"id":"item-2","title":"Successor work","status":"pending"}]"#
            .to_string(),
    );
    automation_repo.create(automation.clone()).await.unwrap();
    let mut pending = run(automation.id.clone());
    pending.id = AutomationRunId::from_string("run-2");
    pending.run_index = 2;
    pending.base_from_run_id = Some(AutomationRunId::from_string("run-1"));
    run_repo.create_run(pending.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    let started = provisioner
        .provision_pending_run(&automation, &pending)
        .await
        .unwrap()
        .expect("successor run should be provisioned");

    assert_eq!(started.status, AutomationRunStatus::Running);
    let stored = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist");
    let goal_items_json = stored.goal_items_json.as_deref().unwrap();
    assert_eq!(
        goal_item_status(goal_items_json, "item-2").as_deref(),
        Some("in_progress")
    );
    assert_eq!(
        automation_updated_events(&event_emitter.events(), &automation.id).len(),
        1
    );
}

#[tokio::test]
async fn provision_pending_run_does_not_rewrite_already_in_progress_goal_item() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"Active","status":"in_progress"},{"id":"item-2","title":"Later","status":"pending"}]"#
            .to_string(),
    );
    automation_repo.create(automation.clone()).await.unwrap();
    let before = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist before provisioning");
    let mut pending = run(automation.id.clone());
    pending.id = AutomationRunId::from_string("run-2");
    pending.run_index = 2;
    run_repo.create_run(pending.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    let started = provisioner
        .provision_pending_run(&automation, &pending)
        .await
        .unwrap()
        .expect("pending run should be provisioned");

    assert_eq!(started.status, AutomationRunStatus::Running);
    let after = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist after provisioning");
    assert_eq!(after.goal_items_json, before.goal_items_json);
    assert_eq!(after.updated_at, before.updated_at);
    assert!(
        automation_updated_events(&event_emitter.events(), &automation.id).is_empty(),
        "already in-progress goal item should not emit an automation update"
    );
}

#[tokio::test]
async fn provision_goal_item_sync_self_reverts_when_run_closed_after_mark() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"Race target","status":"pending"},{"id":"item-2","title":"Later","status":"pending"}]"#
            .to_string(),
    );
    automation_repo.create(automation.clone()).await.unwrap();
    let mut cancelled = run(automation.id.clone());
    cancelled.status = AutomationRunStatus::Cancelled;
    run_repo.create_run(cancelled.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    provisioner
        .sync_current_goal_item_started(&automation.id, &cancelled.id)
        .await;

    let stored = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist");
    let goal_items_json = stored.goal_items_json.as_deref().unwrap();
    assert_eq!(
        goal_item_status(goal_items_json, "item-1").as_deref(),
        Some("pending")
    );
    assert_eq!(
        goal_item_status(goal_items_json, "item-2").as_deref(),
        Some("pending")
    );
    assert_eq!(
        automation_updated_events(&event_emitter.events(), &automation.id).len(),
        2,
        "stale mark should be followed by a self-revert update"
    );
}

#[tokio::test]
async fn provision_goal_item_sync_self_reverts_when_judge_failed_loses_goal_authority() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"Race target","status":"pending"},{"id":"item-2","title":"Later","status":"pending"}]"#
            .to_string(),
    );
    automation_repo.create(automation.clone()).await.unwrap();
    let mut failed = run(automation.id.clone());
    failed.status = AutomationRunStatus::AgentFailed;
    failed.judge_state = AutomationJudgeState::Failed;
    run_repo.create_run(failed.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    provisioner
        .sync_current_goal_item_started(&automation.id, &failed.id)
        .await;

    let stored = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist");
    let goal_items_json = stored.goal_items_json.as_deref().unwrap();
    assert_eq!(
        goal_item_status(goal_items_json, "item-1").as_deref(),
        Some("pending")
    );
    assert_eq!(
        automation_updated_events(&event_emitter.events(), &automation.id).len(),
        2,
        "judge-failed terminal run should be followed by a self-revert update"
    );
}

#[tokio::test]
async fn provision_goal_item_sync_keeps_completed_judge_settled_goal_authority() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"Race target","status":"pending"},{"id":"item-2","title":"Later","status":"pending"}]"#
            .to_string(),
    );
    automation_repo.create(automation.clone()).await.unwrap();
    let mut completed = run(automation.id.clone());
    completed.status = AutomationRunStatus::Completed;
    completed.judge_state = AutomationJudgeState::Done;
    run_repo.create_run(completed.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    provisioner
        .sync_current_goal_item_started(&automation.id, &completed.id)
        .await;

    let stored = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist");
    let goal_items_json = stored.goal_items_json.as_deref().unwrap();
    assert_eq!(
        goal_item_status(goal_items_json, "item-1").as_deref(),
        Some("in_progress")
    );
    assert_eq!(
        automation_updated_events(&event_emitter.events(), &automation.id).len(),
        1,
        "goal-authoritative terminal run should keep the started item update"
    );
}

#[tokio::test]
async fn provision_first_run_does_not_rewrite_when_all_goal_items_are_terminal() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some(
        r#"[{"id":"item-1","title":"Finished","status":"done"},{"id":"item-2","title":"Dropped","status":"skipped"}]"#
            .to_string(),
    );
    automation_repo.create(automation.clone()).await.unwrap();
    let before = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist before provisioning");
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    let started = provisioner
        .provision_first_run(&automation)
        .await
        .unwrap()
        .expect("first run should still be provisioned");

    assert_eq!(started.status, AutomationRunStatus::Running);
    let after = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should exist after provisioning");
    assert_eq!(after.goal_items_json, before.goal_items_json);
    assert_eq!(after.updated_at, before.updated_at);
    assert!(
        automation_updated_events(&event_emitter.events(), &automation.id).is_empty(),
        "terminal goal items should not emit an automation update"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn provision_first_run_skips_malformed_goal_items_json_but_starts_and_warns() {
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let subscriber = tracing_subscriber::registry().with(CapturingWarnLayer {
        captured: Arc::clone(&captured),
    });
    let _guard = subscriber.set_default();

    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let event_emitter = Arc::new(RecordingEmitter::default());
    let mut automation = automation("automation-1");
    automation.goal_items_json = Some("not-json".to_string());
    automation_repo.create(automation.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo.clone(),
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter),
        event_emitter.clone(),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    let started = provisioner
        .provision_first_run(&automation)
        .await
        .unwrap()
        .expect("malformed goal items should not block provisioning");

    assert_eq!(started.status, AutomationRunStatus::Running);
    let stored = automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should remain persisted");
    assert_eq!(stored.goal_items_json.as_deref(), Some("not-json"));
    assert!(
        automation_updated_events(&event_emitter.events(), &automation.id).is_empty(),
        "malformed goal items should not emit an automation update"
    );
    assert!(
        captured
            .lock()
            .unwrap()
            .iter()
            .any(|message| message.contains("Failed to sync automation goal item progress")),
        "expected malformed goal-items warning, got {:?}",
        captured.lock().unwrap()
    );
}

#[tokio::test]
async fn provision_first_run_noops_or_rejects_when_not_ready() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
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
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let mut automation = automation("automation-1");
    automation.base_ref = "ralphx/automation-workspace/automation-setup".to_string();
    automation.base_display_name = Some("Automation branch".to_string());
    automation_repo.create(automation.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo,
        run_repo.clone(),
        conversation_repo.clone(),
        workspace_repo.clone(),
        Arc::new(starter.clone()),
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    let started = provisioner
        .provision_first_run(&automation)
        .await
        .unwrap()
        .expect("first run should be provisioned");

    assert_eq!(started.status, AutomationRunStatus::Running);
    assert_eq!(started.run_index, 1);
    assert_eq!(
        started.base_ref_used,
        "ralphx/automation-workspace/automation-setup"
    );
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
        Some(AgentConversationWorkspaceMode::Plan)
    );
    assert_eq!(conversation.title.as_deref(), Some("Large migration run 1"));

    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .unwrap()
        .expect("fake starter should create workspace");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
    assert!(workspace.auto_publish_initial_pr_enabled);

    let requests = starter.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].conversation_id, *conversation_id);
    assert_eq!(requests[0].run_prompt, "Build the first PR");
    assert_eq!(requests[0].run_mode, "plan");
    assert_eq!(requests[0].provider_harness, "codex");
    assert_eq!(requests[0].model_id, "gpt-5.4");
    assert_eq!(requests[0].base_ref_kind, "local_branch");
    assert_eq!(
        requests[0].base_ref,
        "ralphx/automation-workspace/automation-setup"
    );
    assert_eq!(
        requests[0].base_display_name.as_deref(),
        Some("Automation branch")
    );

    let latest = run_repo
        .latest_for_automation(&automation.id)
        .await
        .unwrap()
        .expect("latest run should exist");
    assert_eq!(latest.id, started.id);
    assert_eq!(latest.status, AutomationRunStatus::Running);
    assert_eq!(latest.run_prompt, "Build the first PR");
}

#[tokio::test]
async fn provision_first_run_phase_basis_never_postdates_agent_spawn() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let starter = RecordingStarter::new(Arc::clone(&workspace_repo));
    let automation = automation("automation-1");
    automation_repo.create(automation.clone()).await.unwrap();
    let provisioner = AutomationRunProvisioner::new(
        automation_repo,
        run_repo,
        conversation_repo,
        workspace_repo,
        Arc::new(starter.clone()),
        Arc::new(NoopAutomationEventEmitter),
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
    );

    let started = provisioner
        .provision_first_run(&automation)
        .await
        .unwrap()
        .expect("first run should be provisioned");

    // The freshness guard requires agent_run.started_at >= agent_phase_started_at.
    // The spawned agent's run row is created inside the starter, so the phase
    // basis MUST be captured before the starter runs; otherwise the first plan
    // turn always reads as stale and the run can never park at the plan gate.
    let spawn_time = starter.invoked_at()[0];
    let phase_basis = started
        .agent_phase_started_at
        .expect("entering Running must stamp agent_phase_started_at");
    assert!(
        phase_basis <= spawn_time,
        "agent_phase_started_at ({phase_basis}) must not postdate the agent spawn ({spawn_time})"
    );
}

#[tokio::test]
async fn provision_pending_run_noops_for_non_pending_and_conflicts_on_stale_status() {
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
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
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
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
    let run_repo = Arc::new(MemoryAutomationRunRepository::new(
        automation_repo.shared_state(),
    ));
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
        Arc::new(MemoryArtifactRepository::new()),
        notification_service(),
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
