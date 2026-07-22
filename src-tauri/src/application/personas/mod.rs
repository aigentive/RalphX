use std::sync::Arc;

use chrono::Utc;
use ralphx_domain::personas::validation::validate_persona_content;
use serde::Serialize;
use serde_json::{json, Value};

use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, Persona,
    PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId,
};
use crate::domain::repositories::{ChatConversationRepository, PersonaRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;
use persona_transactions::{CREATED_BY_AGENT, CREATED_BY_USER};

mod persona_transactions;
mod persona_update_approval;

pub use persona_update_approval::draft_applied_payload;

/// Prefix used by every caller that reports a persona that cannot be bound.
pub const PERSONA_UNAVAILABLE_PREFIX: &str = "[Persona unavailable:";
/// Prefix used by every persona surface when the feature is disabled.
pub const PERSONA_FEATURE_DISABLED_PREFIX: &str = "[Personas disabled:";
/// Stable IPC code for a manual persona draft compare-and-swap rejection.
pub const PERSONA_DRAFT_CONFLICT_CODE: &str = "PERSONA_DRAFT_CONFLICT:";
pub const PERSONA_REFINE_SCOPE_MISMATCH_PREFIX: &str = "PERSONA_REFINE_SCOPE_MISMATCH:";

/// The draft-update event contract intentionally excludes persona content/body.
pub fn draft_updated_payload(persona: &Persona) -> Value {
    json!({
        "draft_id": persona.id.as_str(),
        "version": persona.version,
        "content_hash": persona.content_hash,
        "artifact_id": persona.artifact_id.as_ref().map(|id| id.as_str()),
    })
}

/// Draft-update payload scoped to the PersonaBuilder conversation that owns the save.
pub fn builder_draft_updated_payload(persona: &Persona, conversation_id: &str) -> Value {
    let mut payload = draft_updated_payload(persona);
    payload["builder_conversation_id"] = json!(conversation_id);
    payload
}

/// Derived, read-only usage facts for one persona. Never denormalized; always
/// computed live from `chat_conversations.persona_id` and `agent_runs` attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaUsage {
    pub persona_id: String,
    pub bound_conversation_count: i64,
    pub last_run_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SavePersonaDraftInput {
    pub project_id: Option<ProjectId>,
    pub slug: String,
    pub content: String,
    pub source_session_id: Option<String>,
    pub source_persona_id: Option<PersonaId>,
    pub source_content_hash: Option<String>,
}

pub fn validate_persona_project_id(project_id: ProjectId) -> AppResult<ProjectId> {
    let trimmed = project_id.as_str().trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(
            "persona project id cannot be empty".to_string(),
        ));
    }
    Ok(ProjectId::from_string(trimmed.to_string()))
}

/// Application authority for persona lifecycle changes.
///
/// Feature state is deliberately passed to each entry point rather than cached here:
/// Tauri and HTTP own separate AppState instances and must enforce their current boundary.
#[derive(Clone)]
pub struct PersonaService {
    db: DbConnection,
    persona_repo: Arc<dyn PersonaRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
}

impl PersonaService {
    pub fn new(
        db: DbConnection,
        persona_repo: Arc<dyn PersonaRepository>,
        chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    ) -> Self {
        Self {
            db,
            persona_repo,
            chat_conversation_repo,
        }
    }

    pub async fn create_draft(
        &self,
        feature_enabled: bool,
        input: SavePersonaDraftInput,
    ) -> AppResult<Persona> {
        let persona = self.build_draft(feature_enabled, input).await?;
        self.persist_draft(persona, CREATED_BY_USER).await
    }

