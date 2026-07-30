use crate::application::chat_service::chat_service_context::build_initial_prompt;
use crate::domain::entities::ChatContextType;
use crate::infrastructure::agents::claude::format_stream_json_input;

pub mod agent_followups;
pub mod agent_issues;
pub mod agent_tasks;
#[cfg(test)]
mod agent_tasks_tests;
pub mod agent_workflows;
#[cfg(test)]
mod agent_workflows_tests;
pub use agent_workflows::*;
pub mod agent_workspace_review_approval;
pub mod agent_workspaces;
pub mod api_keys;
pub mod artifacts;
pub mod automations;
pub mod branch_update;
pub mod conversations;
pub mod coordination;
pub mod execution;
pub mod external;
pub mod external_auth;
pub mod git;
pub mod ideation;
pub mod internal;
pub mod managed_team;
pub mod issues;
pub mod memory;
pub mod permissions;
pub mod personas;
pub mod plan_complexity;
pub mod projects;
pub mod questions;
pub mod reviews;
pub mod session_linking;
pub mod steps;
pub mod tasks;
pub mod ticket_attachments;
#[cfg(test)]
mod ticket_attachments_tests;
pub mod validation;
pub mod verification;
pub mod worker;

pub use agent_followups::*;
pub use agent_issues::*;
pub use agent_tasks::*;
pub use agent_workspace_review_approval::*;
pub use agent_workspaces::*;
pub use api_keys::*;
pub use artifacts::*;
pub use automations::*;
pub use branch_update::*;
pub use conversations::*;
pub use coordination::*;
pub use execution::*;
pub use external::*;
#[allow(unused_imports)]
pub use external_auth::*;
pub use git::*;
pub use ideation::*;
pub use internal::*;
pub use managed_team::*;
pub use issues::*;
pub use memory::*;
pub use permissions::*;
pub use personas::*;
pub use plan_complexity::*;
pub use projects::*;
pub use questions::*;
pub use reviews::*;
pub use session_linking::*;
pub use steps::*;
pub use tasks::*;
pub use ticket_attachments::*;
pub use validation::*;
pub use verification::*;
pub use worker::*;

// Re-export parent types and helpers for handlers to use
pub use super::helpers::*;
pub use super::types::*;

pub(crate) fn format_interactive_stdin_message(
    context_type: ChatContextType,
    context_id: &str,
    message: &str,
) -> String {
    let stdin_prompt = build_initial_prompt(context_type, context_id, message, &[], 0);
    format_stream_json_input(&stdin_prompt)
}

pub(crate) fn trusted_conversation_id(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get("x-ralphx-conversation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
