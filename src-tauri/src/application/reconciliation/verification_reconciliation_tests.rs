use super::*;

use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::domain::entities::{
    IdeationSession, IdeationSessionId, IdeationSessionStatus, ProjectId, SessionOrigin,
    VerificationGap, VerificationRoundSnapshot, VerificationRunSnapshot, VerificationStatus,
};
use crate::domain::repositories::IdeationSessionRepository;
use crate::infrastructure::memory::MemoryIdeationSessionRepository;

fn make_service(
    repo: Arc<MemoryIdeationSessionRepository>,
    config: VerificationReconciliationConfig,
) -> VerificationReconciliationService {
    VerificationReconciliationService::new(
        repo as Arc<dyn IdeationSessionRepository>,
        config,
    )
}

fn default_config() -> VerificationReconciliationConfig {
    VerificationReconciliationConfig {
        stale_after_secs: 5400,                    // 90 min
        auto_verify_stale_secs: 600,               // 10 min
        interval_secs: 300,
        external_session_stale_secs: 7200,         // 2 hours
        external_session_startup_grace_secs: None, // falls back to external_session_stale_secs
    }
}

/// Helper: create a stuck verification session with given generation, age, and status.
/// Returns the session ID. Replaces the 8-line IdeationSession setup pattern.
async fn setup_stuck_session(
    repo: &Arc<MemoryIdeationSessionRepository>,
    project_id: ProjectId,
    gen: i32,
    age_secs: i64,
    status: VerificationStatus,
) -> IdeationSessionId {
    let mut session = IdeationSession::new(project_id);
    session.verification_status = status;
    session.verification_in_progress = true;
    session.verification_generation = gen;
    session.updated_at = Utc::now() - Duration::seconds(age_secs);
    let id = session.id.clone();
    repo.create(session).await.unwrap();
    id
}

fn assert_convergence_reason(session: &IdeationSession, expected_reason: &str) {
    assert_eq!(
        session.verification_convergence_reason.as_deref(),
        Some(expected_reason),
        "expected native convergence_reason={expected_reason}"
    );
}

fn make_snapshot(
    generation: i32,
    status: VerificationStatus,
    in_progress: bool,
    current_round: u32,
    max_rounds: u32,
    convergence_reason: Option<&str>,
    round_scores: &[u32],
) -> VerificationRunSnapshot {
    VerificationRunSnapshot {
        generation,
        status,
        in_progress,
        current_round,
        max_rounds,
        best_round_index: None,
        convergence_reason: convergence_reason.map(ToString::to_string),
        current_gaps: Vec::<VerificationGap>::new(),
        rounds: round_scores
            .iter()
            .enumerate()
            .map(|(idx, gap_score)| VerificationRoundSnapshot {
                round: (idx + 1) as u32,
                gap_score: *gap_score,
                fingerprints: vec![],
                gaps: vec![],
                parse_failed: false,
            })
            .collect(),
    }
}

#[tokio::test]
async fn test_reconciliation_resets_stuck_session_after_timeout() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session stuck in verification for 2 hours (> 90-min threshold)
    let session_id =
        setup_stuck_session(&repo, project_id, 0, 7200, VerificationStatus::Reviewing).await;

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 1, "exactly one stuck session should be reset");

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.verification_status,
        VerificationStatus::Unverified,
        "verification_status must be reset to Unverified"
    );
    assert!(
        !after.verification_in_progress,
        "verification_in_progress must be cleared"
    );
}

#[tokio::test]
async fn test_reconciliation_skips_session_under_timeout() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session is only 30 min old — still within the 90-min window
    let session_id =
        setup_stuck_session(&repo, project_id, 0, 1800, VerificationStatus::Reviewing).await;

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 0, "no sessions should be reset when under timeout");

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.verification_status,
        VerificationStatus::Reviewing,
        "status must remain unchanged for fresh sessions"
    );
    assert!(
        after.verification_in_progress,
        "in_progress flag must remain set for fresh sessions"
    );
}

#[tokio::test]
async fn test_reconciliation_ignores_sessions_not_in_progress() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session is old but verification_in_progress=false — not stuck
    let mut session = IdeationSession::new(project_id);
    session.verification_status = VerificationStatus::Verified;
    session.verification_in_progress = false;
    session.updated_at = Utc::now() - Duration::hours(10);

    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 0, "completed sessions must not be reset");

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(after.verification_status, VerificationStatus::Verified);
}

#[tokio::test]
async fn test_reconciliation_resets_multiple_stuck_sessions() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // 3 stuck sessions (> 90 min old)
    for _ in 0..3 {
        let mut session = IdeationSession::new(project_id.clone());
        session.verification_in_progress = true;
        session.updated_at = Utc::now() - Duration::hours(3);
        repo.create(session).await.unwrap();
    }

    // 1 fresh session (within timeout — must not be reset)
    let mut fresh = IdeationSession::new(project_id.clone());
    fresh.verification_in_progress = true;
    fresh.updated_at = Utc::now() - Duration::minutes(10);
    repo.create(fresh).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 3, "only the 3 stale sessions should be reset");
}

#[tokio::test]
async fn test_startup_scan_resets_stuck_sessions() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let mut session = IdeationSession::new(project_id);
    session.verification_in_progress = true;
    session.updated_at = Utc::now() - Duration::hours(2);

    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    svc.startup_scan().await;

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert!(!after.verification_in_progress);
    assert_eq!(after.verification_status, VerificationStatus::Unverified);
}

#[tokio::test]
async fn test_reconciliation_empty_repo_returns_zero() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_reconciler_clears_legacy_metadata_on_reset() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session stuck in verification for 2 hours with stale pre-native verification state
    let mut session = IdeationSession::new(project_id);
    session.verification_status = VerificationStatus::Reviewing;
    session.verification_in_progress = true;
    session.updated_at = Utc::now() - Duration::hours(2);

    let session_id = session.id.clone();
    repo.create(session).await.unwrap();
    repo.save_verification_run_snapshot(
        &session_id,
        &VerificationRunSnapshot {
            generation: 0,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 2,
            max_rounds: 5,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: vec![],
            rounds: vec![],
        },
    )
    .await
    .unwrap();

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 1, "one stuck session should be reset");

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(after.verification_status, VerificationStatus::Unverified);
    assert!(!after.verification_in_progress);
    assert_eq!(after.verification_current_round, None);
    assert_eq!(after.verification_max_rounds, None);
    assert_eq!(after.verification_gap_count, 0);
    assert_eq!(after.verification_gap_score, None);
    assert_eq!(after.verification_convergence_reason, None);
}

#[tokio::test]
async fn test_reconciler_auto_verify_shorter_threshold() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let config = VerificationReconciliationConfig {
        stale_after_secs: 5400,
        auto_verify_stale_secs: 600,
        ..Default::default()
    };

    // Auto-verify session (generation > 0) stuck for 15 minutes — should be reset (> 10 min)
    let auto_id =
        setup_stuck_session(&repo, project_id.clone(), 1, 900, VerificationStatus::Reviewing)
            .await;

    // Manual verify session (generation == 0) stuck for 15 minutes — should NOT be reset (< 90 min)
    let manual_id =
        setup_stuck_session(&repo, project_id.clone(), 0, 900, VerificationStatus::Reviewing)
            .await;

    let svc = make_service(repo.clone(), config);
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 1, "only the auto-verify session should be reset");

    let auto_after = repo.get_by_id(&auto_id).await.unwrap().unwrap();
    assert_eq!(
        auto_after.verification_status,
        VerificationStatus::Unverified,
        "auto-verify session must be reset"
    );

    let manual_after = repo.get_by_id(&manual_id).await.unwrap().unwrap();
    assert_eq!(
        manual_after.verification_status,
        VerificationStatus::Reviewing,
        "manual-verify session must NOT be reset (shorter than 90-min threshold)"
    );
}

