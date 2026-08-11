// Core apply-proposals service — transport-agnostic ideation acceptance logic.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::application::{
    agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path,
    agent_task_pipeline_service::validate_start_authority_sync,
    AppState,
};
use crate::application::git_service::GitService;
use crate::domain::entities::ideation::PLAN_CONTRACT_V2;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ExecutionPlan,
    ExecutionPlanId, IdeationSessionId, IdeationSessionStatus, InternalStatus, PlanBranch,
    PlanBranchId, Project, ProjectId, SessionOrigin, Task, TaskCategory, TaskProposal,
    TaskProposalId, TaskStep,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::git_auth::{inspect_repository_capability, RepositoryCapability};


// ============================================================================
// Types (descended from commands layer)
// ============================================================================

/// Returns true if the proposal belongs to the local project (not a foreign cross-project proposal).
/// Uses canonicalized path comparison with fallback to raw PathBuf.
pub(crate) fn is_local_proposal(proposal: &TaskProposal, project_dir: &std::path::Path) -> bool {
    match &proposal.target_project {
        None => true,
        Some(tp) => {
            let tp_path = std::fs::canonicalize(tp).unwrap_or_else(|_| std::path::PathBuf::from(tp));
            tp_path == project_dir
        }
    }
}

/// Generate a URL-safe slug from a project name
fn slug_from_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Ensures a base branch exists in the given git repository.
async fn ensure_base_branch_exists(
    repo_path: &Path,
    base_branch: &str,
    project_default: Option<&str>,
) -> Result<bool, String> {
    let exists = GitService::branch_exists(repo_path, base_branch)
        .await
        .map_err(|e| format!("Failed to check branch existence: {}", e))?;
    if !exists {
        let source = project_default.unwrap_or("main");
        GitService::create_branch(repo_path, base_branch, source)
            .await
            .map_err(|e| format!("Failed to create branch '{}' from '{}': {}", base_branch, source, e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Response for TaskProposal
#[derive(Debug, Serialize)]
pub struct TaskProposalResponse {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub affected_paths: Vec<String>,
    pub suggested_priority: String,
    pub priority_score: i32,
    pub priority_reason: Option<String>,
    pub estimated_complexity: String,
    pub user_priority: Option<String>,
    pub user_modified: bool,
    pub status: String,
    pub selected: bool,
    pub created_task_id: Option<String>,
    pub plan_artifact_id: Option<String>,
    pub plan_version_at_creation: Option<u32>,
    pub blueprint_artifact_id: Option<String>,
    pub blueprint_version_at_creation: Option<u32>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    /// Optional target project ID for cross-project proposals
    pub target_project: Option<String>,
    /// Partial failure contract: non-fatal dependency errors encountered during create/update
    #[serde(default)]
    pub dependency_errors: Vec<String>,
    /// Whether the auto-accept pipeline was triggered for this session
    pub auto_accept_triggered: bool,
}

impl From<TaskProposal> for TaskProposalResponse {
    fn from(proposal: TaskProposal) -> Self {
        let steps: Vec<String> = proposal
            .steps
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let acceptance_criteria: Vec<String> = proposal
            .acceptance_criteria
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let affected_paths: Vec<String> = proposal
            .affected_paths
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Self {
            id: proposal.id.as_str().to_string(),
            session_id: proposal.session_id.as_str().to_string(),
            title: proposal.title,
            description: proposal.description,
            category: proposal.category.to_string(),
            steps,
            acceptance_criteria,
            affected_paths,
            suggested_priority: proposal.suggested_priority.to_string(),
            priority_score: proposal.priority_score,
            priority_reason: proposal.priority_reason,
            estimated_complexity: proposal.estimated_complexity.to_string(),
            user_priority: proposal.user_priority.map(|p| p.to_string()),
            user_modified: proposal.user_modified,
            status: proposal.status.to_string(),
            selected: proposal.selected,
            created_task_id: proposal.created_task_id.map(|id| id.as_str().to_string()),
            plan_artifact_id: proposal.plan_artifact_id.map(|id| id.as_str().to_string()),
            plan_version_at_creation: proposal.plan_version_at_creation,
            blueprint_artifact_id: proposal
                .blueprint_artifact_id
                .map(|id| id.as_str().to_string()),
            blueprint_version_at_creation: proposal.blueprint_version_at_creation,
            sort_order: proposal.sort_order,
            created_at: proposal.created_at.to_rfc3339(),
            updated_at: proposal.updated_at.to_rfc3339(),
            target_project: proposal.target_project.clone(),
            dependency_errors: Vec::new(),
            auto_accept_triggered: false,
        }
    }
}

/// Input for apply proposals
#[derive(Debug, Deserialize)]
pub struct ApplyProposalsInput {
    pub session_id: String,
    pub proposal_ids: Vec<String>,
    pub target_column: String,
    /// Per-plan override for base branch (None = use project default)
    #[serde(default)]
    pub base_branch_override: Option<String>,
}

/// Core result of apply proposals — transport-agnostic, usable from Tauri IPC and HTTP contexts.
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

struct TxOutput {
    execution_plan_id: crate::domain::entities::ExecutionPlanId,
    plan_branch_id: PlanBranchId,
    /// Tasks created (with final internal_status/blocked_reason already set when use_auto_status)
    created_tasks: Vec<Task>,
    dependencies_created: usize,
    warnings: Vec<String>,
    any_ready_tasks: bool,
}

pub(crate) fn recheck_exact_plan_verification(
    conn: &rusqlite::Connection,
    session_id: &str,
    expected_plan_id: Option<&str>,
    expected_blueprint_id: Option<&str>,
    expected_contract_version: i32,
    required: bool,
) -> AppResult<()> {
    if !required {
        return Ok(());
    }
    let (
        current_plan_id,
        current_blueprint_id,
        verified_plan_id,
        verified_blueprint_id,
        contract_version,
    ): (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i32,
    ) = conn
        .query_row(
            "SELECT plan_artifact_id, plan_blueprint_artifact_id,
                    verified_plan_artifact_id, verified_plan_blueprint_artifact_id,
                    plan_contract_version
             FROM ideation_sessions
             WHERE id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to recheck plan verification proof: {}",
                error
            ))
        })?;
    let exact_overview = current_plan_id.is_some()
        && current_plan_id.as_deref() == expected_plan_id
        && verified_plan_id.as_deref() == current_plan_id.as_deref();
    let exact_contract = contract_version == expected_contract_version;
    let exact_blueprint = contract_version < PLAN_CONTRACT_V2
        || (current_blueprint_id.is_some()
            && current_blueprint_id.as_deref() == expected_blueprint_id
            && verified_blueprint_id.as_deref() == current_blueprint_id.as_deref());
    if !exact_overview || !exact_contract || !exact_blueprint {
        return Err(AppError::Validation(
            "Plan changed or lost exact verification proof before acceptance; verify the current plan and accept again"
                .to_string(),
        ));
    }
    Ok(())
}

fn finalize_session_acceptance(
    conn: &rusqlite::Connection,
    session_id: &str,
    require_pending_confirmation: bool,
) -> AppResult<()> {
    let rows = if require_pending_confirmation {
        conn.execute(
            "UPDATE ideation_sessions
             SET status = 'accepted', acceptance_status = 'accepted',
                 converted_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
             WHERE id = ?1 AND status = 'active' AND acceptance_status = 'pending'",
            [session_id],
        )
    } else {
        conn.execute(
            "UPDATE ideation_sessions
             SET status = 'accepted',
                 converted_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
             WHERE id = ?1 AND status = 'active'",
            [session_id],
        )
    }
    .map_err(|error| AppError::Database(format!("Failed to accept ideation session: {}", error)))?;
    if rows != 1 {
        return Err(AppError::Validation(if require_pending_confirmation {
            "Session is no longer awaiting acceptance".to_string()
        } else {
            "Session is no longer active".to_string()
        }));
    }
    Ok(())
}

// ============================================================================
// Transaction Phase Helpers
// ============================================================================

pub(crate) fn phase_insert_execution_plan(
    conn: &rusqlite::Connection,
    session_id_str: &str,
) -> AppResult<ExecutionPlan> {
    let active_plan_exists = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM execution_plans
                WHERE session_id = ?1 AND status = 'active'
             )",
            [session_id_str],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| {
            AppError::Database(format!("Failed to check active execution plan: {error}"))
        })?;
    if active_plan_exists {
        return Err(AppError::Conflict(
            "This task pipeline already has an active execution plan".to_string(),
        ));
    }

    let exec_plan = ExecutionPlan::new(IdeationSessionId::from_string(session_id_str.to_string()));
    conn.execute(
        "INSERT INTO execution_plans (id, session_id, status, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            exec_plan.id.as_str(),
            exec_plan.session_id.as_str(),
            exec_plan.status.to_db_string(),
            exec_plan.created_at.to_rfc3339(),
        ],
    )
    .map_err(|e| AppError::Database(format!("Failed to create execution plan: {}", e)))?;
    Ok(exec_plan)
}

