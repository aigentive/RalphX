use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

use crate::application::automation::judge::{
    AutomationJudgeDecision, AutomationJudgeNextBaseBranch, AutomationJudgeVerdict,
};
use crate::application::automation::service::{
    ApplyAutomationJudgeVerdictInput, AutomationRunNowAction, AutomationService,
    CreateAutomationDraftInput, CreateAutomationRunInput, CreateMergedBaseSuccessorRunInput,
    UpdateAutomationConfigInput, UpdateAutomationSettingsInput,
};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, NoopAutomationEventEmitter,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AutomationConfigPatch, AutomationRepository, AutomationRunRepository, AutomationSettingsPatch,
};
use crate::error::AppError;
use crate::infrastructure::memory::{MemoryAutomationRepository, MemoryAutomationRunRepository};

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
    let automation_repo = Arc::new(MemoryAutomationRepository::new());
    let run_repo = Arc::new(MemoryAutomationRunRepository::new());
    let service = AutomationService::new(automation_repo.clone(), run_repo.clone(), event_emitter);
    (service, automation_repo, run_repo)
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
        conversation_id: Some(ChatConversationId::from_string(format!(
            "conversation-{run_index}"
        ))),
        run_prompt: format!("Run {run_index} prompt"),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
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
        judge_verdict_json: None,
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

    async fn compare_and_swap_judge_state(
        &self,
        _id: &AutomationRunId,
        _from: AutomationJudgeState,
        _to: AutomationJudgeState,
        _judge_verdict_json: Option<String>,
        _judge_model_id: Option<String>,
        _judge_lease_expires_at: Option<chrono::DateTime<Utc>>,
        _error_detail: Option<String>,
    ) -> crate::error::AppResult<bool> {
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
async fn service_status_controls_fail_when_compare_and_swap_loses() {
    let emitter = Arc::new(RecordingEmitter::default());

    let pause_repo = Arc::new(LostStatusAutomationRepository::new(
        AutomationStatus::Active,
        AutomationStatus::Stopped,
    ));
    let pause_service = AutomationService::new(
        pause_repo.clone(),
        Arc::new(MemoryAutomationRunRepository::new()),
        emitter.clone(),
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
        Arc::new(MemoryAutomationRunRepository::new()),
        emitter.clone(),
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
        Arc::new(MemoryAutomationRunRepository::new()),
        emitter.clone(),
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
                run_id: run.id.clone()
            },
            AutomationEvent::AutomationRunUpdated { run_id: run.id },
            AutomationEvent::AutomationRunUpdated {
                run_id: second_run.id.clone()
            },
            AutomationEvent::AutomationUpdated {
                automation_id: second.id
            },
            AutomationEvent::AutomationRunUpdated {
                run_id: second_run.id
            },
        ]
    );
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
    assert!(emitter
        .events()
        .contains(&AutomationEvent::AutomationUpdated {
            automation_id: active.id
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
        .contains(&AutomationEvent::AutomationRunUpdated { run_id: run.id }));
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
    );

    let outcome = service.skip_judge(&active.id, &run.id).await.unwrap();

    assert!(!outcome.scheduled);
    assert_eq!(outcome.reason.as_deref(), Some("judge already started"));
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
    let not_ready = service
        .trigger_run_now(&active_with_cancelled.id)
        .await
        .unwrap();
    assert_eq!(not_ready.reason.as_deref(), Some("latest run is not ready"));

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

    let active_unmet = automation("automation-unmet", AutomationStatus::Active);
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

    let active_loop = automation("automation-loop", AutomationStatus::Active);
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
        next_run_prompt: None,
        next_base_branch: None,
    }
}