/// ImportedVerified sessions must never be reset by the reconciler.
/// They appear in the stale-sessions query (in_progress=true, old enough) but should be
/// skipped because their pre-verified status must be preserved.
#[tokio::test]
async fn test_reconciler_skips_imported_verified_sessions() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // ImportedVerified session that would otherwise be considered stale (> 90 min old, in_progress=true)
    let imported_id = setup_stuck_session(
        &repo,
        project_id.clone(),
        0,
        10800,
        VerificationStatus::ImportedVerified,
    )
    .await;

    // A normal Reviewing session that IS stale — should be reset
    let stuck_id =
        setup_stuck_session(&repo, project_id.clone(), 0, 10800, VerificationStatus::Reviewing)
            .await;

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    // Only the Reviewing session should be reset; ImportedVerified is preserved
    assert_eq!(count, 1, "only the stuck Reviewing session should be reset");

    let imported_after = repo.get_by_id(&imported_id).await.unwrap().unwrap();
    assert_eq!(
        imported_after.verification_status,
        VerificationStatus::ImportedVerified,
        "ImportedVerified status must not be changed by reconciler"
    );

    let stuck_after = repo.get_by_id(&stuck_id).await.unwrap().unwrap();
    assert_eq!(
        stuck_after.verification_status,
        VerificationStatus::Unverified,
        "Stuck Reviewing session must be reset to Unverified"
    );
}

/// ImportedVerified-only repo: reconciler resets 0 sessions.
#[tokio::test]
async fn test_reconciler_only_imported_verified_resets_zero() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Two ImportedVerified sessions, both stale
    for _ in 0..2 {
        setup_stuck_session(
            &repo,
            project_id.clone(),
            0,
            18000,
            VerificationStatus::ImportedVerified,
        )
        .await;
    }

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 0, "no ImportedVerified sessions should be reset");
}

#[tokio::test]
async fn test_orphaned_verification_child_reconciled() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent session stuck in verification for 2 hours (> 90-min threshold)
    let parent_id =
        setup_stuck_session(&repo, project_id.clone(), 1, 7200, VerificationStatus::Reviewing)
            .await;

    // Orphaned verification child session (not archived)
    let mut child = IdeationSession::new(project_id.clone());
    child.session_purpose = crate::domain::entities::ideation::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    child.updated_at = Utc::now() - Duration::hours(2);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 1, "parent session should be reset");

    // Parent should be reset
    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::Unverified);
    assert!(!parent_after.verification_in_progress);

    // Child should be archived
    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "orphaned verification child must be archived by reconciler"
    );
}

#[tokio::test]
async fn test_reconciler_manual_session_reset_after_long_threshold() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let config = VerificationReconciliationConfig {
        stale_after_secs: 5400,
        auto_verify_stale_secs: 600,
        ..Default::default()
    };

    // Manual verify session stuck for 2 hours — should be reset (> 90 min)
    let session_id =
        setup_stuck_session(&repo, project_id, 0, 7200, VerificationStatus::Reviewing).await;

    let svc = make_service(repo.clone(), config);
    let count = svc.scan_and_reset(false).await;

    assert_eq!(count, 1, "manual session stuck > 90 min should be reset");

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(after.verification_status, VerificationStatus::Unverified);
}

// ---------------------------------------------------------------------------
// reconcile_verification_on_child_complete tests
// ---------------------------------------------------------------------------

/// Helper: create a linked parent+child pair in the repo.
/// Parent has verification_in_progress=true; child has session_purpose=Verification.
async fn make_parent_child_pair(
    repo: &Arc<crate::infrastructure::memory::MemoryIdeationSessionRepository>,
    parent_snapshot: Option<VerificationRunSnapshot>,
) -> (
    crate::domain::entities::IdeationSessionId,
    crate::domain::entities::IdeationSessionId,
) {
    let project_id = ProjectId::new();

    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::Reviewing;
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();
    if let Some(snapshot) = parent_snapshot {
        repo.save_verification_run_snapshot(&parent_id, &snapshot)
            .await
            .unwrap();
    }

    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    (parent_id, child_id)
}

#[tokio::test]
async fn test_reconcile_child_complete_convergence_zero_blocking() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::Reviewing,
                true,
                2,
                5,
                Some("zero_blocking"),
                &[0],
            )),
        )
        .await;

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::Verified,
        "zero_blocking convergence should map to Verified"
    );
    assert!(
        !parent_after.verification_in_progress,
        "in_progress must be cleared after reconciliation"
    );

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must be archived after reconciliation"
    );
}

#[tokio::test]
async fn test_reconcile_child_complete_uses_native_verification_snapshot_when_metadata_missing() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) = make_parent_child_pair(&repo, None).await;
    let parent = repo.get_by_id(&parent_id).await.unwrap().unwrap();

    repo.save_verification_run_snapshot(
        &parent_id,
        &crate::domain::entities::VerificationRunSnapshot {
            generation: parent.verification_generation,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 2,
            max_rounds: 5,
            best_round_index: Some(2),
            convergence_reason: Some("zero_blocking".to_string()),
            current_gaps: vec![],
            rounds: vec![
                crate::domain::entities::VerificationRoundSnapshot {
                    round: 1,
                    gap_score: 7,
                    fingerprints: vec!["gap-1".to_string()],
                    gaps: vec![],
                    parse_failed: false,
                },
                crate::domain::entities::VerificationRoundSnapshot {
                    round: 2,
                    gap_score: 0,
                    fingerprints: vec!["gap-2".to_string()],
                    gaps: vec![],
                    parse_failed: false,
                },
            ],
        },
    )
    .await
    .unwrap();

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::Verified,
        "native zero_blocking convergence should map to Verified even without any older seed fixture"
    );
    assert!(!parent_after.verification_in_progress);
}

#[tokio::test]
async fn test_reconcile_child_complete_convergence_jaccard_converged() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::Reviewing,
                true,
                3,
                5,
                Some("jaccard_converged"),
                &[],
            )),
        )
        .await;

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::Verified);
    assert!(!parent_after.verification_in_progress);
}

#[tokio::test]
async fn test_reconcile_child_complete_max_rounds_needs_revision() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::Reviewing,
                true,
                5,
                5,
                Some("max_rounds"),
                &[],
            )),
        )
        .await;

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::NeedsRevision,
        "max_rounds convergence should map to NeedsRevision"
    );
    assert!(!parent_after.verification_in_progress);

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived
    );
}

#[tokio::test]
async fn test_reconcile_child_complete_actionable_needs_revision_requests_auto_continue() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::NeedsRevision,
                true,
                1,
                5,
                None,
                &[4],
            )),
        )
        .await;
    repo.update_verification_state(&parent_id, VerificationStatus::NeedsRevision, true)
        .await
        .unwrap();

    let result = reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    match result {
        Some(ReconcileVerificationChildCompletion::AutoContinue(request)) => {
            assert_eq!(request.snapshot.status, VerificationStatus::NeedsRevision);
            assert!(request.snapshot.in_progress);
            assert!(
                request
                    .continuation_message
                    .contains("Continue the active verification loop in this same session")
            );
        }
        other => panic!("expected auto-continue outcome, got {:?}", other.map(|_| ())),
    }

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::NeedsRevision);
    assert!(
        parent_after.verification_in_progress,
        "auto-continue must keep parent verification marked in progress"
    );

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must stay active for same-session auto-continue"
    );
}

#[tokio::test]
async fn test_reconcile_child_complete_pending_review_round_requests_auto_continue() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) = make_parent_child_pair(
        &repo,
        Some(VerificationRunSnapshot {
            generation: 0,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 1,
            max_rounds: 5,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: vec![],
            rounds: vec![],
        }),
    )
    .await;
    repo.update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    repo.update_verification_state(&parent_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let result = reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    match result {
        Some(ReconcileVerificationChildCompletion::AutoContinue(request)) => {
            assert_eq!(request.snapshot.status, VerificationStatus::Reviewing);
            assert!(request.snapshot.in_progress);
            assert_eq!(request.snapshot.current_round, 1);
            assert!(
                request
                    .continuation_message
                    .contains("required verification critics may still be settling")
            );
        }
        other => panic!("expected auto-continue outcome, got {:?}", other.map(|_| ())),
    }

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::Reviewing);
    assert!(
        parent_after.verification_in_progress,
        "pending round must keep parent verification in progress"
    );

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must stay active while the pending round is being resumed"
    );
}

#[tokio::test]
async fn test_reconcile_child_complete_crashed_mid_round() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::Reviewing,
                true,
                2,
                5,
                None,
                &[5],
            )),
        )
        .await;

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::NeedsRevision,
        "crashed mid-round should result in NeedsRevision"
    );
    assert!(!parent_after.verification_in_progress);

    // Verify convergence_reason was set to agent_crashed_mid_round in metadata
    assert_convergence_reason(&parent_after, "agent_crashed_mid_round");

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived
    );
}

#[tokio::test]
async fn test_reconcile_child_complete_no_snapshot() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    // No native snapshot at all — agent completed without any updates
    let (parent_id, child_id) = make_parent_child_pair(&repo, None).await;

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::Unverified,
        "no snapshot should result in Unverified"
    );
    assert!(!parent_after.verification_in_progress);

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived
    );
}