    pub async fn validate_refine_source(
        &self,
        feature_enabled: bool,
        source_id: &PersonaId,
        conversation_project_id: Option<&ProjectId>,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let source = self
            .persona_repo
            .get_by_id(source_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {source_id}")))?;
        if source.status != PersonaStatus::Active {
            return Err(AppError::PersonaUnavailable(format!(
                "{PERSONA_UNAVAILABLE_PREFIX} persona {source_id} is not active]"
            )));
        }
        if source.project_id.as_ref() != conversation_project_id {
            return Err(AppError::Validation(format!(
                "{PERSONA_REFINE_SCOPE_MISMATCH_PREFIX} source persona {source_id} scope does not match the builder conversation scope"
            )));
        }
        Ok(source)
    }

    pub async fn create_seeded_bound_draft(
        &self,
        feature_enabled: bool,
        conversation: &ChatConversation,
        source_id: &PersonaId,
    ) -> AppResult<Persona> {
        if conversation.agent_mode != Some(AgentConversationWorkspaceMode::PersonaBuilder) {
            return Err(AppError::Validation(
                "Seeded persona drafts require a persona_builder conversation".to_string(),
            ));
        }
        let conversation_project_id = match conversation.context_type {
            ChatContextType::Project => Some(validate_persona_project_id(ProjectId::from_string(
                conversation.context_id.clone(),
            ))?),
            ChatContextType::Standalone => None,
            _ => {
                return Err(AppError::Validation(
                    "Persona builder conversations must use Project or Standalone context"
                        .to_string(),
                ))
            }
        };
        let source = self
            .validate_refine_source(feature_enabled, source_id, conversation_project_id.as_ref())
            .await?;
        self.create_bound_draft(
            feature_enabled,
            &conversation.id,
            SavePersonaDraftInput {
                project_id: conversation_project_id,
                slug: source.slug,
                content: source.content,
                source_session_id: Some(conversation.id.as_str()),
                source_persona_id: Some(source.id),
                source_content_hash: Some(source.content_hash),
            },
        )
        .await
    }

    pub async fn create_project_builder_conversation(
        &self,
        feature_enabled: bool,
        project_id: ProjectId,
        source_id: Option<&PersonaId>,
    ) -> AppResult<ChatConversation> {
        ensure_enabled(feature_enabled)?;
        let project_id = validate_persona_project_id(project_id)?;
        if let Some(source_id) = source_id {
            self.validate_refine_source(feature_enabled, source_id, Some(&project_id))
                .await?;
            if let Some(draft) = self
                .persona_repo
                .get_draft_by_source_persona_id(source_id)
                .await?
            {
                if let Some(conversation) = self
                    .chat_conversation_repo
                    .get_by_builder_draft_id(draft.id.as_str())
                    .await?
                    .filter(|conversation| {
                        conversation.context_type == ChatContextType::Project
                            && conversation.context_id == project_id.as_str()
                            && conversation.agent_mode
                                == Some(AgentConversationWorkspaceMode::PersonaBuilder)
                    })
                {
                    return Ok(conversation);
                }
            }
        }

        let mut conversation = ChatConversation::new_project(project_id);
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::PersonaBuilder));
        conversation.set_title("Persona builder".to_string());
        let conversation = self.chat_conversation_repo.create(conversation).await?;
        if let Some(source_id) = source_id {
            if let Err(error) = self
                .create_seeded_bound_draft(feature_enabled, &conversation, source_id)
                .await
            {
                if let Err(cleanup_error) = self.chat_conversation_repo.delete(&conversation.id).await
                {
                    tracing::warn!(
                        conversation_id = %conversation.id,
                        error = %cleanup_error,
                        "Failed to delete PersonaBuilder conversation after draft seeding failed"
                    );
                }
                return Err(error);
            }
        }
        self.chat_conversation_repo
            .get_by_id(&conversation.id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(
                    "PersonaBuilder conversation was not found after creation".to_string(),
                )
            })
    }

    async fn build_draft(
        &self,
        feature_enabled: bool,
        input: SavePersonaDraftInput,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let project_id = input
            .project_id
            .map(validate_persona_project_id)
            .transpose()?;
        let parsed = validate_persona_content(&input.slug, &input.content)?;
        if input.source_persona_id.is_none() {
            self.ensure_live_slug_available(&input.slug, project_id.as_ref(), None)
                .await?;
        }
        let now = Utc::now();
        Ok(Persona {
            id: PersonaId::new(),
            artifact_id: None,
            project_id,
            slug: input.slug,
            name: parsed.frontmatter.name,
            description: parsed.frontmatter.description,
            content: input.content,
            status: PersonaStatus::Draft,
            version: 1,
            content_hash: parsed.content_hash,
            source_session_id: input.source_session_id,
            source_persona_id: input.source_persona_id,
            source_content_hash: input.source_content_hash,
            source_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn update_draft(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        content: &str,
        expected_content_hash: Option<&str>,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Draft).await?;
        if let Some(expected) = expected_content_hash {
            if expected != persona.content_hash {
                return Err(AppError::PersonaDraftConflict {
                    expected: expected.to_string(),
                    actual: persona.content_hash.clone(),
                });
            }
        }
        let parsed = validate_persona_content(&persona.slug, content)?;
        self.update_content_with_artifact(
            id,
            content,
            parsed,
            expected_content_hash,
            PersonaStatus::Draft,
            CREATED_BY_USER,
        )
        .await
    }

    pub async fn update_draft_as_agent(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        content: &str,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Draft).await?;
        let parsed = validate_persona_content(&persona.slug, content)?;
        self.update_content_with_artifact(
            id,
            content,
            parsed,
            None,
            PersonaStatus::Draft,
            CREATED_BY_AGENT,
        )
        .await
    }

    pub async fn get_draft(&self, feature_enabled: bool, id: &PersonaId) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        self.require_status(id, PersonaStatus::Draft).await
    }

    pub async fn approve_persona(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Draft).await?;
        if persona.source_persona_id.is_some() {
            return self.apply_seeded_draft(id).await;
        }
        validate_persona_content(&persona.slug, &persona.content)?;
        self.ensure_active_slug_available(&persona.slug, persona.project_id.as_ref(), Some(id))
            .await?;
        self.approve_plain_draft(id).await
    }

    pub async fn update_persona(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        content: &str,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.require_status(id, PersonaStatus::Active).await?;
        let parsed = validate_persona_content(&persona.slug, content)?;
        self.update_content_with_artifact(
            id,
            content,
            parsed,
            None,
            PersonaStatus::Active,
            CREATED_BY_USER,
        )
        .await
    }

    pub async fn hard_delete_draft(&self, feature_enabled: bool, id: &PersonaId) -> AppResult<()> {
        ensure_enabled(feature_enabled)?;
        let draft = self.require_status(id, PersonaStatus::Draft).await?;
        if draft.source_persona_id.is_some()
            || self
                .chat_conversation_repo
                .get_by_builder_draft_id(id.as_str())
                .await?
                .is_some()
        {
            return self.delete_bound_draft(id).await;
        }
        self.delete_unbound_draft(id).await
    }

    pub async fn list_personas(
        &self,
        feature_enabled: bool,
        scope: PersonaScopeFilter,
    ) -> AppResult<Vec<Persona>> {
        ensure_enabled(feature_enabled)?;
        self.persona_repo.list(scope).await
    }

    /// One aggregated read for every persona's derived usage; errors propagate
    /// (fail closed) instead of collapsing into zero counts.
    pub async fn list_persona_usage(&self, feature_enabled: bool) -> AppResult<Vec<PersonaUsage>> {
        ensure_enabled(feature_enabled)?;
        // Cross-table derived read; like archive_persona this is intentionally a
        // SQLite-side query because usage spans conversations and agent runs.
        let _ = &self.persona_repo;
        self.db
            .run(|conn| {
                let mut statement = conn.prepare(
                    "SELECT p.id,
                            (SELECT COUNT(*) FROM chat_conversations c WHERE c.persona_id = p.id),
                            (SELECT MAX(r.started_at) FROM agent_runs r WHERE r.persona_id = p.id)
                     FROM personas p",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok(PersonaUsage {
                            persona_id: row.get(0)?,
                            bound_conversation_count: row.get(1)?,
                            last_run_at: row.get(2)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn get_persona(&self, feature_enabled: bool, id: &PersonaId) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        self.persona_repo
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))
    }

    pub async fn ensure_bindable(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        project_id: &ProjectId,
    ) -> AppResult<Persona> {
        self.ensure_bindable_to_scope(feature_enabled, id, Some(project_id))
            .await
    }

    pub async fn ensure_bindable_to_scope(
        &self,
        feature_enabled: bool,
        id: &PersonaId,
        project_id: Option<&ProjectId>,
    ) -> AppResult<Persona> {
        ensure_enabled(feature_enabled)?;
        let persona = self.persona_repo.get_by_id(id).await?;
        match persona.filter(|persona| {
            persona.status == PersonaStatus::Active
                && match project_id {
                    Some(project_id) => persona.is_bindable_to_project(project_id),
                    None => persona.project_id.is_none(),
                }
        }) {
            Some(persona) => Ok(persona),
            None => Err(AppError::PersonaUnavailable(format!(
                "{PERSONA_UNAVAILABLE_PREFIX} persona {id} is not active]"
            ))),
        }
    }

    async fn ensure_live_slug_available(
        &self,
        slug: &str,
        project_id: Option<&ProjectId>,
        excluding: Option<&PersonaId>,
    ) -> AppResult<()> {
        let occupied = self
            .persona_repo
            .list(PersonaScopeFilter::All)
            .await?
            .into_iter()
            .any(|persona| {
                persona.slug == slug
                    && persona.project_id.as_ref() == project_id
                    && persona.status != PersonaStatus::Archived
                    && excluding.is_none_or(|id| id != &persona.id)
            });
        if occupied {
            return Err(AppError::Validation(format!(
                "Persona slug `{slug}` is already in use"
            )));
        }
        Ok(())
    }

    async fn ensure_active_slug_available(
        &self,
        slug: &str,
        project_id: Option<&ProjectId>,
        excluding: Option<&PersonaId>,
    ) -> AppResult<()> {
        let occupied = self
            .persona_repo
            .get_active_by_slug(slug, project_id)
            .await?
            .is_some_and(|persona| excluding.is_none_or(|id| id != &persona.id));
        if occupied {
            return Err(AppError::Validation(format!(
                "Persona slug `{slug}` is already in use"
            )));
        }
        Ok(())
    }

    async fn require_status(&self, id: &PersonaId, status: PersonaStatus) -> AppResult<Persona> {
        let persona = self
            .persona_repo
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
        if persona.status != status {
            return Err(AppError::Validation(format!(
                "Persona {id} must be {status}"
            )));
        }
        Ok(persona)
    }
}

