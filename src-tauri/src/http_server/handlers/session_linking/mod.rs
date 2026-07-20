use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use tracing::error;

use crate::application::chat_service::ChatService;
use crate::application::verification_event_emitters::emit_verification_status_changed;
use crate::domain::entities::{
    ChatContextType, IdeationSession, IdeationSessionId, IdeationSessionStatus, SessionLink,
    SessionPurpose, SessionRelationship, VerificationStatus,
};
use crate::infrastructure::sqlite::SqliteIdeationSessionRepository as SessionRepo;

use super::super::types::{
    CreateChildSessionRequest, CreateChildSessionResponse, HttpServerState, ParentContextResponse,
    ParentProposalSummary, ParentSessionSummary,
};

mod create;
mod parent_context;
mod shared;

pub use create::create_child_session;
pub use parent_context::get_parent_session_context;
pub(crate) use shared::build_ideation_chat_service;
pub use shared::synthesize_verification_prompt;

use shared::{json_error, load_parent_context, rollback_verification_state};

type JsonError = (StatusCode, Json<serde_json::Value>);