pub(crate) fn phase_upsert_plan_branch(
    conn: &rusqlite::Connection,
    plan_artifact_id_tx: &Option<ArtifactId>,
    session_id_str: &str,
    project_id_str: &str,
    base_branch_override_tx: &Option<String>,
    project_base_branch_tx: &Option<String>,
    project_name_tx: &str,
    effective_pr_eligible_tx: bool,
    execution_plan_id: &ExecutionPlanId,
    branch_name_override_tx: &Option<String>,
) -> AppResult<(PlanBranchId, String)> {
    let effective_plan_id_str = plan_artifact_id_tx
        .as_ref()
        .map(|id| id.as_str().to_string())
        .unwrap_or_else(|| session_id_str.to_string());
    let base_branch = base_branch_override_tx.clone().unwrap_or_else(|| {
        project_base_branch_tx
            .as_deref()
            .unwrap_or("main")
            .to_string()
    });
    let project_slug = slug_from_name(project_name_tx);
    let exec_plan_str = execution_plan_id.as_str();
    let short_id = &exec_plan_str[..8.min(exec_plan_str.len())];
    let branch_name = branch_name_override_tx
        .clone()
        .unwrap_or_else(|| format!("ralphx/{}/plan-{}", project_slug, short_id));

    let branch = PlanBranch::new(
        ArtifactId::from_string(effective_plan_id_str),
        IdeationSessionId::from_string(session_id_str.to_string()),
        ProjectId::from_string(project_id_str.to_string()),
        branch_name,
        base_branch.clone(),
    );
    let branch_with_plan = PlanBranch {
        execution_plan_id: Some(execution_plan_id.clone()),
        pr_eligible: effective_pr_eligible_tx,
        base_branch_override: base_branch_override_tx.clone(),
        ..branch
    };

    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch, status, merge_task_id, created_at, merged_at, execution_plan_id, pr_eligible, base_branch_override)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(session_id) DO UPDATE SET
           plan_artifact_id=excluded.plan_artifact_id,
           project_id=excluded.project_id,
           branch_name=excluded.branch_name,
           source_branch=excluded.source_branch,
           status=excluded.status,
           merge_task_id=excluded.merge_task_id,
           merged_at=excluded.merged_at,
           execution_plan_id=excluded.execution_plan_id,
           pr_eligible=CASE
             WHEN plan_branches.pr_number IS NULL THEN excluded.pr_eligible
             ELSE plan_branches.pr_eligible
           END,
           base_branch_override=excluded.base_branch_override",
        rusqlite::params![
            branch_with_plan.id.as_str(),
            branch_with_plan.plan_artifact_id.as_str(),
            branch_with_plan.session_id.as_str(),
            branch_with_plan.project_id.as_str(),
            branch_with_plan.branch_name,
            branch_with_plan.source_branch,
            branch_with_plan.status.to_db_string(),
            branch_with_plan.merge_task_id.as_ref().map(|t| t.as_str().to_string()),
            branch_with_plan.created_at.to_rfc3339(),
            branch_with_plan.merged_at.map(|dt| dt.to_rfc3339()),
            branch_with_plan.execution_plan_id.as_ref().map(|id| id.as_str().to_string()),
            branch_with_plan.pr_eligible as i64,
            branch_with_plan.base_branch_override.clone(),
        ],
    )
    .map_err(|e| AppError::Database(format!("Failed to upsert plan branch: {}", e)))?;

    // Re-fetch by session_id to get the persisted row's id
    // (UPSERT may have preserved an existing row's id)
    let mut stmt = conn
        .prepare("SELECT * FROM plan_branches WHERE session_id = ?1")
        .map_err(|e| AppError::Database(format!("Failed to prepare branch fetch: {}", e)))?;
    let persisted = stmt
        .query_row(rusqlite::params![session_id_str], |row| {
            PlanBranch::from_row(row)
        })
        .map_err(|e| AppError::Database(format!("Failed to fetch upserted branch: {}", e)))?;

    Ok((persisted.id, base_branch))
}

