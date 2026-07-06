// Merge Pipeline Query Commands
//
// Provides visibility into the merge pipeline: active merges, waiting merges,
// and tasks needing attention (merge_conflict, merge_incomplete).

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

use crate::application::merge_pipeline_visibility::ArchivedParentMergeVisibility;
use crate::application::AppState;
use crate::commands::execution_commands::ActiveProjectState;
use crate::domain::entities::ProjectId;
use crate::domain::entities::{InternalStatus, PlanBranch, Project, Task, TaskCategory};
use crate::domain::state_machine::transition_handler::{
    has_main_merge_deferred_metadata, has_merge_deferred_metadata,
};

fn is_visible_merge_pipeline_status(status: InternalStatus) -> bool {
    matches!(
        status,
        InternalStatus::Merging
            | InternalStatus::WaitingOnPr
            | InternalStatus::PendingMerge
            | InternalStatus::MergeConflict
            | InternalStatus::MergeIncomplete
    )
}

fn is_visible_merge_pipeline_task(task: &Task) -> bool {
    task.archived_at.is_none() && is_visible_merge_pipeline_status(task.internal_status)
}

/// A task in the merge pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePipelineTask {
    /// Task ID
    pub task_id: String,
    /// Task title
    pub title: String,
    /// Current internal status
    pub internal_status: String,
    /// Source branch (task branch)
    pub source_branch: String,
    /// Target branch (resolved merge target)
    pub target_branch: String,
    /// Whether this merge is deferred (waiting for another merge)
    pub is_deferred: bool,
    /// Whether this merge to main is deferred because agents are still running
    pub is_main_merge_deferred: bool,
    /// Blocking branch name (for deferred merges)
    pub blocking_branch: Option<String>,
    /// Conflict file paths (for merge_conflict tasks)
    pub conflict_files: Option<Vec<String>>,
    /// Error context (for merge_incomplete tasks)
    pub error_context: Option<String>,
}

/// Response for get_merge_pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePipelineResponse {
    /// Tasks currently merging (status = merging)
    pub active: Vec<MergePipelineTask>,
    /// Tasks waiting to merge (status = pending_merge)
    pub waiting: Vec<MergePipelineTask>,
    /// Tasks needing attention (status = merge_conflict | merge_incomplete)
    pub needs_attention: Vec<MergePipelineTask>,
}

