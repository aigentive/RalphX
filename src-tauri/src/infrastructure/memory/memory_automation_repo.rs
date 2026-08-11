use std::sync::{Arc, RwLock};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(test)]
use std::{collections::HashSet, sync::Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ralphx_domain::entities::automation::is_signal_terminal_automation_run;
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;

use crate::domain::entities::{
    is_open_automation_run, judge_transition_clears_verdict, Automation, AutomationId,
    AutomationJudgeState, AutomationPlanJudgeState, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AutomationConfigPatch, AutomationRepository, AutomationRunPublicationMetadata,
    AutomationRunRepository, AutomationSettingsPatch,
};
use crate::error::{AppError, AppResult};

pub type MemoryAutomationState = Arc<RwLock<Vec<Automation>>>;

pub struct MemoryAutomationRepository {
    automations: MemoryAutomationState,
}

impl MemoryAutomationRepository {
    pub fn new() -> Self {
        Self::with_shared_state(Self::new_shared_state())
    }

    pub fn new_shared_state() -> MemoryAutomationState {
        Arc::new(RwLock::new(Vec::new()))
    }

    pub fn with_shared_state(automations: MemoryAutomationState) -> Self {
        Self { automations }
    }

    pub fn shared_state(&self) -> MemoryAutomationState {
        Arc::clone(&self.automations)
    }
}

impl Default for MemoryAutomationRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AutomationRepository for MemoryAutomationRepository {
    async fn create(&self, automation: Automation) -> AppResult<Automation> {
        self.automations.write().unwrap().push(automation.clone());
        Ok(automation)
    }

    async fn get_by_id(&self, id: &AutomationId) -> AppResult<Option<Automation>> {
        Ok(self
            .automations
            .read()
            .unwrap()
            .iter()
            .find(|automation| automation.id == *id)
            .cloned())
    }

    async fn list(&self, project_id: Option<ProjectId>) -> AppResult<Vec<Automation>> {
        let mut rows: Vec<_> = self
            .automations
            .read()
            .unwrap()
            .iter()
            .filter(|automation| {
                project_id
                    .as_ref()
                    .is_none_or(|project_id| automation.project_id == *project_id)
            })
            .cloned()
            .collect();
        rows.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.as_str().cmp(left.id.as_str()))
        });
        Ok(rows)
    }

    async fn list_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Automation>> {
        self.list(Some(project_id.clone())).await
    }

    async fn update_settings(
        &self,
        id: &AutomationId,
        patch: AutomationSettingsPatch,
    ) -> AppResult<Option<Automation>> {
        let mut automations = self.automations.write().unwrap();
        let Some(automation) = automations
            .iter_mut()
            .find(|automation| automation.id == *id)
        else {
            return Ok(None);
        };

        if let Some(name) = patch.name {
            automation.name = name;
        }
        if let Some(max_runs) = patch.max_runs {
            automation.max_runs = max_runs;
        }
        if let Some(max_consecutive_failures) = patch.max_consecutive_failures {
            automation.max_consecutive_failures = max_consecutive_failures;
        }
        if let Some(plan_approval_mode) = patch.plan_approval_mode {
            automation.plan_approval_mode = plan_approval_mode;
        }
        if let Some(pr_merge_mode) = patch.pr_merge_mode {
            automation.pr_merge_mode = pr_merge_mode;
        }
        if let Some(plan_deep_verification) = patch.plan_deep_verification {
            automation.plan_deep_verification = plan_deep_verification;
        }
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_config(
        &self,
        id: &AutomationId,
        patch: AutomationConfigPatch,
    ) -> AppResult<Option<Automation>> {
        let mut automations = self.automations.write().unwrap();
        let Some(automation) = automations
            .iter_mut()
            .find(|automation| automation.id == *id)
        else {
            return Ok(None);
        };

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
        if let Some(logical_effort) = patch.logical_effort {
            automation.logical_effort = Some(logical_effort);
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
        if let Some(base_display_name) = patch.base_display_name {
            automation.base_display_name = Some(base_display_name);
        }
        if let Some(goal_items_json) = patch.goal_items_json {
            automation.goal_items_json = Some(goal_items_json);
        }
        if let Some(chain_mode) = patch.chain_mode {
            automation.chain_mode = chain_mode;
        }
        if let Some(completion_signal) = patch.completion_signal {
            automation.completion_signal = completion_signal;
        }
        if let Some(setup_analysis_summary) = patch.setup_analysis_summary {
            automation.setup_analysis_summary = Some(setup_analysis_summary);
        }
        if let Some(spec_artifact_id) = patch.spec_artifact_id {
            automation.spec_artifact_id = Some(spec_artifact_id);
        }
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_goal_items_json(
        &self,
        id: &AutomationId,
        goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>> {
        let mut automations = self.automations.write().unwrap();
        let Some(automation) = automations
            .iter_mut()
            .find(|automation| automation.id == *id)
        else {
            return Ok(None);
        };
        automation.goal_items_json = goal_items_json;
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_goal_items_json_if_unchanged(
        &self,
        id: &AutomationId,
        expected_goal_items_json: Option<String>,
        goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>> {
        let mut automations = self.automations.write().unwrap();
        let Some(automation) = automations
            .iter_mut()
            .find(|automation| automation.id == *id)
        else {
            return Ok(None);
        };
        if automation.goal_items_json != expected_goal_items_json {
            return Ok(None);
        }
        automation.goal_items_json = goal_items_json;
        automation.updated_at = Utc::now();
        Ok(Some(automation.clone()))
    }

    async fn update_authoring_state_if_unchanged(
        &self,
        id: &AutomationId,
        expected_updated_at: DateTime<Utc>,
        authoring_state_json: Option<String>,
    ) -> AppResult<bool> {
        let mut automations = self.automations.write().unwrap();
        let Some(automation) = automations
            .iter_mut()
            .find(|automation| automation.id == *id)
        else {
            return Ok(false);
        };
        if automation.updated_at != expected_updated_at {
            return Ok(false);
        }
        automation.authoring_state_json = authoring_state_json;
        automation.updated_at = Utc::now();
        Ok(true)
    }

    async fn compare_and_swap_status(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        to: AutomationStatus,
        paused_reason_code: Option<String>,
        paused_reason_detail: Option<String>,
    ) -> AppResult<bool> {
        let mut automations = self.automations.write().unwrap();
        let Some(automation) = automations
            .iter_mut()
            .find(|automation| automation.id == *id)
        else {
            return Ok(false);
        };
        if automation.status != from {
            return Ok(false);
        }
        automation.status = to;
        automation.paused_reason_code = paused_reason_code;
        automation.paused_reason_detail = paused_reason_detail;
        automation.updated_at = Utc::now();
        Ok(true)
    }

    async fn delete_terminal(&self, id: &AutomationId) -> AppResult<bool> {
        let mut automations = self.automations.write().unwrap();
        let Some(position) = automations
            .iter()
            .position(|automation| automation.id == *id)
        else {
            return Ok(false);
        };
        if !matches!(
            automations[position].status,
            AutomationStatus::Completed | AutomationStatus::Stopped
        ) {
            return Ok(false);
        }
        automations.remove(position);
        Ok(true)
    }

    async fn delete_attachments_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> AppResult<usize> {
        // The in-memory automation repo does not model attachment rows; deletion
        // is a no-op that mirrors the SQLite contract (returns rows affected).
        Ok(0)
    }

    async fn delete_context_refs_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> AppResult<usize> {
        // The in-memory automation repo does not model context-ref rows; deletion
        // is a no-op that mirrors the SQLite contract (returns rows affected).
        Ok(0)
    }
}

pub struct MemoryAutomationRunRepository {
    automation_state: MemoryAutomationState,
    runs: RwLock<Vec<AutomationRun>>,
    #[cfg(test)]
    lose_next_running_to_published_cas: AtomicBool,
    #[cfg(test)]
    lose_next_published_to_merged_cas: AtomicBool,
    #[cfg(test)]
    published_run_error_updates: AtomicUsize,
    #[cfg(test)]
    failed_find_run_conversation_ids: Mutex<HashSet<String>>,
}

impl MemoryAutomationRunRepository {
    pub fn new(automation_state: MemoryAutomationState) -> Self {
        Self {
            automation_state,
            runs: RwLock::new(Vec::new()),
            #[cfg(test)]
            lose_next_running_to_published_cas: AtomicBool::new(false),
            #[cfg(test)]
            lose_next_published_to_merged_cas: AtomicBool::new(false),
            #[cfg(test)]
            published_run_error_updates: AtomicUsize::new(0),
            #[cfg(test)]
            failed_find_run_conversation_ids: Mutex::new(HashSet::new()),
        }
    }

    #[cfg(test)]
    pub fn lose_next_running_to_published_cas(&self) {
        self.lose_next_running_to_published_cas
            .store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub fn lose_next_published_to_merged_cas(&self) {
        self.lose_next_published_to_merged_cas
            .store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub fn published_run_error_update_count(&self) -> usize {
        self.published_run_error_updates.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub fn fail_find_run_for_conversation(&self, conversation_id: &ChatConversationId) {
        self.failed_find_run_conversation_ids
            .lock()
            .unwrap()
            .insert(conversation_id.as_str());
    }

    fn has_conflicting_open_run(runs: &[AutomationRun], candidate: &AutomationRun) -> bool {
        is_open_automation_run(candidate.status, candidate.judge_state)
            && runs.iter().any(|run| {
                run.automation_id == candidate.automation_id
                    && run.id != candidate.id
                    && is_open_automation_run(run.status, run.judge_state)
            })
    }

    fn automation_is_active(automations: &[Automation], automation_id: &AutomationId) -> bool {
        automations.iter().any(|automation| {
            automation.id == *automation_id && automation.status == AutomationStatus::Active
        })
    }
}

#[async_trait]
impl AutomationRunRepository for MemoryAutomationRunRepository {
    async fn create_run(&self, run: AutomationRun) -> AppResult<AutomationRun> {
        let mut runs = self.runs.write().unwrap();
        if Self::has_conflicting_open_run(&runs, &run) {
            return Err(AppError::Conflict(
                "automation already has an open run".to_string(),
            ));
        }
        if runs.iter().any(|existing| {
            existing.automation_id == run.automation_id && existing.run_index == run.run_index
        }) {
            return Err(AppError::Conflict(
                "automation run index already exists".to_string(),
            ));
        }
        runs.push(run.clone());
        Ok(run)
    }

    async fn get_by_id(&self, id: &AutomationRunId) -> AppResult<Option<AutomationRun>> {
        Ok(self
            .runs
            .read()
            .unwrap()
            .iter()
            .find(|run| run.id == *id)
            .cloned())
    }

    async fn list_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Vec<AutomationRun>> {
        let mut rows: Vec<_> = self
            .runs
            .read()
            .unwrap()
            .iter()
            .filter(|run| run.automation_id == *automation_id)
            .cloned()
            .collect();
        rows.sort_by_key(|run| run.run_index);
        Ok(rows)
    }

    async fn latest_for_automation(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Option<AutomationRun>> {
        Ok(self
            .runs
            .read()
            .unwrap()
            .iter()
            .filter(|run| run.automation_id == *automation_id)
            .max_by_key(|run| run.run_index)
            .cloned())
    }

    async fn find_run_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AutomationRun>> {
        #[cfg(test)]
        if self
            .failed_find_run_conversation_ids
            .lock()
            .unwrap()
            .contains(&conversation_id.as_str())
        {
            return Err(AppError::Infrastructure(
                "configured automation run lookup failure".to_string(),
            ));
        }
        Ok(self
            .runs
            .read()
            .unwrap()
            .iter()
            .filter(|run| run.conversation_id.as_ref() == Some(conversation_id))
            .max_by_key(|run| run.run_index)
            .cloned())
    }

    async fn compare_and_swap_status(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        #[cfg(test)]
        if from == AutomationRunStatus::Running
            && to == AutomationRunStatus::Published
            && self
                .lose_next_running_to_published_cas
                .swap(false, Ordering::SeqCst)
        {
            return Ok(false);
        }
        #[cfg(test)]
        if from == AutomationRunStatus::Published
            && to == AutomationRunStatus::Merged
            && self
                .lose_next_published_to_merged_cas
                .swap(false, Ordering::SeqCst)
        {
            return Ok(false);
        }

        let mut runs = self.runs.write().unwrap();
        let Some(position) = runs.iter().position(|run| run.id == *id) else {
            return Ok(false);
        };
        if runs[position].status != from {
            return Ok(false);
        }
        let mut updated = runs[position].clone();
        let now = Utc::now();
        updated.status = to;
        updated.error_code = error_code;
        updated.error_detail = error_detail;
        if to == AutomationRunStatus::Running {
            updated.agent_phase_started_at = Some(now);
        }
        if matches!(
            to,
            AutomationRunStatus::Merged
                | AutomationRunStatus::Completed
                | AutomationRunStatus::PrClosed
                | AutomationRunStatus::AgentFailed
                | AutomationRunStatus::Cancelled
        ) {
            updated.finished_at.get_or_insert(now);
        }
        updated.updated_at = now;
        if Self::has_conflicting_open_run(&runs, &updated) {
            return Err(AppError::Conflict(
                "automation already has an open run".to_string(),
            ));
        }
        runs[position] = updated;
        Ok(true)
    }

    async fn compare_and_swap_status_with_agent_phase_started_at(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        agent_phase_started_at: chrono::DateTime<Utc>,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        let mut runs = self.runs.write().unwrap();
        let Some(position) = runs.iter().position(|run| run.id == *id) else {
            return Ok(false);
        };
        if runs[position].status != from {
            return Ok(false);
        }
        let mut updated = runs[position].clone();
        let now = Utc::now();
        updated.status = to;
        updated.error_code = error_code;
        updated.error_detail = error_detail;
        updated.agent_phase_started_at = Some(agent_phase_started_at);
        if matches!(
            to,
            AutomationRunStatus::Merged
                | AutomationRunStatus::Completed
                | AutomationRunStatus::PrClosed
                | AutomationRunStatus::AgentFailed
                | AutomationRunStatus::Cancelled
        ) {
            updated.finished_at.get_or_insert(now);
        }
        updated.updated_at = now;
        if Self::has_conflicting_open_run(&runs, &updated) {
            return Err(AppError::Conflict(
                "automation already has an open run".to_string(),
            ));
        }
        runs[position] = updated;
        Ok(true)
    }

    async fn compare_and_swap_status_clearing_plan_pending_instructions(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        let mut runs = self.runs.write().unwrap();
        let Some(position) = runs.iter().position(|run| run.id == *id) else {
            return Ok(false);
        };
        if runs[position].status != from {
            return Ok(false);
        }
        let mut updated = runs[position].clone();
        let now = Utc::now();
        updated.status = to;
        updated.error_code = error_code;
        updated.error_detail = error_detail;
        updated.plan_pending_instructions = None;
        if to == AutomationRunStatus::Running {
            updated.agent_phase_started_at = Some(now);
        }
        if matches!(
            to,
            AutomationRunStatus::Merged
                | AutomationRunStatus::Completed
                | AutomationRunStatus::PrClosed
                | AutomationRunStatus::AgentFailed
                | AutomationRunStatus::Cancelled
        ) {
            updated.finished_at.get_or_insert(now);
        }
        updated.updated_at = now;
        if Self::has_conflicting_open_run(&runs, &updated) {
            return Err(AppError::Conflict(
                "automation already has an open run".to_string(),
            ));
        }
        runs[position] = updated;
        Ok(true)
    }

    async fn update_start_metadata(
        &self,
        id: &AutomationRunId,
        conversation_id: &ChatConversationId,
        branch_name: Option<String>,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        if run.status != AutomationRunStatus::Provisioning {
            return Ok(None);
        }
        let now = Utc::now();
        run.conversation_id = Some(conversation_id.clone());
        run.branch_name = branch_name;
        if run.started_at.is_none() {
            run.started_at = Some(now);
        }
        run.updated_at = now;
        Ok(Some(run.clone()))
    }

    async fn update_publication_metadata(
        &self,
        id: &AutomationRunId,
        metadata: AutomationRunPublicationMetadata,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        if !matches!(
            run.status,
            AutomationRunStatus::Running | AutomationRunStatus::Published
        ) {
            return Ok(None);
        }
        run.pr_number = metadata.pr_number;
        run.pr_url = metadata.pr_url;
        run.pr_title = metadata.pr_title;
        run.pr_head_ref_name = metadata.pr_head_ref_name;
        run.pr_base_ref_name = metadata.pr_base_ref_name;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn clear_publication_metadata(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        run.pr_number = None;
        run.pr_url = None;
        run.pr_title = None;
        run.pr_head_ref_name = None;
        run.pr_base_ref_name = None;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn update_merge_metadata(
        &self,
        id: &AutomationRunId,
        merge_commit_sha: Option<String>,
        pr_merged_at: Option<chrono::DateTime<Utc>>,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        if run.status != AutomationRunStatus::Published {
            return Ok(None);
        }
        run.merge_commit_sha = merge_commit_sha;
        run.pr_merged_at = pr_merged_at;
        run.signal_check_failures = 0;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn compare_and_swap_status_with_merge_metadata(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        merge_commit_sha: Option<String>,
        pr_merged_at: Option<chrono::DateTime<Utc>>,
    ) -> AppResult<bool> {
        #[cfg(test)]
        if from == AutomationRunStatus::Published
            && to == AutomationRunStatus::Merged
            && self
                .lose_next_published_to_merged_cas
                .swap(false, Ordering::SeqCst)
        {
            return Ok(false);
        }

        let mut runs = self.runs.write().unwrap();
        let Some(position) = runs.iter().position(|run| run.id == *id) else {
            return Ok(false);
        };
        if runs[position].status != from {
            return Ok(false);
        }
        let mut updated = runs[position].clone();
        let now = Utc::now();
        updated.status = to;
        updated.error_code = None;
        updated.error_detail = None;
        updated.merge_commit_sha = merge_commit_sha;
        updated.pr_merged_at = pr_merged_at;
        updated.signal_check_failures = 0;
        if matches!(
            to,
            AutomationRunStatus::Merged
                | AutomationRunStatus::Completed
                | AutomationRunStatus::PrClosed
                | AutomationRunStatus::AgentFailed
                | AutomationRunStatus::Cancelled
        ) {
            updated.finished_at.get_or_insert(now);
        }
        updated.updated_at = now;
        if Self::has_conflicting_open_run(&runs, &updated) {
            return Err(AppError::Conflict(
                "automation already has an open run".to_string(),
            ));
        }
        runs[position] = updated;
        Ok(true)
    }

    async fn increment_signal_check_failures(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        if run.status != AutomationRunStatus::Published {
            return Ok(None);
        }
        run.signal_check_failures += 1;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn reset_signal_check_failures(
        &self,
        id: &AutomationRunId,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        if run.status != AutomationRunStatus::Published {
            return Ok(None);
        }
        if run.signal_check_failures != 0 {
            run.signal_check_failures = 0;
            run.updated_at = Utc::now();
        }
        Ok(Some(run.clone()))
    }

    async fn update_published_run_error(
        &self,
        id: &AutomationRunId,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<Option<AutomationRun>> {
        #[cfg(test)]
        self.published_run_error_updates
            .fetch_add(1, Ordering::SeqCst);

        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        if run.status != AutomationRunStatus::Published {
            return Ok(None);
        }
        run.error_code = error_code;
        run.error_detail = error_detail;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn compare_and_swap_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
        guard: AutomationJudgeTransitionGuard,
        judge_verdict_json: Option<String>,
        judge_model_id: Option<String>,
        judge_lease_expires_at: Option<DateTime<Utc>>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(false);
        };
        if run.judge_state != from {
            return Ok(false);
        }
        let guard_matches = match guard {
            AutomationJudgeTransitionGuard::Dispatch => true,
            AutomationJudgeTransitionGuard::Settle(expected_lease) => {
                run.judge_lease_expires_at == Some(expected_lease)
            }
            AutomationJudgeTransitionGuard::LegacyNullLease => run.judge_lease_expires_at.is_none(),
        };
        if !guard_matches {
            return Ok(false);
        }
        let clear_judge_verdict =
            judge_transition_clears_verdict(to, judge_verdict_json.as_deref());
        run.judge_state = to;
        if clear_judge_verdict {
            run.judge_verdict_json = None;
            run.judge_model_id = None;
        } else if let Some(verdict) = judge_verdict_json {
            run.judge_verdict_json = Some(verdict);
        }
        if let Some(model_id) = judge_model_id {
            run.judge_model_id = Some(model_id);
        }
        if to == AutomationJudgeState::InProgress {
            run.judge_lease_expires_at = judge_lease_expires_at;
        } else {
            run.judge_lease_expires_at = None;
        }
        run.error_detail = error_detail;
        run.updated_at = Utc::now();
        Ok(true)
    }

    async fn compare_and_swap_plan_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationPlanJudgeState,
        to: AutomationPlanJudgeState,
        plan_judge_verdict_json: Option<String>,
        plan_judge_lease_expires_at: Option<DateTime<Utc>>,
    ) -> AppResult<bool> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(false);
        };
        if run.plan_judge_state != from {
            return Ok(false);
        }
        run.plan_judge_state = to;
        if let Some(verdict) = plan_judge_verdict_json {
            run.plan_judge_verdict_json = Some(verdict);
        }
        if to == AutomationPlanJudgeState::InProgress {
            run.plan_judge_lease_expires_at = plan_judge_lease_expires_at;
        } else {
            run.plan_judge_lease_expires_at = None;
        }
        run.updated_at = Utc::now();
        Ok(true)
    }

    async fn clear_plan_judge_verdict(&self, id: &AutomationRunId) -> AppResult<bool> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(false);
        };
        run.plan_judge_verdict_json = None;
        run.updated_at = Utc::now();
        Ok(true)
    }

    async fn clear_judge_state(&self, id: &AutomationRunId) -> AppResult<()> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(());
        };
        run.judge_state = AutomationJudgeState::None;
        run.judge_verdict_json = None;
        run.updated_at = Utc::now();
        Ok(())
    }

    async fn clear_plan_judge_state(&self, id: &AutomationRunId) -> AppResult<bool> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(false);
        };
        let changed = run.plan_judge_state != AutomationPlanJudgeState::None
            || run.plan_judge_lease_expires_at.is_some()
            || run.plan_judge_verdict_json.is_some();
        if !changed {
            return Ok(false);
        }
        run.plan_judge_state = AutomationPlanJudgeState::None;
        run.plan_judge_lease_expires_at = None;
        run.plan_judge_verdict_json = None;
        run.updated_at = Utc::now();
        Ok(true)
    }

    async fn set_plan_pending_instructions(
        &self,
        id: &AutomationRunId,
        plan_pending_instructions: Option<String>,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        run.plan_pending_instructions = plan_pending_instructions;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn set_plan_revision_round(
        &self,
        id: &AutomationRunId,
        plan_revision_round: i64,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        run.plan_revision_round = plan_revision_round;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn set_plan_last_parked_artifact_id(
        &self,
        id: &AutomationRunId,
        plan_last_parked_artifact_id: Option<String>,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        run.plan_last_parked_artifact_id = plan_last_parked_artifact_id;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn set_plan_last_parked_artifact_ids(
        &self,
        id: &AutomationRunId,
        plan_last_parked_artifact_id: Option<String>,
        plan_last_parked_blueprint_artifact_id: Option<String>,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        run.plan_last_parked_artifact_id = plan_last_parked_artifact_id;
        run.plan_last_parked_blueprint_artifact_id = plan_last_parked_blueprint_artifact_id;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn set_plan_reminder_count(
        &self,
        id: &AutomationRunId,
        plan_reminder_count: i64,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        run.plan_reminder_count = plan_reminder_count;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn set_agent_phase_started_at(
        &self,
        id: &AutomationRunId,
        agent_phase_started_at: Option<DateTime<Utc>>,
    ) -> AppResult<Option<AutomationRun>> {
        let mut runs = self.runs.write().unwrap();
        let Some(run) = runs.iter_mut().find(|run| run.id == *id) else {
            return Ok(None);
        };
        run.agent_phase_started_at = agent_phase_started_at;
        run.updated_at = Utc::now();
        Ok(Some(run.clone()))
    }

    async fn clear_finished_at(&self, id: &AutomationRunId) -> AppResult<()> {
        if let Some(run) = self
            .runs
            .write()
            .unwrap()
            .iter_mut()
            .find(|run| run.id == *id)
        {
            run.finished_at = None;
            run.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn create_judge_successor_run(
        &self,
        automation_id: &AutomationId,
        previous_run_id: &AutomationRunId,
        successor: AutomationRun,
    ) -> AppResult<Option<AutomationRun>> {
        if successor.automation_id != *automation_id
            || successor.base_from_run_id.as_ref() != Some(previous_run_id)
        {
            return Err(AppError::Validation(
                "automation judge successor does not match the judged run".to_string(),
            ));
        }

        let automations = self.automation_state.read().unwrap();
        if !Self::automation_is_active(&automations, automation_id) {
            return Ok(None);
        }

        let mut runs = self.runs.write().unwrap();
        let Some(previous) = runs.iter().find(|run| run.id == *previous_run_id) else {
            return Ok(None);
        };
        let is_latest = runs
            .iter()
            .filter(|run| run.automation_id == *automation_id)
            .all(|run| run.run_index <= previous.run_index);
        if previous.automation_id != *automation_id
            || !is_latest
            || previous.judge_state != AutomationJudgeState::Done
            || !is_signal_terminal_automation_run(previous.status)
        {
            return Ok(None);
        }
        if runs.iter().any(|existing| {
            existing.automation_id == successor.automation_id
                && existing.run_index == successor.run_index
        }) {
            return Err(AppError::Conflict(
                "automation run index already exists".to_string(),
            ));
        }
        if Self::has_conflicting_open_run(&runs, &successor) {
            return Err(AppError::Conflict(
                "automation already has an open run".to_string(),
            ));
        }

        runs.push(successor.clone());
        Ok(Some(successor))
    }

    async fn skip_judge_and_create_successor_run(
        &self,
        automation_id: &AutomationId,
        previous_run_id: &AutomationRunId,
        successor: AutomationRun,
    ) -> AppResult<Option<AutomationRun>> {
        if successor.automation_id != *automation_id
            || successor.base_from_run_id.as_ref() != Some(previous_run_id)
        {
            return Err(AppError::Validation(
                "automation skip-judge successor does not match the skipped run".to_string(),
            ));
        }

        let automations = self.automation_state.read().unwrap();
        if !Self::automation_is_active(&automations, automation_id) {
            return Ok(None);
        }

        let mut runs = self.runs.write().unwrap();
        let Some(previous_position) = runs.iter().position(|run| run.id == *previous_run_id) else {
            return Ok(None);
        };
        let previous = &runs[previous_position];
        let is_latest = runs
            .iter()
            .filter(|run| run.automation_id == *automation_id)
            .all(|run| run.run_index <= previous.run_index);
        if previous.automation_id != *automation_id
            || !is_latest
            || !matches!(
                previous.judge_state,
                AutomationJudgeState::None | AutomationJudgeState::Failed
            )
            || !is_signal_terminal_automation_run(previous.status)
        {
            return Ok(None);
        }
        if runs.iter().any(|existing| {
            existing.automation_id == successor.automation_id
                && existing.run_index == successor.run_index
        }) {
            return Err(AppError::Conflict(
                "automation run index already exists".to_string(),
            ));
        }

        let now = Utc::now();
        let mut updated_runs = runs.clone();
        let mut skipped = previous.clone();
        skipped.judge_state = AutomationJudgeState::Skipped;
        skipped.judge_lease_expires_at = None;
        skipped.error_detail = None;
        skipped.updated_at = now;
        updated_runs[previous_position] = skipped;

        if Self::has_conflicting_open_run(&updated_runs, &successor) {
            return Err(AppError::Conflict(
                "automation already has an open run".to_string(),
            ));
        }
        updated_runs.push(successor.clone());
        *runs = updated_runs;
        Ok(Some(successor))
    }

    async fn delete_for_automation(&self, automation_id: &AutomationId) -> AppResult<usize> {
        let mut runs = self.runs.write().unwrap();
        let before = runs.len();
        runs.retain(|run| run.automation_id != *automation_id);
        Ok(before - runs.len())
    }

    async fn delete_run_if_deletable(
        &self,
        automation_id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<usize> {
        let mut runs = self.runs.write().unwrap();
        let latest_run_index = runs
            .iter()
            .filter(|run| run.automation_id == *automation_id)
            .map(|run| run.run_index)
            .max();
        let Some(position) = runs.iter().position(|run| {
            run.id == *run_id
                && run.automation_id == *automation_id
                && matches!(
                    run.status,
                    AutomationRunStatus::AgentFailed | AutomationRunStatus::Cancelled
                )
                && run.judge_state != AutomationJudgeState::InProgress
                && Some(run.run_index) == latest_run_index
        }) else {
            return Ok(0);
        };
        runs.remove(position);
        Ok(1)
    }
}
