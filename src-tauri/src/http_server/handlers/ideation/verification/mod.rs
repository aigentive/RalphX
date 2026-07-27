use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::error;

use crate::application::app_state::AppState;
use crate::application::chat_service::{AgentRunCompletedPayload, ChatService, SendMessageOptions};
use crate::application::verification_event_emitters::emit_verification_status_changed;
use crate::application::InteractiveProcessKey;
use crate::domain::entities::{ChatContextType, IdeationSessionId, IdeationSessionStatus};
use crate::domain::repositories::ExternalEventsRepository;
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::domain::state_machine::services::WebhookPublisher;
use crate::error::AppError;
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use crate::http_server::types::{
    HttpServerState, RevertAndSkipRequest, SuccessResponse, UpdateVerificationRequest,
    VerificationInfraFailureRequest, VerificationResponse,
};

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
