use crate::application::AppState;
use crate::domain::entities::IdeationSessionId;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, ChatContextType, PlanBranch,
    Task, TaskCategory,
};
use crate::domain::execution::{workspace_session_title, ExecutionTaskAgentWorkspace};

pub(crate) async fn resolve_agent_workspace_target_for_ideation_session(
    state: &AppState,
    session_id: &IdeationSessionId,
) -> Result<Option<ExecutionTaskAgentWorkspace>, String> {
    let Some(session) = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_linked_ideation_session_id(session_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.project_id != session.project_id
    {
        return Ok(None);
    }

    let Some(conversation) = state
        .chat_conversation_repo
        .get_by_id(&workspace.conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let context_matches = match conversation.context_type {
        ChatContextType::Ideation => conversation.context_id == session.id.as_str(),
        ChatContextType::Project => conversation.context_id == session.project_id.as_str(),
        _ => false,
    };
    if conversation.archived_at.is_some() || !context_matches {
        return Ok(None);
    }

    Ok(Some(ExecutionTaskAgentWorkspace {
        conversation_id: workspace.conversation_id.as_str().to_string(),
        project_id: workspace.project_id.as_str().to_string(),
        title: workspace_session_title(conversation.title.as_deref()),
    }))
}

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
        .filter(|workspace| workspace.status == AgentConversationWorkspaceStatus::Active)
        .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
}

#[cfg(test)]
#[path = "execution_task_navigation_tests.rs"]
mod tests;
