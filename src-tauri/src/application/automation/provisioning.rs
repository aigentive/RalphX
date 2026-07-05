use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use tauri::Runtime;

use crate::application::agent_conversation_start_service::{
    AgentConversationStartDeps, AgentConversationStartService,
    AgentWorkspaceSourcePullRequestInput, StartAgentConversationInput,
};
use crate::application::automation::service::{AutomationService, CreateAutomationRunInput};
use crate::application::automation::transition::{
    AutomationEventEmitter, AutomationTransitionService,
};
use crate::application::{AppState, TeamService};
use crate::commands::ExecutionState;
use crate::domain::agents::LogicalEffort;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, Automation, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, ChatConversation, ChatConversationId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AutomationRepository, AutomationRunRepository,
    ChatConversationRepository,
};
use crate::domain::services::{
    ComposerArtifactReference, ComposerIntegrationReference, ComposerProjectReference,
};
use crate::error::{AppError, AppResult};

const AUTOMATION_RUN_BASE_BRANCH_MODE: &str = "isolated";
const AUTOMATION_START_FAILED_CODE: &str = "start_failed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationRunStartRequest {
    pub project_id: String,
    pub conversation_id: ChatConversationId,
    pub run_prompt: String,
    pub provider_harness: String,
    pub model_id: String,
    pub logical_effort: Option<String>,
    pub run_mode: String,
    pub base_ref_kind: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_source_pull_request_json: Option<String>,
    pub composer_project_references: Vec<ComposerProjectReference>,
    pub composer_integration_references: Vec<ComposerIntegrationReference>,
    pub composer_artifact_references: Vec<ComposerArtifactReference>,
}

impl AutomationRunStartRequest {
    pub fn from_automation_run(
        automation: &Automation,
        run: &AutomationRun,
        conversation_id: ChatConversationId,
    ) -> Self {
        Self {
            project_id: automation.project_id.as_str().to_string(),
            conversation_id,
            run_prompt: run.run_prompt.clone(),
            provider_harness: automation.provider_harness.clone(),
            model_id: automation.model_id.clone(),
            logical_effort: automation.logical_effort.clone(),
            run_mode: automation.run_mode.clone(),
            base_ref_kind: run.base_ref_kind.clone(),
            base_ref: run.base_ref_used.clone(),
            base_display_name: automation.base_display_name.clone(),
            base_source_pull_request_json: automation.base_source_pull_request_json.clone(),
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
        }
    }

