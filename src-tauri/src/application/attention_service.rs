use std::sync::Arc;

use crate::application::services::pr_auto_merge_status::AUTO_MERGE_ENABLE_WARNING_CODE;
use crate::application::{AppState, NotificationContextResolver, PermissionState, QuestionState};
use crate::domain::entities::{
    AgentConversationWorkspaceStatus, AgentWorkspacePrReviewMonitorStatus, AttentionItem,
    AutomationRunStatus, AutomationStatus, ChatConversationId, InternalStatus,
    NotificationCategory, NotificationTarget, NotificationTargetKind, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AutomationRepository, AutomationRunRepository,
    ChatConversationRepository, IdeationSessionRepository, PlanArtifactApprovalRepository,
    ProjectRepository, TaskRepository,
};
use crate::error::AppResult;

#[path = "attention_service_items.rs"]
mod attention_service_items;
use attention_service_items::*;

const ATTENTION_TASK_LIMIT: u32 = 1_000;
/// Aggregates live, human-actionable state for the notification center.
///
/// Reads are fail-closed: any repository error aborts the entire result so callers retain their
/// last known attention count instead of presenting a partial list as complete. Items are ordered
/// by UI urgency group (agent requests, reviews, tasks, automations, git), newest first within a
/// group.
pub struct AttentionService {
    db: crate::infrastructure::sqlite::DbConnection,
    task_repo: Arc<dyn TaskRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    automation_repo: Arc<dyn AutomationRepository>,
    automation_run_repo: Arc<dyn AutomationRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<dyn PlanArtifactApprovalRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    permission_state: Arc<PermissionState>,
    question_state: Arc<QuestionState>,
    notification_context: NotificationContextResolver,
}

impl AttentionService {
    pub fn from_app_state(state: &AppState) -> Self {
        Self {
            db: state.db.clone(),
            task_repo: Arc::clone(&state.task_repo),
            project_repo: Arc::clone(&state.project_repo),
            automation_repo: Arc::clone(&state.automation_repo),
            automation_run_repo: Arc::clone(&state.automation_run_repo),
            ideation_session_repo: Arc::clone(&state.ideation_session_repo),
            plan_approval_repo: Arc::clone(&state.plan_approval_repo),
            chat_conversation_repo: Arc::clone(&state.chat_conversation_repo),
            agent_workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
            permission_state: Arc::clone(&state.permission_state),
            question_state: Arc::clone(&state.question_state),
            notification_context: NotificationContextResolver::from_app_state(state),
        }
    }