/// Derive PR eligibility for a newly created plan branch from the live remote
/// topology and the user's future-PR preference. A failed inspection cannot
/// silently become a local merge because it would mutate the task pipeline
/// before the repository authority is known.
pub(crate) fn derive_plan_branch_pr_eligibility(
    github_pr_enabled: bool,
    capability: RepositoryCapability,
) -> AppResult<bool> {
    match capability {
        RepositoryCapability::Github { .. } => Ok(github_pr_enabled),
        RepositoryCapability::LocalOnly | RepositoryCapability::OtherRemote { .. } => Ok(false),
        RepositoryCapability::InspectionFailed { message } => Err(AppError::Validation(format!(
            "Cannot determine repository capability before creating the plan branch: {message}"
        ))),
    }
}

pub(crate) async fn inspect_plan_branch_pr_eligibility(project: &Project) -> AppResult<bool> {
    if !project.github_pr_enabled {
        return Ok(false);
    }

    derive_plan_branch_pr_eligibility(
        true,
        inspect_repository_capability(std::path::Path::new(&project.working_directory)).await,
    )
}

pub(crate) fn phase_insert_tasks_and_steps(
    conn: &rusqlite::Connection,
    proposals_tx: &[TaskProposal],
    project_id_str: &str,
    session_id_str: &str,
    plan_artifact_id_tx: &Option<ArtifactId>,
    use_auto_status_tx: bool,
    proposal_deps_tx: &HashMap<String, Vec<String>>,
    execution_plan_id: &ExecutionPlanId,
) -> AppResult<(Vec<Task>, HashMap<String, String>, bool)> {
    let mut created_tasks: Vec<Task> = Vec::new();
    let mut proposal_to_task: HashMap<String, String> = HashMap::new();
    let mut any_ready_tasks = false;

    // Build proposal_id → title map for blocked_reason computation
    let proposal_titles_map: HashMap<String, String> = proposals_tx
        .iter()
        .map(|p| (p.id.as_str().to_string(), p.title.clone()))
        .collect();

    for proposal in proposals_tx {
        let mut task = Task::new(
            ProjectId::from_string(project_id_str.to_string()),
            proposal.title.clone(),
        );
        task.description = proposal.description.clone();
        task.category = TaskCategory::Regular;
        task.internal_status = InternalStatus::Backlog;
        task.ideation_session_id = Some(IdeationSessionId::from_string(session_id_str.to_string()));
        task.execution_plan_id = Some(execution_plan_id.clone());
        task.priority = proposal.priority_score;
        task.source_proposal_id = Some(proposal.id.clone());
        // Set plan_artifact_id during INSERT — eliminates the Phase 2 update loop
        task.plan_artifact_id = proposal
            .plan_artifact_id
            .clone()
            .or_else(|| plan_artifact_id_tx.clone());
        if proposal.blueprint_version_at_creation.is_some()
            && proposal.blueprint_artifact_id.is_none()
        {
            return Err(AppError::Validation(format!(
                "Proposal {} blueprint lineage is incomplete; regenerate proposals from the current plan bundle",
                proposal.id
            )));
        }
        task.plan_blueprint_artifact_id = proposal.blueprint_artifact_id.clone();

        // Compute auto-status from pre-fetched proposal_deps (no async calls needed)
        if use_auto_status_tx {
            let has_blockers = proposal_deps_tx
                .get(proposal.id.as_str())
                .map(|deps| {
                    deps.iter()
                        .any(|dep_id| proposals_tx.iter().any(|p| p.id.as_str() == dep_id))
                })
                .unwrap_or(false);
            if has_blockers {
                let dep_ids = proposal_deps_tx
                    .get(proposal.id.as_str())
                    .cloned()
                    .unwrap_or_default();
                let blocker_names: Vec<String> = dep_ids
                    .iter()
                    .filter_map(|dep_id| proposal_titles_map.get(dep_id))
                    .cloned()
                    .collect();
                task.internal_status = InternalStatus::Blocked;
                task.blocked_reason = Some(format!("Waiting for: {}", blocker_names.join(", ")));
            } else {
                task.internal_status = InternalStatus::Ready;
                any_ready_tasks = true;
            }
        }

        conn.execute(
            "INSERT INTO tasks (id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, worktree_path, merge_commit_sha, metadata, merge_pipeline_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            rusqlite::params![
                task.id.as_str(),
                task.project_id.as_str(),
                task.category.to_string(),
                task.title.clone(),
                task.description.clone(),
                task.priority,
                task.internal_status.as_str(),
                task.needs_review_point,
                task.source_proposal_id.as_ref().map(|id| id.as_str()),
                task.plan_artifact_id.as_ref().map(|id| id.as_str()),
                task.plan_blueprint_artifact_id.as_ref().map(|id| id.as_str()),
                task.ideation_session_id.as_ref().map(|id| id.as_str()),
                task.execution_plan_id.as_ref().map(|id| id.as_str()),
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
                task.started_at.map(|dt| dt.to_rfc3339()),
                task.completed_at.map(|dt| dt.to_rfc3339()),
                task.archived_at.map(|dt| dt.to_rfc3339()),
                task.blocked_reason.clone(),
                task.task_branch.clone(),
                task.worktree_path.clone(),
                task.merge_commit_sha.clone(),
                task.metadata.clone(),
                task.merge_pipeline_active.clone(),
            ],
        )
        .map_err(|e| AppError::Database(format!("Failed to create task: {}", e)))?;

        // Insert task steps
        if let Some(steps_json) = &proposal.steps {
            if let Ok(step_titles) = serde_json::from_str::<Vec<String>>(steps_json) {
                for (idx, title) in step_titles.into_iter().enumerate() {
                    let step =
                        TaskStep::new(task.id.clone(), title, idx as i32, "proposal".to_string());
                    conn.execute(
                        "INSERT INTO task_steps (id, task_id, title, description, status, sort_order, depends_on, created_by, completion_note, created_at, updated_at, started_at, completed_at, parent_step_id, scope_context)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                        rusqlite::params![
                            step.id.as_str(),
                            step.task_id.as_str(),
                            step.title,
                            step.description,
                            step.status.to_db_string(),
                            step.sort_order,
                            step.depends_on.as_ref().map(|id| id.as_str()),
                            step.created_by,
                            step.completion_note,
                            step.created_at.to_rfc3339(),
                            step.updated_at.to_rfc3339(),
                            step.started_at.map(|dt| dt.to_rfc3339()),
                            step.completed_at.map(|dt| dt.to_rfc3339()),
                            step.parent_step_id.as_ref().map(|id| id.as_str()),
                            step.scope_context,
                        ],
                    )
                    .map_err(|e| AppError::Database(format!("Failed to create task step: {}", e)))?;
                }
            }
        }

        proposal_to_task.insert(
            proposal.id.as_str().to_string(),
            task.id.as_str().to_string(),
        );
        created_tasks.push(task);
    }

    Ok((created_tasks, proposal_to_task, any_ready_tasks))
}

