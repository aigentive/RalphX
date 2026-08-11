// External API handlers — Phase 4 + Phase 5
//
// These endpoints are exposed to external consumers (via the external MCP server)
// and require API key authentication + project scope enforcement.
//
// All endpoints extract `ProjectScope` from the X-RalphX-Project-Scope header
// (injected by the external MCP server) and enforce scope boundaries via
// `ProjectScopeGuard::assert_project_scope`.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::error;

use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::application::task_cleanup_service::TaskCleanupService;
use crate::commands::ideation_commands::{apply_proposals_core, ApplyProposalsInput};
use crate::domain::entities::{
    ideation::IdeationSession, task::Task, types::ProjectId, ChatContextType, IdeationSessionFlow,
    IdeationSessionId, InternalStatus, SessionOrigin, TaskId,
};
use crate::domain::services::text_similarity::{jaccard_similarity, tokenize_for_similarity};
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use ralphx_domain::entities::EventType;

use super::{HttpError, HttpServerState};

mod attention_capacity;
mod ideation_runtime;
mod ideation_start;
mod pipeline_overview;
mod projects;
mod sessions;
mod stream_events;
mod task_batch;
mod task_operations;
mod webhooks;

pub use attention_capacity::*;
pub use ideation_runtime::*;
pub use ideation_start::*;
pub use pipeline_overview::*;
pub use projects::*;
pub use sessions::*;
pub use stream_events::*;
pub use task_batch::*;
pub use task_operations::*;
pub use webhooks::*;

/// SQLite error message fragment for UNIQUE constraint violations.
/// Used to detect idempotency key race conditions on concurrent inserts.
pub(crate) const SQLITE_UNIQUE_VIOLATION: &str = "unique";