#[tokio::test]
async fn test_reconcile_child_complete_imported_verified_guard() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    // Parent is ImportedVerified — must not be reconciled
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::ImportedVerified;
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::ImportedVerified,
        "ImportedVerified status must not be changed by reconciliation"
    );
    // in_progress left untouched (guard returns before updating)
    assert!(parent_after.verification_in_progress);
}

#[tokio::test]
async fn test_reconcile_child_complete_orphan_sibling_archived() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::Reviewing;
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Primary child (the one completing)
    let mut child1 = IdeationSession::new(project_id.clone());
    child1.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child1.parent_session_id = Some(parent_id.clone());
    let child1_id = child1.id.clone();
    repo.create(child1).await.unwrap();

    // Orphan sibling child (should be archived by orphan cleanup)
    let mut child2 = IdeationSession::new(project_id);
    child2.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child2.parent_session_id = Some(parent_id.clone());
    let child2_id = child2.id.clone();
    repo.create(child2).await.unwrap();

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child1_id,
        &dyn_repo,
        None,
    )
    .await;

    let child1_after = repo.get_by_id(&child1_id).await.unwrap().unwrap();
    assert_eq!(
        child1_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "primary child must be archived"
    );

    let child2_after = repo.get_by_id(&child2_id).await.unwrap().unwrap();
    assert_eq!(
        child2_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "orphan sibling child must be archived by orphan cleanup"
    );
}

// ---------------------------------------------------------------------------
// scan_and_reset(cold_boot: true) tests
// ---------------------------------------------------------------------------

/// Cold boot resets ALL in-progress sessions regardless of updated_at timestamp.
/// This is the key difference from scan_and_reset(false) which uses TTL thresholds.
#[tokio::test]
async fn test_scan_and_reset_cold_boot_ignores_ttl() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // 5-minute-old auto-verify session — would NOT be reset by scan_and_reset(false)
    let recent_auto_id =
        setup_stuck_session(&repo, project_id.clone(), 1, 300, VerificationStatus::Reviewing)
            .await;

    // 30-minute-old manual-verify session — would NOT be reset by scan_and_reset(false)
    let mid_manual_id =
        setup_stuck_session(&repo, project_id.clone(), 0, 1800, VerificationStatus::Reviewing)
            .await;

    // 2-hour-old session — would also be reset by scan_and_reset(false)
    let old_id =
        setup_stuck_session(&repo, project_id.clone(), 0, 7200, VerificationStatus::Reviewing)
            .await;

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(true).await;

    assert_eq!(count, 3, "all three sessions must be reset on cold boot");

    // ALL three sessions must be reset — cold boot ignores TTL
    for (id, label) in [
        (&recent_auto_id, "recent auto-verify (5 min)"),
        (&mid_manual_id, "mid manual-verify (30 min)"),
        (&old_id, "old session (2 hours)"),
    ] {
        let after = repo.get_by_id(id).await.unwrap().unwrap();
        assert_eq!(
            after.verification_status,
            VerificationStatus::Unverified,
            "{} must be reset to Unverified",
            label
        );
        assert!(
            !after.verification_in_progress,
            "{} in_progress must be cleared",
            label
        );
        assert_eq!(after.verification_convergence_reason, None);
    }
}

/// Cold boot must preserve ImportedVerified status.
#[tokio::test]
async fn test_scan_and_reset_cold_boot_skips_imported_verified() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // ImportedVerified session with in_progress=true (should not be touched)
    let imported_id = setup_stuck_session(
        &repo,
        project_id.clone(),
        0,
        300,
        VerificationStatus::ImportedVerified,
    )
    .await;

    // Normal in-progress session (should be reset)
    let normal_id =
        setup_stuck_session(&repo, project_id.clone(), 0, 300, VerificationStatus::Reviewing)
            .await;

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(true).await;

    assert_eq!(count, 1, "only the normal session should be reset");

    // ImportedVerified must be unchanged
    let imported_after = repo.get_by_id(&imported_id).await.unwrap().unwrap();
    assert_eq!(
        imported_after.verification_status,
        VerificationStatus::ImportedVerified,
        "ImportedVerified must not be changed by cold boot reset"
    );
    assert!(
        imported_after.verification_in_progress,
        "ImportedVerified in_progress must remain set"
    );

    // Normal session must be reset
    let normal_after = repo.get_by_id(&normal_id).await.unwrap().unwrap();
    assert_eq!(normal_after.verification_status, VerificationStatus::Unverified);
    assert!(!normal_after.verification_in_progress);
}

/// Orphaned verification children linked to reset parents must be archived on cold boot.
#[tokio::test]
async fn test_scan_and_reset_cold_boot_archives_orphaned_children() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent with in-progress verification (5 minutes old — below any TTL threshold)
    let parent_id =
        setup_stuck_session(&repo, project_id.clone(), 0, 300, VerificationStatus::Reviewing)
            .await;

    // Orphaned verification child session
    let mut child = IdeationSession::new(project_id.clone());
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(true).await;

    assert_eq!(count, 1, "parent session should be reset");

    // Parent must be reset and stale summary fields cleared.
    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::Unverified);
    assert!(!parent_after.verification_in_progress);
    assert_eq!(parent_after.verification_convergence_reason, None);

    // Child must be archived
    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "orphaned verification child must be archived during cold boot reset"
    );
}

/// Empty repo: scan_and_reset(cold_boot: true) is a no-op.
#[tokio::test]
async fn test_scan_and_reset_cold_boot_empty_repo_is_noop() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(true).await;
    assert_eq!(count, 0);
}

/// Cold boot clears stale pre-native verification state instead of carrying it forward.
#[tokio::test]
async fn test_scan_and_reset_cold_boot_clears_legacy_metadata() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session with existing metadata — cold boot should clear it
    let mut session = IdeationSession::new(project_id);
    session.verification_status = VerificationStatus::Reviewing;
    session.verification_in_progress = true;
    session.updated_at = Utc::now() - Duration::minutes(5);
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();
    repo.save_verification_run_snapshot(
        &session_id,
        &VerificationRunSnapshot {
            generation: 0,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 2,
            max_rounds: 5,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: vec![],
            rounds: vec![],
        },
    )
    .await
    .unwrap();

    let svc = make_service(repo.clone(), default_config());
    let count = svc.scan_and_reset(true).await;

    assert_eq!(count, 1);

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(after.verification_status, VerificationStatus::Unverified);
    assert!(!after.verification_in_progress);
    assert_eq!(after.verification_current_round, None);
    assert_eq!(after.verification_max_rounds, None);
    assert_eq!(after.verification_gap_count, 0);
    assert_eq!(after.verification_gap_score, None);
    assert_eq!(after.verification_convergence_reason, None);

    let snapshot = repo
        .get_verification_run_snapshot(&session_id, 0)
        .await
        .unwrap()
        .expect("active-generation snapshot should be reset alongside the summary");
    assert_eq!(snapshot.status, VerificationStatus::Unverified);
    assert!(!snapshot.in_progress);
    assert_eq!(snapshot.current_round, 0);
    assert_eq!(snapshot.max_rounds, 0);
    assert!(snapshot.current_gaps.is_empty());
    assert!(snapshot.rounds.is_empty());
    assert!(snapshot.convergence_reason.is_none());
}

// ---------------------------------------------------------------------------
// reset_verification_on_child_error tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_reset_verification_on_child_error_agent_error() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) = make_parent_child_pair(&repo, None).await;

    reset_verification_on_child_error::<tauri::Wry>(&child_id, &dyn_repo, None, "agent_error")
        .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::Unverified,
        "agent error should reset parent to Unverified"
    );
    assert!(
        !parent_after.verification_in_progress,
        "in_progress must be cleared after error reset"
    );

    assert_eq!(parent_after.verification_convergence_reason, None);

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must be archived after error reset"
    );
}