pub(crate) fn phase_insert_dependencies(
    conn: &rusqlite::Connection,
    proposals_tx: &[TaskProposal],
    proposal_deps_tx: &HashMap<String, Vec<String>>,
    proposal_to_task: &HashMap<String, String>,
) -> AppResult<(usize, Vec<String>)> {
    let mut dependencies_created = 0usize;
    let mut warnings: Vec<String> = Vec::new();
    for proposal in proposals_tx {
        if let Some(deps) = proposal_deps_tx.get(proposal.id.as_str()) {
            for dep_proposal_id in deps {
                let task_id = proposal_to_task.get(proposal.id.as_str());
                let dep_task_id = proposal_to_task.get(dep_proposal_id.as_str());
                if let (Some(task_id), Some(dep_task_id)) = (task_id, dep_task_id) {
                    let dep_row_id = uuid::Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT OR IGNORE INTO task_dependencies (id, task_id, depends_on_task_id) VALUES (?1, ?2, ?3)",
                        rusqlite::params![
                            dep_row_id,
                            task_id.as_str(),
                            dep_task_id.as_str()
                        ],
                    )
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to create task dependency: {}",
                            e
                        ))
                    })?;
                    dependencies_created += 1;
                } else {
                    warnings.push(format!(
                        "Dependency from {} to {} not preserved (not in selection)",
                        proposal.id, dep_proposal_id
                    ));
                }
            }
        }
    }
    Ok((dependencies_created, warnings))
}

pub(crate) fn phase_update_proposals(
    conn: &rusqlite::Connection,
    proposals_tx: &[TaskProposal],
    proposal_to_task: &HashMap<String, String>,
    now_str: &str,
) -> AppResult<()> {
    for proposal in proposals_tx {
        if let Some(task_id) = proposal_to_task.get(proposal.id.as_str()) {
            conn.execute(
                "UPDATE task_proposals SET created_task_id = ?2, updated_at = ?3 WHERE id = ?1",
                rusqlite::params![proposal.id.as_str(), task_id.as_str(), now_str],
            )
            .map_err(|e| AppError::Database(format!("Failed to link proposal to task: {}", e)))?;
        }
    }
    Ok(())
}

