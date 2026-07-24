use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rusqlite::Connection;
use tracing::error;

use super::*;
use crate::application::verification_event_emitters::emit_verification_status_changed;
use crate::domain::entities::{
    Artifact, ArtifactBucketId, ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactRelation,
    ArtifactRelationId, ArtifactRelationType, ArtifactType, IdeationSession, IdeationSessionFlow,
    IdeationSessionId, VerificationStatus,
};
use crate::domain::repositories::IdeationSessionRepository;
use crate::domain::services::running_agent_registry::{RunningAgentKey, RunningAgentRegistry};
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    SqliteArtifactRepository as ArtifactRepo, SqliteIdeationSessionRepository as SessionRepo,
    SqliteTaskProposalRepository as ProposalRepo,
};

mod approval;
mod create;
mod edit;
mod events;
mod linking;
mod query;
mod shared;
mod team_artifacts;
#[cfg(test)]
mod team_artifacts_tests;
mod update;

pub use approval::approve_plan_artifact;
pub use create::{create_plan_artifact, create_plan_artifact_with_headers};
pub use edit::edit_plan_artifact;
pub use linking::link_proposals_to_plan;
pub use query::{get_artifact_history, get_session_plan};
pub use shared::{apply_edits, check_verification_freeze, EditError};
pub use team_artifacts::{create_team_artifact, get_team_artifacts};
pub use update::update_plan_artifact;

use events::emit_plan_update_events;
use shared::{
    attach_plan_approval, delete_current_bundle_relation_sync, finalize_plan_update, map_app_err,
    next_artifact_version_sync, plan_approval_view_sync, reconcile_plan_notifications,
    resolve_artifact_mutation_authority, resolve_caller_session_id,
    retarget_verification_authority_sync, PlanApprovalView,
};
