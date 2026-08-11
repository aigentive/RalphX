use std::collections::HashSet;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, IdeationSessionId, PlanBranch,
    PlanBranchId, Task, TaskCategory,
};

#[derive(Debug, Default)]
pub(crate) struct ArchivedParentMergeVisibility {
    archived_plan_branch_ids: HashSet<PlanBranchId>,
    archived_ideation_session_ids: HashSet<IdeationSessionId>,
}

impl ArchivedParentMergeVisibility {
    pub(crate) fn from_workspaces(workspaces: &[AgentConversationWorkspace]) -> Self {
        let mut archived_plan_branch_ids = HashSet::new();
        let mut archived_ideation_session_ids = HashSet::new();
        let mut active_plan_branch_ids = HashSet::new();
        let mut active_ideation_session_ids = HashSet::new();

        for workspace in workspaces {
            match workspace.status {
                AgentConversationWorkspaceStatus::Archived => {
                    if let Some(plan_branch_id) = workspace.linked_plan_branch_id.clone() {
                        archived_plan_branch_ids.insert(plan_branch_id);
                    }

                    if let Some(session_id) = workspace.linked_ideation_session_id.clone() {
                        archived_ideation_session_ids.insert(session_id);
                    }
                }
                AgentConversationWorkspaceStatus::Active => {
                    if let Some(plan_branch_id) = workspace.linked_plan_branch_id.clone() {
                        active_plan_branch_ids.insert(plan_branch_id);
                    }

                    if let Some(session_id) = workspace.linked_ideation_session_id.clone() {
                        active_ideation_session_ids.insert(session_id);
                    }
                }
                AgentConversationWorkspaceStatus::Missing => {}
            }
        }

        archived_plan_branch_ids.retain(|id| !active_plan_branch_ids.contains(id));
        archived_ideation_session_ids.retain(|id| !active_ideation_session_ids.contains(id));

        Self {
            archived_plan_branch_ids,
            archived_ideation_session_ids,
        }
    }

    pub(crate) fn hides_task(&self, task: &Task, plan_branches: &[PlanBranch]) -> bool {
        if task
            .ideation_session_id
            .as_ref()
            .is_some_and(|session_id| self.archived_ideation_session_ids.contains(session_id))
        {
            return true;
        }

        plan_branches.iter().any(|branch| {
            self.archived_plan_branch_ids.contains(&branch.id) && branch_owns_task(branch, task)
        })
    }
}

fn branch_owns_task(branch: &PlanBranch, task: &Task) -> bool {
    if task.category == TaskCategory::PlanMerge
        && branch
            .merge_task_id
            .as_ref()
            .is_some_and(|merge_task_id| merge_task_id == &task.id)
    {
        return true;
    }

    if branch
        .execution_plan_id
        .as_ref()
        .zip(task.execution_plan_id.as_ref())
        .is_some_and(|(branch_execution_plan_id, task_execution_plan_id)| {
            branch_execution_plan_id == task_execution_plan_id
        })
    {
        return true;
    }

    task.ideation_session_id
        .as_ref()
        .is_some_and(|session_id| branch.session_id == *session_id)
}
