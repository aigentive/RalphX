use super::*;
use crate::domain::entities::{
    SessionPurpose, VerificationGap, VerificationRoundSnapshot, VerificationRunSnapshot,
    VerificationStatus,
};

#[tokio::test]
async fn test_create_and_get() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new_with_title(project_id.clone(), "Test Session");

    repo.create(session.clone()).await.unwrap();

    let retrieved = repo.get_by_id(&session.id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().id, session.id);
}

#[tokio::test]
async fn test_get_by_project() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());

    repo.create(session).await.unwrap();

    let sessions = repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(sessions.len(), 1);
}

#[tokio::test]
async fn test_update_status() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    repo.create(session).await.unwrap();
    repo.update_status(&session_id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let updated = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(updated.status, IdeationSessionStatus::Archived);
    assert!(updated.archived_at.is_some());
}

#[tokio::test]
async fn test_delete() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    repo.create(session).await.unwrap();
    repo.delete(&session_id).await.unwrap();

    let result = repo.get_by_id(&session_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_children() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let mut child1 = IdeationSession::new(project_id.clone());
    child1.parent_session_id = Some(parent.id.clone());
    let mut child2 = IdeationSession::new(project_id.clone());
    child2.parent_session_id = Some(parent.id.clone());

    repo.create(parent.clone()).await.unwrap();
    repo.create(child1.clone()).await.unwrap();
    repo.create(child2.clone()).await.unwrap();

    let children = repo.get_children(&parent.id).await.unwrap();
    assert_eq!(children.len(), 2);
}

#[tokio::test]
async fn test_get_children_returns_empty_for_sessions_without_children() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    let children = repo.get_children(&session.id).await.unwrap();
    assert!(children.is_empty());
}

#[tokio::test]
async fn test_get_latest_child_session_id_filters_by_purpose_and_archive_state() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let mut general_child = IdeationSession::new(project_id.clone());
    general_child.parent_session_id = Some(parent.id.clone());
    general_child.created_at = parent.created_at + chrono::Duration::minutes(1);

    let mut verification_child = IdeationSession::new(project_id.clone());
    verification_child.parent_session_id = Some(parent.id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    verification_child.created_at = parent.created_at + chrono::Duration::minutes(2);

    let mut archived_verification_child = IdeationSession::new(project_id.clone());
    archived_verification_child.parent_session_id = Some(parent.id.clone());
    archived_verification_child.session_purpose = SessionPurpose::Verification;
    archived_verification_child.status = IdeationSessionStatus::Archived;
    archived_verification_child.created_at = parent.created_at + chrono::Duration::minutes(3);

    repo.create(parent.clone()).await.unwrap();
    repo.create(general_child).await.unwrap();
    repo.create(verification_child.clone()).await.unwrap();
    repo.create(archived_verification_child.clone()).await.unwrap();

    let latest_including_archived = repo
        .get_latest_child_session_id(&parent.id, Some(SessionPurpose::Verification), true)
        .await
        .unwrap();
    assert_eq!(
        latest_including_archived,
        Some(archived_verification_child.id.clone())
    );

    let latest_active = repo
        .get_latest_child_session_id(&parent.id, Some(SessionPurpose::Verification), false)
        .await
        .unwrap();
    assert_eq!(latest_active, Some(verification_child.id));
}

#[tokio::test]
async fn test_get_ancestor_chain_three_levels_deep() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let level1 = IdeationSession::new(project_id.clone());
    let mut level2 = IdeationSession::new(project_id.clone());
    level2.parent_session_id = Some(level1.id.clone());
    let mut level3 = IdeationSession::new(project_id.clone());
    level3.parent_session_id = Some(level2.id.clone());

    repo.create(level1.clone()).await.unwrap();
    repo.create(level2.clone()).await.unwrap();
    repo.create(level3.clone()).await.unwrap();

    let chain = repo.get_ancestor_chain(&level3.id).await.unwrap();
    // Should return: [level2, level1] (direct parent to root)
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].id, level2.id);
    assert_eq!(chain[1].id, level1.id);
}

#[tokio::test]
async fn test_get_ancestor_chain_single_parent() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let mut child = IdeationSession::new(project_id.clone());
    child.parent_session_id = Some(parent.id.clone());

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    let chain = repo.get_ancestor_chain(&child.id).await.unwrap();
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].id, parent.id);
}

#[tokio::test]
async fn test_get_ancestor_chain_no_parent() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    let chain = repo.get_ancestor_chain(&session.id).await.unwrap();
    assert!(chain.is_empty());
}

#[tokio::test]
async fn test_set_parent() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let child = IdeationSession::new(project_id.clone());

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    repo.set_parent(&child.id, Some(&parent.id)).await.unwrap();

    let updated_child = repo.get_by_id(&child.id).await.unwrap().unwrap();
    assert_eq!(updated_child.parent_session_id, Some(parent.id.clone()));
}