#[tokio::test]
async fn test_reset_verification_on_child_error_actionable_needs_revision_requests_auto_continue() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::NeedsRevision,
                true,
                2,
                5,
                None,
                &[3],
            )),
        )
        .await;
    repo.update_verification_state(&parent_id, VerificationStatus::NeedsRevision, true)
        .await
        .unwrap();

    let result = reset_verification_on_child_error::<tauri::Wry>(
        &child_id,
        &dyn_repo,
        None,
        "agent_error",
    )
    .await;

    match result {
        Some(ReconcileVerificationChildCompletion::AutoContinue(request)) => {
            assert_eq!(request.snapshot.status, VerificationStatus::NeedsRevision);
            assert!(request.snapshot.in_progress);
            assert!(
                request
                    .continuation_message
                    .contains("Continue the active verification loop in this same session")
            );
        }
        other => panic!("expected auto-continue outcome, got {:?}", other.map(|_| ())),
    }

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::NeedsRevision);
    assert!(
        parent_after.verification_in_progress,
        "auto-continue must keep parent verification marked in progress"
    );

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must stay active for same-session auto-continue"
    );
}

#[tokio::test]
async fn test_reset_verification_on_child_error_pending_review_round_requests_auto_continue() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) = make_parent_child_pair(
        &repo,
        Some(VerificationRunSnapshot {
            generation: 0,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 2,
            max_rounds: 5,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: vec![],
            rounds: vec![],
        }),
    )
    .await;
    repo.update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    repo.update_verification_state(&parent_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let result = reset_verification_on_child_error::<tauri::Wry>(
        &child_id,
        &dyn_repo,
        None,
        "agent_error",
    )
    .await;

    match result {
        Some(ReconcileVerificationChildCompletion::AutoContinue(request)) => {
            assert_eq!(request.snapshot.status, VerificationStatus::Reviewing);
            assert!(request.snapshot.in_progress);
            assert_eq!(request.snapshot.current_round, 2);
            assert!(
                request
                    .continuation_message
                    .contains("required verification critics may still be settling")
            );
        }
        other => panic!("expected auto-continue outcome, got {:?}", other.map(|_| ())),
    }

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::Reviewing);
    assert!(
        parent_after.verification_in_progress,
        "pending round must keep parent verification in progress"
    );

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must stay active while the pending round is being resumed"
    );
}

#[tokio::test]
async fn test_reset_verification_on_child_error_user_stopped() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) = make_parent_child_pair(&repo, None).await;

    reset_verification_on_child_error::<tauri::Wry>(&child_id, &dyn_repo, None, "user_stopped")
        .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::Unverified,
        "user stop should reset parent to Unverified"
    );
    assert!(!parent_after.verification_in_progress);

    assert_eq!(parent_after.verification_convergence_reason, None);
}

#[tokio::test]
async fn test_reset_verification_noop_for_non_verification_child() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    // Parent with in_progress=true
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = true;
    parent.verification_status = VerificationStatus::Reviewing;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Child is General purpose (NOT Verification) — should be a no-op
    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::General;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    reset_verification_on_child_error::<tauri::Wry>(&child_id, &dyn_repo, None, "agent_error")
        .await;

    // Parent should be unchanged (not a verification child)
    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::Reviewing);
    assert!(parent_after.verification_in_progress, "parent must be unchanged for general child");
}

#[tokio::test]
async fn test_reset_verification_imported_verified_guard() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    // Parent is ImportedVerified — must not be reset
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::ImportedVerified;
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    reset_verification_on_child_error::<tauri::Wry>(&child_id, &dyn_repo, None, "agent_error")
        .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::ImportedVerified,
        "ImportedVerified must not be overwritten by error reset"
    );
}

#[tokio::test]
async fn test_escalated_to_parent_maps_to_needs_revision() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::Reviewing,
                true,
                3,
                5,
                Some("escalated_to_parent"),
                &[],
            )),
        )
        .await;

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::NeedsRevision,
        "escalated_to_parent convergence should map to NeedsRevision"
    );
    assert!(
        !parent_after.verification_in_progress,
        "in_progress must be cleared after escalation"
    );
}

#[tokio::test]
async fn test_user_stopped_maps_to_skipped() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let dyn_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        repo.clone();

    let (parent_id, child_id) =
        make_parent_child_pair(
            &repo,
            Some(make_snapshot(
                0,
                VerificationStatus::Reviewing,
                true,
                2,
                5,
                Some("user_stopped"),
                &[],
            )),
        )
        .await;

    reconcile_verification_on_child_complete::<tauri::Wry>(
        &parent_id,
        &child_id,
        &dyn_repo,
        None,
    )
    .await;

    let parent_after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::Skipped,
        "user_stopped convergence should map to Skipped (not NeedsRevision)"
    );
    assert!(
        !parent_after.verification_in_progress,
        "in_progress must be cleared after user_stopped"
    );
}

// ---------------------------------------------------------------------------
// scan_and_archive_stale_external_sessions tests
// ---------------------------------------------------------------------------

/// Helper: create an external session with a given phase and created_at offset.
fn make_external_session(
    project_id: ProjectId,
    phase: &str,
    created_at_offset: Duration,
) -> IdeationSession {
    let mut session = IdeationSession::new(project_id);
    session.origin = SessionOrigin::External;
    session.external_activity_phase = Some(phase.to_string());
    session.created_at = Utc::now() - created_at_offset;
    session.updated_at = Utc::now() - created_at_offset;
    session
}

/// Cold boot also uses the TTL and preserves recent sessions.
#[tokio::test]
async fn test_startup_scan_uses_ttl_for_external_session_archival() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // External session in 'created' phase — just created (10 min ago)
    let recent = make_external_session(project_id.clone(), "created", Duration::minutes(10));
    let recent_id = recent.id.clone();
    repo.create(recent).await.unwrap();

    // External session in 'error' phase — 3 hours old
    let old_error = make_external_session(project_id.clone(), "error", Duration::hours(3));
    let old_error_id = old_error.id.clone();
    repo.create(old_error).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2 hours
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(true).await;

    // Recent session must survive; stale session must archive.
    let recent_after = repo.get_by_id(&recent_id).await.unwrap().unwrap();
    assert_eq!(
        recent_after.status,
        IdeationSessionStatus::Active,
        "recent 'created' session within TTL must NOT be archived on cold boot"
    );

    let old_error_after = repo.get_by_id(&old_error_id).await.unwrap().unwrap();
    assert_eq!(
        old_error_after.status,
        IdeationSessionStatus::Archived,
        "stale 'error' session past TTL must still be archived on cold boot"
    );
}

/// Cold boot: 10-minute-old `created` session within the 2h grace period must survive.
#[tokio::test]
async fn test_cold_boot_preserves_recent_created_session() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let session = make_external_session(project_id.clone(), "created", Duration::minutes(10));
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2h TTL
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(true).await; // cold boot

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "10-min-old 'created' session must NOT be archived on cold boot (within 2h TTL)"
    );
}

/// Cold boot: `created` session older than the 2h grace period must be archived.
#[tokio::test]
async fn test_cold_boot_archives_old_created_session() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let session = make_external_session(project_id.clone(), "created", Duration::hours(3));
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2h TTL
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(true).await; // cold boot

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Archived,
        "3h-old 'created' session must be archived on cold boot (past 2h TTL)"
    );
}

/// Cold boot: `error` session older than the 2h grace period must be archived.
#[tokio::test]
async fn test_cold_boot_archives_old_error_session() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let session = make_external_session(project_id.clone(), "error", Duration::hours(3));
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2h TTL
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(true).await; // cold boot

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Archived,
        "3h-old 'error' session must be archived on cold boot (past 2h TTL)"
    );
}

/// Periodic scan (`cold_boot=false`) continues to archive sessions past TTL, unchanged by the
/// cold-boot TTL fix.
#[tokio::test]
async fn test_periodic_scan_unchanged() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Old 'error' session — 4h (> 2h TTL): must be archived
    let old_error = make_external_session(project_id.clone(), "error", Duration::hours(4));
    let old_error_id = old_error.id.clone();
    repo.create(old_error).await.unwrap();

    // Old 'created' session — 3h (> 2h TTL): must be archived
    let old_created = make_external_session(project_id.clone(), "created", Duration::hours(3));
    let old_created_id = old_created.id.clone();
    repo.create(old_created).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2h TTL
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await; // periodic — NOT cold boot

    let old_error_after = repo.get_by_id(&old_error_id).await.unwrap().unwrap();
    assert_eq!(
        old_error_after.status,
        IdeationSessionStatus::Archived,
        "periodic scan: 4h-old 'error' session must be archived"
    );

    let old_created_after = repo.get_by_id(&old_created_id).await.unwrap().unwrap();
    assert_eq!(
        old_created_after.status,
        IdeationSessionStatus::Archived,
        "periodic scan: 3h-old 'created' session must be archived"
    );
}

