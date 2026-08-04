use std::sync::Arc;

use async_trait::async_trait;

use super::reopen_tests::{run, setup_smart_resume_fixture, RecordingRedriver, ReopenFixture};
use super::resume_orchestrator::resume_automation_smart_with_redriver;
use crate::application::app_state::ApplicationExecutionState;
use crate::application::AppState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationRunStatus, AutomationStatus,
    ProjectId,
};
use crate::domain::repositories::{
    AutomationConfigPatch, AutomationRepository, AutomationSettingsPatch,
};
use crate::error::{AppError, AppResult};

#[derive(Clone, Copy, Debug)]
enum ResumeLoadErrorCase {
    Database,
    TaskNotFound,
    ProjectNotFound,
    InvalidTransition,
    Validation,
    FeatureDisabled,
    PersonaUnavailable,
    Agent,
    StaleSession,
    NotFound,
    Infrastructure,
    GitOperation,
    GitAuth,
    ExecutionBlocked,
    BranchFreshnessConflict,
    ReviewWorktreeMissing,
    ReviewWorktreeConflictMarkers,
    WorkspaceReviewUnfinishedGitOperation,
    DuplicatePr,
    ImportVersionUnsupported,
    ImportInvalidFormat,
    ImportInvalidDependency,
    Conflict,
    PersonaDraftConflict,
    PersonaAlreadyApproved,
    ConversationFolderReferenceLimit,
    ConversationFolderReferenceDuplicate,
    ConversationFolderReferenceUnsupportedContext,
    ConversationFolderReferenceAppDataUnavailable,
    SessionNamerStandaloneWorkspaceUnavailable,
    SeededAgentConversationAlreadyStarted,
    StandaloneWorkspaceMissing,
    PersonaBuilderTextAttachmentOnly,
}

impl ResumeLoadErrorCase {
    fn error(self) -> AppError {
        match self {
            Self::Database => AppError::Database("source failure".to_string()),
            Self::TaskNotFound => AppError::TaskNotFound("source failure".to_string()),
            Self::ProjectNotFound => AppError::ProjectNotFound("source failure".to_string()),
            Self::InvalidTransition => AppError::InvalidTransition {
                from: "paused".to_string(),
                to: "active".to_string(),
            },
            Self::Validation => AppError::Validation("source failure".to_string()),
            Self::FeatureDisabled => AppError::FeatureDisabled("source failure".to_string()),
            Self::PersonaUnavailable => AppError::PersonaUnavailable("source failure".to_string()),
            Self::Agent => AppError::Agent("source failure".to_string()),
            Self::StaleSession => AppError::StaleSession {
                session_id: "session-1".to_string(),
                conversation_id: "conversation-1".to_string(),
            },
            Self::NotFound => AppError::NotFound("source failure".to_string()),
            Self::Infrastructure => AppError::Infrastructure("source failure".to_string()),
            Self::GitOperation => AppError::GitOperation("source failure".to_string()),
            Self::GitAuth => AppError::GitAuth("source failure".to_string()),
            Self::ExecutionBlocked => AppError::ExecutionBlocked("source failure".to_string()),
            Self::BranchFreshnessConflict => AppError::BranchFreshnessConflict,
            Self::ReviewWorktreeMissing => AppError::ReviewWorktreeMissing,
            Self::ReviewWorktreeConflictMarkers => AppError::ReviewWorktreeConflictMarkers,
            Self::WorkspaceReviewUnfinishedGitOperation => {
                AppError::WorkspaceReviewUnfinishedGitOperation
            }
            Self::DuplicatePr => AppError::DuplicatePr,
            Self::ImportVersionUnsupported => AppError::ImportVersionUnsupported { version: 99 },
            Self::ImportInvalidFormat => AppError::ImportInvalidFormat {
                detail: "source failure".to_string(),
            },
            Self::ImportInvalidDependency => AppError::ImportInvalidDependency {
                detail: "source failure".to_string(),
            },
            Self::Conflict => AppError::Conflict("source failure".to_string()),
            Self::PersonaDraftConflict => AppError::PersonaDraftConflict {
                expected: "expected".to_string(),
                actual: "actual".to_string(),
            },
            Self::PersonaAlreadyApproved => AppError::PersonaAlreadyApproved,
            Self::ConversationFolderReferenceLimit => AppError::ConversationFolderReferenceLimit {
                conversation_id: "conversation-1".to_string(),
                limit: 5,
            },
            Self::ConversationFolderReferenceDuplicate => {
                AppError::ConversationFolderReferenceDuplicate {
                    conversation_id: "conversation-1".to_string(),
                    folder_path: "folder".to_string(),
                }
            }
            Self::ConversationFolderReferenceUnsupportedContext => {
                AppError::ConversationFolderReferenceUnsupportedContext
            }
            Self::ConversationFolderReferenceAppDataUnavailable => {
                AppError::ConversationFolderReferenceAppDataUnavailable {
                    detail: "source failure".to_string(),
                }
            }
            Self::SessionNamerStandaloneWorkspaceUnavailable => {
                AppError::SessionNamerStandaloneWorkspaceUnavailable {
                    conversation_id: "conversation-1".to_string(),
                    detail: "source failure".to_string(),
                }
            }
            Self::SeededAgentConversationAlreadyStarted => {
                AppError::SeededAgentConversationAlreadyStarted {
                    conversation_id: "conversation-1".to_string(),
                }
            }
            Self::StandaloneWorkspaceMissing => AppError::StandaloneWorkspaceMissing {
                conversation_id: "conversation-1".to_string(),
            },
            Self::PersonaBuilderTextAttachmentOnly => AppError::PersonaBuilderTextAttachmentOnly,
        }
    }

