//! Helper functions for HTTP server handlers
//!
//! Extracted from http_server.rs to manage file size and maintain separation of concerns.
//! Contains parsing, transformation, and context aggregation functions.

use std::path::PathBuf;
use std::str::FromStr;

use crate::application::{AppState, CreateProposalOptions, UpdateProposalOptions, UpdateSource};
use crate::application::git_service::GitService;
use crate::commands::ideation_commands::{apply_proposals_core, is_local_proposal, ApplyProposalsInput, TaskProposalResponse};
use crate::domain::services::{check_proposal_verification_gate, resolve_effective_gate_policy, ProposalOperation};
use crate::domain::entities::{
    AcceptanceStatus, Artifact, ArtifactContent, ArtifactSummary, ArtifactType, Complexity,
    IdeationSession, IdeationSessionId, IdeationSessionStatus, InternalStatus, Priority,
    ProposalCategory, ScopeDriftStatus, TaskContext, TaskId, TaskProposal,
    TaskProposalId, ValidationCacheData, ValidationCacheMetadata,
};
use crate::domain::review::{compute_out_of_scope_blocker_fingerprint, compute_scope_drift};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{
    SqliteArtifactRepository as ArtifactRepo, SqliteIdeationSessionRepository as SessionRepo,
    SqliteTaskProposalRepository as ProposalRepo,
};
use crate::infrastructure::sqlite::sqlite_ideation_settings_repo::get_settings_sync;
use ralphx_domain::repositories::IdeationSessionRepository;
use tauri::Emitter;

// ============================================================================
// Parsing Functions
// ============================================================================

/// Parse a category string to ProposalCategory enum
///
/// Accepts: "feature", "fix"/"bug", "refactor", "test"/"testing",
/// "docs"/"documentation", "setup"/"infrastructure"/"infra",
/// "performance"/"perf", "security"/"sec", "devops"/"dev_ops"/"ci_cd"/"cicd",
/// "research"/"investigation", "design", "chore"/"maintenance"
pub fn parse_category(s: &str) -> Result<ProposalCategory, String> {
    match s.to_lowercase().as_str() {
        "feature" => Ok(ProposalCategory::Feature),
        "fix" | "bug" => Ok(ProposalCategory::Fix),
        "refactor" => Ok(ProposalCategory::Refactor),
        "test" | "testing" => Ok(ProposalCategory::Test),
        "docs" | "documentation" => Ok(ProposalCategory::Docs),
        "setup" | "infrastructure" | "infra" => Ok(ProposalCategory::Setup),
        "performance" | "perf" => Ok(ProposalCategory::Performance),
        "security" | "sec" => Ok(ProposalCategory::Security),
        "devops" | "dev_ops" | "ci_cd" | "cicd" => Ok(ProposalCategory::DevOps),
        "research" | "investigation" => Ok(ProposalCategory::Research),
        "design" => Ok(ProposalCategory::Design),
        "chore" | "maintenance" => Ok(ProposalCategory::Chore),
        _ => Err(format!(
            "Invalid category: '{}'. Valid: setup, feature, fix, refactor, docs, test, performance, security, devops, research, design, chore",
            s
        )),
    }
}

/// Parse a priority string to Priority enum
///
/// Accepts: "critical"/"urgent", "high", "medium"/"med", "low"
pub fn parse_priority(s: &str) -> Result<Priority, String> {
    match s.to_lowercase().as_str() {
        "critical" | "urgent" => Ok(Priority::Critical),
        "high" => Ok(Priority::High),
        "medium" | "med" => Ok(Priority::Medium),
        "low" => Ok(Priority::Low),
        _ => Err(format!("Invalid priority: {}", s)),
    }
}

/// Parse an internal status string to InternalStatus enum
pub fn parse_internal_status(s: &str) -> Result<InternalStatus, String> {
    InternalStatus::from_str(s).map_err(|e| e.to_string())
}

/// Parse an artifact type string to ArtifactType enum
pub fn parse_artifact_type(s: &str) -> Result<ArtifactType, String> {
    match s.to_lowercase().as_str() {
        "prd" => Ok(ArtifactType::Prd),
        "specification" => Ok(ArtifactType::Specification),
        "research" | "researchdocument" | "research_document" => Ok(ArtifactType::ResearchDocument),
        "design" | "designdoc" | "design_doc" => Ok(ArtifactType::DesignDoc),
        "code_change" | "codechanges" => Ok(ArtifactType::CodeChange),
        "diff" => Ok(ArtifactType::Diff),
        "test_result" | "testresult" => Ok(ArtifactType::TestResult),
        "task_spec" | "taskspec" => Ok(ArtifactType::TaskSpec),
        "review_feedback" | "reviewfeedback" => Ok(ArtifactType::ReviewFeedback),
        "approval" => Ok(ArtifactType::Approval),
        "findings" => Ok(ArtifactType::Findings),
        "recommendations" => Ok(ArtifactType::Recommendations),
        "context" => Ok(ArtifactType::Context),
        "previous_work" | "previouswork" => Ok(ArtifactType::PreviousWork),
        "research_brief" | "researchbrief" => Ok(ArtifactType::ResearchBrief),
        _ => Err(format!("Invalid artifact type: {}", s)),
    }
}

fn normalize_affected_paths(raw: &str) -> Result<Vec<String>, AppError> {
    let paths: Vec<String> = serde_json::from_str(raw).map_err(|e| {
        AppError::Validation(format!("affected_paths must be a JSON array of strings: {e}"))
    })?;
    let normalized = paths
        .into_iter()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        return Err(AppError::Validation(
            "affected_paths must include at least one non-empty file path or directory prefix"
                .to_string(),
        ));
    }

    Ok(normalized)
}

fn validate_affected_paths_json(raw: Option<&String>) -> AppResult<()> {
    if let Some(raw) = raw {
        let _ = normalize_affected_paths(raw)?;
    }
    Ok(())
}

fn proposal_requires_affected_paths(category: ProposalCategory) -> bool {
    !matches!(category, ProposalCategory::Research | ProposalCategory::Design)
}

// ============================================================================
// Transformation Functions
// ============================================================================

/// Create a 500-character preview of artifact content
///
/// Truncates large artifacts with "..." suffix, preserves smaller artifacts in full.
pub fn create_artifact_preview(artifact: &Artifact) -> String {
    let full_content = match &artifact.content {
        ArtifactContent::Inline { text } => text.clone(),
        ArtifactContent::File { path } => {
            format!("[File artifact at: {}]", path)
        }
    };

    if full_content.chars().count() <= 500 {
        full_content
    } else {
        let truncated: String = full_content.chars().take(500).collect();
        format!("{truncated}...")
    }
}