/// Periodic scan archives sessions past the TTL, preserves recent ones.
#[tokio::test]
async fn test_periodic_scan_archives_past_ttl_preserves_recent() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Old 'created' session — 3 hours old (> 2h TTL)
    let old = make_external_session(project_id.clone(), "created", Duration::hours(3));
    let old_id = old.id.clone();
    repo.create(old).await.unwrap();

    // Recent 'created' session — 30 minutes old (< 2h TTL)
    let recent = make_external_session(project_id.clone(), "created", Duration::minutes(30));
    let recent_id = recent.id.clone();
    repo.create(recent).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2 hours
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let old_after = repo.get_by_id(&old_id).await.unwrap().unwrap();
    assert_eq!(
        old_after.status,
        IdeationSessionStatus::Archived,
        "old 'created' session past TTL must be archived"
    );

    let recent_after = repo.get_by_id(&recent_id).await.unwrap().unwrap();
    assert_eq!(
        recent_after.status,
        IdeationSessionStatus::Active,
        "recent 'created' session within TTL must NOT be archived"
    );
}

/// External sessions with active phases ('planning', 'verifying', etc.) are not archived.
#[tokio::test]
async fn test_periodic_scan_skips_sessions_with_active_phases() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session in 'planning' phase — old (3 hours) but not in archivable phase
    let planning = make_external_session(project_id.clone(), "planning", Duration::hours(3));
    let planning_id = planning.id.clone();
    repo.create(planning).await.unwrap();

    // Session in 'verifying' phase — old
    let verifying = make_external_session(project_id.clone(), "verifying", Duration::hours(3));
    let verifying_id = verifying.id.clone();
    repo.create(verifying).await.unwrap();

    // Session in 'created' phase — old (should be archived)
    let created = make_external_session(project_id.clone(), "created", Duration::hours(3));
    let created_id = created.id.clone();
    repo.create(created).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200,
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    // Only 'created' phase session should be archived
    let planning_after = repo.get_by_id(&planning_id).await.unwrap().unwrap();
    assert_eq!(
        planning_after.status,
        IdeationSessionStatus::Active,
        "'planning' phase sessions must not be archived by stale archival scan"
    );

    let verifying_after = repo.get_by_id(&verifying_id).await.unwrap().unwrap();
    assert_eq!(
        verifying_after.status,
        IdeationSessionStatus::Active,
        "'verifying' phase sessions must not be archived by stale archival scan"
    );

    let created_after = repo.get_by_id(&created_id).await.unwrap().unwrap();
    assert_eq!(
        created_after.status,
        IdeationSessionStatus::Archived,
        "'created' phase session past TTL must be archived"
    );
}

/// Internal sessions are never affected by stale external session archival.
#[tokio::test]
async fn test_stale_external_scan_skips_internal_sessions() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Internal session (default origin) — very old, no phase
    let mut internal = IdeationSession::new(project_id.clone());
    internal.created_at = Utc::now() - Duration::hours(10);
    internal.updated_at = Utc::now() - Duration::hours(10);
    let internal_id = internal.id.clone();
    repo.create(internal).await.unwrap();

    // External session in 'created' phase — old (should be archived)
    let external = make_external_session(project_id.clone(), "created", Duration::hours(3));
    let external_id = external.id.clone();
    repo.create(external).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200,
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let internal_after = repo.get_by_id(&internal_id).await.unwrap().unwrap();
    assert_eq!(
        internal_after.status,
        IdeationSessionStatus::Active,
        "internal sessions must not be archived by stale external session scan"
    );

    let external_after = repo.get_by_id(&external_id).await.unwrap().unwrap();
    assert_eq!(
        external_after.status,
        IdeationSessionStatus::Archived,
        "external 'created' session past TTL must be archived"
    );
}

/// Reopened external sessions (NULL phase) must not be re-archived by stale scan.
#[tokio::test]
async fn test_periodic_scan_skips_reopened_session_with_null_phase() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Create a session that looks "old" by created_at AND updated_at,
    // but has NULL external_activity_phase (simulating a manually reopened session).
    // After reopen, update_external_activity_phase(None) is called, clearing the phase.
    let mut session = IdeationSession::new(project_id.clone());
    session.origin = SessionOrigin::External;
    session.external_activity_phase = None; // NULL phase — reopened session
    session.created_at = Utc::now() - Duration::hours(5);
    session.updated_at = Utc::now() - Duration::hours(5);
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2 hours
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "reopened session with NULL phase must NOT be archived — phase IN ('created','error') filter excludes NULL"
    );
}

/// External sessions with recent updated_at must not be archived even if created_at is old.
#[tokio::test]
async fn test_periodic_scan_spares_session_with_recent_updated_at() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Old by created_at (5 hours), but updated_at is recent (10 minutes).
    // This simulates a session that has had recent activity (new messages, plan updates, etc.)
    let mut session = IdeationSession::new(project_id.clone());
    session.origin = SessionOrigin::External;
    session.external_activity_phase = Some("created".to_string());
    session.created_at = Utc::now() - Duration::hours(5);
    session.updated_at = Utc::now() - Duration::minutes(10); // fresh activity
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2 hours TTL
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "session with recent updated_at must NOT be archived — archival now uses updated_at not created_at"
    );
}

/// Verified external sessions remain visible even if their phase would otherwise be archivable.
#[tokio::test]
async fn test_stale_external_scan_preserves_verified_sessions() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let mut session = make_external_session(project_id, "created", Duration::hours(5));
    session.verification_status = VerificationStatus::Verified;
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200,
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "verified external session should be preserved even if phase is archivable"
    );
}

/// Cold boot preserves a verified session with 0 active proposals regardless of age.
///
/// Regression for `is_recovery_exempt()`: a verified session with no active proposals must
/// survive cold-boot archival even if it is older than the startup grace TTL. The user may
/// still need to act on the verification result.
///
/// Note: the memory repo always returns 0 for `count_active_proposals`, which is the correct
/// baseline — the no-proposals path is the primary recovery-exempt case.
#[tokio::test]
async fn test_cold_boot_preserves_verified_no_proposals_even_if_old() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Verified external session — 5 hours old (well past the 2h TTL)
    let mut session = make_external_session(project_id.clone(), "created", Duration::hours(5));
    session.verification_status = VerificationStatus::Verified;
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2 hours
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(true).await; // cold boot

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "verified external session with 0 proposals must NOT be archived on cold boot (recovery-exempt)"
    );
}

/// Cold boot preserves externally visible ideation sessions that startup recovery already claimed.
#[tokio::test]
async fn test_startup_scan_preserves_startup_recovery_claimed_external_sessions() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let session = make_external_session(project_id, "created", Duration::hours(5));
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200,
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    let mut claimed = HashSet::new();
    claimed.insert(session_id.as_str().to_string());
    svc.startup_scan_excluding_external_archive_sessions(&claimed)
        .await;

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "startup-recovery-claimed external session must not be archived during cold boot"
    );
}

/// Stall detection marks sessions with no recent activity as 'stalled'.
#[tokio::test]
async fn test_stall_detection_marks_sessions_stalled() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // External session in 'planning' phase — updated 3 hours ago (> 2h threshold)
    let mut stalled = make_external_session(project_id.clone(), "planning", Duration::hours(1));
    stalled.updated_at = Utc::now() - Duration::hours(3);
    let stalled_id = stalled.id.clone();
    repo.create(stalled).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2 hours
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let after = repo.get_by_id(&stalled_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "stalled session must remain Active (only phase changes, not status)"
    );
    assert_eq!(
        after.external_activity_phase.as_deref(),
        Some("stalled"),
        "stall detection must update phase to 'stalled'"
    );
}

/// Stall detection skips sessions with recent activity.
#[tokio::test]
async fn test_stall_detection_skips_recent_sessions() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // External session in 'planning' phase — recently updated (30 min ago)
    let mut active = make_external_session(project_id.clone(), "planning", Duration::hours(1));
    active.updated_at = Utc::now() - Duration::minutes(30);
    let active_id = active.id.clone();
    repo.create(active).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200, // 2 hours
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let after = repo.get_by_id(&active_id).await.unwrap().unwrap();
    assert_eq!(
        after.external_activity_phase.as_deref(),
        Some("planning"),
        "recently active session must not be marked stalled"
    );
}

