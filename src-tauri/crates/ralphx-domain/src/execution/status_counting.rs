use crate::entities::{ChatContextType, InternalStatus, ProjectId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopedExecutionSubject {
    Ideation {
        project_id: ProjectId,
        is_idle: bool,
    },
    Task {
        context_type: ChatContextType,
        project_id: ProjectId,
        status: InternalStatus,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExecutionStatusCounts {
    pub running_count: u32,
    pub total_project_active: u32,
    pub ideation_active: u32,
    pub ideation_idle: u32,
}

pub fn context_matches_running_status(
    context_type: ChatContextType,
    status: InternalStatus,
) -> bool {
    match context_type {
        ChatContextType::TaskExecution => {
            status == InternalStatus::Executing || status == InternalStatus::ReExecuting
        }
        ChatContextType::Review => status == InternalStatus::Reviewing,
        ChatContextType::Merge => status == InternalStatus::Merging,
        ChatContextType::BranchUpdate => matches!(
            status,
            InternalStatus::UpdatingPlanBranch | InternalStatus::UpdatingTaskBranch
        ),
        ChatContextType::Task
        | ChatContextType::Ideation
        | ChatContextType::Delegation
        | ChatContextType::Project
        | ChatContextType::Standalone => false,
    }
}

pub fn count_execution_status(
    subjects: impl IntoIterator<Item = ScopedExecutionSubject>,
    project_id: Option<&ProjectId>,
) -> ExecutionStatusCounts {
    let mut counts = ExecutionStatusCounts::default();

    for subject in subjects {
        match subject {
            ScopedExecutionSubject::Ideation {
                project_id: subject_project_id,
                is_idle,
            } => {
                if let Some(filter_project_id) = project_id {
                    if subject_project_id != *filter_project_id {
                        continue;
                    }
                }

                if is_idle {
                    counts.ideation_idle += 1;
                } else {
                    counts.ideation_active += 1;
                    counts.total_project_active += 1;
                }
            }
            ScopedExecutionSubject::Task {
                context_type,
                project_id: subject_project_id,
                status,
            } => {
                if let Some(filter_project_id) = project_id {
                    if subject_project_id != *filter_project_id {
                        continue;
                    }
                }

                if !context_matches_running_status(context_type, status) {
                    continue;
                }

                counts.running_count += 1;
                counts.total_project_active += 1;
            }
        }
    }

    counts
}

#[cfg(test)]
#[path = "status_counting_tests.rs"]
mod tests;
