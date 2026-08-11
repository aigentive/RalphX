use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use ralphx_domain::entities::automation::latest_run_holds_goal_authority;

use crate::application::agent_conversation_start_service::{
    AgentWorkspaceSourcePullRequestInput, StartAgentConversationInput,
};
use crate::application::agent_conversation_workspace::reject_persona_builder_workspace_mode;
use crate::application::automation::judge::{
    build_automation_run_context_block, mark_current_goal_item_in_progress,
};
use crate::application::automation::service::{AutomationService, CreateAutomationRunInput};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, AutomationTransitionService,
};
use crate::application::NotificationService;
use crate::domain::agents::LogicalEffort;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, Automation, AutomationId, AutomationPromptAuthor,
    AutomationRun, AutomationRunId, AutomationRunStatus, ChatConversation, ChatConversationId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ArtifactRepository, AutomationRepository,
    AutomationRunRepository, ChatConversationRepository,
};
use crate::domain::services::{
    ComposerArtifactReference, ComposerIntegrationReference, ComposerProjectReference,
};
use crate::error::{AppError, AppResult};

const AUTOMATION_RUN_BASE_BRANCH_MODE: &str = "isolated";
const AUTOMATION_START_FAILED_CODE: &str = "start_failed";
pub(crate) const AUTOMATION_PLAN_PHASE_CONTRACT_BLOCK: &str = r#"<automation_plan_phase>
You are in the automation run planning phase. Explore the codebase, then author the run plan artifact with the scope, files to inspect or change, approach, risks, and how it advances the current goal item. Use the plan tools to create or update the plan artifact, then end the turn. Implementation continues in a later resumed turn. Do not narrate next steps, approval mechanics, or manual actions (approve/verify/implement) at the end of your turn — the system handles continuation.
</automation_plan_phase>"#;

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
    pub automation_context: Option<String>,
}

