//! Send-time persona resolution precedence:
//! 1. disabled feature → suppress
//! 2. explicit suppression → suppress without a read
//! 3. external MCP → suppress
//! 4. Project context is required for both explicit and inherited directives
//! 5. all directives suppress for Automation/PersonaBuilder mode and verification
//! 6. inherited personas additionally suppress for agent overrides
//! 7. explicit and inherited personas share active/project bindability enforcement
//! 8. inheritance reads only `conversation.persona_id`, then requires a bindable persona
//!    and a safe render.
//!
//! Ideation conversations do exist as `chat_conversations` rows and flow through `send_message`.
//! The explicit Project-context gate, rather than a session-keyed rationale, is the rule.

use std::sync::Arc;

use thiserror::Error;

use crate::application::persona_prompt::{render_persona_block, ResolvedPersona};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, PersonaDirective, PersonaId,
    ProjectId,
};
use crate::domain::repositories::PersonaRepository;

/// Inputs from the send boundary that determine whether a persona may be carried.
#[derive(Debug, Clone, Copy)]
pub struct PersonaResolveFlags {
    pub feature_enabled: bool,
    pub is_external_mcp: bool,
    pub agent_name_override_set: bool,
    pub agent_conversation_mode: Option<AgentConversationWorkspaceMode>,
    pub is_verification: bool,
}

/// A body-independent failure while resolving a persona for a send.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PersonaError {
    #[error("Persona repository unavailable: {0}")]
    Repository(String),
    #[error("Persona `{persona_id}` is unavailable")]
    Unavailable { persona_id: String },
    #[error("Persona render rejected: {0}")]
    RenderRejected(String),
}

pub const PERSONA_PROJECT_SCOPE_MISMATCH: &str = "project_scope_mismatch";

/// Resolves the sole persona authority for a send.
///
/// # Errors
///
/// Returns a typed error when the authoritative repository read fails, a referenced persona is
/// unavailable, or the stored active persona fails render-time safety checks.
pub async fn resolve_persona_for_send(
    conversation: &ChatConversation,
    directive: &PersonaDirective,
    flags: PersonaResolveFlags,
    repo: Arc<dyn PersonaRepository>,
) -> Result<Option<ResolvedPersona>, PersonaError> {
    if !flags.feature_enabled {
        return Ok(None);
    }

    if matches!(directive, PersonaDirective::Suppress) {
        return Ok(None);
    }

    if flags.is_external_mcp {
        return Ok(None);
    }

    if conversation.context_type != ChatContextType::Project {
        return Ok(None);
    }

    if matches!(
        flags.agent_conversation_mode,
        Some(
            AgentConversationWorkspaceMode::Automation
                | AgentConversationWorkspaceMode::PersonaBuilder
        )
    ) || flags.is_verification
    {
        return Ok(None);
    }

    let conversation_project_id = ProjectId::from_string(conversation.context_id.clone());

    if let PersonaDirective::Explicit(persona_id) = directive {
        return resolve_persona_by_id(persona_id, &conversation_project_id, repo)
            .await
            .map(Some);
    }

    if flags.agent_name_override_set {
        return Ok(None);
    }

    let Some(persona_id) = conversation.persona_id.as_deref() else {
        return Ok(None);
    };

    resolve_persona_by_id(
        &PersonaId::from_string(persona_id),
        &conversation_project_id,
        repo,
    )
    .await
    .map(Some)
}

async fn resolve_persona_by_id(
    persona_id: &PersonaId,
    project_id: &ProjectId,
    repo: Arc<dyn PersonaRepository>,
) -> Result<ResolvedPersona, PersonaError> {
    let persona = repo
        .get_by_id(persona_id)
        .await
        .map_err(|error| PersonaError::Repository(error.to_string()))?
        .ok_or_else(|| PersonaError::Unavailable {
            persona_id: persona_id.to_string(),
        })?;

    if !persona.is_bindable() {
        return Err(PersonaError::Unavailable {
            persona_id: persona.id.to_string(),
        });
    }

    if !persona.is_bindable_to_project(project_id) {
        return Ok(ResolvedPersona {
            id: persona.id,
            slug: persona.slug,
            version: persona.version,
            content_hash: persona.content_hash,
            block: String::new(),
            skipped_reason: Some(PERSONA_PROJECT_SCOPE_MISMATCH),
        });
    }

    render_persona_block(&persona).map_err(|error| PersonaError::RenderRejected(error.to_string()))
}