#[tokio::test]
async fn test_set_parent_with_null() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let parent = IdeationSession::new(project_id.clone());
    let mut child = IdeationSession::new(project_id.clone());
    child.parent_session_id = Some(parent.id.clone());

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    repo.set_parent(&child.id, None).await.unwrap();

    let updated_child = repo.get_by_id(&child.id).await.unwrap().unwrap();
    assert!(updated_child.parent_session_id.is_none());
}

// ==================== VERIFICATION STATE TESTS ====================

#[tokio::test]
async fn test_update_verification_state_roundtrip() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Default
    let (status, in_progress) = repo
        .get_verification_status(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, VerificationStatus::Unverified);
    assert!(!in_progress);

    // Update
    repo.update_verification_state(
        &session.id,
        VerificationStatus::Reviewing,
        true
    )
    .await
    .unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.verification_status, VerificationStatus::Reviewing);
    assert!(found.verification_in_progress);
}

#[tokio::test]
async fn test_reset_verification_clears_all_3_columns_when_not_in_progress() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Set to needs_revision, not in progress
    repo.update_verification_state(
        &session.id,
        VerificationStatus::NeedsRevision,
        false
    )
    .await
    .unwrap();

    repo.reset_verification(&session.id).await.unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.verification_status, VerificationStatus::Unverified);
    assert!(!found.verification_in_progress);
    assert_eq!(
        found.verification_generation, 1,
        "reset_verification must increment generation to fence stale verifier callbacks"
    );
}

#[tokio::test]
async fn test_reset_verification_is_noop_when_in_progress() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Set to reviewing with in_progress = true
    repo.update_verification_state(
        &session.id,
        VerificationStatus::Reviewing,
        true
    )
    .await
    .unwrap();

    // Reset should be a no-op because in_progress = true
    repo.reset_verification(&session.id).await.unwrap();

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(found.verification_status, VerificationStatus::Reviewing);
    assert!(found.verification_in_progress);
}

#[tokio::test]
async fn test_get_verification_status_returns_none_for_nonexistent() {
    let repo = MemoryIdeationSessionRepository::new();
    let id = IdeationSessionId::new();
    let result = repo.get_verification_status(&id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_verification_status_prefers_active_generation_snapshot_over_stale_summary() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let mut session = IdeationSession::new(project_id);
    session.verification_generation = 3;
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    repo.save_verification_run_snapshot(
        &session_id,
        &VerificationRunSnapshot {
            generation: 3,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 2,
            max_rounds: 5,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: vec![VerificationGap {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: "Missing regression".to_string(),
                why_it_matters: None,
                source: Some("completeness".to_string()),
            }],
            rounds: vec![],
        },
    )
    .await
    .unwrap();

    repo.update_verification_state(&session_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let (status, in_progress) = repo
        .get_verification_status(&session_id)
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(
        status,
        VerificationStatus::Reviewing,
        "active-generation snapshot must override stale summary status"
    );
    assert!(
        in_progress,
        "active-generation snapshot must override stale summary in_progress flag"
    );
}

#[tokio::test]
async fn test_save_and_get_verification_run_snapshot_roundtrip() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id);
    repo.create(session.clone()).await.unwrap();

    let snapshot = VerificationRunSnapshot {
        generation: 4,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 2,
        max_rounds: 5,
        best_round_index: Some(1),
        convergence_reason: Some("escalated_to_parent".to_string()),
        current_gaps: vec![VerificationGap {
            severity: "high".to_string(),
            category: "testing".to_string(),
            description: "Missing regression".to_string(),
            why_it_matters: Some("Plan can regress at runtime".to_string()),
            source: Some("completeness".to_string()),
        }],
        rounds: vec![
            VerificationRoundSnapshot {
                round: 1,
                gap_score: 10,
                fingerprints: vec!["gap-auth".to_string()],
                gaps: vec![VerificationGap {
                    severity: "critical".to_string(),
                    category: "security".to_string(),
                    description: "Auth missing".to_string(),
                    why_it_matters: None,
                    source: Some("completeness".to_string()),
                }],
                parse_failed: false,
            },
            VerificationRoundSnapshot {
                round: 2,
                gap_score: 3,
                fingerprints: vec!["gap-regression".to_string()],
                gaps: vec![VerificationGap {
                    severity: "high".to_string(),
                    category: "testing".to_string(),
                    description: "Missing regression".to_string(),
                    why_it_matters: Some("Plan can regress at runtime".to_string()),
                    source: Some("feasibility".to_string()),
                }],
                parse_failed: true,
            },
        ],
    };

    repo.save_verification_run_snapshot(&session.id, &snapshot)
        .await
        .unwrap();

    let found = repo
        .get_verification_run_snapshot(&session.id, 4)
        .await
        .unwrap()
        .expect("snapshot must exist");
    assert_eq!(found, snapshot);
}

// ==================== ARCHIVE CLEARS VERIFICATION_IN_PROGRESS TESTS ====================