pub(crate) fn phase_insert_merge_task(
    conn: &rusqlite::Connection,
    branch_id: &PlanBranchId,
    base_branch_name: &str,
    project_id_str: &str,
    plan_artifact_id_tx: &Option<ArtifactId>,
    session_id_str: &str,
    execution_plan_id: &ExecutionPlanId,
    created_tasks: &[Task],
) -> AppResult<()> {
    let plan_title = format!("Merge plan into {}", base_branch_name);
    let mut merge_task = Task::new_with_category(
        ProjectId::from_string(project_id_str.to_string()),
        plan_title,
        TaskCategory::PlanMerge,
    );
    merge_task.description = Some(format!(
        "Auto-created merge task: merges feature branch into {}",
        base_branch_name
    ));
    // Only set plan_artifact_id when real artifact exists (FK safety for tasks table)
    merge_task.plan_artifact_id = plan_artifact_id_tx.clone();
    merge_task.ideation_session_id =
        Some(IdeationSessionId::from_string(session_id_str.to_string()));
    merge_task.execution_plan_id = Some(execution_plan_id.clone());
    merge_task.internal_status = InternalStatus::Blocked;
    merge_task.blocked_reason = Some("Waiting for all plan tasks to complete".to_string());

    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, worktree_path, merge_commit_sha, metadata, merge_pipeline_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        rusqlite::params![
            merge_task.id.as_str(),
            merge_task.project_id.as_str(),
            merge_task.category.to_string(),
            merge_task.title.clone(),
            merge_task.description.clone(),
            merge_task.priority,
            merge_task.internal_status.as_str(),
            merge_task.needs_review_point,
            merge_task.source_proposal_id.as_ref().map(|id| id.as_str()),
            merge_task.plan_artifact_id.as_ref().map(|id| id.as_str()),
            merge_task.ideation_session_id.as_ref().map(|id| id.as_str()),
            merge_task.execution_plan_id.as_ref().map(|id| id.as_str()),
            merge_task.created_at.to_rfc3339(),
            merge_task.updated_at.to_rfc3339(),
            merge_task.started_at.map(|dt| dt.to_rfc3339()),
            merge_task.completed_at.map(|dt| dt.to_rfc3339()),
            merge_task.archived_at.map(|dt| dt.to_rfc3339()),
            merge_task.blocked_reason.clone(),
            merge_task.task_branch.clone(),
            merge_task.worktree_path.clone(),
            merge_task.merge_commit_sha.clone(),
            merge_task.metadata.clone(),
            merge_task.merge_pipeline_active.clone(),
        ],
    )
    .map_err(|e| AppError::Database(format!("Failed to create merge task: {}", e)))?;

    // Add blockedBy deps: merge task blocked by all plan tasks.
    // These do NOT increment `dependencies_created` (plan-to-plan edges only).
    for task in created_tasks {
        let dep_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO task_dependencies (id, task_id, depends_on_task_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![dep_id, merge_task.id.as_str(), task.id.as_str()],
        )
        .map_err(|e| {
            AppError::Database(format!("Failed to add merge task dependency: {}", e))
        })?;
    }

    // Update plan_branch.merge_task_id
    conn.execute(
        "UPDATE plan_branches SET merge_task_id = ?2 WHERE id = ?1",
        rusqlite::params![branch_id.as_str(), merge_task.id.as_str()],
    )
    .map_err(|e| AppError::Database(format!("Failed to set merge task ID: {}", e)))?;

    Ok(())
}

pub(crate) async fn load_linked_agent_conversation_workspace(
    app_state: &AppState,
    session_id: &IdeationSessionId,
    project_id: &ProjectId,
) -> AppResult<Option<AgentConversationWorkspace>> {
    let workspaces = app_state
        .agent_conversation_workspace_repo
        .get_by_project_id(project_id)
        .await
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to load agent conversation workspaces for project {}: {}",
                project_id.as_str(),
                error
            ))
        })?;

    Ok(workspaces
        .iter()
        .find(|workspace| {
            workspace.mode == AgentConversationWorkspaceMode::Tasks
                && workspace.task_pipeline_session_id.as_ref() == Some(session_id)
        })
        .cloned()
        .or_else(|| {
            workspaces
                .into_iter()
                .find(|workspace| workspace.linked_ideation_session_id.as_ref() == Some(session_id))
        }))
}

/// Core apply-proposals logic — no Tauri types.
///
/// Contains all proposal-to-task creation logic, dependency setup, and session
/// status transition to Accepted. Returns transport-agnostic [`ApplyProposalsResult`]
/// that can be used from both the Tauri IPC command and the HTTP endpoint (Wave 2).
///
/// All DB writes (ExecutionPlan, PlanBranch UPSERT, Tasks, TaskSteps, dependencies,
/// proposal linking, merge task) are wrapped in a single atomic `db.run_transaction`.
/// On failure the entire transaction rolls back — no orphan ExecutionPlans possible.
///
/// # Errors
///
/// Returns [`AppError::Validation`] for business rule violations, [`AppError::NotFound`]
/// for missing entities, and [`AppError::Database`] for persistence failures.
pub async fn apply_proposals_core(
    app_state: &AppState,
    execution_state: &Arc<crate::application::app_state::ApplicationExecutionState>,
    input: ApplyProposalsInput,
) -> AppResult<ApplyProposalsResult> {
    apply_proposals_core_inner(app_state, execution_state, input, false, None).await
}

/// Apply every selected proposal under a still-pending human confirmation.
///
/// The pending confirmation is consumed in the same transaction that accepts
/// the session and creates its execution-plan/task rows. Verification admission
/// failures therefore leave the confirmation pending for a later retry.
pub async fn apply_pending_proposals_core(
    app_state: &AppState,
    execution_state: &Arc<crate::application::app_state::ApplicationExecutionState>,
    input: ApplyProposalsInput,
) -> AppResult<ApplyProposalsResult> {
    apply_proposals_core_inner(app_state, execution_state, input, true, None).await
}

pub(crate) async fn apply_supervised_proposals_core(
    app_state: &AppState,
    execution_state: &Arc<crate::application::app_state::ApplicationExecutionState>,
    input: ApplyProposalsInput,
    conversation_id: String,
) -> AppResult<ApplyProposalsResult> {
    apply_proposals_core_inner(
        app_state,
        execution_state,
        input,
        false,
        Some(conversation_id),
    )
    .await
}