// ============================================================================
// Session Guard
// ============================================================================

/// Assert that a session can be mutated (not Archived or Accepted).
///
/// Returns `Ok(())` for Active sessions.
/// Returns `AppError::Validation` for Archived/Accepted sessions, preventing
/// silent mutation of immutable sessions.
///
/// # Reference pattern
/// `create_task_proposal` (Tauri IPC) is the original protected handler.
pub fn assert_session_mutable(session: &IdeationSession) -> AppResult<()> {
    match session.status {
        IdeationSessionStatus::Archived | IdeationSessionStatus::Accepted => {
            Err(AppError::Validation(format!(
                "Cannot modify {} session. Reopen it first.",
                session.status
            )))
        }
        IdeationSessionStatus::Active => Ok(()),
    }
}

/// Emit a `dependency:added` event to the frontend.
///
/// Guards with `if let Some(handle) = &state.app_handle` so tests and HTTP-only
/// contexts don't fail. Payload matches `DependencyEventSchema` in `useIdeationEvents.ts`:
/// `{ proposalId: String, dependsOnId: String }`.
pub fn emit_dependency_added(state: &AppState, proposal_id: &str, depends_on_id: &str) {
    if let Some(app_handle) = &state.app_handle {
        let _ = app_handle.emit(
            "dependency:added",
            serde_json::json!({
                "proposalId": proposal_id,
                "dependsOnId": depends_on_id
            }),
        );
    }
}

// ============================================================================
// Proposal Implementation Functions
// ============================================================================

/// Create proposal — all checks and INSERT in a single DB transaction.
///
/// Session existence, active status, plan artifact requirement, and sort_order count
/// are verified inside `db.run_transaction()` to prevent TOCTOU races. Events and
/// dependency analysis are emitted after the transaction returns.
///
/// # Errors
/// - `AppError::NotFound` if session or plan artifact doesn't exist
/// - `AppError::Validation` if session is not active or has no plan artifact
/// - Database errors from the proposal repository
pub async fn create_proposal_impl(
    state: &AppState,
    session_id: IdeationSessionId,
    options: CreateProposalOptions,
) -> AppResult<(TaskProposal, Vec<String>, bool)> {
    validate_affected_paths_json(options.affected_paths.as_ref())?;
    let expected_proposal_count = options.expected_proposal_count;

    // Single lock: all checks + INSERT in one transaction (TOCTOU prevention).
    // Events emitted after db.run_transaction() returns (acceptable crash-consistency gap).
    let (proposal, new_count) = state
        .db
        .run_transaction(move |conn| {
            // Check session exists and is active
            let session = SessionRepo::get_by_id_sync(conn, session_id.as_str())?
                .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

            if session.status != IdeationSessionStatus::Active {
                return Err(AppError::Validation(format!(
                    "Cannot add proposal to {} session",
                    session.status
                )));
            }

            // Set-once gating: validate or lock expected_proposal_count
            if let Some(provided_count) = expected_proposal_count {
                match session.expected_proposal_count {
                    None => {
                        // First proposal: lock the expected count on this session
                        SessionRepo::set_expected_proposal_count_sync(
                            conn,
                            session_id.as_str(),
                            provided_count,
                        )?;
                    }
                    Some(stored_count) if stored_count != provided_count => {
                        return Err(AppError::Validation(format!(
                            "expected_proposal_count mismatch: session expects {}, got {}",
                            stored_count, provided_count
                        )));
                    }
                    Some(_) => {
                        // Matches stored value — ok to proceed
                    }
                }
            }

            // Cross-project gate: block proposal creation if plan has not been cross-project-checked
            if session.plan_artifact_id.is_some() && !session.cross_project_checked {
                return Err(AppError::Validation(
                    "Cross-project check required: call cross_project_guide before creating proposals"
                        .to_string(),
                ));
            }

            // Verification gate: block creation if plan hasn't been verified (when enabled)
            {
                let settings = get_settings_sync(conn)?;
                let policy = resolve_effective_gate_policy(&settings, session.origin);
                let parent_status = if session.plan_artifact_id.is_none()
                    && session.inherited_plan_artifact_id.is_some()
                {
                    session
                        .parent_session_id
                        .as_ref()
                        .and_then(|pid| {
                            SessionRepo::get_by_id_sync(conn, pid.as_str())
                                .ok()
                                .flatten()
                        })
                        .map(|p| p.verification_status)
                } else {
                    None
                };
                check_proposal_verification_gate(
                    &session,
                    &policy,
                    parent_status,
                    ProposalOperation::Create,
                )
                .map_err(AppError::from)?;
            }

            // Enforce plan artifact requirement
            let plan_artifact_id = session.plan_artifact_id.ok_or_else(|| {
                AppError::Validation(
                    "Proposals can only be created when a plan artifact exists for this session. \
                     Use create_plan_artifact first."
                        .to_string(),
                )
            })?;

            // Fetch artifact version for auto-linking
            let artifact = ArtifactRepo::get_by_id_sync(conn, plan_artifact_id.as_str())?
                .ok_or_else(|| {
                    AppError::NotFound(format!("Plan artifact {} not found", plan_artifact_id))
                })?;

            // Stale plan guard — ensure agent has read the current plan version
            if let Some(last_read) = session.plan_version_last_read {
                if (artifact.metadata.version as i32) > last_read {
                    return Err(AppError::Validation(format!(
                        "Plan has been updated since you last read it (current: v{}, last read: v{}). \
                         Call get_session_plan to read the latest plan before creating proposals.",
                        artifact.metadata.version, last_read
                    )));
                }
            }
            // NULL plan_version_last_read → legacy session, no gate (backward compat)

            // Count proposals for sort_order (within same lock — no TOCTOU)
            let count = ProposalRepo::count_by_session_sync(conn, session_id.as_str())?;

            // Build proposal with auto-linked plan artifact
            let mut proposal = TaskProposal::new(
                session_id,
                options.title,
                options.category,
                options.suggested_priority,
            );
            proposal.description = options.description;
            proposal.steps = options.steps;
            proposal.acceptance_criteria = options.acceptance_criteria;
            proposal.affected_paths = options.affected_paths;
            proposal.sort_order = count as i32;
            proposal.plan_version_at_creation = Some(artifact.metadata.version);
            proposal.plan_artifact_id = Some(plan_artifact_id);
            if let Some(complexity_str) = options.estimated_complexity {
                if let Ok(c) = complexity_str.parse::<Complexity>() {
                    proposal.estimated_complexity = c;
                }
            }
            proposal.target_project = options.target_project;

            let created = ProposalRepo::create_sync(conn, proposal)?;
            // Count active (non-archived) proposals after INSERT for expected-count comparison
            let new_count = SessionRepo::count_active_by_session_sync(conn, created.session_id.as_str())?;
            Ok((created, new_count))
        })
        .await?;

    // Emit event after transaction (acceptable crash-consistency gap)
    if let Some(app_handle) = &state.app_handle {
        let response = TaskProposalResponse::from(proposal.clone());
        let _ = app_handle.emit(
            "proposal:created",
            serde_json::json!({ "proposal": response }),
        );
    }


    // Process depends_on deps in separate db.run() calls (AD5: deadlock avoidance)
    // Each dep: validate session membership + cycle check + insert + emit
    let mut dep_errors: Vec<String> = Vec::new();
    let had_depends_on = !options.depends_on.is_empty();

    for dep_id_str in options.depends_on {
        let dep_id = TaskProposalId::from_string(dep_id_str.clone());
        let proposal_id_clone = proposal.id.clone();
        let session_id_clone = proposal.session_id.clone();

        // Validate: dep proposal exists and belongs to same session
        let dep_proposal = match state.task_proposal_repo.get_by_id(&dep_id).await {
            Err(e) => {
                dep_errors.push(format!("Dep on {} rejected: {}", dep_id.as_str(), e));
                continue;
            }
            Ok(None) => {
                dep_errors.push(format!("Dep on {} rejected: proposal not found", dep_id.as_str()));
                continue;
            }
            Ok(Some(p)) => p,
        };

        // Session membership check
        if dep_proposal.session_id != session_id_clone {
            dep_errors.push(format!("Dep on {} rejected: not in same session", dep_id.as_str()));
            continue;
        }
        // Self-dependency check
        if dep_proposal.id == proposal_id_clone {
            dep_errors.push(format!("Dep on {} rejected: self-dependency not allowed", dep_id.as_str()));
            continue;
        }

        // Cycle check
        match state
            .proposal_dependency_repo
            .would_create_cycle(&proposal_id_clone, &dep_id)
            .await
        {
            Err(e) => {
                dep_errors.push(format!("Dep on {} rejected: cycle check failed: {}", dep_id.as_str(), e));
                continue;
            }
            Ok(true) => {
                dep_errors.push(format!("Dep on {} rejected: would create cycle", dep_id.as_str()));
                continue;
            }
            Ok(false) => {}
        }

        // Insert dep with source="agent"
        match state
            .proposal_dependency_repo
            .add_dependency(&proposal_id_clone, &dep_id, None, Some("agent"))
            .await
        {
            Err(e) => {
                dep_errors.push(format!("Dep on {} rejected: insert failed: {}", dep_id.as_str(), e));
                continue;
            }
            Ok(_) => {
                emit_dependency_added(state, proposal_id_clone.as_str(), dep_id.as_str());
            }
        }
    }

    // Set dependencies_acknowledged if agent specified deps at creation
    if had_depends_on {
        if let Err(e) = state
            .ideation_session_repo
            .set_dependencies_acknowledged(proposal.session_id.as_str())
            .await
        {
            tracing::warn!(
                "Failed to set dependencies_acknowledged for session {}: {}",
                proposal.session_id.as_str(),
                e
            );
        }
    }

    // Signal to the caller whether the session is ready to finalize (expected count reached).
    // The caller is responsible for invoking finalize_proposals explicitly.
    let ready_to_finalize = if let Some(expected) = options.expected_proposal_count {
        new_count == expected as i64
    } else {
        false
    };

    Ok((proposal, dep_errors, ready_to_finalize))
}

