use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use tracing::error;

use crate::application::chat_service::ChatService;
use crate::domain::entities::{
    ChatContextType, IdeationSession, IdeationSessionId, IdeationSessionStatus, SessionLink,
    SessionPurpose, SessionRelationship, VerificationStatus,
};
use crate::application::verification_event_emitters::{emit_verification_started, emit_verification_status_changed};
use crate::infrastructure::agents::claude::{
    get_team_constraints, team_constraints_config, validate_child_team_config, TeamConstraints,
};
use crate::infrastructure::sqlite::SqliteIdeationSessionRepository as SessionRepo;

use super::super::types::{
    CreateChildSessionRequest, CreateChildSessionResponse, HttpServerState, ParentContextResponse,
    ParentProposalSummary, ParentSessionSummary, TeamConfigInput,
};

mod create;
mod parent_context;
mod shared;
mod verification;

pub use create::create_child_session;
pub use parent_context::get_parent_session_context;
pub use shared::{session_is_team_mode, synthesize_verification_prompt};
pub(crate) use shared::build_ideation_chat_service;
pub(crate) use verification::create_verification_child_session;

use shared::{
    json_error, load_parent_context, rollback_verification_state, validate_resolved_team_config,
};

type JsonError = (StatusCode, Json<serde_json::Value>);
