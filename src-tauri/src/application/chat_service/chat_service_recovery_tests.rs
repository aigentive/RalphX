use super::build_ideation_recovery_metadata;

use std::sync::Arc;

use crate::domain::entities::{IdeationSession, ProjectId, VerificationStatus};
use crate::domain::repositories::{IdeationSessionRepository, TaskProposalRepository};
use crate::infrastructure::memory::{
    MemoryIdeationSessionRepository, MemoryTaskProposalRepository,
};

fn make_repos() -> (
    Arc<MemoryIdeationSessionRepository>,
    Arc<MemoryTaskProposalRepository>,
) {
    (
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryTaskProposalRepository::new()),
    )
}

#[tokio::test]
async fn test_recovery_metadata_includes_verification_fields_when_in_progress() {
    let (session_repo, proposal_repo) = make_repos();
    let project_id = ProjectId::new();

    let mut session = IdeationSession::new(project_id);
    session.verification_status = VerificationStatus::Reviewing;
    session.verification_in_progress = true;
    session.verification_current_round = Some(2);

    let session_id = session.id.clone();
    session_repo.create(session).await.unwrap();

    let session_repo_dyn: Arc<dyn IdeationSessionRepository> =
        session_repo.clone() as Arc<dyn IdeationSessionRepository>;
    let proposal_repo_dyn: Arc<dyn TaskProposalRepository> =
        proposal_repo as Arc<dyn TaskProposalRepository>;

    let metadata = build_ideation_recovery_metadata(
        session_id.as_str(),
        Some(&session_repo_dyn),
        Some(&proposal_repo_dyn),
        None::<&tauri::AppHandle>,
    )
    .await;

    assert!(metadata.is_some(), "metadata must be returned for valid session");
    let m = metadata.unwrap();
    assert_eq!(m.verification_status, "reviewing");
    assert!(m.verification_in_progress, "must capture in_progress=true before reset");
    assert_eq!(m.current_round, 2, "must extract current_round from summary fields");

    // Recovery resets verification state when in_progress=true
    let after = session_repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.verification_status,
        VerificationStatus::Unverified,
        "verification_status must be reset after recovery"
    );
    assert!(
        !after.verification_in_progress,
        "verification_in_progress must be cleared after recovery"
    );
}

#[tokio::test]
async fn test_recovery_metadata_no_reset_when_not_in_progress() {
    let (session_repo, proposal_repo) = make_repos();
    let project_id = ProjectId::new();

    let mut session = IdeationSession::new(project_id);
    session.verification_status = VerificationStatus::Verified;
    session.verification_in_progress = false;

    let session_id = session.id.clone();
    session_repo.create(session).await.unwrap();

    let session_repo_dyn: Arc<dyn IdeationSessionRepository> =
        session_repo.clone() as Arc<dyn IdeationSessionRepository>;
    let proposal_repo_dyn: Arc<dyn TaskProposalRepository> =
        proposal_repo as Arc<dyn TaskProposalRepository>;

    let metadata = build_ideation_recovery_metadata(
        session_id.as_str(),
        Some(&session_repo_dyn),
        Some(&proposal_repo_dyn),
        None::<&tauri::AppHandle>,
    )
    .await;

    assert!(metadata.is_some());
    let m = metadata.unwrap();
    assert_eq!(m.verification_status, "verified");
    assert!(!m.verification_in_progress);
    assert_eq!(m.current_round, 0, "current_round is 0 when no summary is present");

    // Status must NOT be reset since verification was not in-progress
    let after = session_repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.verification_status,
        VerificationStatus::Verified,
        "verification_status must be preserved when not in-progress"
    );
}

#[tokio::test]
async fn test_recovery_metadata_returns_none_for_missing_session() {
    let (session_repo, proposal_repo) = make_repos();

    let session_repo_dyn: Arc<dyn IdeationSessionRepository> =
        session_repo as Arc<dyn IdeationSessionRepository>;
    let proposal_repo_dyn: Arc<dyn TaskProposalRepository> =
        proposal_repo as Arc<dyn TaskProposalRepository>;

    let metadata = build_ideation_recovery_metadata(
        "nonexistent-session-id",
        Some(&session_repo_dyn),
        Some(&proposal_repo_dyn),
        None::<&tauri::AppHandle>,
    )
    .await;

    assert!(metadata.is_none(), "must return None for missing sessions");
}

#[tokio::test]
async fn test_recovery_metadata_returns_none_when_repos_absent() {
    let metadata = build_ideation_recovery_metadata("any-id", None, None, None::<&tauri::AppHandle>).await;
    assert!(metadata.is_none(), "must return None when repos are not provided");
}