/// Stall detection skips sessions already in 'error' or 'stalled' phase.
#[tokio::test]
async fn test_stall_detection_skips_error_and_already_stalled() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session already in 'error' — old, should not be re-marked
    let mut errored = make_external_session(project_id.clone(), "error", Duration::hours(1));
    errored.updated_at = Utc::now() - Duration::hours(5);
    let errored_id = errored.id.clone();
    repo.create(errored).await.unwrap();

    // Session already in 'stalled' — old, should not be re-marked
    let mut already_stalled =
        make_external_session(project_id.clone(), "stalled", Duration::hours(1));
    already_stalled.updated_at = Utc::now() - Duration::hours(5);
    let already_stalled_id = already_stalled.id.clone();
    repo.create(already_stalled).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200,
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(false).await;

    let errored_after = repo.get_by_id(&errored_id).await.unwrap().unwrap();
    assert_eq!(
        errored_after.external_activity_phase.as_deref(),
        Some("error"),
        "'error' phase must not be overwritten by stall detection"
    );

    let stalled_after = repo.get_by_id(&already_stalled_id).await.unwrap().unwrap();
    assert_eq!(
        stalled_after.external_activity_phase.as_deref(),
        Some("stalled"),
        "already 'stalled' phase must not be re-written"
    );
}

/// Cold boot does not run stall detection — only archival.
/// Sessions with active phases but old updated_at remain unchanged on cold boot.
#[tokio::test]
async fn test_cold_boot_does_not_run_stall_detection() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // External session in 'planning' — old enough that stall detection would mark it
    let mut planning = make_external_session(project_id.clone(), "planning", Duration::hours(1));
    planning.updated_at = Utc::now() - Duration::hours(5);
    let planning_id = planning.id.clone();
    repo.create(planning).await.unwrap();

    let config = VerificationReconciliationConfig {
        external_session_stale_secs: 7200,
        ..Default::default()
    };
    let svc = make_service(repo.clone(), config);
    svc.scan_and_archive_stale_external_sessions(true).await; // cold boot

    // 'planning' phase not in ('created', 'error') — not archived on cold boot
    // Stall detection skipped on cold boot — phase unchanged
    let after = repo.get_by_id(&planning_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "'planning' phase session must not be archived by cold boot (wrong phase)"
    );
    assert_eq!(
        after.external_activity_phase.as_deref(),
        Some("planning"),
        "cold boot must not run stall detection — phase must remain 'planning'"
    );
}

// ---------------------------------------------------------------------------
// Recovery scanner tests (scan_for_recoverable_orphans + startup_scan)
// ---------------------------------------------------------------------------

use crate::application::chat_service::MockChatService;
use crate::application::interactive_process_registry::InteractiveProcessRegistry;
use crate::application::reconciliation::recovery_queue::{
    create_recovery_queue, RecoveryQueueConfig,
};
use crate::domain::services::{MemoryRunningAgentRegistry, RunningAgentKey};

/// Build a service with registry + queue for recovery scanner tests.
/// Returns the service, registry, repo, and processor (keep alive to hold channel open).
async fn make_service_with_recovery(
    repo: Arc<MemoryIdeationSessionRepository>,
) -> (
    VerificationReconciliationService,
    Arc<MemoryRunningAgentRegistry>,
    crate::application::reconciliation::recovery_queue::RecoveryQueueProcessor,
) {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let mock_chat_service: Arc<dyn crate::application::chat_service::ChatService> =
        Arc::new(MockChatService::new());
    let (queue, processor) = create_recovery_queue(
        registry.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        ipr,
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        mock_chat_service,
        None,
        RecoveryQueueConfig::default(),
    );
    let svc = VerificationReconciliationService::new(
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        default_config(),
    )
    .with_recovery_queue(Arc::new(queue))
    .with_running_agent_registry(
        registry.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
    );
    (svc, registry, processor)
}

/// Create a parent ideation session with verification_in_progress = true.
fn make_parent_in_progress(project_id: ProjectId) -> IdeationSession {
    let mut s = IdeationSession::new(project_id);
    s.verification_in_progress = true;
    s.verification_status = VerificationStatus::Reviewing;
    s
}

/// Create a verification child session linked to a parent.
fn make_verification_child(
    project_id: ProjectId,
    parent_id: &crate::domain::entities::IdeationSessionId,
) -> IdeationSession {
    let mut child = IdeationSession::new(project_id.clone());
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    child
}

#[tokio::test]
async fn test_scan_for_recoverable_orphans_submits_dead_verification_agent() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Create parent with in-progress verification
    let parent = make_parent_in_progress(project_id.clone());
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Create verification child
    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, _processor) = make_service_with_recovery(repo.clone()).await;

    // Register child session in registry with pid=0 (dead)
    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    let claimed = svc.scan_for_recoverable_orphans().await;

    assert!(
        claimed.contains(parent_id.as_str()),
        "parent_id should be claimed when dead verification agent is found: claimed={claimed:?}"
    );
    assert_eq!(claimed.len(), 1);
}

#[tokio::test]
async fn test_scan_for_recoverable_orphans_skips_alive_process() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let parent = make_parent_in_progress(project_id.clone());
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, _processor) = make_service_with_recovery(repo.clone()).await;

    // Register with a real (alive) PID — current process is always alive
    registry
        .register(
            RunningAgentKey {
                context_type: "ideation".to_string(),
                context_id: child_id.as_str().to_string(),
            },
            std::process::id(),
            "test-conv".to_string(),
            "test-run".to_string(),
            None,
            None,
        )
        .await;

    let claimed = svc.scan_for_recoverable_orphans().await;

    assert!(
        claimed.is_empty(),
        "alive process should not be claimed for recovery"
    );
}

#[tokio::test]
async fn test_scan_for_recoverable_orphans_skips_non_verification_session() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Session is General purpose, not Verification
    let mut session = IdeationSession::new(project_id);
    session.session_purpose = crate::domain::entities::SessionPurpose::General;
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    let (svc, registry, _processor) = make_service_with_recovery(repo.clone()).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: session_id.as_str().to_string(),
        })
        .await;

    let claimed = svc.scan_for_recoverable_orphans().await;
    assert!(claimed.is_empty(), "General purpose session must not be claimed");
}

#[tokio::test]
async fn test_scan_for_recoverable_orphans_skips_archived_child() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let parent = make_parent_in_progress(project_id.clone());
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Child is Archived — should be skipped
    let mut child = make_verification_child(project_id, &parent_id);
    child.status = IdeationSessionStatus::Archived;
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, _processor) = make_service_with_recovery(repo.clone()).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    let claimed = svc.scan_for_recoverable_orphans().await;
    assert!(claimed.is_empty(), "Archived child must not be claimed");
}

#[tokio::test]
async fn test_scan_for_recoverable_orphans_skips_imported_verified_parent() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = true;
    parent.verification_status = VerificationStatus::ImportedVerified;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, _processor) = make_service_with_recovery(repo.clone()).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    let claimed = svc.scan_for_recoverable_orphans().await;
    assert!(
        claimed.is_empty(),
        "ImportedVerified parent must not be claimed for recovery"
    );
}

#[tokio::test]
async fn test_scan_for_recoverable_orphans_skips_parent_not_in_progress() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent has verification_in_progress = false — already resolved
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = false;
    parent.verification_status = VerificationStatus::Verified;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, _processor) = make_service_with_recovery(repo.clone()).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    let claimed = svc.scan_for_recoverable_orphans().await;
    assert!(
        claimed.is_empty(),
        "Parent not in progress must not be claimed for recovery"
    );
}

#[tokio::test]
async fn test_scan_and_reset_excluding_skips_claimed_parent_ids() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let parent = make_parent_in_progress(project_id);
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let svc = make_service(repo.clone(), default_config());

    // Build skip set containing the parent's ID
    let mut skip_set = std::collections::HashSet::new();
    skip_set.insert(parent_id.as_str().to_string());

    let count = svc.scan_and_reset_excluding(true, &skip_set).await;

    assert_eq!(count, 0, "claimed session must not be reset");

    let after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert!(
        after.verification_in_progress,
        "claimed session must remain in_progress — recovery owns it"
    );
}

#[tokio::test]
async fn test_startup_scan_without_registry_resets_all_in_progress() {
    // Backward compat: startup_scan without registry/queue falls through to full reset
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let parent = make_parent_in_progress(project_id);
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Service without registry or queue (test/degraded mode)
    let svc = make_service(repo.clone(), default_config());
    svc.startup_scan().await;

    let after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert!(
        !after.verification_in_progress,
        "Without registry, startup_scan should reset all in-progress sessions"
    );
}

