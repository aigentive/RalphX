// Chat Service Helper Functions
//
// Extracted from chat_service.rs to improve modularity and reduce file size.

use crate::domain::agents::{
    standard_harness_behavior, AgentHarnessKind, HarnessEffortStrategy, HarnessModelLabelStrategy,
    LogicalEffort,
};
use crate::domain::entities::{ChatContextType, MessageRole};
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_BRANCH_UPDATER, AGENT_CHAT_PROJECT, AGENT_CHAT_TASK, AGENT_MERGER,
    AGENT_ORCHESTRATOR_IDEATION, AGENT_ORCHESTRATOR_IDEATION_READONLY, AGENT_REVIEWER,
    AGENT_REVIEW_CHAT, AGENT_REVIEW_HISTORY, AGENT_WORKER,
};

/// Agent Resolution System
///
/// Determines which agent to use based on context type AND optionally entity status.
/// This enables dynamic agent switching based on workflow state.
///
/// ## Examples
///
/// - Review context + "reviewing" status → `ralphx-execution-reviewer` (AI performs review)
/// - Review context + "review_passed" status → `ralphx-review-chat` (user discusses with AI)
///
/// ## Adding New Rules
///
/// 1. Add a pattern to `resolve_agent()` for status-specific behavior
/// 2. Create the canonical agent definition under `agents/<agent>/`
/// 3. Add tools to MCP allowlist in `ralphx-mcp-server/src/tools.ts`
///
/// Priority: Status-specific rules are checked first, then defaults.
pub fn resolve_agent(context_type: &ChatContextType, entity_status: Option<&str>) -> &'static str {
    // Status-specific rules are checked first.
    if let Some(status) = entity_status {
        match (context_type, status) {
            // Review: after AI review passes, use chat agent for user discussion
            (ChatContextType::Review, "review_passed") => return AGENT_REVIEW_CHAT,

            // Review: approved tasks use read-only history agent for retrospective discussion
            (ChatContextType::Review, "approved") => return AGENT_REVIEW_HISTORY,

            // Ideation: accepted plans use read-only agent (no mutation tools)
            (ChatContextType::Ideation, "accepted") => return AGENT_ORCHESTRATOR_IDEATION_READONLY,

            _ => {}
        }
    }

    // Default rules (context-only, backward compatible)
    match context_type {
        ChatContextType::Ideation => AGENT_ORCHESTRATOR_IDEATION,
        ChatContextType::Delegation => AGENT_CHAT_PROJECT,
        ChatContextType::Task => AGENT_CHAT_TASK,
        ChatContextType::Project => AGENT_CHAT_PROJECT,
        ChatContextType::TaskExecution => AGENT_WORKER,
        ChatContextType::Review => AGENT_REVIEWER,
        ChatContextType::Merge => AGENT_MERGER,
        ChatContextType::BranchUpdate => AGENT_BRANCH_UPDATER,
        // Standalone conversations normally resolve their agent identity via
        // `conversation.agent_mode` (see `agent_name_for_conversation_mode`); this
        // context-only fallback only applies when no mode/override is present.
        ChatContextType::Standalone => AGENT_CHAT_PROJECT,
    }
}

pub fn harness_supports_rx_native_team(harness: AgentHarnessKind) -> bool {
    standard_harness_behavior(harness).team.rx_native_team
}

pub fn effective_effort_for_harness(
    harness: AgentHarnessKind,
    claude_effort: Option<&str>,
    logical_effort: Option<LogicalEffort>,
) -> String {
    match standard_harness_behavior(harness).effort_strategy {
        HarnessEffortStrategy::ClaudeEffortFirst => claude_effort
            .map(str::to_string)
            .or_else(|| logical_effort.map(|effort| effort.to_string()))
            .unwrap_or_else(|| "medium".to_string()),
        HarnessEffortStrategy::LogicalOnly => logical_effort
            .map(|effort| effort.to_string())
            .unwrap_or_else(|| "medium".to_string()),
    }
}

pub fn effective_model_label_for_harness(
    harness: AgentHarnessKind,
    effective_model_id: &str,
) -> String {
    match standard_harness_behavior(harness).model_label_strategy {
        HarnessModelLabelStrategy::ClaudeMapped => {
            crate::infrastructure::agents::claude::model_labels::model_id_to_label(
                effective_model_id,
            )
        }
        HarnessModelLabelStrategy::RawModelId => effective_model_id.to_string(),
    }
}

