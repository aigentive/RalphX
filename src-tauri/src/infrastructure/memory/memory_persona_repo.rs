use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{Persona, PersonaId, PersonaStatus};
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
                existing.slug == persona.slug && existing.status == PersonaStatus::Active
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

    async fn get_active_by_slug(&self, slug: &str) -> AppResult<Option<Persona>> {
        Ok(self
            .personas
            .read()
            .await
            .values()
            .find(|persona| persona.slug == slug && persona.status == PersonaStatus::Active)
            .cloned())
    }

    async fn list(&self) -> AppResult<Vec<Persona>> {
        let mut personas = self
            .personas
            .read()
            .await
            .values()
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

    async fn update_content(
        &self,
        id: &PersonaId,
        content: &str,
        content_hash: &str,
    ) -> AppResult<()> {
        let mut personas = self.personas.write().await;
        let persona = personas
            .get_mut(id)
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?;
        persona.content = content.to_string();
        persona.content_hash = content_hash.to_string();
        persona.version += 1;
        persona.updated_at = Utc::now();
        Ok(())
    }

    async fn set_status(&self, id: &PersonaId, status: PersonaStatus) -> AppResult<()> {
        let mut personas = self.personas.write().await;
        if status == PersonaStatus::Active {
            let slug = personas
                .get(id)
                .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?
                .slug
                .clone();
            if personas.values().any(|persona| {
                &persona.id != id && persona.slug == slug && persona.status == PersonaStatus::Active
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