async fn apply_proposals_core_inner(
    app_state: &AppState,
    execution_state: &Arc<crate::application::app_state::ApplicationExecutionState>,
    input: ApplyProposalsInput,
    require_pending_confirmation: bool,
    supervised_task_pipeline_conversation_id: Option<String>,
) -> AppResult<ApplyProposalsResult> {
    let session_id = IdeationSessionId::from_string(input.session_id);

    // Status will be determined automatically based on dependencies:
    // - Tasks with no blockers → Ready
    // - Tasks with blockers → Blocked
    // The target_column field is kept for backwards compatibility but ignored when "auto"
    let use_auto_status = input.target_column.to_lowercase() == "auto";

    // Get the session to know the project_id
    let session = app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(app_state)
        .authorize_session(
            Some(&session_id),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await?;

    if session.status != IdeationSessionStatus::Active {
        return Err(AppError::Validation(
            "Cannot apply proposals from an inactive session".to_string(),
        ));
    }
    if require_pending_confirmation
        && session.acceptance_status != Some(crate::domain::entities::AcceptanceStatus::Pending)
    {
        return Err(AppError::Validation(
            "Session is not in pending_acceptance state".to_string(),
        ));
    }

    // Acceptance is the only automatic-verification boundary. Draft creation and
    // revision stay uninterrupted; a required unverified plan queues one visible
    // Verify Plan turn here and asks the caller to retry after proof is recorded.
    let ideation_settings = app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .map_err(|e| AppError::Database(format!("Failed to get ideation settings: {}", e)))?;
    let effective_policy =
        crate::domain::services::resolve_effective_gate_policy(&ideation_settings, session.origin);
    let chat_service =
        app_state.build_chat_service_with_execution_state(Arc::clone(execution_state));
    crate::application::plan_verification_service::ensure_plan_verification_for_acceptance(
        app_state,
        &chat_service,
        &session,
        &effective_policy,
    )
    .await?;

    let proposal_ids: HashSet<TaskProposalId> = input
        .proposal_ids
        .into_iter()
        .map(TaskProposalId::from_string)
        .collect();
    let requested_proposal_ids = proposal_ids
        .iter()
        .map(|proposal_id| proposal_id.as_str().to_string())
        .collect::<Vec<_>>();

    // Validate that all proposals exist and belong to this session
    let all_proposals = app_state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    if let Some(existing_plan) = app_state
        .execution_plan_repo
        .get_active_for_session(&session_id)
        .await
        .map_err(|e| {
            AppError::Database(format!("Failed to check existing execution plan: {}", e))
        })?
    {
        let has_applied_proposals = all_proposals.iter().any(|p| p.created_task_id.is_some());
        let existing_tasks = app_state
            .task_repo
            .get_by_ideation_session(&session_id)
            .await
            .map_err(|e| AppError::Database(format!("Failed to inspect existing tasks: {}", e)))?;

        // Plans with 0 tasks and 0 applied proposals are always orphans from failed finalize
        // attempts — supersede them immediately without any age heuristic.
        if !has_applied_proposals && existing_tasks.is_empty() {
            if let Some(orphan_branch) = app_state
                .plan_branch_repo
                .get_by_execution_plan_id(&existing_plan.id)
                .await
                .map_err(|e| {
                    AppError::Database(format!(
                        "Failed to inspect orphan execution plan branch: {}",
                        e
                    ))
                })?
            {
                app_state
                    .plan_branch_repo
                    .delete(&orphan_branch.id)
                    .await
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to delete orphan execution plan branch: {}",
                            e
                        ))
                    })?;
            }

            app_state
                .execution_plan_repo
                .mark_superseded(&existing_plan.id)
                .await
                .map_err(|e| {
                    AppError::Database(format!("Failed to supersede orphan execution plan: {}", e))
                })?;

            tracing::warn!(
                execution_plan_id = %existing_plan.id,
                session_id = %session_id,
                "apply_proposals_core: superseded orphan active ExecutionPlan before retry"
            );
        } else {
            tracing::warn!(
                "apply_proposals_core: active ExecutionPlan {} already exists for session {} — skipping duplicate",
                existing_plan.id,
                session_id
            );
            return Ok(ApplyProposalsResult {
                created_task_ids: vec![],
                dependencies_created: 0,
                tasks_created: 0,
                message: None,
                warnings: vec![format!(
                    "Execution plan {} already active for this session — skipped to prevent duplicates",
                    existing_plan.id
                )],
                session_converted: false,
                execution_plan_id: Some(existing_plan.id.as_str().to_string()),
                project_id: session.project_id.as_str().to_string(),
                session_id: session_id.as_str().to_string(),
                any_ready_tasks: false,
                is_user_title: session
                    .title_source
                    .as_deref()
                    .map(|s| s == "user")
                    .unwrap_or(false),
                proposal_titles: all_proposals.iter().map(|p| p.title.clone()).collect(),
            });
        }
    }

    let proposals_to_apply: Vec<TaskProposal> = all_proposals
        .iter()
        .filter(|p| proposal_ids.contains(&p.id))
        .cloned()
        .collect();

    if proposals_to_apply.len() != proposal_ids.len() {
        return Err(AppError::Validation(
            "Some proposals not found in session".to_string(),
        ));
    }

    // ========================================================================
    // PHASE 0: Feature Branch Pre-Check (before ExecutionPlan + task creation)
    // ========================================================================
    // Load project and check/create base branch BEFORE creating any DB rows.
    // A branch failure here leaves no orphaned ExecutionPlan or Task records.
    let plan_artifact_id: Option<ArtifactId> = session.plan_artifact_id.clone();

    let project = app_state
        .project_repo
        .get_by_id(&session.project_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Project not found: {}",
                session.project_id.as_str()
            ))
        })?;
    // This must happen before base-branch preparation and before the pipeline
    // transaction so an inspection failure cannot create partial plan state.
    let effective_plan_pr_eligible = inspect_plan_branch_pr_eligibility(&project).await?;
    let linked_agent_workspace =
        load_linked_agent_conversation_workspace(app_state, &session_id, &session.project_id)
            .await?;

    if let Some(workspace) = linked_agent_workspace.as_ref() {
        let owns_supervised_pipeline = workspace.mode == AgentConversationWorkspaceMode::Tasks
            && workspace.task_pipeline_session_id.as_ref() == Some(&session_id);
        if !matches!(
            workspace.mode,
            AgentConversationWorkspaceMode::Ideation | AgentConversationWorkspaceMode::Autopilot
        ) && !owns_supervised_pipeline
        {
            return Err(AppError::Validation(
                "Linked agent conversation workspace does not own this task pipeline".to_string(),
            ));
        }
        resolve_valid_agent_conversation_workspace_path(&project, workspace).await?;
    }

    let session_base_ref = session
        .analysis
        .base_ref
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned();
    let effective_base_branch_override = if session.origin == SessionOrigin::Internal {
        session_base_ref
            .clone()
            .or(input.base_branch_override.clone())
    } else {
        input
            .base_branch_override
            .clone()
            .or(session_base_ref.clone())
    };

    // Ensure base branch exists before creating any rows — failure returns early cleanly.
    if let Some(base_branch) = effective_base_branch_override.as_deref() {
        let repo_path = std::path::PathBuf::from(&project.working_directory);
        let was_created =
            ensure_base_branch_exists(&repo_path, base_branch, project.base_branch.as_deref())
                .await
                .map_err(|e| AppError::Validation(e))?;
        if was_created {
            tracing::info!(
                "apply_proposals_core: auto-created base branch '{}' from project default",
                base_branch
            );
        }
    }

    // ========================================================================
    // FOREIGN PROPOSAL GUARD: filter out proposals targeting other projects
    // ========================================================================
    // Belt-and-suspenders: the HTTP path pre-filters, but the Tauri command
    // (apply_proposals_to_kanban) passes all user-selected proposals unfiltered.
    let project_dir = std::fs::canonicalize(&project.working_directory)
        .unwrap_or_else(|_| std::path::PathBuf::from(&project.working_directory));

    let total_count = proposals_to_apply.len();
    let proposals_to_apply: Vec<TaskProposal> = proposals_to_apply
        .into_iter()
        .filter(|p| {
            if is_local_proposal(p, &project_dir) {
                true
            } else {
                tracing::warn!(
                    proposal_id = p.id.as_str(),
                    target_project = ?p.target_project,
                    "apply_proposals_core: skipping foreign proposal (belongs to different project)"
                );
                false
            }
        })
        .collect();

    let session_converted = all_proposals
        .iter()
        .filter(|proposal| is_local_proposal(proposal, &project_dir))
        .filter(|proposal| proposal.created_task_id.is_none())
        .all(|proposal| proposal_ids.contains(&proposal.id));

    // All proposals were foreign — transition session to Accepted and return early.
    if proposals_to_apply.is_empty() {
        if !session_converted {
            return Err(AppError::Validation(
                "No local proposals were selected for acceptance".to_string(),
            ));
        }
        let foreign_skipped = total_count;
        let session_id_tx = session_id.as_str().to_string();
        let expected_plan_id_tx = plan_artifact_id.as_ref().map(ToString::to_string);
        let expected_blueprint_id_tx = session
            .plan_blueprint_artifact_id
            .as_ref()
            .map(ToString::to_string);
        let expected_contract_version_tx = session.plan_contract_version;
        let require_verification_tx = effective_policy.require_verification_for_accept;
        app_state
            .db
            .run_transaction(move |conn| {
                recheck_exact_plan_verification(
                    conn,
                    &session_id_tx,
                    expected_plan_id_tx.as_deref(),
                    expected_blueprint_id_tx.as_deref(),
                    expected_contract_version_tx,
                    require_verification_tx,
                )?;
                finalize_session_acceptance(conn, &session_id_tx, require_pending_confirmation)
            })
            .await?;
        return Ok(ApplyProposalsResult {
            created_task_ids: vec![],
            dependencies_created: 0,
            tasks_created: 0,
            message: Some(format!(
                "No local proposals to finalize ({} foreign skipped). \
                 Call migrate_proposals to move them to target sessions.",
                foreign_skipped
            )),
            warnings: vec![],
            session_converted: true,
            execution_plan_id: None,
            project_id: session.project_id.as_str().to_string(),
            session_id: session_id.as_str().to_string(),
            any_ready_tasks: false,
            is_user_title: session
                .title_source
                .as_deref()
                .map(|s| s == "user")
                .unwrap_or(false),
            proposal_titles: vec![],
        });
    }

    // Dependency acknowledgment gate: multi-proposal sessions must have acknowledged dependency ordering.
    // Gate lives here (not in finalize_proposals_impl) so ALL callers are protected:
    // internal MCP, Tauri IPC (apply_proposals_to_kanban), and external MCP all go through apply_proposals_core.
    // Gate is placed AFTER foreign-proposal filtering so all-foreign sessions bypass it cleanly.
    if proposals_to_apply.len() >= 2 && !session.dependencies_acknowledged {
        return Err(AppError::Validation(format!(
            "Cannot finalize: dependency ordering has not been reviewed for {} proposals. \
             Either set dependencies via create_task_proposal(depends_on) or \
             update_task_proposal(add_depends_on/add_blocks), or call \
             analyze_session_dependencies to review and acknowledge parallel execution.",
            proposals_to_apply.len()
        )));
    }

    // ========================================================================
    // PRE-FETCH: Collect proposal deps (async) before entering the transaction
    // ========================================================================
    let mut proposal_deps: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();
    for proposal in &proposals_to_apply {
        let deps = app_state
            .proposal_dependency_repo
            .get_dependencies(&proposal.id)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        proposal_deps.insert(proposal.id.clone(), deps);
    }

    // ========================================================================
    // ATOMIC TRANSACTION: ExecutionPlan + PlanBranch UPSERT + Tasks + Steps +
    //                     Dependencies + Proposal linking + Merge task
    // ========================================================================

    // Clone all data the closure needs (must be Send + 'static — no borrows into closure)
    let session_id_str = session_id.as_str().to_string();
    let project_id_str = session.project_id.as_str().to_string();
    let plan_artifact_id_tx = plan_artifact_id.clone();
    let plan_blueprint_artifact_id_tx = session.plan_blueprint_artifact_id.clone();
    let plan_contract_version_tx = session.plan_contract_version;
    let use_auto_status_tx = use_auto_status;
    let base_branch_override_tx = effective_base_branch_override.clone();
    let agent_workspace_branch_name_tx = linked_agent_workspace
        .as_ref()
        .map(|workspace| workspace.branch_name.clone());
    let project_base_branch_tx = project.base_branch.clone();
    let project_name_tx = project.name.clone();
    let effective_plan_pr_eligible_tx = effective_plan_pr_eligible;
    let require_verification_for_accept_tx = effective_policy.require_verification_for_accept;
    let session_converted_tx = session_converted;
    let proposals_tx = proposals_to_apply.clone();
    let supervised_conversation_id_tx = supervised_task_pipeline_conversation_id;
    let requested_proposal_ids_tx = requested_proposal_ids;
    // Convert to String-keyed map so the closure is 'static
    let proposal_deps_tx: HashMap<String, Vec<String>> = proposal_deps
        .iter()
        .map(|(k, vs)| {
            (
                k.as_str().to_string(),
                vs.iter().map(|v| v.as_str().to_string()).collect(),
            )
        })
        .collect();

    let tx_output = app_state
        .db
        .run_transaction(move |conn| {
            crate::application::tasks_feature_policy::authorize_tasks_session_sync(
                conn,
                Some(&session_id_str),
                crate::domain::ideation::TasksFeatureAction::Progress,
            )?;
            if let Some(conversation_id) = supervised_conversation_id_tx.as_deref() {
                validate_start_authority_sync(
                    conn,
                    conversation_id,
                    &session_id_str,
                    &requested_proposal_ids_tx,
                )?;
            }
            recheck_exact_plan_verification(
                conn,
                &session_id_str,
                plan_artifact_id_tx.as_ref().map(ArtifactId::as_str),
                plan_blueprint_artifact_id_tx
                    .as_ref()
                    .map(ArtifactId::as_str),
                plan_contract_version_tx,
                require_verification_for_accept_tx,
            )?;

            // ----------------------------------------------------------------
            // (a) INSERT execution_plan
            // ----------------------------------------------------------------
            let exec_plan = phase_insert_execution_plan(conn, &session_id_str)?;
            let execution_plan_id = exec_plan.id.clone();

            // ----------------------------------------------------------------
            // (b) UPSERT plan_branch (always create feature branch)
            // ----------------------------------------------------------------
            let pending_merge = phase_upsert_plan_branch(
                conn,
                &plan_artifact_id_tx,
                &session_id_str,
                &project_id_str,
                &base_branch_override_tx,
                &project_base_branch_tx,
                &project_name_tx,
                effective_plan_pr_eligible_tx,
                &execution_plan_id,
                &agent_workspace_branch_name_tx,
            )?;

            // ----------------------------------------------------------------
            // (c) INSERT tasks + task_steps
            // ----------------------------------------------------------------
            let (created_tasks, proposal_to_task, any_ready_tasks) = phase_insert_tasks_and_steps(
                conn,
                &proposals_tx,
                &project_id_str,
                &session_id_str,
                &plan_artifact_id_tx,
                use_auto_status_tx,
                &proposal_deps_tx,
                &execution_plan_id,
            )?;

            // ----------------------------------------------------------------
            // (d) INSERT task_dependencies
            // ----------------------------------------------------------------
            let (dependencies_created, warnings) = phase_insert_dependencies(
                conn,
                &proposals_tx,
                &proposal_deps_tx,
                &proposal_to_task,
            )?;

            // ----------------------------------------------------------------
            // (e) UPDATE proposals with created_task_id
            // ----------------------------------------------------------------
            let now_str = chrono::Utc::now().to_rfc3339();
            phase_update_proposals(conn, &proposals_tx, &proposal_to_task, &now_str)?;

            // ----------------------------------------------------------------
            // (f) INSERT merge task if feature branch
            // ----------------------------------------------------------------
            let (ref branch_id, ref base_branch_name) = pending_merge;
            phase_insert_merge_task(
                conn,
                branch_id,
                base_branch_name,
                &project_id_str,
                &plan_artifact_id_tx,
                &session_id_str,
                &execution_plan_id,
                &created_tasks,
            )?;

            if session_converted_tx {
                finalize_session_acceptance(conn, &session_id_str, require_pending_confirmation)?;
            }

            Ok(TxOutput {
                execution_plan_id,
                plan_branch_id: branch_id.clone(),
                created_tasks,
                dependencies_created,
                warnings,
                any_ready_tasks,
            })
        })
        .await?;

    if let Some(workspace) = linked_agent_workspace.as_ref() {
        app_state
            .agent_conversation_workspace_repo
            .update_links(
                &workspace.conversation_id,
                Some(&session_id),
                Some(&tx_output.plan_branch_id),
            )
            .await
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to link agent conversation workspace to plan branch: {}",
                    error
                ))
            })?;
    }

    let is_user_title = session
        .title_source
        .as_deref()
        .map(|s| s == "user")
        .unwrap_or(false);

    let proposal_titles: Vec<String> = proposals_to_apply.iter().map(|p| p.title.clone()).collect();

    let tasks_created = tx_output.created_tasks.len();
    let dependencies_created = tx_output.dependencies_created;
    let message = Some(format!(
        "Created {} task{} with {} proposal {}.",
        tasks_created,
        if tasks_created == 1 { "" } else { "s" },
        dependencies_created,
        if dependencies_created == 1 {
            "dependency"
        } else {
            "dependencies"
        }
    ));

    Ok(ApplyProposalsResult {
        created_task_ids: tx_output
            .created_tasks
            .into_iter()
            .map(|t| t.id.as_str().to_string())
            .collect(),
        dependencies_created,
        tasks_created,
        message,
        warnings: tx_output.warnings,
        session_converted,
        execution_plan_id: Some(tx_output.execution_plan_id.as_str().to_string()),
        project_id: session.project_id.as_str().to_string(),
        session_id: session_id.as_str().to_string(),
        any_ready_tasks: tx_output.any_ready_tasks,
        is_user_title,
        proposal_titles,
    })
}
