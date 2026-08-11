use std::{path::Path, time::Instant};

use serde::Serialize;
use tauri::{Emitter, Runtime};

use super::AgentWorkspaceSourcePullRequestInput;
use crate::application::agent_conversation_workspace::{
    AgentConversationWorkspaceBranchNameHint, AgentConversationWorkspacePrAutomationDefaults,
};
use crate::application::agent_planning_session_titles::hydrate_agent_conversation_planning_session_title;
use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::AppState;
use crate::domain::agents::{
    default_effort_for_provider, default_efforts_for_provider, AgentHarnessKind,
    AgentModelRegistrySnapshot, LogicalEffort,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceSourcePullRequest, ChatContextType,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionFlow, Project, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::ComposerIntegrationReference;

mod spawn_glue;
mod validation;
mod workspace_flow;

pub(crate) use spawn_glue::*;
pub(crate) use validation::*;
pub(crate) use workspace_flow::*;

#[cfg(test)]
#[path = "../helpers_tests.rs"]
mod tests;