/// Update proposal — fetch, validate, and UPDATE in a single DB transaction.
///
/// `assert_session_mutable()` is called inside the transaction (bug fix: IPC update
/// path was previously missing this guard). When `options.source == TauriIpc`, sets
/// `user_modified = true` per changed field and calls `proposal.touch()`. Events and
/// dependency analysis are emitted after the transaction returns.
///
/// # Errors
/// - `AppError::NotFound` if proposal or session doesn't exist
/// - `AppError::Validation` if session is Archived or Accepted
/// - Database errors from the proposal repository
pub async fn update_proposal_impl(
    state: &AppState,
    proposal_id: &TaskProposalId,
    options: UpdateProposalOptions,
) -> AppResult<(TaskProposal, Vec<String>)> {
    if let Some(raw) = options.affected_paths.as_ref().and_then(|value| value.as_ref()) {
        validate_affected_paths_json(Some(raw))?;
    }
    let pid = proposal_id.as_str().to_string();

    // Single lock: fetch + validate + UPDATE in one transaction.
    // Events emitted after db.run_transaction() returns (acceptable crash-consistency gap).
    let updated = state
        .db
        .run_transaction(move |conn| {
            // Fetch proposal
            let mut proposal = conn
                .query_row(
                    "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                            suggested_priority, priority_score, priority_reason, priority_factors,
                            estimated_complexity, user_priority, user_modified, status, selected,
                            created_task_id, plan_artifact_id, plan_version_at_creation, sort_order, created_at, updated_at, archived_at,
                            target_project, affected_paths
                     FROM task_proposals WHERE id = ?1",
                    [&pid],
                    |row| TaskProposal::from_row(row),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        AppError::NotFound(format!("Proposal {} not found", pid))
                    }
                    other => AppError::from(other),
                })?;

            // Guard: reject mutations on Archived/Accepted sessions (bug fix: IPC update was ungated)
            let session =
                SessionRepo::get_by_id_sync(conn, proposal.session_id.as_str())?.ok_or_else(
                    || AppError::NotFound(format!("Session {} not found", proposal.session_id)),
                )?;
            assert_session_mutable(&session)?;

            // Verification gate: block update if verification in progress or needs revision
            {
                let settings = get_settings_sync(conn)?;
                let policy = resolve_effective_gate_policy(&settings, session.origin);
                let parent_status = if session.plan_artifact_id.is_none()
                    && session.inherited_plan_artifact_id.is_some()
                {
                    session
                        .parent_session_id
                        .as_ref()
                        .and_then(|pid| {
                            SessionRepo::get_by_id_sync(conn, pid.as_str())
                                .ok()
                                .flatten()
                        })
                        .map(|p| p.verification_status)
                } else {
                    None
                };
                check_proposal_verification_gate(
                    &session,
                    &policy,
                    parent_status,
                    ProposalOperation::Update,
                )
                .map_err(AppError::from)?;
            }

            let is_ipc = matches!(options.source, UpdateSource::TauriIpc);

            // Apply updates; track user_modified per field when source is TauriIpc
            if let Some(title) = options.title {
                proposal.title = title;
                if is_ipc {
                    proposal.user_modified = true;
                }
            }
            if let Some(description) = options.description {
                proposal.description = description;
                if is_ipc {
                    proposal.user_modified = true;
                }
            }
            if let Some(category) = options.category {
                proposal.category = category;
                if is_ipc {
                    proposal.user_modified = true;
                }
            }
            if let Some(steps) = options.steps {
                proposal.steps = steps;
                if is_ipc {
                    proposal.user_modified = true;
                }
            }
            if let Some(acceptance_criteria) = options.acceptance_criteria {
                proposal.acceptance_criteria = acceptance_criteria;
                if is_ipc {
                    proposal.user_modified = true;
                }
            }
            if let Some(affected_paths) = options.affected_paths {
                proposal.affected_paths = affected_paths;
                if is_ipc {
                    proposal.user_modified = true;
                }
            }
            if let Some(priority) = options.user_priority {
                proposal.user_priority = Some(priority);
                if is_ipc {
                    proposal.user_modified = true;
                }
            }
            if let Some(complexity_str) = options.estimated_complexity {
                if let Ok(complexity) = complexity_str.parse::<Complexity>() {
                    proposal.estimated_complexity = complexity;
                    if is_ipc {
                        proposal.user_modified = true;
                    }
                }
            }
            if let Some(target_project) = options.target_project {
                proposal.target_project = target_project;
            }

            // Touch timestamp when user-originated (matches IPC command behaviour)
            if is_ipc {
                proposal.touch();
            }

            ProposalRepo::update_sync(conn, &proposal)
        })
        .await?;

    // Emit event after transaction (acceptable crash-consistency gap)
    if let Some(app_handle) = &state.app_handle {
        let response = TaskProposalResponse::from(updated.clone());
        let _ = app_handle.emit(
            "proposal:updated",
            serde_json::json!({ "proposal": response }),
        );
    }


    // Process add_depends_on and add_blocks deps in separate db.run() calls (AD5: deadlock avoidance)
    let mut dep_errors: Vec<String> = Vec::new();
    let had_dep_changes = !options.add_depends_on.is_empty() || !options.add_blocks.is_empty();
    let proposal_id_for_deps = updated.id.clone();
    let session_id_for_deps = updated.session_id.clone();

    // Process add_depends_on (A depends on each target)
    for dep_id_str in options.add_depends_on {
        let dep_id = TaskProposalId::from_string(dep_id_str.clone());
        let pid = proposal_id_for_deps.clone();
        let sid = session_id_for_deps.clone();

        let dep_proposal = match state.task_proposal_repo.get_by_id(&dep_id).await {
            Err(e) => { dep_errors.push(format!("add_depends_on {} rejected: {}", dep_id.as_str(), e)); continue; }
            Ok(None) => { dep_errors.push(format!("add_depends_on {} rejected: proposal not found", dep_id.as_str())); continue; }
            Ok(Some(p)) => p,
        };

        if dep_proposal.session_id != sid {
            dep_errors.push(format!("add_depends_on {} rejected: not in same session", dep_id.as_str())); continue;
        }
        if dep_proposal.id == pid {
            dep_errors.push(format!("add_depends_on {} rejected: self-dependency", dep_id.as_str())); continue;
        }

        match state.proposal_dependency_repo.would_create_cycle(&pid, &dep_id).await {
            Err(e) => { dep_errors.push(format!("add_depends_on {} rejected: cycle check failed: {}", dep_id.as_str(), e)); continue; }
            Ok(true) => { dep_errors.push(format!("add_depends_on {} rejected: would create cycle", dep_id.as_str())); continue; }
            Ok(false) => {}
        }

        match state.proposal_dependency_repo.add_dependency(&pid, &dep_id, None, Some("agent")).await {
            Err(e) => { dep_errors.push(format!("add_depends_on {} rejected: insert failed: {}", dep_id.as_str(), e)); continue; }
            Ok(_) => { emit_dependency_added(state, pid.as_str(), dep_id.as_str()); }
        }
    }

    // Process add_blocks (each target depends on A — reversed direction)
    for blocker_id_str in options.add_blocks {
        let blocker_id = TaskProposalId::from_string(blocker_id_str.clone());
        let pid = proposal_id_for_deps.clone();
        let sid = session_id_for_deps.clone();

        let dep_proposal = match state.task_proposal_repo.get_by_id(&blocker_id).await {
            Err(e) => { dep_errors.push(format!("add_blocks {} rejected: {}", blocker_id.as_str(), e)); continue; }
            Ok(None) => { dep_errors.push(format!("add_blocks {} rejected: proposal not found", blocker_id.as_str())); continue; }
            Ok(Some(p)) => p,
        };

        if dep_proposal.session_id != sid {
            dep_errors.push(format!("add_blocks {} rejected: not in same session", blocker_id.as_str())); continue;
        }
        if dep_proposal.id == pid {
            dep_errors.push(format!("add_blocks {} rejected: self-dependency", blocker_id.as_str())); continue;
        }

        // For add_blocks: blocker depends on pid, so cycle check is would_create_cycle(blocker, pid)
        match state.proposal_dependency_repo.would_create_cycle(&blocker_id, &pid).await {
            Err(e) => { dep_errors.push(format!("add_blocks {} rejected: cycle check failed: {}", blocker_id.as_str(), e)); continue; }
            Ok(true) => { dep_errors.push(format!("add_blocks {} rejected: would create cycle", blocker_id.as_str())); continue; }
            Ok(false) => {}
        }

        // Insert: blocker depends on pid (reversed)
        match state.proposal_dependency_repo.add_dependency(&blocker_id, &pid, None, Some("agent")).await {
            Err(e) => { dep_errors.push(format!("add_blocks {} rejected: insert failed: {}", blocker_id.as_str(), e)); continue; }
            Ok(_) => { emit_dependency_added(state, blocker_id.as_str(), pid.as_str()); }
        }
    }

    // Set dependencies_acknowledged if agent set deps via update
    if had_dep_changes {
        if let Err(e) = state
            .ideation_session_repo
            .set_dependencies_acknowledged(updated.session_id.as_str())
            .await
        {
            tracing::warn!(
                "Failed to set dependencies_acknowledged for session {}: {}",
                updated.session_id.as_str(),
                e
            );
        }
    }

    Ok((updated, dep_errors))
}