fn clear_manual_role_persona_defaults_sync(
    conn: &rusqlite::Connection,
    persona_id: &str,
) -> AppResult<usize> {
    conn.execute(
        "DELETE FROM manual_role_defaults
         WHERE json_extract(value_json, '$.personaId') = ?1",
        [persona_id],
    )
    .map_err(|error| AppError::Database(error.to_string()))
}

fn ensure_enabled(feature_enabled: bool) -> AppResult<()> {
    if feature_enabled {
        Ok(())
    } else {
        Err(AppError::FeatureDisabled(format!(
            "{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"
        )))
    }
}

#[path = "persona_service_test_support.rs"]
mod persona_service_test_support;
#[path = "persona_service_artifact_tests.rs"]
mod persona_service_artifact_tests;
#[path = "persona_service_lifecycle_tests.rs"]
mod persona_service_lifecycle_tests;
#[path = "persona_service_validation_tests.rs"]
mod persona_service_validation_tests;
#[path = "persona_service_usage_restore_tests.rs"]
mod persona_service_usage_restore_tests;

#[path = "persona_update_approval_test_support.rs"]
mod persona_update_approval_test_support;
#[path = "persona_update_approval_transaction_tests.rs"]
mod persona_update_approval_transaction_tests;
#[path = "persona_update_approval_binding_tests.rs"]
mod persona_update_approval_binding_tests;
#[path = "persona_update_approval_recovery_tests.rs"]
mod persona_update_approval_recovery_tests;
