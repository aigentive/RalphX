use async_trait::async_trait;

use crate::entities::{
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationStatus,
    ProjectId,
};
use crate::error::AppResult;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationSettingsPatch {
    pub name: Option<String>,
    pub max_runs: Option<i64>,
    pub max_consecutive_failures: Option<i64>,
    pub plan_approval_mode: Option<AutomationPlanApprovalMode>,
    pub pr_merge_mode: Option<AutomationPrMergeMode>,
    pub plan_deep_verification: Option<bool>,
}

/// Partial patch for the automation configuration fields the setup agent
/// populates after draft creation. Every field is apply-only-if-`Some`; a
/// `None` field leaves the existing column value untouched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationConfigPatch {
    pub goal_prompt: Option<String>,
    pub first_run_prompt: Option<String>,
    pub provider_harness: Option<String>,
    pub model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub run_mode: Option<String>,
    pub base_ref_kind: Option<String>,
    pub base_ref: Option<String>,
    pub base_display_name: Option<String>,
    pub goal_items_json: Option<String>,
    pub chain_mode: Option<String>,
    pub completion_signal: Option<String>,
    pub setup_analysis_summary: Option<String>,
    pub spec_artifact_id: Option<String>,
}

#[async_trait]
pub trait AutomationRepository: Send + Sync {
    async fn create(&self, automation: Automation) -> AppResult<Automation>;

    async fn get_by_id(&self, id: &AutomationId) -> AppResult<Option<Automation>>;

    async fn list(&self, project_id: Option<ProjectId>) -> AppResult<Vec<Automation>>;

    async fn list_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Automation>>;

    async fn update_settings(
        &self,
        id: &AutomationId,
        patch: AutomationSettingsPatch,
    ) -> AppResult<Option<Automation>>;

    /// Apply the setup-agent config patch, writing only the fields present in
    /// `patch` and returning the updated row (or `None` if the id is unknown).
    async fn update_config(
        &self,
        id: &AutomationId,
        patch: AutomationConfigPatch,
    ) -> AppResult<Option<Automation>>;

    async fn update_goal_items_json(
        &self,
        id: &AutomationId,
        goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>>;

    async fn update_goal_items_json_if_unchanged(
        &self,
        id: &AutomationId,
        expected_goal_items_json: Option<String>,
        goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>>;

    /// Persist automation authoring/verification state only if the automation
    /// row has not changed since the verifier loaded it.
    async fn update_authoring_state_if_unchanged(
        &self,
        id: &AutomationId,
        expected_updated_at: chrono::DateTime<chrono::Utc>,
        authoring_state_json: Option<String>,
    ) -> AppResult<bool>;

    async fn compare_and_swap_status(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        to: AutomationStatus,
        paused_reason_code: Option<String>,
        paused_reason_detail: Option<String>,
    ) -> AppResult<bool>;

    async fn delete_terminal(&self, id: &AutomationId) -> AppResult<bool>;

    /// Delete all attachment rows owned by an automation. FK cascades are OFF in
    /// production, so this must be called explicitly during automation deletion.
    async fn delete_attachments_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<usize>;

    /// Delete all context-ref rows owned by an automation. FK cascades are OFF in
    /// production, so this must be called explicitly during automation deletion.
    async fn delete_context_refs_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<usize>;
}
