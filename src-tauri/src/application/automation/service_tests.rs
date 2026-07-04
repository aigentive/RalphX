use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;

use crate::application::automation::service::{
    AutomationService, CreateAutomationDraftInput, CreateAutomationRunInput,
    UpdateAutomationSettingsInput,
};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, NoopAutomationEventEmitter,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRunStatus,
    AutomationStatus, ProjectId,
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
}

#[tokio::test]
async fn service_creates_lists_gets_and_updates_mechanical_settings() {
    let emitter = Arc::new(RecordingEmitter::default());
    let (service, automation_repo, _run_repo) = service_with_emitter(emitter.clone());
    let project_id = ProjectId::from_string("project-1".to_string());

    let draft = service
        .create_draft(CreateAutomationDraftInput {
            project_id: project_id.clone(),
            name: Some("  Large migration  ".to_string()),
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