    pub fn into_start_input(self) -> AppResult<StartAgentConversationInput> {
        Ok(StartAgentConversationInput {
            project_id: self.project_id,
            content: self.run_prompt,
            conversation_id: Some(self.conversation_id.as_str().to_string()),
            parent_conversation_id: None,
            title: None,
            provider_harness: trim_optional_string(self.provider_harness),
            model_override: trim_optional_string(self.model_id),
            logical_effort: parse_logical_effort(self.logical_effort)?,
            codex_fast_mode: None,
            mode: trim_optional_string(self.run_mode),
            base_ref_kind: trim_optional_string(self.base_ref_kind),
            base_branch_mode: Some(AUTOMATION_RUN_BASE_BRANCH_MODE.to_string()),
            base_ref: trim_optional_string(self.base_ref),
            base_display_name: self
                .base_display_name
                .and_then(|value| trim_optional_string(value)),
            base_source_pull_request: parse_source_pull_request(
                self.base_source_pull_request_json,
            )?,
            composer_project_references: self.composer_project_references,
            composer_integration_references: self.composer_integration_references,
            composer_artifact_references: self.composer_artifact_references,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationRunStartOutcome {
    pub branch_name: Option<String>,
}

#[async_trait]
pub trait AutomationRunStarter: Send + Sync {
    async fn start_run(
        &self,
        request: AutomationRunStartRequest,
    ) -> AppResult<AutomationRunStartOutcome>;
}

pub struct AgentConversationAutomationRunStarter<R: Runtime + 'static> {
    state: AppState,
    execution_state: Arc<ExecutionState>,
    team_service: Option<Arc<TeamService>>,
    app_handle: tauri::AppHandle<R>,
}

impl<R: Runtime + 'static> AgentConversationAutomationRunStarter<R> {
    pub fn new(
        state: AppState,
        execution_state: Arc<ExecutionState>,
        team_service: Option<Arc<TeamService>>,
        app_handle: tauri::AppHandle<R>,
    ) -> Self {
        Self {
            state,
            execution_state,
            team_service,
            app_handle,
        }
    }
}

#[async_trait]
impl<R: Runtime + 'static> AutomationRunStarter for AgentConversationAutomationRunStarter<R> {
    async fn start_run(
        &self,
        request: AutomationRunStartRequest,
    ) -> AppResult<AutomationRunStartOutcome> {
        let start_input = request.into_start_input()?;
        let result = AgentConversationStartService::new(AgentConversationStartDeps {
            state: &self.state,
            execution_state: &self.execution_state,
            team_service: self.team_service.clone(),
            app_handle: self.app_handle.clone(),
        })
        .start(start_input)
        .await
        .map_err(AppError::Agent)?;

        Ok(AutomationRunStartOutcome {
            branch_name: result.workspace.map(|workspace| workspace.branch_name),
        })
    }
}

pub struct AutomationRunProvisioner {
    automation_repo: Arc<dyn AutomationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    service: AutomationService,
    transition_service: AutomationTransitionService,
    starter: Arc<dyn AutomationRunStarter>,
}

impl AutomationRunProvisioner {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        starter: Arc<dyn AutomationRunStarter>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
    ) -> Self {
        let service = AutomationService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            Arc::clone(&event_emitter),
        );
        let transition_service = AutomationTransitionService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            event_emitter,
        );
        Self {
            automation_repo,
            run_repo,
            conversation_repo,
            workspace_repo,
            service,
            transition_service,
            starter,
        }
    }

    pub async fn provision_first_run(
        &self,
        automation: &Automation,
    ) -> AppResult<Option<AutomationRun>> {
        if self
            .run_repo
            .latest_for_automation(&automation.id)
            .await?
            .is_some()
        {
            return Ok(None);
        }

        let run_prompt = automation
            .first_run_prompt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "automation first_run_prompt is required before provisioning".to_string(),
                )
            })?
            .to_string();

        let run = self
            .service
            .create_run(CreateAutomationRunInput {
                automation_id: automation.id.clone(),
                run_prompt,
                prompt_author: AutomationPromptAuthor::SetupAgent,
                base_ref_kind: automation.base_ref_kind.clone(),
                base_ref_used: automation.base_ref.clone(),
                base_from_run_id: None,
            })
            .await?;

        self.transition_run_or_conflict(
            &run.id,
            AutomationRunStatus::Pending,
            AutomationRunStatus::Provisioning,
            None,
            None,
        )
        .await?;

        let result = self.start_provisioning_run(automation, &run).await;
        match result {
            Ok(started_run) => Ok(Some(started_run)),
            Err(error) => {
                self.mark_run_agent_failed(&run.id, error.to_string()).await;
                Err(error)
            }
        }
    }

    async fn start_provisioning_run(
        &self,
        automation: &Automation,
        run: &AutomationRun,
    ) -> AppResult<AutomationRun> {
        self.automation_repo
            .get_by_id(&automation.id)
            .await?
            .ok_or_else(|| automation_not_found(&automation.id))?;
        let conversation = self.create_run_draft_conversation(automation, run).await?;
        let request = AutomationRunStartRequest::from_automation_run(
            automation,
            run,
            conversation.id.clone(),
        );
        let outcome = self.starter.start_run(request).await?;
        self.workspace_repo
            .update_auto_publish_initial_pr_preference(&conversation.id, true)
            .await?;
        self.run_repo
            .update_start_metadata(&run.id, &conversation.id, outcome.branch_name)
            .await?
            .ok_or_else(|| automation_run_not_found(&run.id))?;
        self.transition_run_or_conflict(
            &run.id,
            AutomationRunStatus::Provisioning,
            AutomationRunStatus::Running,
            None,
            None,
        )
        .await?;
        self.run_repo
            .get_by_id(&run.id)
            .await?
            .ok_or_else(|| automation_run_not_found(&run.id))
    }

    async fn create_run_draft_conversation(
        &self,
        automation: &Automation,
        run: &AutomationRun,
    ) -> AppResult<ChatConversation> {
        let mode = AgentConversationWorkspaceMode::from_str(automation.run_mode.trim()).map_err(
            |error| {
                AppError::Validation(format!(
                    "automation run_mode is not valid for run provisioning: {error}"
                ))
            },
        )?;
        let mut conversation = ChatConversation::new_project(automation.project_id.clone());
        conversation.automation_id = Some(automation.id.clone());
        conversation.automation_run_id = Some(run.id.clone());
        conversation.set_agent_mode(Some(mode));
        conversation.set_title(format!("{} run {}", automation.name, run.run_index));
        self.conversation_repo.create(conversation).await
    }

    async fn transition_run_or_conflict(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<()> {
        let changed = self
            .transition_service
            .transition_run_status(id, from, to, error_code, error_detail)
            .await?;
        if !changed {
            return Err(AppError::Conflict(format!(
                "automation run {} status changed before transition {} -> {}",
                id.as_str(),
                from.as_str(),
                to.as_str()
            )));
        }
        Ok(())
    }

    async fn mark_run_agent_failed(&self, id: &AutomationRunId, detail: String) {
        if let Err(error) = self
            .transition_run_or_conflict(
                id,
                AutomationRunStatus::Provisioning,
                AutomationRunStatus::AgentFailed,
                Some(AUTOMATION_START_FAILED_CODE.to_string()),
                Some(detail),
            )
            .await
        {
            tracing::warn!(
                run_id = %id,
                error = %error,
                "Failed to mark automation run as agent_failed after provisioning error"
            );
        }
    }
}

fn trim_optional_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_logical_effort(value: Option<String>) -> AppResult<Option<LogicalEffort>> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(LogicalEffort::from_str)
        .transpose()
        .map_err(AppError::Validation)
}

fn parse_source_pull_request(
    value: Option<String>,
) -> AppResult<Option<AgentWorkspaceSourcePullRequestInput>> {
    let Some(value) = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    serde_json::from_str(value).map(Some).map_err(|error| {
        AppError::Validation(format!(
            "automation base_source_pull_request_json is invalid: {error}"
        ))
    })
}

fn automation_not_found(id: &crate::domain::entities::AutomationId) -> AppError {
    AppError::NotFound(format!("automation {} not found", id.as_str()))
}

fn automation_run_not_found(id: &AutomationRunId) -> AppError {
    AppError::NotFound(format!("automation run {} not found", id.as_str()))
}