/// Archive proposal — fetch session, assert mutability, and ARCHIVE in a single DB transaction.
///
/// Fixes existing bug: HTTP delete handler had no `assert_session_mutable()` guard, allowing
/// MCP agents to archive proposals from Archived/Accepted sessions.
///
/// # Errors
/// - `AppError::NotFound` if proposal or session doesn't exist
/// - `AppError::Validation` if session is Archived or Accepted
/// - Database errors from the proposal repository
pub async fn archive_proposal_impl(
    state: &AppState,
    proposal_id: TaskProposalId,
) -> AppResult<IdeationSessionId> {
    let pid = proposal_id.as_str().to_string();

    // Single lock: fetch proposal+session, assert mutability, ARCHIVE — all in one transaction.
    // Events emitted after db.run_transaction() returns (acceptable crash-consistency gap).
    let session_id = state
        .db
        .run_transaction(move |conn| {
            // Fetch session_id from proposal
            let session_id_str: String = match conn.query_row(
                "SELECT session_id FROM task_proposals WHERE id = ?1",
                [&pid],
                |row| row.get(0),
            ) {
                Ok(s) => s,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    return Err(AppError::NotFound(format!("Proposal {} not found", pid)));
                }
                Err(e) => return Err(AppError::from(e)),
            };

            let session_id = IdeationSessionId::from_string(session_id_str);

            // Guard: reject mutations on Archived/Accepted sessions (bug fix: HTTP delete was ungated)
            let session =
                SessionRepo::get_by_id_sync(conn, session_id.as_str())?.ok_or_else(|| {
                    AppError::NotFound(format!("Session {} not found", session_id))
                })?;
            assert_session_mutable(&session)?;

            // Verification gate: block delete if verification in progress or needs revision
            {
                let settings = get_settings_sync(conn)?;
                let policy = resolve_effective_gate_policy(&settings, session.origin);
                let parent_status = if session.plan_artifact_id.is_none()
                    && session.inherited_plan_artifact_id.is_some()
                {
                    session
                        .parent_session_id
                        .as_ref()
                        .and_then(|pid| {
                            SessionRepo::get_by_id_sync(conn, pid.as_str())
                                .ok()
                                .flatten()
                        })
                        .map(|p| p.verification_status)
                } else {
                    None
                };
                check_proposal_verification_gate(
                    &session,
                    &policy,
                    parent_status,
                    ProposalOperation::Delete,
                )
                .map_err(AppError::from)?;
            }

            // Archive proposal scoped to session (prevents cross-session deletions)
            let proposal_id_typed = TaskProposalId::from_string(pid.clone());
            conn.execute(
                "DELETE FROM proposal_dependencies
                 WHERE proposal_id = ?1 OR depends_on_proposal_id = ?1",
                rusqlite::params![proposal_id_typed.as_str()],
            )?;
            ProposalRepo::archive_sync(conn, &proposal_id_typed)?;

            Ok(session_id)
        })
        .await?;

    // Emit event after transaction (acceptable crash-consistency gap)
    if let Some(app_handle) = &state.app_handle {
        let _ = app_handle.emit(
            "proposal:archived",
            serde_json::json!({ "proposalId": proposal_id.as_str() }),
        );
    }


    Ok(session_id)
}

