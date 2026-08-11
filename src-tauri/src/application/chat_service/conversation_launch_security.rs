use crate::application::agent_lane_resolution::ResolvedAgentSpawnSettings;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentRun, ChatContextType, ChatConversation,
};
use crate::infrastructure::agents::claude::ClaudePermissionPolicy;
use crate::infrastructure::agents::CodexLaunchSecurityPolicy;

/// Provider-neutral launch security selected from authoritative conversation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConversationLaunchSecurityClass {
    /// Existing MCP workflows retain their configured provider compatibility contract.
    ConfiguredMcp,
    /// Standalone Chat requires provider-native approval and containment boundaries.
    StandaloneContainedChat,
}

impl ConversationLaunchSecurityClass {
    fn effective_codex_security(
        self,
        harness: AgentHarnessKind,
    ) -> Option<CodexLaunchSecurityPolicy> {
        (harness == AgentHarnessKind::Codex).then_some(self.codex_security_policy())
    }

    pub(super) const fn claude_permission_policy(self) -> ClaudePermissionPolicy {
        match self {
            Self::ConfiguredMcp => ClaudePermissionPolicy::InheritConfigured,
            Self::StandaloneContainedChat => ClaudePermissionPolicy::StandalonePromptBoundary,
        }
    }

    pub(super) const fn codex_security_policy(self) -> CodexLaunchSecurityPolicy {
        match self {
            Self::ConfiguredMcp => CodexLaunchSecurityPolicy::McpCompatibility,
            Self::StandaloneContainedChat => CodexLaunchSecurityPolicy::StandaloneContained,
        }
    }

    /// Applies the backend-owned effective security projection to persisted run metadata
    /// and command construction inputs without rewriting the configured user preference.
    pub(super) fn apply_to_effective_spawn_settings(
        self,
        settings: &mut ResolvedAgentSpawnSettings,
    ) {
        let Some(policy) = self.effective_codex_security(settings.effective_harness) else {
            return;
        };
        settings.approval_policy = Some(policy.approval_policy().to_string());
        settings.sandbox_mode = Some(policy.sandbox_mode().to_string());
    }

    /// Projects the same effective security used by command construction into
    /// queue-continuation metadata without changing configured provider defaults.
    pub(super) fn apply_to_agent_run(self, run: &mut AgentRun) {
        let Some(policy) = run
            .harness
            .and_then(|harness| self.effective_codex_security(harness))
        else {
            return;
        };
        run.approval_policy = Some(policy.approval_policy().to_string());
        run.sandbox_mode = Some(policy.sandbox_mode().to_string());
    }
}

/// Classifies one launch from backend-owned context and effective workspace mode.
///
/// Both enums are deliberately matched exhaustively so a newly added context or
/// workspace mode requires an explicit security decision.
pub(super) const fn conversation_launch_security_class(
    context_type: ChatContextType,
    effective_mode: Option<AgentConversationWorkspaceMode>,
) -> ConversationLaunchSecurityClass {
    match context_type {
        ChatContextType::Standalone => match effective_mode {
            Some(AgentConversationWorkspaceMode::PersonaBuilder) => {
                ConversationLaunchSecurityClass::ConfiguredMcp
            }
            None
            | Some(
                AgentConversationWorkspaceMode::Chat
                | AgentConversationWorkspaceMode::Edit
                | AgentConversationWorkspaceMode::Plan
                | AgentConversationWorkspaceMode::Tasks
                | AgentConversationWorkspaceMode::Autopilot
                | AgentConversationWorkspaceMode::Ideation
                | AgentConversationWorkspaceMode::ReviewPr
                | AgentConversationWorkspaceMode::Automation,
            ) => ConversationLaunchSecurityClass::StandaloneContainedChat,
        },
        ChatContextType::Ideation
        | ChatContextType::Delegation
        | ChatContextType::Task
        | ChatContextType::Project
        | ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge
        | ChatContextType::BranchUpdate => ConversationLaunchSecurityClass::ConfiguredMcp,
    }
}

/// Rejects duplicated caller identity that disagrees with the persisted row.
pub(super) fn validate_conversation_launch_identity(
    conversation: &ChatConversation,
    conversation_id: &str,
    context_type: ChatContextType,
    context_id: &str,
) -> Result<(), String> {
    if conversation.id.as_str() != conversation_id {
        return Err(format!(
            "conversation id mismatch: persisted {}, requested {}",
            conversation.id, conversation_id
        ));
    }
    if conversation.context_type != context_type {
        return Err(format!(
            "conversation context type mismatch: persisted {}, requested {}",
            conversation.context_type, context_type
        ));
    }
    if conversation.context_id.as_str() != context_id {
        return Err(format!(
            "conversation context id mismatch: persisted {}, requested {}",
            conversation.context_id, context_id
        ));
    }
    Ok(())
}