pub fn provider_origin_for_harness(
    harness: AgentHarnessKind,
    agent_name: Option<&str>,
) -> (Option<String>, Option<String>) {
    match harness {
        AgentHarnessKind::Codex => (Some("openai".to_string()), None),
        AgentHarnessKind::Claude => {
            let profile =
                crate::infrastructure::agents::claude::get_effective_settings_profile(agent_name)
                    .map(str::to_string);
            let upstream_provider = profile
                .as_deref()
                .and_then(classify_claude_profile_provider)
                .map(str::to_string)
                .or_else(|| infer_claude_provider_from_settings(agent_name).map(str::to_string))
                .or(Some("anthropic".to_string()));
            (upstream_provider, profile)
        }
    }
}

fn infer_claude_provider_from_settings(agent_name: Option<&str>) -> Option<&'static str> {
    let settings = crate::infrastructure::agents::claude::get_effective_settings(agent_name)?;
    let base_url = settings
        .get("env")
        .and_then(|value| value.get("ANTHROPIC_BASE_URL"))
        .and_then(|value| value.as_str());

    match base_url {
        Some(url) if url.contains("z.ai") => Some("z_ai"),
        Some(url) if url.contains("openrouter.ai") => Some("openrouter"),
        Some(_) => Some("anthropic"),
        None => None,
    }
}

fn classify_claude_profile_provider(profile: &str) -> Option<&'static str> {
    if profile.contains("z_ai") {
        Some("z_ai")
    } else if profile.contains("openrouter") {
        Some("openrouter")
    } else if profile == "default" {
        Some("anthropic")
    } else {
        None
    }
}

pub fn harness_supports_merge_completion_watcher(harness: AgentHarnessKind) -> bool {
    standard_harness_behavior(harness).supports_merge_completion_watcher
}

/// Decide whether a send must start a new provider session instead of resuming
/// the conversation's stored provider thread.
///
/// Canonical-agent overrides are orchestrated workflow boundaries (for example
/// PR monitoring, CI autofix, and workspace review). They must not inherit an
/// arbitrary interactive chat thread just because it belongs to the same
/// conversation.
pub fn should_start_fresh_provider_session(
    force_new_provider_session: bool,
    provider_switch_requires_fresh_session: bool,
    agent_name_override: Option<&str>,
) -> bool {
    force_new_provider_session
        || provider_switch_requires_fresh_session
        || agent_name_override.is_some()
}

/// A stored provider thread may be resumed only when its most recent known
/// model matches the model selected for this send. Providers do not guarantee
/// that a resumed thread can safely switch models.
pub fn provider_session_model_matches_requested(
    latest_session_model: Option<&str>,
    requested_model: &str,
) -> bool {
    latest_session_model.is_none_or(|model| model == requested_model)
}

/// Map ChatContextType to process name for team config lookup
pub fn context_type_to_process(context_type: &ChatContextType) -> &'static str {
    match context_type {
        ChatContextType::Ideation => "ideation",
        ChatContextType::Delegation => "delegation",
        ChatContextType::Task => "task",
        ChatContextType::Project => "project",
        ChatContextType::TaskExecution => "execution",
        ChatContextType::Review => "review",
        ChatContextType::Merge => "merge",
        ChatContextType::BranchUpdate => "branch_update",
        ChatContextType::Standalone => "standalone",
    }
}

/// Legacy function for backward compatibility
/// Use `resolve_agent()` when entity status is available
pub fn get_agent_name(context_type: &ChatContextType) -> &'static str {
    resolve_agent(context_type, None)
}

/// Get the message role for a context type
pub fn get_assistant_role(context_type: &ChatContextType) -> MessageRole {
    match context_type {
        ChatContextType::TaskExecution => MessageRole::Worker,
        ChatContextType::Review => MessageRole::Reviewer,
        ChatContextType::Merge => MessageRole::Merger,
        ChatContextType::BranchUpdate => MessageRole::Reviewer,
        _ => MessageRole::Orchestrator,
    }
}

#[cfg(test)]
#[path = "chat_service_helpers_tests.rs"]
mod tests;