/// Finalize proposals — synchronously apply all active proposals for a session.
///
/// Called explicitly by the agent after all proposals and dependencies have been set.
/// Validates session is Active and proposal count matches `expected_proposal_count`,
/// then calls `apply_proposals_core` synchronously and returns the result.
///
/// # Errors
/// - `AppError::NotFound` if session doesn't exist
/// - `AppError::Validation` if session is not Active or count mismatch
/// - Errors from `apply_proposals_core`
pub async fn finalize_proposals_impl(
    state: &AppState,
    session_id: &str,
    _is_external: bool,
) -> AppResult<crate::http_server::types::FinalizeProposalsResponse> {
    // Fetch session and validate it is Active
    let session_id_typed = IdeationSessionId::from_string(session_id.to_string());
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    if session.status != IdeationSessionStatus::Active {
        return Err(AppError::Validation(format!(
            "Cannot finalize proposals for {} session",
            session.status
        )));
    }

    // Fetch project to get working_directory for local/foreign classification
    let project = state
        .project_repo
        .get_by_id(&session.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Project {} not found", session.project_id)))?;

    // Fetch active (non-archived) proposals
    let all_proposals = state
        .task_proposal_repo
        .get_by_session(&session_id_typed)
        .await?;
    let active_proposals: Vec<_> = all_proposals
        .into_iter()
        .filter(|p| p.archived_at.is_none())
        .collect();

    // Partition into local vs foreign proposals
    let project_dir = std::fs::canonicalize(&project.working_directory)
        .unwrap_or_else(|_| PathBuf::from(&project.working_directory));

    let (local_proposals, foreign_proposals): (Vec<_>, Vec<_>) = active_proposals
        .into_iter()
        .partition(|p| is_local_proposal(p, &project_dir));

    let count_local = local_proposals.len() as u32;
    let count_foreign = foreign_proposals.len() as u32;
    let count_total = count_local + count_foreign;

    let proposals_missing_scope = local_proposals
        .iter()
        .filter(|proposal| proposal_requires_affected_paths(proposal.category))
        .filter_map(|proposal| match proposal.affected_paths.as_ref() {
            Some(raw) => match normalize_affected_paths(raw) {
                Ok(_) => None,
                Err(_) => Some(proposal.title.clone()),
            },
            None => Some(proposal.title.clone()),
        })
        .collect::<Vec<_>>();

    // Validate count matches expected_proposal_count against TOTAL (local + foreign)
    if let Some(expected) = session.expected_proposal_count {
        if count_total != expected {
            return Err(AppError::Validation(format!(
                "Proposal count mismatch: session expects {}, found {} ({} local + {} foreign)",
                expected, count_total, count_local, count_foreign
            )));
        }
    }

    if !proposals_missing_scope.is_empty() {
        return Err(AppError::Validation(format!(
            "Cannot finalize proposals until every implementation-scoped local proposal declares coarse affected_paths. Missing scope for: {}. Update the proposal(s) with repo-relative file paths or directory prefixes, then retry finalize. Pure research/design proposals may omit affected_paths when no credible repo-change scope exists.",
            proposals_missing_scope.join(", ")
        )));
    }

    // ─── Acceptance Gate ───────────────────────────────────────────────────────
    // Resolve effective policy from (settings, session.origin) — external overrides
    // may change whether require_accept_for_finalize applies for this session.
    // Fail-safe-closed: return error on settings fetch failure to prevent silent bypass.
    {
        let ideation_settings = state
            .ideation_settings_repo
            .get_settings()
            .await
            .map_err(|e| AppError::Database(format!("Failed to fetch ideation settings for acceptance gate: {}", e)))?;
        let effective_policy = resolve_effective_gate_policy(&ideation_settings, session.origin);
        if effective_policy.require_accept_for_finalize {
            // Set acceptance_status to Pending (CAS: only if currently None)
            state
                .ideation_session_repo
                .update_acceptance_status(&session_id_typed, None, Some(AcceptanceStatus::Pending))
                .await?;

            // Emit Tauri event to notify frontend
            if let Some(ref handle) = state.app_handle {
                let _ = handle.emit(
                    "ideation:finalize_pending_confirmation",
                    serde_json::json!({
                        "sessionId": session_id,
                        "sessionTitle": session.title,
                    }),
                );
            }

            return Ok(crate::http_server::types::FinalizeProposalsResponse {
                created_task_ids: vec![],
                dependencies_created: 0,
                tasks_created: 0,
                message: Some("Waiting for user confirmation to apply proposals".to_string()),
                session_status: "active".to_string(),
                execution_plan_id: None,
                warnings: vec![],
                project_id: session.project_id.to_string(),
                skipped_foreign_count: count_foreign,
                any_ready_tasks: false,
                status: "pending_acceptance".to_string(),
                session_title: session.title.clone(),
                project_name: Some(project.name.clone()),
            });
        }
    }
    // ─── End Acceptance Gate ────────────────────────────────────────────────────

    // Short-circuit if no local proposals — all have been migrated to foreign projects
    if count_local == 0 {
        // All proposals are foreign (migrated) — transition session to Accepted
        state
            .ideation_session_repo
            .update_status(&session_id_typed, IdeationSessionStatus::Accepted)
            .await?;
        return Ok(crate::http_server::types::FinalizeProposalsResponse {
            created_task_ids: vec![],
            dependencies_created: 0,
            tasks_created: 0,
            message: Some(format!(
                "No local proposals to finalize ({} foreign skipped)",
                count_foreign
            )),
            session_status: "accepted".to_string(),
            execution_plan_id: None,
            warnings: vec![],
            project_id: session.project_id.to_string(),
            skipped_foreign_count: count_foreign,
            any_ready_tasks: false,
            status: "success".to_string(),
            session_title: session.title.clone(),
            project_name: Some(project.name.clone()),
        });
    }

    let proposal_ids: Vec<String> = local_proposals
        .into_iter()
        .map(|p| p.id.as_str().to_string())
        .collect();

    let input = ApplyProposalsInput {
        session_id: session_id.to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = apply_proposals_core(state, input).await?;

    let session_status = if result.session_converted {
        "accepted".to_string()
    } else {
        "active".to_string()
    };

    Ok(crate::http_server::types::FinalizeProposalsResponse {
        created_task_ids: result.created_task_ids,
        dependencies_created: result.dependencies_created as u32,
        tasks_created: result.tasks_created as u32,
        message: result.message,
        session_status,
        execution_plan_id: result.execution_plan_id,
        warnings: result.warnings,
        project_id: result.project_id,
        skipped_foreign_count: count_foreign,
        any_ready_tasks: result.any_ready_tasks,
        status: "success".to_string(),
        session_title: session.title.clone(),
        project_name: Some(project.name.clone()),
    })
}

/// Apply proposals core for an already-validated session.
///
/// Used by `accept_finalize` to execute the finalization after user confirmation.
/// Looks up the session and its active local proposals, then calls `apply_proposals_core`.
///
/// # Errors
/// - `AppError::NotFound` if session or project not found
/// - Errors from `apply_proposals_core`
pub async fn apply_proposals_core_for_session(
    state: &AppState,
    session_id: &str,
) -> AppResult<crate::commands::ideation_commands::ApplyProposalsResult> {
    let session_id_typed = IdeationSessionId::from_string(session_id.to_string());

    let session = state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    let project = state
        .project_repo
        .get_by_id(&session.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Project {} not found", session.project_id)))?;

    let all_proposals = state
        .task_proposal_repo
        .get_by_session(&session_id_typed)
        .await?;
    let active_proposals: Vec<_> = all_proposals
        .into_iter()
        .filter(|p| p.archived_at.is_none())
        .collect();

    let project_dir = std::fs::canonicalize(&project.working_directory)
        .unwrap_or_else(|_| PathBuf::from(&project.working_directory));

    let proposal_ids: Vec<String> = active_proposals
        .into_iter()
        .filter(|p| is_local_proposal(p, &project_dir))
        .map(|p| p.id.as_str().to_string())
        .collect();

    let input = ApplyProposalsInput {
        session_id: session_id.to_string(),
        proposal_ids,
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    apply_proposals_core(state, input).await
}

// ============================================================================
// Task Context Function
// ============================================================================

/// Get task context - implementation that manually aggregates context
///
/// Replicates the logic from TaskContextService but works with trait objects.
/// Fetches task, associated proposal, plan artifact, related artifacts, and steps.
/// Generates context hints based on available data.
///
/// # Errors
/// - `AppError::NotFound` if task doesn't exist
/// - Database errors from any repository
pub async fn get_task_context_impl(state: &AppState, task_id: &TaskId) -> AppResult<TaskContext> {
    // 1. Fetch task by ID
    let task = state
        .task_repo
        .get_by_id(task_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Task not found: {}", task_id)))?;

    // 2. If source_proposal_id present, fetch proposal and create TaskProposalSummary
    let source_proposal = if let Some(proposal_id) = &task.source_proposal_id {
        match state.task_proposal_repo.get_by_id(proposal_id).await? {
            Some(proposal) => {
                // Parse acceptance_criteria from JSON string to Vec<String>
                let acceptance_criteria: Vec<String> = proposal
                    .acceptance_criteria
                    .as_ref()
                    .and_then(|json_str| serde_json::from_str(json_str).ok())
                    .unwrap_or_default();

                Some(crate::domain::entities::TaskProposalSummary {
                    id: proposal.id.clone(),
                    title: proposal.title.clone(),
                    description: proposal.description.clone().unwrap_or_default(),
                    acceptance_criteria,
                    implementation_notes: None,
                    plan_version_at_creation: proposal.plan_version_at_creation,
                    priority_score: proposal.priority_score,
                    affected_paths: proposal
                        .affected_paths
                        .as_ref()
                        .and_then(|json_str| serde_json::from_str(json_str).ok())
                        .unwrap_or_default(),
                })
            }
            None => None,
        }
    } else {
        None
    };

    // 3. If plan_artifact_id present, fetch artifact and create ArtifactSummary
    let plan_artifact = if let Some(artifact_id) = &task.plan_artifact_id {
        match state.artifact_repo.get_by_id(artifact_id).await? {
            Some(artifact) => {
                let content_preview = create_artifact_preview(&artifact);
                Some(ArtifactSummary {
                    id: artifact.id.clone(),
                    title: artifact.name.clone(),
                    artifact_type: artifact.artifact_type,
                    current_version: artifact.metadata.version,
                    content_preview,
                })
            }
            None => None,
        }
    } else {
        None
    };

    // 4. Fetch related artifacts
    let related_artifacts = if let Some(artifact_id) = &task.plan_artifact_id {
        let related = state.artifact_repo.get_related(artifact_id).await?;
        related
            .into_iter()
            .map(|artifact| {
                let content_preview = create_artifact_preview(&artifact);
                ArtifactSummary {
                    id: artifact.id.clone(),
                    title: artifact.name.clone(),
                    artifact_type: artifact.artifact_type,
                    current_version: artifact.metadata.version,
                    content_preview,
                }
            })
            .collect()
    } else {
        vec![]
    };

    let (actual_changed_files, scope_drift_status, out_of_scope_files) =
        compute_task_scope_drift(state, &task, source_proposal.as_ref()).await?;

    // 5. Fetch steps for the task
    let steps = state.task_step_repo.get_by_task(task_id).await?;

    // 6. Calculate step progress summary if steps exist
    let step_progress = if !steps.is_empty() {
        Some(crate::domain::entities::StepProgressSummary::from_steps(
            task_id, &steps,
        ))
    } else {
        None
    };

    // 7. Fetch task dependencies (blockers and dependents) via TaskDependencyRepository
    let blocker_ids = state.task_dependency_repo.get_blockers(task_id).await?;
    let mut blocked_by: Vec<crate::domain::entities::TaskDependencySummary> = Vec::new();
    for blocker_id in &blocker_ids {
        if let Some(blocker_task) = state.task_repo.get_by_id(blocker_id).await? {
            if !blocker_task
                .internal_status
                .is_active_dependency_blocker()
            {
                continue;
            }
            blocked_by.push(crate::domain::entities::TaskDependencySummary {
                id: blocker_task.id.clone(),
                title: blocker_task.title.clone(),
                internal_status: blocker_task.internal_status,
            });
        }
    }

    let dependent_ids = state.task_dependency_repo.get_blocked_by(task_id).await?;
    let mut blocks: Vec<crate::domain::entities::TaskDependencySummary> = Vec::new();
    for dep_id in &dependent_ids {
        if let Some(dep_task) = state.task_repo.get_by_id(dep_id).await? {
            blocks.push(crate::domain::entities::TaskDependencySummary {
                id: dep_task.id.clone(),
                title: dep_task.title.clone(),
                internal_status: dep_task.internal_status,
            });
        }
    }

    // 8. Compute tier from dependency depth
    let tier = if blocked_by.is_empty() {
        Some(1)
    } else {
        Some((blocked_by.len() as u32) + 1)
    };

    // 9. Generate context hints
    let mut context_hints = Vec::new();

    // CRITICAL: Dependency hints come first
    if !blocked_by.is_empty() {
        let incomplete: Vec<_> = blocked_by
            .iter()
            .filter(|b| b.internal_status.is_active_dependency_blocker())
            .collect();
        if !incomplete.is_empty() {
            let names: Vec<_> = incomplete.iter().map(|t| t.title.as_str()).collect();
            context_hints.push(format!(
                "BLOCKED: Task cannot proceed - waiting for: {}",
                names.join(", ")
            ));
        } else {
            context_hints.push("All blocking tasks completed - ready to execute".to_string());
        }
    }

    if !blocks.is_empty() {
        let names: Vec<_> = blocks.iter().map(|t| t.title.as_str()).collect();
        context_hints.push(format!(
            "Downstream impact: completing this task unblocks: {}",
            names.join(", ")
        ));
    }

    // CRITICAL: Branch safety hint — agents must stay on their assigned branch
    if let Some(ref branch) = task.task_branch {
        context_hints.push(format!(
            "GIT BRANCH: You are on branch '{}'. Do NOT checkout other branches (especially main/master). All work must stay on this branch.",
            branch
        ));
    }

    if source_proposal.is_some() {
        context_hints.push(
            "Task was created from ideation proposal - check acceptance criteria".to_string(),
        );
        if let Some(proposal) = &source_proposal {
            if !proposal.affected_paths.is_empty() {
                context_hints.push(format!(
                    "PLANNED SCOPE: proposal expects work under {}",
                    proposal.affected_paths.join(", ")
                ));
            }
        }
    }
    if plan_artifact.is_some() {
        context_hints.push(
            "Implementation plan available - use get_artifact to read, then extract ONLY the section relevant to YOUR task"
                .to_string(),
        );
        context_hints.push(format!(
            "SCOPE: The plan contains multiple tasks. Execute ONLY work for: \"{}\". Other tasks have their own workers.",
            task.title
        ));
    }
    if !related_artifacts.is_empty() {
        context_hints.push(format!(
            "{} related artifact{} found - may contain useful context",
            related_artifacts.len(),
            if related_artifacts.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if !steps.is_empty() {
        context_hints.push(format!(
            "Task has {} step{} defined - use get_task_steps to see them",
            steps.len(),
            if steps.len() == 1 { "" } else { "s" }
        ));
    }
    if matches!(scope_drift_status, ScopeDriftStatus::ScopeExpansion) {
        context_hints.push(format!(
            "SCOPE DRIFT DETECTED: changed files outside planned scope: {}",
            out_of_scope_files.join(", ")
        ));
    }
    if task.description.is_some() {
        context_hints.push("Task has description with additional details".to_string());
    }

    // Surface restart_note from task metadata (one-shot, cleared after agent reads in on_enter_states)
    if let Some(ref metadata_str) = task.metadata {
        if let Ok(meta) = serde_json::from_str::<serde_json::Value>(metadata_str) {
            if let Some(note) = meta.get("restart_note").and_then(|v| v.as_str()) {
                context_hints.push(format!("RESTART NOTE from user: {}", note));
            }
        }
    }

    if context_hints.is_empty() {
        context_hints.push("No additional context artifacts found - proceed with task description and acceptance criteria".to_string());
    }

    // 10. Compute validation cache hint (if cache present in metadata)
    let validation_cache = compute_validation_cache(&task, &mut context_hints).await;
    let out_of_scope_blocker_fingerprint =
        compute_out_of_scope_blocker_fingerprint(&task.id, &out_of_scope_files);
    let followup_sessions = load_task_followup_sessions(state, &task).await?;

    // 11. Return TaskContext
    let task_branch = task.task_branch.clone();
    let worktree_path = task.worktree_path.clone();
    Ok(TaskContext {
        task,
        source_proposal,
        plan_artifact,
        related_artifacts,
        steps,
        step_progress,
        context_hints,
        blocked_by,
        blocks,
        tier,
        task_branch,
        worktree_path,
        validation_cache,
        actual_changed_files,
        scope_drift_status,
        out_of_scope_files,
        out_of_scope_blocker_fingerprint,
        followup_sessions,
    })
}

async fn load_task_followup_sessions(
    state: &AppState,
    task: &crate::domain::entities::Task,
) -> AppResult<Vec<crate::domain::entities::FollowupSessionSummary>> {
    let Some(session_id) = &task.ideation_session_id else {
        return Ok(Vec::new());
    };

    let children = state.ideation_session_repo.get_children(session_id).await?;
    Ok(children
        .into_iter()
        .filter(|session| session.source_task_id.as_ref() == Some(&task.id))
        .map(|session| crate::domain::entities::FollowupSessionSummary {
            id: session.id.as_str().to_string(),
            title: session.title.clone(),
            status: session.status.to_string(),
            source_context_type: session.source_context_type.clone(),
            spawn_reason: session.spawn_reason.clone(),
            blocker_fingerprint: session.blocker_fingerprint.clone(),
        })
        .collect())
}

async fn compute_task_scope_drift(
    state: &AppState,
    task: &crate::domain::entities::Task,
    source_proposal: Option<&crate::domain::entities::TaskProposalSummary>,
) -> AppResult<(Vec<String>, ScopeDriftStatus, Vec<String>)> {
    let Some(proposal) = source_proposal else {
        return Ok((Vec::new(), ScopeDriftStatus::Unbounded, Vec::new()));
    };
    if proposal.affected_paths.is_empty() {
        return Ok((Vec::new(), ScopeDriftStatus::Unbounded, Vec::new()));
    }

    let Some(repo_path) = resolve_task_context_repo_path(state, task).await? else {
        return Ok((Vec::new(), ScopeDriftStatus::Unbounded, Vec::new()));
    };

    if let Some(expected_branch) = &task.task_branch {
        let current_branch = GitService::get_current_branch(&repo_path).await?;
        if current_branch != *expected_branch {
            return Ok((Vec::new(), ScopeDriftStatus::Unbounded, Vec::new()));
        }
    }

    let Some(base_branch) = resolve_task_context_base_branch(state, task).await? else {
        return Ok((Vec::new(), ScopeDriftStatus::Unbounded, Vec::new()));
    };

    let diff = GitService::get_diff_stats(&repo_path, &base_branch).await?;
    let changed_files = diff.changed_files;
    let (status, out_of_scope_files) =
        compute_scope_drift(&changed_files, &proposal.affected_paths);
    Ok((changed_files, status, out_of_scope_files))
}

async fn resolve_task_context_repo_path(
    state: &AppState,
    task: &crate::domain::entities::Task,
) -> AppResult<Option<PathBuf>> {
    if let Some(path) = &task.worktree_path {
        return Ok(Some(PathBuf::from(path)));
    }

    Ok(state
        .project_repo
        .get_by_id(&task.project_id)
        .await?
        .map(|project| PathBuf::from(project.working_directory)))
}

async fn resolve_task_context_base_branch(
    state: &AppState,
    task: &crate::domain::entities::Task,
) -> AppResult<Option<String>> {
    if let Some(exec_plan_id) = &task.execution_plan_id {
        if let Some(plan_branch) = state
            .plan_branch_repo
            .get_by_execution_plan_id(exec_plan_id)
            .await?
        {
            return Ok(Some(plan_branch.branch_name));
        }
    }

    if let Some(session_id) = &task.ideation_session_id {
        if let Some(plan_branch) = state.plan_branch_repo.get_by_session_id(session_id).await? {
            return Ok(Some(plan_branch.branch_name));
        }
    }

    Ok(state
        .project_repo
        .get_by_id(&task.project_id)
        .await?
        .map(|project| project.base_branch_or_default().to_string()))
}

/// Pure function to compute the validation hint and human-readable message from a cache entry
/// and the current HEAD SHA.
///
/// No git calls, no side effects — fully unit-testable.
///
/// Returns `(validation_hint, hint_message)` where `validation_hint` is one of:
/// - `"skip_tests"` — tests passed on the same SHA
/// - `"skip_test_validation"` — no tests ran at execution time
/// - `"run_tests"` — SHA mismatch, tests failed, or cache is otherwise stale
pub fn compute_validation_hint(
    cache: &ValidationCacheMetadata,
    current_sha: &str,
) -> (String, String) {
    if current_sha != cache.commit_sha {
        (
            "run_tests".to_string(),
            format!(
                "Cache stale (SHA changed from {} to {}). Run tests normally.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())],
                &current_sha[..8.min(current_sha.len())]
            ),
        )
    } else if !cache.tests_ran {
        (
            "skip_test_validation".to_string(),
            format!(
                "No tests were run during execution (commit {}). Skip test validation.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())]
            ),
        )
    } else if cache.tests_passed {
        (
            "skip_tests".to_string(),
            format!(
                "Tests passed on commit {}. Skip test re-run.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())]
            ),
        )
    } else {
        // tests_ran=true but tests_passed=false → must re-run
        (
            "run_tests".to_string(),
            format!(
                "Tests failed on commit {}. Run tests normally.",
                &cache.commit_sha[..8.min(cache.commit_sha.len())]
            ),
        )
    }
}

/// Parse validation cache from task metadata, compute hint by comparing HEAD SHA.
/// Returns None if no cache exists or worktree_path is missing.
/// Also appends a human-readable hint to context_hints when cache is available.
async fn compute_validation_cache(
    task: &crate::domain::entities::Task,
    context_hints: &mut Vec<String>,
) -> Option<ValidationCacheData> {
    // Parse cache from metadata
    let cache = match ValidationCacheMetadata::from_task_metadata(task.metadata.as_deref()) {
        Ok(Some(c)) => c,
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!(
                "Failed to parse validation_cache from task {} metadata: {}",
                task.id,
                e
            );
            return None;
        }
    };

    // Compute current HEAD SHA to compare against cached SHA
    let worktree_path = task.worktree_path.as_deref()?;
    let path = std::path::Path::new(worktree_path);
    let current_sha = match GitService::get_head_sha(path).await {
        Ok(sha) => sha,
        Err(e) => {
            tracing::warn!(
                "Failed to get HEAD SHA for task {} (skipping validation hint): {}",
                task.id,
                e
            );
            return None;
        }
    };

    // Compute hint based on SHA comparison and test results
    let (validation_hint, hint_message) = compute_validation_hint(&cache, &current_sha);

    context_hints.push(format!(
        "VALIDATION CACHE: {} — {}",
        validation_hint, hint_message
    ));

    Some(ValidationCacheData {
        commit_sha: cache.commit_sha,
        tests_ran: cache.tests_ran,
        tests_passed: cache.tests_passed,
        test_summary: cache.test_summary,
        captured_at: cache.captured_at,
        validation_hint,
        hint_message,
    })
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod helpers_tests;
