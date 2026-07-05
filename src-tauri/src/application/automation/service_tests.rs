use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;

use crate::application::automation::service::{
    AutomationService, CreateAutomationDraftInput, CreateAutomationRunInput,
    CreateMergedBaseSuccessorRunInput, UpdateAutomationSettingsInput,
};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, NoopAutomationEventEmitter,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AutomationRepository, AutomationRunRepository, AutomationSettingsPatch,
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
async fn service_run_now_and_skip_judge_surfaces_fail_closed_until_later_phases() {
    let (service, automation_repo, _run_repo) =
        service_with_emitter(Arc::new(NoopAutomationEventEmitter));
    let active = automation("automation-1", AutomationStatus::Active);
    automation_repo.create(active.clone()).await.unwrap();

    let run_now = service.trigger_run_now(&active.id).await.unwrap();
    assert!(!run_now.scheduled);
    assert!(run_now
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("later scheduler phase"));

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
    let skip = service.skip_judge(&active.id, &run.id).await.unwrap();
    assert_eq!(skip.scheduled, false);
    assert_eq!(
        skip.reason.as_deref(),
        Some("run is not ready for judge skipping")
    );
}
