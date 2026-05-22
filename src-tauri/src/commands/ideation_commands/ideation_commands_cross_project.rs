// Cross-project session creation and proposal migration commands
//
// Provides cross-project orchestration:
// - create_cross_project_session: inherit a verified plan in a target project
// - migrate_proposals: copy proposals from one session to another with dependency remapping

use std::collections::HashMap;

use ralphx_domain::entities::EventType;
use tauri::{Emitter, State};

use crate::application::AppState;
use crate::domain::entities::{
    IdeationSession, IdeationSessionId, IdeationSessionStatus, ProjectId, TaskProposalId,
    VerificationStatus,
};
use crate::domain::services::validate_project_path;
use crate::infrastructure::sqlite::SqliteIdeationSessionRepository;

use super::ideation_commands_types::{
    CreateCrossProjectSessionInput, DroppedDependency, IdeationSessionResponse,
    MigrateProposalsInput, MigrateProposalsResult, MigratedProposalEntry,
};

// ============================================================================
// Core Implementation
// ============================================================================

/// Core implementation for creating a cross-project ideation session.
/// Generic over Runtime to enable unit testing with MockRuntime.
///
/// Logic:
/// 1. Validate the target project path (security check).
/// 2. Resolve target project: query by path, auto-create if not found.
/// 3. Fetch source session, validate its plan is verified.
/// 4. Resolve artifact: prefer plan_artifact_id, fall back to inherited_plan_artifact_id.
/// 5. Validate no circular import (TOCTOU-safe: done inside the INSERT db.run closure).
/// 6. Insert the new session with ImportedVerified status and source tracking fields.
/// 7. Emit events and return response.
pub(crate) async fn create_cross_project_session_impl<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    input: CreateCrossProjectSessionInput,
) -> Result<IdeationSessionResponse, String> {
    tracing::info!(
        target_path = %input.target_project_path,
        source_session_id = %input.source_session_id,
        "Creating cross-project session"
    );

    // 1. Validate the target path (canonicalize, blocklist, home dir check)
    let canonical = validate_project_path(&input.target_project_path)
        .map_err(|e| e.to_string())?;
    let canonical_str = canonical.to_string_lossy().to_string();

    // 2. Resolve target project — query by working_directory, auto-create if not found
    let target_project = match state
        .project_repo
        .get_by_working_directory(&canonical_str)
        .await
        .map_err(|e| e.to_string())?
    {
        Some(project) => project,
        None => {
            // Validate the directory exists on disk before auto-creating
            if !canonical.exists() {
                return Err(format!(
                    "Target project path does not exist on disk: {canonical_str}"
                ));
            }

            // Auto-create: name = directory basename
            let name = canonical
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unnamed Project".to_string());

            tracing::info!(path = %canonical_str, name = %name, "Auto-creating target project for cross-project session");

            let new_project = crate::domain::entities::Project::new(name, canonical_str.clone());
            let project_id_str = new_project.id.as_str().to_string();

            let created = state
                .project_repo
                .create(new_project)
                .await
                .map_err(|e| e.to_string())?;

            // Emit project:created event for real-time UI updates
            let _ = app.emit(
                "project:created",
                serde_json::json!({
                    "projectId": project_id_str,
                    "workingDirectory": canonical_str,
                }),
            );

            created
        }
    };

    let target_project_id = target_project.id.as_str().to_string();

    // 3. Fetch source session and validate its plan is verified
    let source_session_id_entity = IdeationSessionId::from_string(input.source_session_id.clone());
    let source_session = state
        .ideation_session_repo
        .get_by_id(&source_session_id_entity)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Source session not found: {}", input.source_session_id))?;

    let is_verified = matches!(
        source_session.verification_status,
        VerificationStatus::Verified
            | VerificationStatus::Skipped
            | VerificationStatus::ImportedVerified
    );
    if !is_verified {
        return Err(format!(
            "Source session plan is not verified (status: {}). \
             Only Verified, Skipped, or ImportedVerified plans can be exported.",
            source_session.verification_status
        ));
    }

    // 4. Resolve artifact: prefer own plan_artifact_id, fall back to inherited_plan_artifact_id
    let artifact_id = source_session
        .plan_artifact_id
        .clone()
        .or_else(|| source_session.inherited_plan_artifact_id.clone())
        .ok_or_else(|| {
            format!(
                "Source session has no plan artifact to inherit (session: {})",
                input.source_session_id
            )
        })?;

    // Build the new session entity
    let new_session_id = IdeationSessionId::new();
    let mut builder = IdeationSession::builder()
        .id(new_session_id)
        .project_id(ProjectId::from_string(target_project_id.clone()))
        .inherited_plan_artifact_id(artifact_id)
        .status(IdeationSessionStatus::Active)
        .verification_status(VerificationStatus::ImportedVerified)
        .source_project_id(source_session.project_id.as_str().to_string())
        .source_session_id(input.source_session_id.clone())
        .cross_project_checked(true)
        .origin(source_session.origin);

    if let Some(title) = input.title {
        builder = builder.title(title);
    }

    let new_session = builder.build();

    // 5+6. Circular import check + INSERT in a single db.run() closure (TOCTOU safety)
    let source_id_for_check = input.source_session_id.clone();
    let target_project_id_for_check = target_project_id.clone();

    let created = state
        .db
        .run(move |conn| {
            // Circular import guard runs in same closure as INSERT (TOCTOU safety)
            SqliteIdeationSessionRepository::validate_no_circular_import_sync(
                conn,
                &source_id_for_check,
                &target_project_id_for_check,
                10, // max chain depth
            )?;

            // Atomically insert the new session
            SqliteIdeationSessionRepository::insert_sync(conn, &new_session)
        })
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("CIRCULAR_IMPORT")
                || msg.contains("SELF_REFERENCE")
                || msg.contains("CHAIN_TOO_DEEP")
            {
                tracing::warn!(error = %msg, "Cross-project import rejected: circular import detected");
            }
            msg
        })?;

    tracing::debug!(
        session_id = %created.id.as_str(),
        project_id = %created.project_id.as_str(),
        "Cross-project session created successfully"
    );

    // 7. Emit ideation:session_created event (same payload shape as regular sessions)
    let payload_json = serde_json::json!({
        "sessionId": created.id.to_string(),
        "projectId": created.project_id.to_string(),
    });
    let _ = app.emit("ideation:session_created", &payload_json);

    // Layer 2: persist to external_events table (non-fatal)
    if let Err(e) = state
        .external_events_repo
        .insert_event(
            "ideation:session_created",
            &created.project_id.to_string(),
            &payload_json.to_string(),
        )
        .await
    {
        tracing::warn!(error = %e, "Failed to persist IdeationSessionCreated event");
    }

    // Layer 3: webhook push (fire-and-forget, non-fatal)
    if let Some(ref publisher) = state.webhook_publisher {
        let _ = publisher
            .publish(
                EventType::IdeationSessionCreated,
                &created.project_id.to_string(),
                payload_json,
            )
            .await;
    }

    Ok(IdeationSessionResponse::from(created))
}