#[tokio::test]
async fn test_startup_scan_claims_orphan_and_skips_reset() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let parent = make_parent_in_progress(project_id.clone());
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, _processor) = make_service_with_recovery(repo.clone()).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    svc.startup_scan().await;

    // The parent was claimed for recovery — must NOT be reset by startup_scan
    let after = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert!(
        after.verification_in_progress,
        "startup_scan must not reset a parent claimed by recovery scanner"
    );
}

#[tokio::test]
async fn test_scan_for_recoverable_orphans_no_registry_returns_empty() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    // Service with no registry
    let svc = make_service(repo.clone(), default_config());
    let claimed = svc.scan_for_recoverable_orphans().await;
    assert!(claimed.is_empty(), "Without registry, no orphans can be found");
}

// ---------------------------------------------------------------------------
// Integration tests: end-to-end verification recovery flow (PDM-172)
//
// These tests exercise the full pipeline:
//   startup_scan() → scan_for_recoverable_orphans() → RecoveryQueue submit
//   → RecoveryQueueProcessor → send_message (via MockChatService)
//
// Pattern: spawn processor, call startup_scan, drop service to close channel,
// await processor task. Deterministic — no sleeps or timers needed.
// ---------------------------------------------------------------------------

use crate::application::chat_service::ChatService;
use std::time::Duration as StdDuration;

/// Build a service wired with RecoveryQueue + an accessible Arc<MockChatService>
/// for integration tests that need to assert on send_message calls and message content.
///
/// Uses zero-delay config to avoid sleep overhead in tests.
async fn make_service_with_tracking(
    repo: Arc<MemoryIdeationSessionRepository>,
) -> (
    VerificationReconciliationService,
    Arc<MemoryRunningAgentRegistry>,
    crate::application::reconciliation::recovery_queue::RecoveryQueueProcessor,
    Arc<MockChatService>,
) {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let chat_service = Arc::new(MockChatService::new());
    let (queue, processor) = create_recovery_queue(
        registry.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        ipr,
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        chat_service.clone() as Arc<dyn ChatService>,
        None,
        RecoveryQueueConfig {
            delay_between_spawns: StdDuration::from_millis(0),
            ..RecoveryQueueConfig::default()
        },
    );
    let svc = VerificationReconciliationService::new(
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        default_config(),
    )
    .with_recovery_queue(Arc::new(queue))
    .with_running_agent_registry(
        registry.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
    );
    (svc, registry, processor, chat_service)
}

fn make_recovery_snapshot(generation: i32, n_rounds: usize) -> VerificationRunSnapshot {
    make_snapshot(
        generation,
        VerificationStatus::Reviewing,
        true,
        n_rounds as u32,
        5,
        None,
        &vec![0; n_rounds],
    )
}

// ---------------------------------------------------------------------------
// Test 1: Full recovery flow — round continuity + send_message called with prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_recovery_flow_sends_recovery_prompt_with_correct_round() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent in-progress with a native snapshot equivalent to 3 rounds (current_round == 3)
    // and verification_generation == 2.
    let mut parent = make_parent_in_progress(project_id.clone());
    parent.verification_generation = 2;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();
    repo.save_verification_run_snapshot(&parent_id, &make_recovery_snapshot(2, 3))
        .await
        .unwrap();

    // Verification child linked to parent
    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, processor, chat_service) =
        make_service_with_tracking(repo.clone()).await;

    // Register child with a dead pid (pid=0) to simulate stale running_agents row
    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    // Spawn processor BEFORE startup_scan (Constraint 9: receiver must be active)
    let proc_task = tokio::spawn(processor.run());

    svc.startup_scan().await;

    // Drop service to close channel → processor exits after draining pending items
    drop(svc);
    proc_task.await.expect("processor task must not panic");

    // Verify: send_message was called exactly once (agent was re-spawned)
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be called once for the orphaned verification agent"
    );

    // Verify: recovery prompt contains round 3 (round continuity — Proof Obligation 1)
    let messages = chat_service.get_sent_messages().await;
    assert_eq!(messages.len(), 1, "exactly one message must have been sent");
    assert!(
        messages[0].contains("Current round: 3"),
         "recovery prompt must include current_round=3 derived from the native verification snapshot; \
         got: {:?}",
        messages[0]
    );
    assert!(
        messages[0].contains("generation: 2"),
        "recovery prompt must include generation=2 (from verification_generation field); \
         got: {:?}",
        messages[0]
    );
    assert!(
        messages[0].contains("<recovery_note>"),
        "recovery prompt must contain <recovery_note> tag for ralphx-plan-verifier Phase 0 RECOVER; \
         got: {:?}",
        messages[0]
    );

    // Verify: parent remains in_progress — startup_scan must not reset it (recovery claimed it)
    let after_parent = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert!(
        after_parent.verification_in_progress,
        "parent must remain verification_in_progress=true — startup_scan must skip claimed sessions"
    );
    assert_eq!(
        after_parent.verification_status,
        VerificationStatus::Reviewing,
        "parent verification_status must stay Reviewing during recovery (no premature reset)"
    );

    // Verify: child was not archived (recovery was successful — MockChatService returns Ok)
    let after_child = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        after_child.status,
        IdeationSessionStatus::Active,
        "child must remain Active after successful recovery spawn"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Fallback on failure — parent reset to Unverified, child archived
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_recovery_fallback_on_send_message_failure() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let mut parent = make_parent_in_progress(project_id.clone());
    parent.verification_generation = 1;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();
    repo.save_verification_run_snapshot(&parent_id, &make_recovery_snapshot(1, 2))
        .await
        .unwrap();

    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, processor, chat_service) =
        make_service_with_tracking(repo.clone()).await;

    // Make send_message fail — simulates recovery spawn failure
    chat_service.set_available(false).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    let proc_task = tokio::spawn(processor.run());
    svc.startup_scan().await;
    drop(svc);
    proc_task.await.expect("processor task must not panic");

    // Fallback: parent must be reset to Unverified (matches current cold-boot behavior)
    let after_parent = repo.get_by_id(&parent_id).await.unwrap().unwrap();
    assert_eq!(
        after_parent.verification_status,
        VerificationStatus::Unverified,
        "parent must be reset to Unverified when recovery spawn fails"
    );
    assert!(
        !after_parent.verification_in_progress,
        "parent verification_in_progress must be cleared after recovery failure"
    );

    // Fallback: child must be archived (unrecoverable — spawn failed)
    let after_child = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        after_child.status,
        IdeationSessionStatus::Archived,
        "child must be archived when recovery spawn fails (Proof Obligation 3)"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Double-spawn prevention via processor cleanup_stale_entry
//
// After the processor runs and cleans the stale running_agents row,
// a second startup_scan finds no orphan → submits nothing → call_count stays at 1.
// This models the "no thundering herd on rapid restart" guarantee.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_double_spawn_prevention_processor_cleans_stale_entry() {
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let parent = make_parent_in_progress(project_id.clone());
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();
    repo.save_verification_run_snapshot(&parent_id, &make_recovery_snapshot(0, 1))
        .await
        .unwrap();

    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, processor, chat_service) =
        make_service_with_tracking(repo.clone()).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    // First startup_scan: discovers orphan, submits item to queue
    let proc_task = tokio::spawn(processor.run());
    svc.startup_scan().await;
    drop(svc);
    proc_task.await.expect("processor task must not panic on first pass");

    // After first pass: stale running_agents row was cleaned by cleanup_stale_entry
    // in the processor. A second startup_scan must find no orphan.
    // Build a fresh service (no queue/registry changes — registry entry was cleaned).
    let registry2 = Arc::new(MemoryRunningAgentRegistry::new());
    let ipr2 = Arc::new(InteractiveProcessRegistry::new());
    let (queue2, processor2) = create_recovery_queue(
        registry2.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        ipr2,
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        chat_service.clone() as Arc<dyn ChatService>,
        None,
        RecoveryQueueConfig {
            delay_between_spawns: StdDuration::from_millis(0),
            ..RecoveryQueueConfig::default()
        },
    );
    // Use the ORIGINAL registry (with entry already cleaned) for the second scan
    let svc2 = VerificationReconciliationService::new(
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        default_config(),
    )
    .with_recovery_queue(Arc::new(queue2))
    .with_running_agent_registry(
        registry.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
    );

    let proc_task2 = tokio::spawn(processor2.run());
    svc2.startup_scan().await;
    drop(svc2);
    proc_task2.await.expect("processor task must not panic on second pass");

    // Second pass found no orphan (entry cleaned) — only 1 send_message call total
    assert_eq!(
        chat_service.call_count(),
        1,
        "send_message must be called exactly once across both startup_scans — \
         double-spawn prevented by cleanup_stale_entry removing the registry entry after first recovery"
    );

    // Suppress unused warning for registry2
    drop(registry2);
}

