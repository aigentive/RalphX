use crate::domain::entities::{IdeationSession, VerificationRunSnapshot, VerificationStatus};
use crate::domain::repositories::IdeationSessionRepository;
use crate::error::AppResult;

pub fn build_blank_verification_snapshot(
    generation: i32,
    status: VerificationStatus,
    in_progress: bool,
) -> VerificationRunSnapshot {
    VerificationRunSnapshot {
        generation,
        status,
        in_progress,
        current_round: 0,
        max_rounds: 0,
        best_round_index: None,
        convergence_reason: None,
        current_gaps: Vec::new(),
        rounds: Vec::new(),
    }
}

pub fn clear_verification_snapshot(
    snapshot: &mut VerificationRunSnapshot,
    status: VerificationStatus,
    in_progress: bool,
) {
    snapshot.status = status;
    snapshot.in_progress = in_progress;
    snapshot.current_round = 0;
    snapshot.max_rounds = 0;
    snapshot.best_round_index = None;
    snapshot.convergence_reason = None;
    snapshot.current_gaps.clear();
    snapshot.rounds.clear();
}

pub async fn load_current_verification_snapshot_or_default<R>(
    repo: &R,
    session: &IdeationSession,
    status: VerificationStatus,
    in_progress: bool,
) -> AppResult<VerificationRunSnapshot>
where
    R: IdeationSessionRepository + ?Sized,
{
    Ok(repo
        .get_verification_run_snapshot(&session.id, session.verification_generation)
        .await?
        .unwrap_or_else(|| {
            build_blank_verification_snapshot(
                session.verification_generation,
                status,
                in_progress,
            )
        }))
}

pub async fn load_effective_verification_status<R>(
    repo: &R,
    session: &IdeationSession,
) -> AppResult<(VerificationStatus, bool)>
where
    R: IdeationSessionRepository + ?Sized,
{
    Ok(repo
        .get_verification_status(&session.id)
        .await?
        .unwrap_or((session.verification_status, session.verification_in_progress)))
}