    fn expected_kind(self) -> ResumeErrorKind {
        match self {
            Self::Database => ResumeErrorKind::Database,
            Self::Validation => ResumeErrorKind::Validation,
            Self::NotFound => ResumeErrorKind::NotFound,
            Self::Infrastructure => ResumeErrorKind::Infrastructure,
            Self::Conflict | Self::InvalidTransition => ResumeErrorKind::Conflict,
            Self::Agent => ResumeErrorKind::Agent,
            Self::ExecutionBlocked => ResumeErrorKind::ExecutionBlocked,
            Self::GitOperation => ResumeErrorKind::GitOperation,
            Self::GitAuth => ResumeErrorKind::GitAuth,
            _ => ResumeErrorKind::Infrastructure,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ResumeErrorKind {
    Database,
    Validation,
    NotFound,
    Infrastructure,
    Conflict,
    Agent,
    ExecutionBlocked,
    GitOperation,
    GitAuth,
}

fn resume_error_kind(error: &AppError) -> ResumeErrorKind {
    match error {
        AppError::Database(_) => ResumeErrorKind::Database,
        AppError::Validation(_) => ResumeErrorKind::Validation,
        AppError::NotFound(_) => ResumeErrorKind::NotFound,
        AppError::Infrastructure(_) => ResumeErrorKind::Infrastructure,
        AppError::Conflict(_) => ResumeErrorKind::Conflict,
        AppError::Agent(_) => ResumeErrorKind::Agent,
        AppError::ExecutionBlocked(_) => ResumeErrorKind::ExecutionBlocked,
        AppError::GitOperation(_) => ResumeErrorKind::GitOperation,
        AppError::GitAuth(_) => ResumeErrorKind::GitAuth,
        other => panic!("unexpected contextualized error: {other:?}"),
    }
}

struct FailingResumeLoadAutomationRepository {
    case: ResumeLoadErrorCase,
}

#[async_trait]
impl AutomationRepository for FailingResumeLoadAutomationRepository {
    async fn create(&self, _automation: Automation) -> AppResult<Automation> {
        Err(self.case.error())
    }

    async fn get_by_id(&self, _id: &AutomationId) -> AppResult<Option<Automation>> {
        Err(self.case.error())
    }

    async fn list(&self, _project_id: Option<ProjectId>) -> AppResult<Vec<Automation>> {
        Err(self.case.error())
    }

    async fn list_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<Automation>> {
        Err(self.case.error())
    }

    async fn update_settings(
        &self,
        _id: &AutomationId,
        _patch: AutomationSettingsPatch,
    ) -> AppResult<Option<Automation>> {
        Err(self.case.error())
    }

    async fn update_config(
        &self,
        _id: &AutomationId,
        _patch: AutomationConfigPatch,
    ) -> AppResult<Option<Automation>> {
        Err(self.case.error())
    }

    async fn update_goal_items_json(
        &self,
        _id: &AutomationId,
        _goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>> {
        Err(self.case.error())
    }

    async fn update_goal_items_json_if_unchanged(
        &self,
        _id: &AutomationId,
        _expected_goal_items_json: Option<String>,
        _goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>> {
        Err(self.case.error())
    }

    async fn update_authoring_state_if_unchanged(
        &self,
        _id: &AutomationId,
        _expected_updated_at: chrono::DateTime<chrono::Utc>,
        _authoring_state_json: Option<String>,
    ) -> AppResult<bool> {
        Err(self.case.error())
    }

    async fn compare_and_swap_status(
        &self,
        _id: &AutomationId,
        _from: AutomationStatus,
        _to: AutomationStatus,
        _paused_reason_code: Option<String>,
        _paused_reason_detail: Option<String>,
    ) -> AppResult<bool> {
        Err(self.case.error())
    }

    async fn delete_terminal(&self, _id: &AutomationId) -> AppResult<bool> {
        Err(self.case.error())
    }

    async fn delete_attachments_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> AppResult<usize> {
        Err(self.case.error())
    }

    async fn delete_context_refs_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> AppResult<usize> {
        Err(self.case.error())
    }
}

async fn seed_prior_failures(fixture: &ReopenFixture, count: i64) {
    for run_index in 1..=count {
        let prior = run(
            &format!("run-prior-{run_index}"),
            &fixture.automation.id,
            run_index,
            AutomationRunStatus::AgentFailed,
            None,
        );
        fixture
            .state
            .automation_run_repo
            .create_run(prior)
            .await
            .expect("prior failed run");
    }
}

#[tokio::test]
async fn resume_automation_smart_reopens_latest_failed_run_despite_exhausted_retry_limit() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Paused,
        Some("judge_stopped_unmet"),
        true,
        3,
    )
    .await;
    seed_prior_failures(&fixture, 2).await;
    let redriver = RecordingRedriver::default();

    let resumed = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.execution_state,
        &fixture.automation.id,
        &redriver,
    )
    .await
    .expect("reopen should bypass retry-only consecutive failure guard");

    assert_eq!(resumed.status, AutomationStatus::Active);
    let reopened = fixture
        .state
        .automation_run_repo
        .get_by_id(&fixture.run.id)
        .await
        .expect("run read")
        .expect("latest run");
    assert_eq!(reopened.status, AutomationRunStatus::Running);
    assert_eq!(reopened.judge_state, AutomationJudgeState::None);
    assert!(reopened.judge_verdict_json.is_none());
    assert_eq!(
        redriver.redrives(),
        vec![(
            fixture.conversation_id,
            super::reopen::AUTOMATION_RUN_CONTINUATION_PROMPT.to_string(),
        )]
    );
}

#[tokio::test]
async fn resume_automation_smart_spawns_retry_when_latest_failure_has_no_conversation() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Paused,
        Some("judge_stopped_unmet"),
        false,
        1,
    )
    .await;
    let resumed = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.execution_state,
        &fixture.automation.id,
        &RecordingRedriver::default(),
    )
    .await
    .expect("non-reopenable latest run should use retry fallback");

    assert_eq!(resumed.status, AutomationStatus::Active);
    let runs = fixture
        .state
        .automation_run_repo
        .list_for_automation(&fixture.automation.id)
        .await
        .expect("automation runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].status, AutomationRunStatus::Pending);
    assert_eq!(runs[1].base_from_run_id, Some(fixture.run.id));
}