/// Extract merge metadata (conflict files, error context) from task metadata JSON
fn extract_merge_metadata(task: &Task) -> (Option<Vec<String>>, Option<String>) {
    use crate::domain::state_machine::transition_handler::parse_metadata;

    let meta = parse_metadata(task);
    if let Some(meta) = meta {
        let conflict_files = meta
            .get("conflict_files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let error_context = meta.get("error").and_then(|v| v.as_str()).map(String::from);

        (conflict_files, error_context)
    } else {
        (None, None)
    }
}

/// Resolve source and target branches from pre-loaded plan branches (no DB queries).
///
/// Mirrors the logic of `resolve_merge_branches()` but uses an in-memory cache
/// of plan branches instead of per-task DB lookups. Used by the poll endpoint
/// to avoid 2 DB queries per task per 5-second poll cycle.
fn resolve_merge_branches_from_cache(
    task: &Task,
    project: &Project,
    plan_branches: &[PlanBranch],
) -> (String, String) {
    let base_branch = project.base_branch.as_deref().unwrap_or("main").to_string();
    let task_branch = task.task_branch.clone().unwrap_or_default();

    // Check if this task IS the merge task for a plan branch
    if task.category == TaskCategory::PlanMerge {
        if let Some(pb) = plan_branches
            .iter()
            .find(|pb| pb.merge_task_id.as_ref() == Some(&task.id))
        {
            return (
                pb.branch_name.clone(),
                pb.base_branch_override.clone().unwrap_or(base_branch),
            );
        }
    }

    // Check if this task belongs to a plan with an active feature branch
    if let Some(ref session_id) = task.ideation_session_id {
        if let Some(pb) = plan_branches.iter().find(|pb| pb.session_id == *session_id) {
            return (task_branch, pb.branch_name.clone());
        }
    }

    (task_branch, base_branch)
}

/// Get the merge pipeline for all projects
///
/// Returns tasks in merge-related states grouped into:
/// - active: tasks currently merging (status = merging)
/// - waiting: tasks waiting to merge (status = pending_merge)
/// - needs_attention: tasks with conflicts or errors (status = merge_conflict | merge_incomplete)
#[tauri::command]
pub async fn get_merge_pipeline(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    state: State<'_, AppState>,
) -> Result<MergePipelineResponse, String> {
    let effective_project_id = match project_id {
        Some(id) => Some(ProjectId::from_string(id)),
        None => active_project_state.get().await,
    };

    let mut active = Vec::new();
    let mut waiting = Vec::new();
    let mut needs_attention = Vec::new();

    let projects = if let Some(pid) = &effective_project_id {
        match state
            .project_repo
            .get_by_id(pid)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(project) => vec![project],
            None => Vec::new(),
        }
    } else {
        state
            .project_repo
            .get_all()
            .await
            .map_err(|e| e.to_string())?
    };

    for project in &projects {
        let tasks = state
            .task_repo
            .get_by_project(&project.id)
            .await
            .map_err(|e| e.to_string())?;

        // Batch-load plan branches once per project (instead of 2 DB queries per task)
        let plan_branches = state
            .plan_branch_repo
            .get_by_project_id(&project.id)
            .await
            .map_err(|e| e.to_string())?;
        let agent_workspaces = state
            .agent_conversation_workspace_repo
            .get_by_project_id(&project.id)
            .await
            .map_err(|e| e.to_string())?;
        let archived_parent_visibility =
            ArchivedParentMergeVisibility::from_workspaces(&agent_workspaces);

        for task in tasks {
            // Merge pipeline should only show live merge work, never archived stale rows.
            if !is_visible_merge_pipeline_task(&task)
                || archived_parent_visibility.hides_task(&task, &plan_branches)
            {
                continue;
            }

            // Resolve merge branches from pre-loaded data (no DB queries)
            let (source_branch, target_branch) =
                resolve_merge_branches_from_cache(&task, project, &plan_branches);

            // Check if deferred
            let is_main_merge_deferred = has_main_merge_deferred_metadata(&task);
            let is_deferred = has_merge_deferred_metadata(&task) || is_main_merge_deferred;

            // Extract merge metadata
            let (conflict_files, error_context) = extract_merge_metadata(&task);

            // Determine blocking branch for deferred tasks
            let blocking_branch = if is_deferred {
                // Find the active merging task for this project to determine blocking branch
                let active_merges = state
                    .task_repo
                    .get_by_status(&task.project_id, InternalStatus::Merging)
                    .await
                    .map_err(|e| e.to_string())?;

                active_merges.first().and_then(|t| t.task_branch.clone())
            } else {
                None
            };

            let pipeline_task = MergePipelineTask {
                task_id: task.id.as_str().to_string(),
                title: task.title.clone(),
                internal_status: task.internal_status.as_str().to_string(),
                source_branch,
                target_branch,
                is_deferred,
                is_main_merge_deferred,
                blocking_branch,
                conflict_files,
                error_context,
            };

            match task.internal_status {
                InternalStatus::Merging => active.push(pipeline_task),
                InternalStatus::PendingMerge | InternalStatus::WaitingOnPr => {
                    waiting.push(pipeline_task)
                }
                InternalStatus::MergeConflict | InternalStatus::MergeIncomplete => {
                    needs_attention.push(pipeline_task)
                }
                _ => {}
            }
        }
    }

    Ok(MergePipelineResponse {
        active,
        waiting,
        needs_attention,
    })
}

/// Get stored merge progress events for a task (hydration endpoint).
///
/// Returns accumulated progress events from the in-memory store.
/// Frontend calls this on mount before subscribing to live events
/// to recover phases that fired before the component mounted.
#[tauri::command]
pub async fn get_merge_progress(
    task_id: String,
) -> Result<Vec<crate::domain::entities::merge_progress_event::MergeProgressEvent>, String> {
    use crate::domain::entities::merge_progress_event::MERGE_PROGRESS_STORE;

    Ok(MERGE_PROGRESS_STORE
        .get(&task_id)
        .map(|entry| entry.value().clone())
        .unwrap_or_default())
}

