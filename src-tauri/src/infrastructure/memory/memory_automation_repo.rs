use std::sync::RwLock;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{
    is_open_automation_run, judge_transition_clears_verdict, Automation, AutomationId,
    AutomationJudgeState, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
    ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AutomationRepository, AutomationRunPublicationMetadata, AutomationRunRepository,
    AutomationSettingsPatch,
};
use crate::error::{AppError, AppResult};

pub struct MemoryAutomationRepository {
    automations: RwLock<Vec<Automation>>,
}

impl MemoryAutomationRepository {
    pub fn new() -> Self {
        Self {
            automations: RwLock::new(Vec::new()),
        }
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
}

pub struct MemoryAutomationRunRepository {
    runs: RwLock<Vec<AutomationRun>>,
}

impl MemoryAutomationRunRepository {
    pub fn new() -> Self {
        Self {
            runs: RwLock::new(Vec::new()),
        }
    }

    fn has_conflicting_open_run(runs: &[AutomationRun], candidate: &AutomationRun) -> bool {
        is_open_automation_run(candidate.status, candidate.judge_state)
            && runs.iter().any(|run| {
                run.automation_id == candidate.automation_id
                    && run.id != candidate.id
                    && is_open_automation_run(run.status, run.judge_state)
            })
    }
}

impl Default for MemoryAutomationRunRepository {
    fn default() -> Self {
        Self::new()
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

    async fn compare_and_swap_status(
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
        if matches!(
            to,
            AutomationRunStatus::Merged
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

    async fn compare_and_swap_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
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

    async fn delete_for_automation(&self, automation_id: &AutomationId) -> AppResult<usize> {
        let mut runs = self.runs.write().unwrap();
        let before = runs.len();
        runs.retain(|run| run.automation_id != *automation_id);
        Ok(before - runs.len())
    }
}
