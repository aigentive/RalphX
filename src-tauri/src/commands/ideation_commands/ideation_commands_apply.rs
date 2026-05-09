// Proposal-to-task conversion and task dependency commands

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::{Manager, State};

use crate::application::{
    agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path,
    session_namer_prompt::build_session_namer_prompt, spawn_ready_task_scheduler_if_needed,
    AppState, TaskCleanupService,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    ArtifactId, ExecutionPlan, ExecutionPlanId, IdeationSessionId, IdeationSessionStatus,
    InternalStatus, PlanBranch, PlanBranchId, ProjectId, SessionOrigin, Task, TaskCategory, TaskId,
    TaskProposal, TaskProposalId, TaskStep,
};
use crate::error::{AppError, AppResult};

use super::ideation_commands_types::{
    ApplyProposalsInput, ApplyProposalsResult, ApplyProposalsResultResponse,
};
use super::is_local_proposal;
use crate::commands::branch_helpers::ensure_base_branch_exists;
use crate::commands::plan_branch_commands::slug_from_name;
use crate::http_server::handlers::ideation::stop_verification_children;

// ============================================================================
// Core Result Type
// ============================================================================

/// Output of the atomic transaction closure — all data needed for post-commit operations.
struct TxOutput {
    execution_plan_id: crate::domain::entities::ExecutionPlanId,
    plan_branch_id: PlanBranchId,
    /// Tasks created (with final internal_status/blocked_reason already set when use_auto_status)
    created_tasks: Vec<Task>,
    dependencies_created: usize,
    warnings: Vec<String>,
    any_ready_tasks: bool,
}

// ============================================================================
// Transaction Phase Helpers
// ============================================================================

