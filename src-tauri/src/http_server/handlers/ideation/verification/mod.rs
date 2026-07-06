use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::error;

use crate::application::app_state::AppState;
use crate::application::chat_service::{AgentRunCompletedPayload, ChatService, SendMessageOptions};
use crate::application::InteractiveProcessKey;
use crate::domain::entities::{
    ChatContextType, IdeationSession, IdeationSessionId, IdeationSessionStatus,
    VerificationRunSnapshot, VerificationStatus,
};
use crate::domain::repositories::ExternalEventsRepository;
use crate::domain::repositories::IdeationSessionRepository;
use crate::application::verification_event_emitters::emit_verification_status_changed;
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::domain::state_machine::services::WebhookPublisher;
use crate::error::{AppError, AppResult};
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use crate::http_server::types::{
    HttpServerState, RevertAndSkipRequest, SuccessResponse, UpdateVerificationRequest,
    VerificationInfraFailureRequest, VerificationResponse,
};

use super::super::session_linking::session_is_team_mode;
use super::{json_error, JsonError};

mod auto_propose;
mod lifecycle;
mod query;
mod update;

#[doc(hidden)]
pub use self::auto_propose::auto_propose_with_retry;
pub use self::lifecycle::{mark_verification_infra_failure, revert_and_skip, stop_verification};
pub use self::query::get_plan_verification;
pub use self::update::post_verification_status;

use self::auto_propose::auto_propose_for_external;
pub(crate) use self::lifecycle::stop_verification_children;
pub(crate) use self::lifecycle::{stop_and_archive_children, ChildFilter};

#[derive(Debug, Clone)]
pub(crate) struct VerificationChildState {
    pub latest_child: Option<IdeationSession>,
    pub has_active_child: bool,
}

pub(crate) async fn load_verification_child_state(
    repo: &Arc<dyn IdeationSessionRepository>,
    session_id: &IdeationSessionId,
) -> AppResult<VerificationChildState> {
    let latest_child = repo.get_latest_verification_child(session_id).await?;
    let has_active_child = !repo.get_verification_children(session_id).await?.is_empty();
    Ok(VerificationChildState {
        latest_child,
        has_active_child,
    })
}

pub(crate) fn is_blank_in_progress_snapshot(snapshot: &VerificationRunSnapshot) -> bool {
    snapshot.status == VerificationStatus::Reviewing
        && snapshot.in_progress
        && snapshot.current_gaps.is_empty()
        && snapshot.rounds.is_empty()
        && snapshot.convergence_reason.is_none()
}

pub(crate) fn is_blank_orphaned_active_generation(
    summary_in_progress: bool,
    snapshot: Option<&VerificationRunSnapshot>,
    child_state: &VerificationChildState,
) -> bool {
    !summary_in_progress
        && !child_state.has_active_child
        && snapshot.is_some_and(is_blank_in_progress_snapshot)
}

pub(crate) async fn repair_blank_orphaned_verification_generation(
    app_state: &AppState,
    session: &IdeationSession,
) -> AppResult<bool> {
    if !session.verification_in_progress {
        return Ok(false);
    }

    let child_state =
        load_verification_child_state(&app_state.ideation_session_repo, &session.id).await?;
    let latest_child_archived = child_state
        .latest_child
        .as_ref()
        .is_some_and(|child| child.status == IdeationSessionStatus::Archived);
    if child_state.has_active_child || !latest_child_archived {
        return Ok(false);
    }

    let Some(snapshot) = app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session.id, session.verification_generation)
        .await?
    else {
        return Ok(false);
    };

    if !is_blank_in_progress_snapshot(&snapshot) {
        return Ok(false);
    }

    let mut repaired_snapshot = snapshot;
    crate::domain::services::clear_verification_snapshot(
        &mut repaired_snapshot,
        VerificationStatus::Unverified,
        false,
    );
    app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session.id, &repaired_snapshot)
        .await?;

    tracing::info!(
        session_id = %session.id.as_str(),
        generation = session.verification_generation,
        "Repaired blank orphaned verification generation before fresh start"
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::AppState;
    use crate::domain::entities::{IdeationSessionBuilder, ProjectId, SessionPurpose};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_repair_blank_orphaned_verification_generation_resets_summary_and_snapshot() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::new();

        let parent = IdeationSessionBuilder::new()
            .project_id(project_id.clone())
            .verification_generation(20)
            .build();
        let parent_id = parent.id.clone();
        app_state
            .ideation_session_repo
            .create(parent)
            .await
            .expect("parent should be created");
        app_state
            .ideation_session_repo
            .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
            .await
            .expect("parent should be marked reviewing");
        app_state
            .ideation_session_repo
            .save_verification_run_snapshot(
                &parent_id,
                &VerificationRunSnapshot {
                    generation: 20,
                    status: VerificationStatus::Reviewing,
                    in_progress: true,
                    current_round: 0,
                    max_rounds: 5,
                    best_round_index: None,
                    convergence_reason: None,
                    current_gaps: vec![],
                    rounds: vec![],
                },
            )
            .await
            .expect("blank active snapshot should persist");

        let child = IdeationSessionBuilder::new()
            .project_id(project_id)
            .parent_session_id(parent_id.clone())
            .session_purpose(SessionPurpose::Verification)
            .build();
        let child_id = child.id.clone();
        app_state
            .ideation_session_repo
            .create(child)
            .await
            .expect("verification child should be created");
        app_state
            .ideation_session_repo
            .update_status(&child_id, IdeationSessionStatus::Archived)
            .await
            .expect("verification child should be archived");

        let parent = app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .expect("parent should load")
            .expect("parent must exist");
        let repaired = repair_blank_orphaned_verification_generation(&app_state, &parent)
            .await
            .expect("repair should succeed");
        assert!(repaired, "blank orphaned generation must be repaired");

        let parent_after = app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .expect("parent should reload")
            .expect("parent must still exist");
        assert_eq!(
            parent_after.verification_status,
            VerificationStatus::Unverified
        );
        assert!(
            !parent_after.verification_in_progress,
            "repair must clear parent in-progress summary"
        );

        let snapshot_after = app_state
            .ideation_session_repo
            .get_verification_run_snapshot(&parent_id, 20)
            .await
            .expect("snapshot should reload")
            .expect("snapshot must still exist");
        assert_eq!(snapshot_after.status, VerificationStatus::Unverified);
        assert!(
            !snapshot_after.in_progress,
            "repair must clear native in-progress state"
        );
        assert_eq!(snapshot_after.current_round, 0);
        assert_eq!(snapshot_after.max_rounds, 0);
        assert!(snapshot_after.current_gaps.is_empty());
        assert!(snapshot_after.rounds.is_empty());
    }
}
