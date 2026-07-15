use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, PlanBranch, Task, TaskCategory,
};
use crate::domain::execution::{workspace_session_title, ExecutionTaskAgentWorkspace};

pub(crate) async fn resolve_agent_workspace_target_for_task(
    state: &AppState,
    task: &Task,
    plan_branches: &[PlanBranch],
    workspaces: &[AgentConversationWorkspace],
) -> Result<Option<ExecutionTaskAgentWorkspace>, String> {
    let Some(workspace) = resolve_agent_workspace_for_task(task, plan_branches, workspaces) else {
        return Ok(None);
    };

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&workspace.conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(
        conversation.map(|conversation| ExecutionTaskAgentWorkspace {
            conversation_id: workspace.conversation_id.as_str().to_string(),
            project_id: workspace.project_id.as_str().to_string(),
            title: workspace_session_title(conversation.title.as_deref()),
        }),
    )
}

pub(crate) fn resolve_agent_workspace_for_task<'a>(
    task: &Task,
    plan_branches: &'a [PlanBranch],
    workspaces: &'a [AgentConversationWorkspace],
) -> Option<&'a AgentConversationWorkspace> {
    let plan_branch = resolve_plan_branch_for_task(task, plan_branches);
    if let Some(plan_branch) = plan_branch {
        if let Some(workspace) =
            latest_navigable_workspace(workspaces.iter().filter(|workspace| {
                workspace.linked_plan_branch_id.as_ref() == Some(&plan_branch.id)
            }))
        {
            return Some(workspace);
        }
    }

    if let Some(session_id) = task
        .ideation_session_id
        .as_ref()
        .or_else(|| plan_branch.map(|branch| &branch.session_id))
    {
        return latest_navigable_workspace(workspaces.iter().filter(|workspace| {
            workspace.linked_ideation_session_id.as_ref() == Some(session_id)
        }));
    }

    None
}

fn resolve_plan_branch_for_task<'a>(
    task: &Task,
    plan_branches: &'a [PlanBranch],
) -> Option<&'a PlanBranch> {
    if task.category == TaskCategory::PlanMerge {
        if let Some(branch) = plan_branches
            .iter()
            .find(|branch| branch.merge_task_id.as_ref() == Some(&task.id))
        {
            return Some(branch);
        }
    }

    if let Some(session_id) = task.ideation_session_id.as_ref() {
        if let Some(branch) = plan_branches
            .iter()
            .find(|branch| branch.session_id == *session_id)
        {
            return Some(branch);
        }
    }

    if let Some(execution_plan_id) = task.execution_plan_id.as_ref() {
        return plan_branches
            .iter()
            .find(|branch| branch.execution_plan_id.as_ref() == Some(execution_plan_id));
    }

    None
}

fn latest_navigable_workspace<'a>(
    workspaces: impl Iterator<Item = &'a AgentConversationWorkspace>,
) -> Option<&'a AgentConversationWorkspace> {
    workspaces
        .filter(|workspace| workspace.status != AgentConversationWorkspaceStatus::Archived)
        .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId,
        PlanBranchId, PlanBranchStatus, ProjectId, TaskId,
    };
    use chrono::Utc;

    fn task(id: &str) -> Task {
        let mut task = Task::new(
            ProjectId::from_string("project-1".to_string()),
            "Task".to_string(),
        );
        task.id = TaskId::from_string(id.to_string());
        task
    }

    fn plan_branch(id: &str, session_id: &str) -> PlanBranch {
        PlanBranch {
            id: PlanBranchId::from_string(id),
            plan_artifact_id: ArtifactId::from_string("artifact-1"),
            session_id: IdeationSessionId::from_string(session_id),
            project_id: ProjectId::from_string("project-1".to_string()),
            branch_name: "feature/plan".to_string(),
            source_branch: "main".to_string(),
            status: PlanBranchStatus::Active,
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

    fn workspace(
        conversation_id: &str,
        status: AgentConversationWorkspaceStatus,
        plan_branch_id: Option<PlanBranchId>,
        session_id: Option<IdeationSessionId>,
    ) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string(conversation_id),
            ProjectId::from_string("project-1".to_string()),
            crate::domain::entities::AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-sha".to_string()),
            "ralphx/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.status = status;
        workspace.linked_plan_branch_id = plan_branch_id;
        workspace.linked_ideation_session_id = session_id;
        workspace
    }

    #[test]
    fn plan_merge_task_resolves_workspace_by_plan_branch_before_session() {
        let mut merge_task = task("merge-task");
        merge_task.category = TaskCategory::PlanMerge;
        let mut branch = plan_branch("plan-branch-1", "session-1");
        branch.merge_task_id = Some(merge_task.id.clone());

        let plan_workspace = workspace(
            "11111111-1111-1111-1111-111111111111",
            AgentConversationWorkspaceStatus::Active,
            Some(branch.id.clone()),
            Some(branch.session_id.clone()),
        );
        let session_workspace = workspace(
            "22222222-2222-2222-2222-222222222222",
            AgentConversationWorkspaceStatus::Active,
            None,
            Some(branch.session_id.clone()),
        );

        let branches = [branch];
        let workspaces = [session_workspace, plan_workspace];
        let resolved = resolve_agent_workspace_for_task(&merge_task, &branches, &workspaces)
            .expect("workspace should resolve");

        assert_eq!(
            resolved.conversation_id.as_str(),
            "11111111-1111-1111-1111-111111111111"
        );
    }

    #[test]
    fn archived_workspace_is_not_a_navigation_target() {
        let mut linked_task = task("task-1");
        linked_task.ideation_session_id = Some(IdeationSessionId::from_string("session-1"));
        let branch = plan_branch("plan-branch-1", "session-1");
        let archived_workspace = workspace(
            "33333333-3333-3333-3333-333333333333",
            AgentConversationWorkspaceStatus::Archived,
            Some(branch.id.clone()),
            Some(branch.session_id.clone()),
        );

        let branches = [branch];
        let workspaces = [archived_workspace];
        let resolved = resolve_agent_workspace_for_task(&linked_task, &branches, &workspaces);

        assert!(resolved.is_none());
    }
}