impl AutomationRunStartRequest {
    pub fn from_automation_run(
        automation: &Automation,
        run: &AutomationRun,
        conversation_id: ChatConversationId,
    ) -> Self {
        let base_display_name = if run.run_index == 1
            || (run.base_ref_kind == automation.base_ref_kind
                && run.base_ref_used == automation.base_ref)
        {
            automation.base_display_name.clone()
        } else {
            trim_optional_string(run.base_ref_used.clone())
        };
        // Spec linkage: when the automation is linked to a Specification artifact,
        // forward it as a lightweight `kind: "spec"` composer reference (fetch-on-demand,
        // never `"plan"` which requires an ideation session) and prepend the spawn-time
        // `<automation_context>` goal/phase block. This single seam covers the first run
        // and every judge-created successor run. Runs without a spec stay unchanged.
        let composer_artifact_references = automation
            .spec_artifact_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|artifact_id| {
                vec![ComposerArtifactReference {
                    artifact_id: artifact_id.to_string(),
                    kind: "spec".to_string(),
                    title: Some("Automation spec".to_string()),
                    session_id: None,
                    version: None,
                    status: None,
                }]
            })
            .unwrap_or_default();
        let automation_context = (!composer_artifact_references.is_empty())
            .then(|| build_automation_run_context_block(automation, run));
        Self {
            project_id: automation.project_id.as_str().to_string(),
            conversation_id,
            run_prompt: run.run_prompt.clone(),
            provider_harness: automation.provider_harness.clone(),
            model_id: automation.model_id.clone(),
            logical_effort: automation.logical_effort.clone(),
            run_mode: AgentConversationWorkspaceMode::Plan.to_string(),
            base_ref_kind: run.base_ref_kind.clone(),
            base_ref: run.base_ref_used.clone(),
            base_display_name,
            base_source_pull_request_json: (run.run_index == 1)
                .then(|| automation.base_source_pull_request_json.clone())
                .flatten(),
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references,
            automation_context,
        }
    }

    pub fn into_start_input(self) -> AppResult<StartAgentConversationInput> {
        reject_persona_builder_workspace_mode(&self.run_mode).map_err(AppError::Validation)?;
        // D5: the `<automation_context>` block is composed at spawn time only. The
        // persisted `run_prompt` on the run stays clean so the judge loop-guard
        // fingerprint (stored prompt vs judge nextRunPrompt) keeps working.
        let content = match &self.automation_context {
            Some(context) if !context.trim().is_empty() => {
                format!(
                    "{AUTOMATION_PLAN_PHASE_CONTRACT_BLOCK}\n{context}\n{}",
                    self.run_prompt
                )
            }
            _ => format!(
                "{AUTOMATION_PLAN_PHASE_CONTRACT_BLOCK}\n{}",
                self.run_prompt
            ),
        };
        Ok(StartAgentConversationInput {
            project_id: Some(self.project_id),
            content,
            persona_id: None,
            source_persona_id: None,
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
            composer_selection_snapshot: None,
            team_intent: None,
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

pub struct AutomationRunProvisioner {
    automation_repo: Arc<dyn AutomationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    service: AutomationService,
    transition_service: AutomationTransitionService,
    event_emitter: Arc<dyn AutomationEventEmitter>,
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
        artifact_repo: Arc<dyn ArtifactRepository>,
        notification_service: Arc<NotificationService>,
    ) -> Self {
        let service = AutomationService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            Arc::clone(&event_emitter),
            artifact_repo,
            notification_service.clone(),
        );
        let transition_service = AutomationTransitionService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            Arc::clone(&event_emitter),
            notification_service,
        );
        Self {
            automation_repo,
            run_repo,
            conversation_repo,
            workspace_repo,
            service,
            transition_service,
            event_emitter,
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

    pub async fn provision_pending_run(
        &self,
        automation: &Automation,
        run: &AutomationRun,
    ) -> AppResult<Option<AutomationRun>> {
        if run.status != AutomationRunStatus::Pending {
            return Ok(None);
        }

        self.transition_run_or_conflict(
            &run.id,
            AutomationRunStatus::Pending,
            AutomationRunStatus::Provisioning,
            None,
            None,
        )
        .await?;

        let result = self.start_provisioning_run(automation, run).await;
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
        // Capture the phase basis BEFORE the agent spawns: the spawned agent
        // run's started_at must never predate agent_phase_started_at, or the
        // current-phase freshness guard treats the first turn as stale and the
        // run can never park at the plan gate.
        let agent_phase_basis = Utc::now();
        let outcome = self.starter.start_run(request).await?;
        self.workspace_repo
            .update_auto_publish_initial_pr_preference(&conversation.id, true)
            .await?;
        self.run_repo
            .update_start_metadata(&run.id, &conversation.id, outcome.branch_name)
            .await?
            .ok_or_else(|| automation_run_not_found(&run.id))?;
        let changed = self
            .transition_service
            .transition_run_status_with_agent_phase_started_at(
                &run.id,
                AutomationRunStatus::Provisioning,
                AutomationRunStatus::Running,
                agent_phase_basis,
                None,
                None,
            )
            .await?;
        if !changed {
            return Err(AppError::Conflict(format!(
                "automation run {} status changed before transition {} -> {}",
                run.id.as_str(),
                AutomationRunStatus::Provisioning.as_str(),
                AutomationRunStatus::Running.as_str()
            )));
        }
        self.sync_current_goal_item_started(&automation.id, &run.id)
            .await;
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
        let mut conversation = ChatConversation::new_project(automation.project_id.clone());
        conversation.automation_id = Some(automation.id.clone());
        conversation.automation_run_id = Some(run.id.clone());
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Plan));
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

    pub(crate) async fn sync_current_goal_item_started(
        &self,
        automation_id: &AutomationId,
        run_id: &AutomationRunId,
    ) {
        let automation = match self.automation_repo.get_by_id(automation_id).await {
            Ok(Some(automation)) => automation,
            Ok(None) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    "Failed to sync automation goal item progress: automation not found"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    error = %error,
                    "Failed to sync automation goal item progress"
                );
                return;
            }
        };

        let updated_goal_items_json =
            match mark_current_goal_item_in_progress(automation.goal_items_json.as_deref()) {
                Ok(Some(updated)) => updated,
                Ok(None) => return,
                Err(error) => {
                    tracing::warn!(
                        automation_id = %automation_id,
                        error = %error,
                        "Failed to sync automation goal item progress"
                    );
                    return;
                }
            };

        match self
            .automation_repo
            .update_goal_items_json_if_unchanged(
                automation_id,
                automation.goal_items_json,
                Some(updated_goal_items_json.clone()),
            )
            .await
        {
            Ok(Some(_)) => {
                self.event_emitter.emit(AutomationEvent::AutomationUpdated {
                    automation_id: automation_id.clone(),
                });
                self.revert_started_goal_item_if_run_closed(
                    automation_id,
                    run_id,
                    updated_goal_items_json,
                )
                .await;
            }
            Ok(None) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    "Skipped automation goal item progress sync because stored goal items changed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    error = %error,
                    "Failed to sync automation goal item progress"
                );
            }
        }
    }

    async fn revert_started_goal_item_if_run_closed(
        &self,
        automation_id: &AutomationId,
        run_id: &AutomationRunId,
        expected_goal_items_json: String,
    ) {
        let should_revert = match self.run_repo.get_by_id(run_id).await {
            Ok(Some(run)) => !latest_run_holds_goal_authority(&run),
            Ok(None) => true,
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    error = %error,
                    "Failed to verify automation run remained open after goal item progress sync"
                );
                true
            }
        };
        if !should_revert {
            return;
        }

        let reverted_goal_items_json =
            match crate::application::automation::judge::revert_in_progress_goal_items_to_pending(
                Some(&expected_goal_items_json),
            ) {
                Ok(Some(updated)) => updated,
                Ok(None) => return,
                Err(error) => {
                    tracing::warn!(
                        automation_id = %automation_id,
                        run_id = %run_id,
                        error = %error,
                        "Failed to revert automation goal item progress after run closed"
                    );
                    return;
                }
            };

        match self
            .automation_repo
            .update_goal_items_json_if_unchanged(
                automation_id,
                Some(expected_goal_items_json),
                Some(reverted_goal_items_json),
            )
            .await
        {
            Ok(Some(_)) => {
                self.event_emitter.emit(AutomationEvent::AutomationUpdated {
                    automation_id: automation_id.clone(),
                });
            }
            Ok(None) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    "Skipped reverting automation goal item progress after run closed because goal items changed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    error = %error,
                    "Failed to persist reverted automation goal item progress after run closed"
                );
            }
        }
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