// ============================================================================
// Tauri Command Wrapper
// ============================================================================

/// Create a new ideation session in a target project by inheriting a verified plan
/// from a source session in another project on the same RalphX instance.
///
/// The target project is resolved by filesystem path; it is auto-created if it doesn't exist
/// in RalphX yet (the directory must exist on disk). The source session's plan must be
/// in a verified state (Verified, Skipped, or ImportedVerified).
#[tauri::command]
pub async fn create_cross_project_session(
    input: CreateCrossProjectSessionInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<IdeationSessionResponse, String> {
    create_cross_project_session_impl(&app, &state, input).await
}

// ============================================================================
// Proposal Migration Implementation
// ============================================================================

/// Core implementation for migrating proposals from one session to another.
///
/// Logic:
/// 1. Fetch all proposals from the source session.
/// 2. Filter by proposal_ids (if provided) and/or target_project_filter.
/// 3. For each selected proposal, clone it with a new UUID, the target session_id,
///    and migrated_from traceability fields.
/// 4. Fetch all dependencies for the source session.
/// 5. For each dependency:
///    - Both ends in migration set → remap to new IDs.
///    - One end outside migration set → drop with warning.
/// 6. Insert new proposals and dependencies.
/// 7. Return migration mappings and dropped dependency warnings.
#[doc(hidden)]
pub async fn migrate_proposals_impl(
    state: &AppState,
    input: MigrateProposalsInput,
) -> Result<MigrateProposalsResult, String> {
    tracing::info!(
        source_session_id = %input.source_session_id,
        target_session_id = %input.target_session_id,
        "Migrating proposals between sessions"
    );

    let source_session_id = IdeationSessionId::from_string(input.source_session_id.clone());
    let target_session_id = IdeationSessionId::from_string(input.target_session_id.clone());

    // 1. Validate both sessions exist
    let _source_session = state
        .ideation_session_repo
        .get_by_id(&source_session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Source session not found: {}", input.source_session_id))?;

    let _target_session = state
        .ideation_session_repo
        .get_by_id(&target_session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Target session not found: {}", input.target_session_id))?;

    // 2. Fetch source proposals
    let source_proposals = state
        .task_proposal_repo
        .get_by_session(&source_session_id)
        .await
        .map_err(|e| e.to_string())?;

    // 3. Filter by proposal_ids and/or target_project_filter
    let selected_proposals: Vec<_> = source_proposals
        .into_iter()
        .filter(|p| {
            // Filter by explicit proposal_ids if provided
            if let Some(ids) = &input.proposal_ids {
                if !ids.contains(&p.id.as_str().to_string()) {
                    return false;
                }
            }
            // Filter by target_project_filter if provided
            if let Some(filter) = &input.target_project_filter {
                if p.target_project.as_deref() != Some(filter.as_str()) {
                    return false;
                }
            }
            true
        })
        .collect();

    if selected_proposals.is_empty() {
        return Ok(MigrateProposalsResult {
            migrated: vec![],
            dropped_dependencies: vec![],
        });
    }

    // Build a set of source IDs for dependency filtering
    let source_ids: std::collections::HashSet<String> = selected_proposals
        .iter()
        .map(|p| p.id.as_str().to_string())
        .collect();

    // 4. Clone proposals with new UUIDs
    let now = chrono::Utc::now();
    let mut id_map: HashMap<String, String> = HashMap::new(); // old_id → new_id
    let mut new_proposals = Vec::new();

    for proposal in &selected_proposals {
        let new_id = TaskProposalId::new();
        id_map.insert(proposal.id.as_str().to_string(), new_id.as_str().to_string());

        let mut cloned = proposal.clone();
        cloned.id = new_id;
        cloned.session_id = target_session_id.clone();
        cloned.migrated_from_session_id = Some(input.source_session_id.clone());
        cloned.migrated_from_proposal_id = Some(proposal.id.as_str().to_string());
        // Reset fields that shouldn't carry over
        cloned.created_task_id = None;
        cloned.target_project = None;
        cloned.created_at = now;
        cloned.updated_at = now;
        new_proposals.push(cloned);
    }

    // 5. Insert new proposals
    for proposal in new_proposals {
        state
            .task_proposal_repo
            .create(proposal)
            .await
            .map_err(|e| format!("Failed to create migrated proposal: {}", e))?;
    }

    // 6. Fetch and remap dependencies
    let all_deps = state
        .proposal_dependency_repo
        .get_all_for_session(&source_session_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut dropped_dependencies = Vec::new();

    for (dep_proposal_id, dep_depends_on_id, reason) in all_deps {
        let from_str = dep_proposal_id.as_str().to_string();
        let to_str = dep_depends_on_id.as_str().to_string();

        let from_in_set = source_ids.contains(&from_str);
        let to_in_set = source_ids.contains(&to_str);

        match (from_in_set, to_in_set) {
            (true, true) => {
                // Both ends migrated — remap to new IDs
                let new_from = id_map.get(&from_str).expect("id_map must contain migrated proposal");
                let new_to = id_map.get(&to_str).expect("id_map must contain migrated proposal");

                let new_from_id = TaskProposalId::from_string(new_from.clone());
                let new_to_id = TaskProposalId::from_string(new_to.clone());

                state
                    .proposal_dependency_repo
                    .add_dependency(
                        &new_from_id,
                        &new_to_id,
                        reason.as_deref(),
                        Some("migration"),
                    )
                    .await
                    .map_err(|e| format!("Failed to create remapped dependency: {}", e))?;
            }
            (true, false) => {
                // Source depends on something outside migration set — drop
                dropped_dependencies.push(DroppedDependency {
                    proposal_id: from_str.clone(),
                    dropped_dep_id: to_str.clone(),
                    reason: format!(
                        "Dependency target '{}' was not included in the migration set",
                        to_str
                    ),
                });
            }
            (false, true) => {
                // Something outside migration set depends on a migrated proposal — drop
                dropped_dependencies.push(DroppedDependency {
                    proposal_id: from_str.clone(),
                    dropped_dep_id: to_str.clone(),
                    reason: format!(
                        "Dependency source '{}' was not included in the migration set",
                        from_str
                    ),
                });
            }
            (false, false) => {
                // Neither end in migration set — skip silently
            }
        }
    }

    let migrated: Vec<MigratedProposalEntry> = selected_proposals
        .iter()
        .map(|p| {
            let source_id = p.id.as_str().to_string();
            let target_id = id_map[&source_id].clone();
            MigratedProposalEntry { source_id, target_id }
        })
        .collect();

    tracing::info!(
        migrated_count = migrated.len() as u64,
        dropped_dep_count = dropped_dependencies.len() as u64,
        "Proposal migration complete"
    );

    Ok(MigrateProposalsResult {
        migrated,
        dropped_dependencies,
    })
}

// ============================================================================
// Tauri Command Wrapper — migrate_proposals
// ============================================================================

/// Migrate proposals from one ideation session to another.
///
/// Proposals are cloned with new UUIDs and traceability fields set.
/// Dependencies between migrated proposals are remapped; cross-session dependencies are dropped.
///
/// # Errors
///
/// Returns an error string if either session is not found, or if a database error occurs.
#[tauri::command]
pub async fn migrate_proposals(
    input: MigrateProposalsInput,
    state: State<'_, AppState>,
) -> Result<MigrateProposalsResult, String> {
    migrate_proposals_impl(&state, input).await
}