fn phase_insert_execution_plan(
    conn: &rusqlite::Connection,
    session_id_str: &str,
) -> AppResult<ExecutionPlan> {
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

fn phase_upsert_plan_branch(
    conn: &rusqlite::Connection,
    plan_artifact_id_tx: &Option<ArtifactId>,
    session_id_str: &str,
    project_id_str: &str,
    base_branch_override_tx: &Option<String>,
    project_base_branch_tx: &Option<String>,
    project_name_tx: &str,
    project_pr_eligible_tx: bool,
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
        pr_eligible: project_pr_eligible_tx,
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
           execution_plan_id=excluded.execution_plan_id,
           pr_eligible=excluded.pr_eligible,
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

fn phase_insert_tasks_and_steps(
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
            "INSERT INTO tasks (id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, worktree_path, merge_commit_sha, metadata, merge_pipeline_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
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

fn phase_insert_dependencies(
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

fn phase_update_proposals(
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

fn phase_insert_merge_task(
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

async fn load_linked_agent_conversation_workspace(
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

    Ok(workspaces.into_iter().find(|workspace| {
        workspace
            .linked_ideation_session_id
            .as_ref()
            .is_some_and(|linked_session_id| linked_session_id == session_id)
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
    input: ApplyProposalsInput,
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

    if session.status != IdeationSessionStatus::Active {
        return Err(AppError::Validation(
            "Cannot apply proposals from an inactive session".to_string(),
        ));
    }

    // Verification gate: block acceptance if plan is not verified (when enforcement is enabled).
    // Resolve the effective policy once from (settings, session.origin) and pass to gate.
    let ideation_settings = app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .map_err(|e| AppError::Database(format!("Failed to get ideation settings: {}", e)))?;
    let effective_policy =
        crate::domain::services::resolve_effective_gate_policy(&ideation_settings, session.origin);
    if let Err(e) = crate::domain::services::check_verification_gate(&session, &effective_policy) {
        return Err(AppError::Validation(e.to_string()));
    }

    let proposal_ids: HashSet<TaskProposalId> = input
        .proposal_ids
        .into_iter()
        .map(TaskProposalId::from_string)
        .collect();

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
        .into_iter()
        .filter(|p| proposal_ids.contains(&p.id))
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
    let linked_agent_workspace = load_linked_agent_conversation_workspace(
        app_state,
        &session_id,
        &session.project_id,
    )
    .await?;

    if let Some(workspace) = linked_agent_workspace.as_ref() {
        if workspace.mode != AgentConversationWorkspaceMode::Ideation {
            return Err(AppError::Validation(
                "Linked agent conversation workspace is not in ideation mode".to_string(),
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

    // All proposals were foreign — transition session to Accepted and return early.
    if proposals_to_apply.is_empty() {
        let foreign_skipped = total_count;
        app_state
            .ideation_session_repo
            .update_status(&session_id, IdeationSessionStatus::Accepted)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
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
    let use_auto_status_tx = use_auto_status;
    let base_branch_override_tx = effective_base_branch_override.clone();
    let agent_workspace_branch_name_tx = linked_agent_workspace
        .as_ref()
        .map(|workspace| workspace.branch_name.clone());
    let project_base_branch_tx = project.base_branch.clone();
    let project_name_tx = project.name.clone();
    let project_pr_eligible_tx = project.github_pr_enabled;
    let proposals_tx = proposals_to_apply.clone();
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
                project_pr_eligible_tx,
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

    // ========================================================================
    // POST-TRANSACTION: session status transition to Accepted
    // ========================================================================

    // Check if all LOCAL proposals in session are now applied.
    // Foreign proposals (target_project pointing to another project) are intentionally
    // excluded — they were migrated elsewhere and should not block session acceptance.
    let remaining = app_state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .into_iter()
        .filter(|p| is_local_proposal(p, &project_dir) && p.created_task_id.is_none())
        .count();

    let session_converted = remaining == 0;
    if session_converted {
        app_state
            .ideation_session_repo
            .update_status(&session_id, IdeationSessionStatus::Accepted)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
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

// ============================================================================
// Apply and Task Dependency Commands
// ============================================================================

/// Apply selected proposals to the Kanban board as tasks (Tauri IPC command).
///
/// Delegates to [`apply_proposals_core`] and adds Tauri-specific side effects:
/// queue-change events, task scheduler trigger for newly Ready tasks, and
/// ralphx-utility-session-namer re-trigger at acceptance.
/// External HTTP callers use [`crate::http_server::handlers::external_apply_proposals`]
/// instead, which skips the scheduler (external agents poll `get_pipeline_overview`).
#[tauri::command]
pub async fn apply_proposals_to_kanban(
    input: ApplyProposalsInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ApplyProposalsResultResponse, String> {
    use crate::commands::emit_queue_changed;

    let result = apply_proposals_core(&state, input)
        .await
        .map_err(|e| e.to_string())?;

    // IPR cleanup: stop the ideation session's interactive Claude CLI process
    // now that the session has been accepted (terminal state).
    // Best-effort: if no process is found, GC will eventually clean up.
    if result.session_converted {
        let task_cleanup = TaskCleanupService::new(
            Arc::clone(&state.task_repo),
            Arc::clone(&state.project_repo),
            Arc::clone(&state.running_agent_registry),
            Some(app.clone()),
        )
        .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));

        let stopped = task_cleanup
            .stop_ideation_session_agent(&result.session_id)
            .await;
        if !stopped {
            tracing::warn!(
                "IPR cleanup: no running process found for accepted session {}",
                result.session_id
            );
        }

        // Stop and archive any running verification child agents (best-effort).
        stop_verification_children(&result.session_id, &state)
            .await
            .ok();
    }

    // Re-trigger ralphx-utility-session-namer if title was not manually set by user.
    // At acceptance, proposals are finalized — namer generates a commit-ready title
    // reflecting the actual work (not just the initial user message).
    // Skip if user has set a custom title (title_source == "user").
    if !result.is_user_title {
        let proposals_context = result.proposal_titles.join("; ");
        let session_id_str = result.session_id.clone();
        if let Err(error) = spawn_session_namer_agent(
            &state,
            SessionNamerTarget::accepted_session(session_id_str, proposals_context),
        )
        .await
        {
            tracing::warn!("Failed to prepare session namer at acceptance: {}", error);
        }
    }

    // Emit queue_changed if any tasks were set to Ready status
    if result.any_ready_tasks {
        let project_id = ProjectId::from_string(result.project_id.clone());
        emit_queue_changed(&state, &project_id, &app).await;

        let execution_state = app.state::<Arc<ExecutionState>>();
        spawn_ready_task_scheduler_if_needed(
            &state,
            Arc::clone(&*execution_state),
            Some(app.clone()),
            true,
        );
    }

    Ok(result.into())
}

/// Get blockers for a task (tasks it depends on)
#[tauri::command]
pub async fn get_task_blockers(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let task_id = TaskId::from_string(task_id);

    state
        .task_dependency_repo
        .get_blockers(&task_id)
        .await
        .map(|blockers| {
            blockers
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect()
        })
        .map_err(|e| e.to_string())
}

/// Get tasks blocked by a task (tasks that depend on this one)
#[tauri::command]
pub async fn get_blocked_tasks(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let task_id = TaskId::from_string(task_id);

    state
        .task_dependency_repo
        .get_blocked_by(&task_id)
        .await
        .map(|blocked| {
            blocked
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect()
        })
        .map_err(|e| e.to_string())
}