    pub async fn list_attention_items(
        &self,
        project_id: Option<&str>,
    ) -> AppResult<Vec<AttentionItem>> {
        let project_filter = project_id.map(|id| ProjectId::from_string(id.to_string()));
        let projects = self.project_repo.get_all().await?;
        let scoped_projects: Vec<_> = projects
            .into_iter()
            .filter(|project| {
                project_filter
                    .as_ref()
                    .is_none_or(|filter| project.id == *filter)
            })
            .collect();
        let mut items = Vec::new();

        for project in &scoped_projects {
            self.collect_tasks(project.id.clone(), &mut items).await?;
            self.collect_plan_approvals(project.id.clone(), &mut items)
                .await?;
        }
        self.collect_permissions(project_filter.as_ref(), &mut items)
            .await?;
        self.collect_questions(project_filter.as_ref(), &mut items)
            .await?;
        self.collect_automations(project_filter.as_ref(), &mut items)
            .await?;
        self.collect_pr_review_monitors(project_filter.as_ref(), &mut items)
            .await?;

        items = self.filter_archived_conversation_targets(items).await?;

        items.sort_by(|left, right| {
            attention_group(left.category)
                .cmp(&attention_group(right.category))
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(items)
    }

    async fn filter_archived_conversation_targets(
        &self,
        items: Vec<AttentionItem>,
    ) -> AppResult<Vec<AttentionItem>> {
        let mut visible = Vec::with_capacity(items.len());
        for item in items {
            if !self.target_conversation_is_archived(&item.target).await? {
                visible.push(item);
            }
        }
        Ok(visible)
    }

    async fn target_conversation_is_archived(
        &self,
        target: &NotificationTarget,
    ) -> AppResult<bool> {
        for conversation_id in [
            target.conversation_id.as_deref(),
            target.setup_conversation_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
            if self
                .chat_conversation_repo
                .get_by_id(&conversation_id)
                .await?
                .is_some_and(|conversation| conversation.archived_at.is_some())
                || self
                    .agent_workspace_repo
                    .get_by_conversation_id(&conversation_id)
                    .await?
                    .is_some_and(|workspace| {
                        workspace.status == AgentConversationWorkspaceStatus::Archived
                    })
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn collect_tasks(
        &self,
        project_id: ProjectId,
        items: &mut Vec<AttentionItem>,
    ) -> AppResult<()> {
        let statuses = vec![
            InternalStatus::ReviewPassed,
            InternalStatus::Escalated,
            InternalStatus::QaFailed,
            InternalStatus::MergeConflict,
            InternalStatus::MergeIncomplete,
            InternalStatus::Failed,
            InternalStatus::Blocked,
        ];
        let tasks = self
            .task_repo
            .list_paginated(
                &project_id,
                Some(statuses),
                0,
                ATTENTION_TASK_LIMIT,
                false,
                None,
                None,
                None,
            )
            .await?;
        for task in tasks {
            let Some(category) = task_attention_category(&task) else {
                continue;
            };
            let created_at = self
                .task_repo
                .get_status_last_entered_at(&task.id, task.internal_status)
                .await?
                .map(|timestamp| timestamp.to_rfc3339());
            items.push(task_attention_item(task, category, created_at));
        }
        Ok(())
    }

    async fn collect_permissions(
        &self,
        project_filter: Option<&ProjectId>,
        items: &mut Vec<AttentionItem>,
    ) -> AppResult<()> {
        for permission in self.permission_state.get_pending_info_strict().await? {
            let resolved = self
                .notification_context
                .resolve_permission_target(
                    permission.task_id.as_deref(),
                    permission.context_id.as_deref(),
                )
                .await?;
            if !is_in_scope(resolved.project_id.as_deref(), project_filter) {
                continue;
            }
            items.push(AttentionItem {
                id: format!("permission:{}", permission.request_id),
                category: NotificationCategory::PermissionRequest,
                title: format!("Permission needed: {}", permission.tool_name),
                detail: permission.context,
                project_id: resolved.project_id,
                created_at: Some(permission.created_at),
                target: resolved.target,
            });
        }
        Ok(())
    }

    async fn collect_questions(
        &self,
        project_filter: Option<&ProjectId>,
        items: &mut Vec<AttentionItem>,
    ) -> AppResult<()> {
        for question in self.question_state.get_pending_info_strict().await? {
            let conversation_id = crate::domain::entities::ChatConversationId::from_string(
                question.session_id.clone(),
            );
            let resolved = self
                .notification_context
                .resolve_conversation_target(&conversation_id)
                .await?;
            if !is_in_scope(resolved.project_id.as_deref(), project_filter) {
                continue;
            }
            items.push(AttentionItem {
                id: format!("question:{}", question.request_id),
                category: NotificationCategory::AgentQuestion,
                title: question
                    .header
                    .unwrap_or_else(|| "Agent has a question".to_string()),
                detail: Some(question.question),
                project_id: resolved.project_id,
                created_at: Some(question.created_at),
                target: resolved.target,
            });
        }
        Ok(())
    }

    async fn collect_automations(
        &self,
        project_filter: Option<&ProjectId>,
        items: &mut Vec<AttentionItem>,
    ) -> AppResult<()> {
        let automations = self.automation_repo.list(project_filter.cloned()).await?;
        for automation in automations {
            if automation.status == AutomationStatus::Paused
                && automation
                    .paused_reason_code
                    .as_deref()
                    .is_some_and(is_actionable_paused_reason)
            {
                items.push(automation_paused_item(&automation));
            }
            for run in self
                .automation_run_repo
                .list_for_automation(&automation.id)
                .await?
            {
                if run.status == AutomationRunStatus::AwaitingPlanApproval {
                    items.push(automation_plan_approval_item(&automation, &run));
                }
                if run.status == AutomationRunStatus::Published
                    && run.error_code.as_deref() == Some(AUTO_MERGE_ENABLE_WARNING_CODE)
                {
                    items.push(automation_auto_merge_attention_item(&automation, &run));
                }
            }
        }
        Ok(())
    }

    async fn collect_plan_approvals(
        &self,
        project_id: ProjectId,
        items: &mut Vec<AttentionItem>,
    ) -> AppResult<()> {
        for session in self
            .ideation_session_repo
            .get_active_by_project(&project_id)
            .await?
        {
            let Some(bundle) = session.plan_artifact_bundle() else {
                continue;
            };
            // Approval must match the session's CURRENT artifact (same rule as
            // automation plan_gate): an approval recorded for an earlier plan
            // draft does not approve a re-drafted plan.
            let current_artifact_approved = self
                .plan_approval_repo
                .get_by_session(&session.id)
                .await?
                .is_some_and(|approval| {
                    approval.artifact_id == bundle.overview_id
                        && approval.blueprint_artifact_id == bundle.blueprint_id
                });
            let approval_deferred = crate::application::plan_approval_notification_service::has_deferred_plan_target_in_db(
                &self.db,
                &session.id,
                &bundle.action_target_id(),
            )
            .await?;
            if current_artifact_approved
                || approval_deferred
                || self
                    .notification_context
                    .session_is_automation_owned(&session)
                    .await?
                || self
                    .notification_context
                    .session_has_implementation_task(&session)
                    .await?
            {
                continue;
            }
            let resolved = self
                .notification_context
                .resolve_ideation_session_target(&session)
                .await?;
            items.push(AttentionItem {
                id: format!("plan:{}:approval", session.id),
                category: NotificationCategory::PlanApproval,
                title: session
                    .title
                    .clone()
                    .unwrap_or_else(|| "Plan awaiting approval".to_string()),
                detail: Some("Review the workspace plan before implementation begins".to_string()),
                project_id: Some(project_id.to_string()),
                created_at: Some(session.updated_at.to_rfc3339()),
                target: resolved.target,
            });
        }
        Ok(())
    }

    async fn collect_pr_review_monitors(
        &self,
        project_filter: Option<&ProjectId>,
        items: &mut Vec<AttentionItem>,
    ) -> AppResult<()> {
        for monitor in self
            .agent_workspace_repo
            .list_active_pr_review_monitors()
            .await?
        {
            if monitor.status != AgentWorkspacePrReviewMonitorStatus::AwaitingUser
                || !is_in_scope(Some(monitor.project_id.as_str()), project_filter)
            {
                continue;
            }
            items.push(AttentionItem {
                id: format!("pr-review:{}", monitor.conversation_id),
                category: NotificationCategory::PrReviewAction,
                title: format!("PR #{} needs your review", monitor.pr_number),
                detail: monitor.last_error.clone(),
                project_id: Some(monitor.project_id.to_string()),
                created_at: Some(monitor.updated_at.to_rfc3339()),
                target: NotificationTarget {
                    kind: NotificationTargetKind::AgentConversation,
                    project_id: Some(monitor.project_id.to_string()),
                    task_id: None,
                    conversation_id: Some(monitor.conversation_id.to_string()),
                    setup_conversation_id: None,
                    automation_id: None,
                    run_id: None,
                },
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "attention_service_tests.rs"]
mod tests;