#[tokio::test]
async fn test_archive_clears_verification_in_progress_when_set() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Set verification_in_progress = true
    repo.update_verification_state(
        &session.id,
        VerificationStatus::Reviewing,
        true
    )
    .await
    .unwrap();

    // Archive should atomically clear the flag
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let updated = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(updated.status, IdeationSessionStatus::Archived);
    assert!(updated.archived_at.is_some());
    assert!(
        !updated.verification_in_progress,
        "verification_in_progress must be cleared on archive"
    );
}

#[tokio::test]
async fn test_archive_does_not_regress_when_verification_in_progress_already_false() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Ensure flag is already false (default)
    let before = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert!(!before.verification_in_progress);

    // Archive — flag must remain false
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let updated = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(updated.status, IdeationSessionStatus::Archived);
    assert!(
        !updated.verification_in_progress,
        "verification_in_progress must remain false after archive"
    );
}

/// ImportedVerified sessions must not be reset by reset_verification —
/// their pre-verified status must be preserved across plan artifact changes.
#[tokio::test]
async fn test_reset_verification_is_noop_for_imported_verified() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Set to ImportedVerified, not in progress
    repo.update_verification_state(
        &session.id,
        VerificationStatus::ImportedVerified,
        false
    )
    .await
    .unwrap();

    // reset_verification should return false (no-op) for ImportedVerified
    let result = repo.reset_verification(&session.id).await.unwrap();
    assert!(!result, "reset_verification must return false for ImportedVerified");

    let found = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        found.verification_status,
        VerificationStatus::ImportedVerified,
        "ImportedVerified status must not be changed by reset_verification"
    );
    assert!(!found.verification_in_progress);
}

// ==================== STALE QUERY EXCLUDES ARCHIVED SESSIONS TESTS ====================

/// Defense-in-depth: archived session with verification_in_progress re-set via
/// update_verification_state must NOT appear in get_stale_in_progress_sessions.
#[tokio::test]
async fn test_get_stale_in_progress_sessions_excludes_archived() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Archive the session (clears verification_in_progress via update_status guard)
    repo.update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    // Re-set verification_in_progress=true after archiving to simulate defense-in-depth scenario
    repo.update_verification_state(
        &session.id,
        VerificationStatus::Reviewing,
        true,
    )
    .await
    .unwrap();

    // Use a future cutoff so updated_at is definitely before it
    let stale_cutoff = chrono::Utc::now() + chrono::Duration::hours(1);
    let results = repo.get_stale_in_progress_sessions(stale_cutoff).await.unwrap();
    assert!(
        results.iter().all(|s| s.id != session.id),
        "archived session must be excluded from stale query even with verification_in_progress=true"
    );
}

/// Active session with stale verification_in_progress=true MUST appear in results.
#[tokio::test]
async fn test_get_stale_in_progress_sessions_includes_active() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Set verification_in_progress=true (session stays Active)
    repo.update_verification_state(
        &session.id,
        VerificationStatus::Reviewing,
        true,
    )
    .await
    .unwrap();

    let stale_cutoff = chrono::Utc::now() + chrono::Duration::hours(1);
    let results = repo.get_stale_in_progress_sessions(stale_cutoff).await.unwrap();
    assert!(
        results.iter().any(|s| s.id == session.id),
        "active stale session must be included in stale query"
    );
}

// ============================================================================
// set_pending_initial_prompt_if_unset tests (capacity-full guard)
// ============================================================================

#[tokio::test]
async fn test_set_pending_if_unset_sets_when_none() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // No existing prompt → returns true and stores value.
    let result = repo
        .set_pending_initial_prompt_if_unset(session.id.as_str(), "First message".to_string())
        .await
        .unwrap();
    assert!(result, "must return true when prompt was None");

    let fetched = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(fetched.pending_initial_prompt.as_deref(), Some("First message"));
}

#[tokio::test]
async fn test_set_pending_if_unset_rejects_when_already_set() {
    let repo = MemoryIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    repo.create(session.clone()).await.unwrap();

    // Pre-set a prompt.
    repo.set_pending_initial_prompt(session.id.as_str(), Some("Existing".to_string()))
        .await
        .unwrap();

    // If-unset must return false without overwriting.
    let result = repo
        .set_pending_initial_prompt_if_unset(session.id.as_str(), "Overwrite".to_string())
        .await
        .unwrap();
    assert!(!result, "must return false when prompt is already set");

    let fetched = repo.get_by_id(&session.id).await.unwrap().unwrap();
    assert_eq!(
        fetched.pending_initial_prompt.as_deref(),
        Some("Existing"),
        "existing prompt must not be overwritten"
    );
}

#[tokio::test]
async fn test_set_pending_if_unset_returns_false_for_missing_session() {
    let repo = MemoryIdeationSessionRepository::new();
    let result = repo
        .set_pending_initial_prompt_if_unset("nonexistent-id", "Hello".to_string())
        .await
        .unwrap();
    assert!(!result, "must return false when session does not exist");
}
