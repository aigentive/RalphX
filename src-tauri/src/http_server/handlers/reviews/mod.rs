use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;

use super::*;
use crate::application::{
    task_diff_base::{
        diff_stats_has_changes, ensure_task_has_non_empty_captured_diff,
        read_captured_task_diff_stats,
    },
    GitService, TaskTransitionService,
};
use crate::domain::entities::{
    canonicalize_agent_conversation_issue, AgentConversationIssue,
    AgentConversationIssueCanonicalInput, AgentConversationIssueOccurrence, ChatConversationId,
    InternalStatus, ProjectId, Review, ReviewNote, ReviewOutcome, ReviewerType, TaskId,
    AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED, AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED,
};
use crate::domain::review::{
    apply_review_outcome, build_ai_review_note, build_followup_activity_event,
    build_review_issue_entities, build_review_note_issues, build_unrelated_drift_followup_draft,
    complete_review_response_message, count_revision_cycles, parse_review_decision,
    parse_review_issues, pending_review_or_new, review_note_content,
    should_spawn_unrelated_drift_followup, update_review_scope_metadata,
    validate_complete_review_policy, RawReviewIssueInput,
};
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::{
    deferred_merge_cleanup, set_no_code_changes_metadata, set_pending_cleanup_metadata,
};
use crate::domain::tools::complete_review::{ReviewToolOutcome, ScopeDriftClassification};
use crate::http_server::handlers::agent_followups::create_followup_agent_conversation_for_request;
use crate::http_server::helpers::get_task_context_impl;
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use crate::http_server::types::CreateFollowupAgentConversationRequest;

mod complete;
mod human;
mod notes;

pub use complete::complete_review;
pub use complete::ensure_task_still_reviewing_before_transition;
pub use human::{approve_task, request_task_changes};
pub use notes::get_review_notes;
