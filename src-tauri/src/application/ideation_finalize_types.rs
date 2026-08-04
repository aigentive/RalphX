use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ApplyProposalsInput {
    pub session_id: String,
    pub proposal_ids: Vec<String>,
    pub target_column: String,
    #[serde(default)]
    pub base_branch_override: Option<String>,
}

#[derive(Debug)]
pub struct ApplyProposalsResult {
    pub created_task_ids: Vec<String>,
    pub dependencies_created: usize,
    pub tasks_created: usize,
    pub message: Option<String>,
    pub warnings: Vec<String>,
    pub session_converted: bool,
    pub execution_plan_id: Option<String>,
    pub project_id: String,
    pub session_id: String,
    pub any_ready_tasks: bool,
    pub is_user_title: bool,
    pub proposal_titles: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ApplyProposalsResultResponse {
    pub created_task_ids: Vec<String>,
    pub dependencies_created: usize,
    pub tasks_created: usize,
    pub message: Option<String>,
    pub warnings: Vec<String>,
    pub session_converted: bool,
    pub execution_plan_id: Option<String>,
}

impl From<ApplyProposalsResult> for ApplyProposalsResultResponse {
    fn from(result: ApplyProposalsResult) -> Self {
        Self {
            created_task_ids: result.created_task_ids,
            dependencies_created: result.dependencies_created,
            tasks_created: result.tasks_created,
            message: result.message,
            warnings: result.warnings,
            session_converted: result.session_converted,
            execution_plan_id: result.execution_plan_id,
        }
    }
}
