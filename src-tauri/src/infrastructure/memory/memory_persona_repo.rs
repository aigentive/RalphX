use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{Persona, PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId};
use crate::domain::repositories::PersonaRepository;
use crate::error::{AppError, AppResult};

pub struct MemoryPersonaRepository {
    personas: RwLock<HashMap<PersonaId, Persona>>,
}

impl MemoryPersonaRepository {
    pub fn new() -> Self {
        Self {
            personas: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryPersonaRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PersonaRepository for MemoryPersonaRepository {
    async fn create(&self, persona: Persona) -> AppResult<Persona> {
        let mut personas = self.personas.write().await;
        if persona.status == PersonaStatus::Active
            && personas.values().any(|existing| {
                existing.slug == persona.slug
                    && existing.status == PersonaStatus::Active
                    && existing.project_id == persona.project_id
            })
        {
            return Err(AppError::Validation(format!(
                "Persona slug `{}` is already in use",
                persona.slug
            )));
        }
        personas.insert(persona.id.clone(), persona.clone());
        Ok(persona)
    }

    async fn get_by_id(&self, id: &PersonaId) -> AppResult<Option<Persona>> {
        Ok(self.personas.read().await.get(id).cloned())
    }

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Persona>> {
        Ok(self
            .personas
            .read()
            .await
            .values()
            .filter(|persona| persona.slug == slug)
            .max_by_key(|persona| persona.created_at)
            .cloned())
    }

    async fn get_active_by_slug(
        &self,
        slug: &str,
        project_id: Option<&ProjectId>,
    ) -> AppResult<Option<Persona>> {
        Ok(self
            .personas
            .read()
            .await
            .values()
            .find(|persona| {
                persona.slug == slug
                    && persona.status == PersonaStatus::Active
                    && persona.project_id.as_ref() == project_id
            })
            .cloned())
    }

    async fn get_draft_by_source_persona_id(
        &self,
        source_persona_id: &PersonaId,
    ) -> AppResult<Option<Persona>> {
        Ok(self
            .personas
            .read()
            .await
            .values()
            .filter(|persona| {
                persona.status == PersonaStatus::Draft
                    && persona.source_persona_id.as_ref() == Some(source_persona_id)
            })
            .max_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.id.as_str().cmp(right.id.as_str()))
            })
            .cloned())
    }

    async fn list(&self, scope: PersonaScopeFilter) -> AppResult<Vec<Persona>> {
        let mut personas = self
            .personas
            .read()
            .await
            .values()
            .filter(|persona| match &scope {
                PersonaScopeFilter::All => true,
                PersonaScopeFilter::GlobalOnly => persona.project_id.is_none(),
                PersonaScopeFilter::GlobalAndProject(project_id) => persona
                    .project_id
                    .as_ref()
                    .is_none_or(|persona_project_id| persona_project_id == project_id),
            })
            .cloned()
            .collect::<Vec<_>>();
        personas.sort_by_key(|persona| std::cmp::Reverse(persona.created_at));
        Ok(personas)
    }

    async fn list_by_status(&self, status: PersonaStatus) -> AppResult<Vec<Persona>> {
        let mut personas = self
            .personas
            .read()
            .await
            .values()
            .filter(|persona| persona.status == status)
            .cloned()
            .collect::<Vec<_>>();
        personas.sort_by_key(|persona| std::cmp::Reverse(persona.created_at));
        Ok(personas)
    }

    async fn set_status(&self, id: &PersonaId, status: PersonaStatus) -> AppResult<()> {
        let mut personas = self.personas.write().await;
        if status == PersonaStatus::Active {
            let target = personas
                .get(id)
                .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
            let slug = target.slug.clone();
            let project_id = target.project_id.clone();
            if personas.values().any(|persona| {
                &persona.id != id
                    && persona.slug == slug
                    && persona.status == PersonaStatus::Active
                    && persona.project_id == project_id
            }) {
                return Err(AppError::Validation(format!(
                    "Persona slug `{slug}` is already in use"
                )));
            }
        }
        let persona = personas
            .get_mut(id)
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
        persona.status = status;
        persona.updated_at = Utc::now();
        Ok(())
    }

    async fn delete(&self, id: &PersonaId) -> AppResult<()> {
        self.personas
            .write()
            .await
            .remove(id)
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
        Ok(())
    }
}