// ---------------------------------------------------------------------------
// Test 4: Parent archived edge case — no recovery attempted, child falls through to reset
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_recovery_skips_orphan_when_parent_not_in_progress() {
    // Edge case: verification child is orphaned, but its parent has
    // verification_in_progress=false (parent already resolved). This scenario
    // arises when the parent was separately reset after the child's agent died.
    // Recovery must be skipped — child falls through to the reset pass.
    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent with verification already resolved (not in progress)
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = false;
    parent.verification_status = VerificationStatus::Unverified;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Child still "running" (stale entry) with this resolved parent
    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let (svc, registry, processor, chat_service) =
        make_service_with_tracking(repo.clone()).await;

    registry
        .set_running(RunningAgentKey {
            context_type: "ideation".to_string(),
            context_id: child_id.as_str().to_string(),
        })
        .await;

    let proc_task = tokio::spawn(processor.run());
    svc.startup_scan().await;
    drop(svc);
    proc_task.await.expect("processor task must not panic");

    // No recovery attempted — parent was not in_progress
    assert_eq!(
        chat_service.call_count(),
        0,
        "send_message must NOT be called when parent is not verification_in_progress"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Mixed recovery — IdeationAgent (priority=10) + VerificationAgent (priority=5)
//
// Both kinds are submitted to the same queue. Priority ordering ensures the
// IdeationAgent item is processed first (higher priority). The VerificationAgent
// item is processed second and triggers send_message. IdeationAgent processing
// is currently a no-op placeholder (PDM-171), so only one send_message call expected.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mixed_recovery_verification_agent_spawned_after_ideation_placeholder() {
    use crate::application::reconciliation::recovery_queue::{
        RecoveryItem, RecoveryKind, RecoveryMetadata,
    };
    use crate::domain::entities::ChatContextType;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let parent = make_parent_in_progress(project_id.clone());
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();
    repo.save_verification_run_snapshot(&parent_id, &make_recovery_snapshot(0, 2))
        .await
        .unwrap();

    let child = make_verification_child(project_id, &parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let chat_service = Arc::new(MockChatService::new());
    let (queue, processor) = create_recovery_queue(
        registry.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
        ipr,
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        chat_service.clone() as Arc<dyn ChatService>,
        None,
        RecoveryQueueConfig {
            delay_between_spawns: StdDuration::from_millis(0),
            ..RecoveryQueueConfig::default()
        },
    );

    // Manually submit an IdeationAgent item at higher priority (simulates PDM-171 orphan)
    let ideation_item = RecoveryItem {
        context_type: ChatContextType::Ideation,
        context_id: "orphaned-ideation-session".to_string(),
        recovery_kind: RecoveryKind::IdeationAgent,
        priority: 10, // higher — processed first
        parent_session_id: None,
        metadata: RecoveryMetadata::default(),
    };
    queue.submit(ideation_item).expect("ideation item submit must succeed");

    // Also submit a VerificationAgent item at lower priority
    let verification_item = RecoveryItem {
        context_type: ChatContextType::Ideation,
        context_id: child_id.as_str().to_string(),
        recovery_kind: RecoveryKind::VerificationAgent,
        priority: 5, // lower — processed second
        parent_session_id: Some(parent_id.as_str().to_string()),
        metadata: RecoveryMetadata {
            current_round: Some(2),
            verification_generation: Some(0),
            conversation_id: Some("test-conv-id".to_string()),
            plan_artifact_id: None,
        },
    };
    queue.submit(verification_item).expect("verification item submit must succeed");

    // Wrap queue in Arc for service (not actually needed for scanning — we submitted manually)
    let svc = VerificationReconciliationService::new(
        repo.clone() as Arc<dyn IdeationSessionRepository>,
        default_config(),
    )
    .with_recovery_queue(Arc::new(queue))
    .with_running_agent_registry(
        registry.clone() as Arc<dyn crate::domain::services::RunningAgentRegistry>,
    );

    let proc_task = tokio::spawn(processor.run());
    drop(svc); // close channel immediately — both items already in buffer
    proc_task.await.expect("processor task must not panic");

    // IdeationAgent is a no-op placeholder — only VerificationAgent triggers send_message
    assert_eq!(
        chat_service.call_count(),
        1,
        "only the VerificationAgent item should trigger send_message; \
         IdeationAgent is a PDM-171 placeholder"
    );

    // Verify the recovery prompt was for round 2 (from the VerificationAgent item metadata)
    let messages = chat_service.get_sent_messages().await;
    assert_eq!(messages.len(), 1, "exactly one recovery message expected");
    assert!(
        messages[0].contains("Current round: 2"),
        "VerificationAgent recovery prompt must include round from item metadata; got: {:?}",
        messages[0]
    );
}

// ---------------------------------------------------------------------------
// archive_resolved_parent_orphans tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_archive_resolved_parent_orphans_archives_when_parent_resolved() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent with verification_in_progress=false (already resolved)
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = false;
    parent.verification_status = VerificationStatus::Verified;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Active verification child linked to that parent
    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    svc.archive_resolved_parent_orphans().await;

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must be archived when parent's verification_in_progress=false"
    );
}

#[tokio::test]
async fn test_archive_resolved_parent_orphans_skips_when_parent_active() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent with verification_in_progress=true (verification still running)
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = true;
    parent.verification_status = VerificationStatus::Reviewing;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Active verification child linked to that parent
    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    svc.archive_resolved_parent_orphans().await;

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must NOT be archived when parent's verification_in_progress=true"
    );
}

#[tokio::test]
async fn test_archive_resolved_parent_orphans_prefers_native_active_round_over_stale_parent_summary() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = false;
    parent.verification_status = VerificationStatus::Unverified;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();
    repo.save_verification_run_snapshot(
        &parent_id,
        &VerificationRunSnapshot {
            generation: 0,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 1,
            max_rounds: 5,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: vec![],
            rounds: vec![],
        },
    )
    .await
    .unwrap();
    repo.update_verification_state(&parent_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    svc.archive_resolved_parent_orphans().await;

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "native in-progress round state must prevent orphan archival even when the parent summary is stale"
    );
}

#[tokio::test]
async fn test_archive_resolved_parent_orphans_preserves_queued_verification_child() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = false;
    parent.verification_status = VerificationStatus::Unverified;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    child.pending_initial_prompt = Some("queued verify".to_string());
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    svc.archive_resolved_parent_orphans().await;

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "queued verification child must remain active for drain/hydration"
    );
    assert_eq!(child_after.pending_initial_prompt.as_deref(), Some("queued verify"));
}

#[tokio::test]
async fn test_archive_resolved_parent_orphans_archives_when_parent_not_found() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Child with a parent_session_id that doesn't exist in the DB
    let nonexistent_parent_id =
        crate::domain::entities::IdeationSessionId::from_string("nonexistent-parent-id".to_string());

    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(nonexistent_parent_id);
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    svc.archive_resolved_parent_orphans().await;

    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "child must be archived when parent session no longer exists"
    );
}

#[tokio::test]
async fn test_archive_resolved_parent_orphans_no_active_children_is_noop() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Only a regular (non-verification) session
    let session = IdeationSession::new(project_id);
    repo.create(session).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    // Should complete without error and no panics
    svc.archive_resolved_parent_orphans().await;
}

#[tokio::test]
async fn test_archive_resolved_parent_orphans_already_archived_child_not_double_archived() {
    let repo = Arc::new(crate::infrastructure::memory::MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Parent already resolved
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_in_progress = false;
    parent.verification_status = VerificationStatus::Verified;
    let parent_id = parent.id.clone();
    repo.create(parent).await.unwrap();

    // Child already archived — should not appear in list_active_verification_children
    let mut child = IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    child.status = crate::domain::entities::IdeationSessionStatus::Archived;
    let child_id = child.id.clone();
    repo.create(child).await.unwrap();

    let svc = make_service(repo.clone(), default_config());
    svc.archive_resolved_parent_orphans().await;

    // Child was already archived — should remain archived (no double-archive error)
    let child_after = repo.get_by_id(&child_id).await.unwrap().unwrap();
    assert_eq!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived,
        "already-archived child must remain archived"
    );
}