#[tokio::test]
async fn resume_automation_smart_returns_clear_error_when_retry_limit_is_exhausted() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Paused,
        Some("judge_stopped_unmet"),
        false,
        3,
    )
    .await;
    seed_prior_failures(&fixture, 2).await;
    let error = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.execution_state,
        &fixture.automation.id,
        &RecordingRedriver::default(),
    )
    .await
    .expect_err("exhausted retry limit should fail clearly");
    let message = error.to_string();

    assert!(!message.trim().is_empty());
    assert!(message.contains("3 consecutive runs failed"));
    assert!(message.contains("limit 3"));
    assert_eq!(
        fixture
            .state
            .automation_repo
            .get_by_id(&fixture.automation.id)
            .await
            .expect("automation read")
            .expect("automation")
            .status,
        AutomationStatus::Paused
    );
}

#[tokio::test]
async fn resume_automation_smart_rejects_already_active_automation() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::Completed,
        AutomationStatus::Active,
        None,
        false,
        1,
    )
    .await;

    let error = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.execution_state,
        &fixture.automation.id,
        &RecordingRedriver::default(),
    )
    .await
    .expect_err("active automation has nothing to resume");

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "automation is already active")
    );
}

#[tokio::test]
async fn resume_automation_smart_does_not_reopen_failed_run_for_completed_automation() {
    let fixture = setup_smart_resume_fixture(
        AutomationRunStatus::AgentFailed,
        AutomationStatus::Completed,
        None,
        true,
        1,
    )
    .await;
    let redriver = RecordingRedriver::default();

    let error = resume_automation_smart_with_redriver(
        &fixture.state,
        &fixture.execution_state,
        &fixture.automation.id,
        &redriver,
    )
    .await
    .expect_err("completed automation must remain terminal");

    assert!(
        matches!(error, AppError::Conflict(ref detail) if detail == "automation is completed and has no work to resume")
    );
    assert_eq!(
        fixture
            .state
            .automation_run_repo
            .get_by_id(&fixture.run.id)
            .await
            .expect("run read")
            .expect("latest run")
            .status,
        AutomationRunStatus::AgentFailed
    );
    assert!(redriver.redrives().is_empty());
}