/// Get stored merge phase list for a task (hydration endpoint).
///
/// Returns the dynamic phase list from the in-memory store.
/// Frontend calls this on mount to recover the phase list that
/// was emitted before the component mounted.
#[tauri::command]
pub async fn get_merge_phase_list(
    task_id: String,
) -> Result<Option<Vec<crate::domain::entities::merge_progress_event::MergePhaseInfo>>, String> {
    use crate::domain::entities::merge_progress_event::MERGE_PHASE_LIST_STORE;

    Ok(MERGE_PHASE_LIST_STORE
        .get(&task_id)
        .map(|entry| entry.value().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        artifact::ArtifactId, types::IdeationSessionId, AgentConversationWorkspace,
        AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus, ChatConversationId,
        IdeationAnalysisBaseRefKind, PlanBranchId, PlanBranchStatus, ProjectId, TaskId,
    };
    use chrono::Utc;

    fn make_project() -> Project {
        Project::new("test-project".into(), "/tmp/test".into())
    }

    fn make_task(id: &str, category: TaskCategory) -> Task {
        let mut task = Task::new(
            ProjectId::from_string("proj-1".to_string()),
            format!("task-{id}"),
        );
        task.id = TaskId::from_string(id.to_string());
        task.category = category;
        task.task_branch = Some(format!("ralphx/task-{id}"));
        task
    }

    fn make_plan_branch(
        session_id: &str,
        branch_name: &str,
        status: PlanBranchStatus,
    ) -> PlanBranch {
        PlanBranch {
            id: PlanBranchId::new(),
            plan_artifact_id: ArtifactId::from_string("art-1"),
            session_id: IdeationSessionId::from_string(session_id),
            project_id: ProjectId::from_string("proj-1".to_string()),
            branch_name: branch_name.to_string(),
            source_branch: "main".to_string(),
            status,
            execution_plan_id: None,
            merge_task_id: None,
            created_at: Utc::now(),
            merged_at: None,
            pr_number: None,
            pr_url: None,
            pr_status: None,
            pr_polling_active: false,
            pr_eligible: false,
            last_polled_at: None,
            pr_push_status: Default::default(),
            merge_commit_sha: None,
            pr_draft: None,
            base_branch_override: None,
        }
    }

    fn make_agent_workspace(
        status: AgentConversationWorkspaceStatus,
        linked_plan_branch_id: Option<PlanBranchId>,
        linked_session_id: Option<IdeationSessionId>,
    ) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-1".to_string()),
            ProjectId::from_string("proj-1".to_string()),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-sha".to_string()),
            "ralphx/agent-conversation-1".to_string(),
            "/tmp/ralphx-agent-conversation-1".to_string(),
        );
        workspace.status = status;
        workspace.linked_plan_branch_id = linked_plan_branch_id;
        workspace.linked_ideation_session_id = linked_session_id;
        workspace
    }

    #[test]
    fn cache_returns_plan_branch_for_merge_task_regardless_of_status() {
        let project = make_project();
        let task = make_task("merge-1", TaskCategory::PlanMerge);

        for status in [
            PlanBranchStatus::Active,
            PlanBranchStatus::Merged,
            PlanBranchStatus::Abandoned,
        ] {
            let mut pb = make_plan_branch("sess-1", "feature/plan-1", status);
            pb.merge_task_id = Some(TaskId::from_string("merge-1".to_string()));

            let (source, target) = resolve_merge_branches_from_cache(&task, &project, &[pb]);
            assert_eq!(
                source, "feature/plan-1",
                "source should be plan branch for status {status:?}"
            );
            assert_eq!(
                target, "main",
                "target should be base branch for status {status:?}"
            );
        }
    }

    #[test]
    fn cache_returns_plan_branch_for_session_task_regardless_of_status() {
        let project = make_project();
        let mut task = make_task("task-1", TaskCategory::Regular);
        task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1"));

        for status in [
            PlanBranchStatus::Active,
            PlanBranchStatus::Merged,
            PlanBranchStatus::Abandoned,
        ] {
            let pb = make_plan_branch("sess-1", "feature/plan-1", status);

            let (source, target) = resolve_merge_branches_from_cache(&task, &project, &[pb]);
            assert_eq!(
                source, "ralphx/task-task-1",
                "source should be task branch for status {status:?}"
            );
            assert_eq!(
                target, "feature/plan-1",
                "target should be plan branch for status {status:?}"
            );
        }
    }

    #[test]
    fn cache_falls_back_to_base_branch_when_no_plan_branch() {
        let project = make_project();
        let mut task = make_task("task-2", TaskCategory::Regular);
        task.ideation_session_id = Some(IdeationSessionId::from_string("sess-99"));

        let (source, target) = resolve_merge_branches_from_cache(&task, &project, &[]);
        assert_eq!(source, "ralphx/task-task-2");
        assert_eq!(target, "main");
    }

    #[test]
    fn cache_merge_task_without_matching_plan_branch_falls_back() {
        let project = make_project();
        let task = make_task("merge-99", TaskCategory::PlanMerge);

        let (source, target) = resolve_merge_branches_from_cache(&task, &project, &[]);
        assert_eq!(source, "ralphx/task-merge-99");
        assert_eq!(target, "main");
    }

    #[test]
    fn archived_pending_merge_task_is_hidden_from_merge_pipeline() {
        let mut task = make_task("archived-pending", TaskCategory::Regular);
        task.internal_status = InternalStatus::PendingMerge;
        task.archived_at = Some(Utc::now());

        assert!(
            !is_visible_merge_pipeline_task(&task),
            "archived pending-merge tasks must not appear in the merge pipeline"
        );
    }

    #[test]
    fn active_pending_merge_task_stays_visible_in_merge_pipeline() {
        let mut task = make_task("live-pending", TaskCategory::Regular);
        task.internal_status = InternalStatus::PendingMerge;

        assert!(
            is_visible_merge_pipeline_task(&task),
            "active pending-merge tasks should remain visible in the merge pipeline"
        );
    }

    #[test]
    fn archived_agent_workspace_hides_linked_plan_merge_task() {
        let mut task = make_task("merge-1", TaskCategory::PlanMerge);
        task.internal_status = InternalStatus::PendingMerge;

        let mut branch = make_plan_branch("sess-1", "feature/plan-1", PlanBranchStatus::Active);
        branch.id = PlanBranchId::from_string("plan-branch-1");
        branch.merge_task_id = Some(task.id.clone());

        let workspace = make_agent_workspace(
            AgentConversationWorkspaceStatus::Archived,
            Some(branch.id.clone()),
            Some(branch.session_id.clone()),
        );
        let visibility = ArchivedParentMergeVisibility::from_workspaces(&[workspace]);

        assert!(
            visibility.hides_task(&task, &[branch]),
            "merge tasks owned by archived Agent workspace parents should be hidden"
        );
    }

    #[test]
    fn active_agent_workspace_does_not_hide_linked_plan_merge_task() {
        let mut task = make_task("merge-1", TaskCategory::PlanMerge);
        task.internal_status = InternalStatus::PendingMerge;

        let mut branch = make_plan_branch("sess-1", "feature/plan-1", PlanBranchStatus::Active);
        branch.id = PlanBranchId::from_string("plan-branch-1");
        branch.merge_task_id = Some(task.id.clone());

        let workspace = make_agent_workspace(
            AgentConversationWorkspaceStatus::Active,
            Some(branch.id.clone()),
            Some(branch.session_id.clone()),
        );
        let visibility = ArchivedParentMergeVisibility::from_workspaces(&[workspace]);

        assert!(
            !visibility.hides_task(&task, &[branch]),
            "active Agent workspace parents should not hide live merge tasks"
        );
    }

    #[test]
    fn active_agent_workspace_link_overrides_archived_link_for_same_plan_branch() {
        let mut task = make_task("merge-1", TaskCategory::PlanMerge);
        task.internal_status = InternalStatus::PendingMerge;

        let mut branch = make_plan_branch("sess-1", "feature/plan-1", PlanBranchStatus::Active);
        branch.id = PlanBranchId::from_string("plan-branch-1");
        branch.merge_task_id = Some(task.id.clone());

        let archived_workspace = make_agent_workspace(
            AgentConversationWorkspaceStatus::Archived,
            Some(branch.id.clone()),
            Some(branch.session_id.clone()),
        );
        let active_workspace = make_agent_workspace(
            AgentConversationWorkspaceStatus::Active,
            Some(branch.id.clone()),
            Some(branch.session_id.clone()),
        );
        let visibility =
            ArchivedParentMergeVisibility::from_workspaces(&[archived_workspace, active_workspace]);

        assert!(
            !visibility.hides_task(&task, &[branch]),
            "active Agent workspace links should win over stale archived links for the same plan"
        );
    }

    #[test]
    fn archived_agent_workspace_hides_linked_session_task() {
        let mut task = make_task("task-1", TaskCategory::Regular);
        task.internal_status = InternalStatus::PendingMerge;
        task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1"));

        let workspace = make_agent_workspace(
            AgentConversationWorkspaceStatus::Archived,
            None,
            task.ideation_session_id.clone(),
        );
        let visibility = ArchivedParentMergeVisibility::from_workspaces(&[workspace]);

        assert!(
            visibility.hides_task(&task, &[]),
            "session-owned tasks should be hidden when their linked Agent workspace parent is archived"
        );
    }
}