#[tokio::test]
async fn resume_automation_smart_rejects_each_non_resumable_state_without_side_effects() {
    for (status, paused_reason, expected_detail) in [
        (
            AutomationStatus::Draft,
            None,
            "automation is still a draft and cannot be resumed",
        ),
        (
            AutomationStatus::Stopped,
            None,
            "automation is stopped and has no failed run that can be reopened; restart it to create a new run",
        ),
        (
            AutomationStatus::Paused,
            Some("human_approval_required"),
            "automation is paused for a reason that cannot be resumed automatically",
        ),
    ] {
        let fixture = setup_smart_resume_fixture(
            AutomationRunStatus::Completed,
            status,
            paused_reason,
            false,
            1,
        )
        .await;
        let redriver = RecordingRedriver::default();

        let error = resume_automation_smart_with_redriver(
            &fixture.state,
            &fixture.execution_state,
            &fixture.automation.id,
            &redriver,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, AppError::Conflict(ref detail) if detail == expected_detail));
        assert_eq!(
            fixture
                .state
                .automation_repo
                .get_by_id(&fixture.automation.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            status
        );
        assert_eq!(
            fixture
                .state
                .automation_run_repo
                .list_for_automation(&fixture.automation.id)
                .await
                .unwrap(),
            vec![fixture.run.clone()]
        );
        assert!(redriver.redrives().is_empty());
    }
}

#[tokio::test]
async fn resume_automation_smart_reports_missing_automation_without_creating_state() {
    let state = AppState::new_test();
    let automation_id = AutomationId::from_string("automation-missing");

    let error = resume_automation_smart_with_redriver(
        &state,
        &Arc::new(ApplicationExecutionState::new()),
        &automation_id,
        &RecordingRedriver::default(),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, AppError::NotFound(ref detail) if detail == "cannot resume automation automation-missing: automation was not found")
    );
    assert!(state
        .automation_run_repo
        .list_for_automation(&automation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn resume_automation_smart_contextualizes_every_app_error_variant_from_load() {
    let cases = [
        ResumeLoadErrorCase::Database,
        ResumeLoadErrorCase::TaskNotFound,
        ResumeLoadErrorCase::ProjectNotFound,
        ResumeLoadErrorCase::InvalidTransition,
        ResumeLoadErrorCase::Validation,
        ResumeLoadErrorCase::FeatureDisabled,
        ResumeLoadErrorCase::PersonaUnavailable,
        ResumeLoadErrorCase::Agent,
        ResumeLoadErrorCase::StaleSession,
        ResumeLoadErrorCase::NotFound,
        ResumeLoadErrorCase::Infrastructure,
        ResumeLoadErrorCase::GitOperation,
        ResumeLoadErrorCase::GitAuth,
        ResumeLoadErrorCase::ExecutionBlocked,
        ResumeLoadErrorCase::BranchFreshnessConflict,
        ResumeLoadErrorCase::ReviewWorktreeMissing,
        ResumeLoadErrorCase::ReviewWorktreeConflictMarkers,
        ResumeLoadErrorCase::WorkspaceReviewUnfinishedGitOperation,
        ResumeLoadErrorCase::DuplicatePr,
        ResumeLoadErrorCase::ImportVersionUnsupported,
        ResumeLoadErrorCase::ImportInvalidFormat,
        ResumeLoadErrorCase::ImportInvalidDependency,
        ResumeLoadErrorCase::Conflict,
        ResumeLoadErrorCase::PersonaDraftConflict,
        ResumeLoadErrorCase::PersonaAlreadyApproved,
        ResumeLoadErrorCase::ConversationFolderReferenceLimit,
        ResumeLoadErrorCase::ConversationFolderReferenceDuplicate,
        ResumeLoadErrorCase::ConversationFolderReferenceUnsupportedContext,
        ResumeLoadErrorCase::ConversationFolderReferenceAppDataUnavailable,
        ResumeLoadErrorCase::SessionNamerStandaloneWorkspaceUnavailable,
        ResumeLoadErrorCase::SeededAgentConversationAlreadyStarted,
        ResumeLoadErrorCase::StandaloneWorkspaceMissing,
        ResumeLoadErrorCase::PersonaBuilderTextAttachmentOnly,
    ];

    for case in cases {
        let mut state = AppState::new_test();
        state.automation_repo = Arc::new(FailingResumeLoadAutomationRepository { case });

        let error = resume_automation_smart_with_redriver(
            &state,
            &Arc::new(ApplicationExecutionState::new()),
            &AutomationId::from_string("automation-load-error"),
            &RecordingRedriver::default(),
        )
        .await
        .unwrap_err();

        assert_eq!(
            resume_error_kind(&error),
            case.expected_kind(),
            "wrong contextualized variant for {case:?}: {error:?}"
        );
        assert!(
            error.to_string().contains("Cannot resume automation:"),
            "missing resume context for {case:?}: {error}"
        );
    }
}
