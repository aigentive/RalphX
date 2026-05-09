use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;

pub struct MemoryAgentProviderSettingsRepository {
    rows: Arc<RwLock<HashMap<AgentHarnessKind, AgentProviderSettings>>>,
}

impl Default for MemoryAgentProviderSettingsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryAgentProviderSettingsRepository {
    pub fn new() -> Self {
        Self {
            rows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_all_providers_enabled(default_provider: AgentHarnessKind) -> Self {
        let mut rows = HashMap::new();
        for provider in crate::domain::agents::STANDARD_AGENT_HARNESSES {
            let mut settings = AgentProviderSettings::disabled_defaults(provider);
            settings.enabled = true;
            settings.is_default = provider == default_provider;
            rows.insert(provider, settings);
        }
        Self {
            rows: Arc::new(RwLock::new(rows)),
        }
    }
}

#[async_trait]
impl AgentProviderSettingsRepository for MemoryAgentProviderSettingsRepository {
    async fn get(
        &self,
        provider: AgentHarnessKind,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>> {
        Ok(self.rows.read().await.get(&provider).cloned())
    }

    async fn list(&self) -> Result<Vec<AgentProviderSettings>, Box<dyn std::error::Error>> {
        let mut rows: Vec<_> = self.rows.read().await.values().cloned().collect();
        rows.sort_by_key(|row| row.provider.to_string());
        Ok(rows)
    }

    async fn get_default(
        &self,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>> {
        Ok(self
            .rows
            .read()
            .await
            .values()
            .find(|row| row.is_default)
            .cloned())
    }

    async fn upsert(
        &self,
        settings: &AgentProviderSettings,
    ) -> Result<AgentProviderSettings, Box<dyn std::error::Error>> {
        let mut next = settings.clone();
        next.updated_at = Utc::now();
        let mut rows = self.rows.write().await;
        if next.is_default {
            for row in rows.values_mut() {
                row.is_default = false;
            }
        }
        rows.insert(next.provider, next.clone());
        Ok(next)
    }
}

#[cfg(test)]
#[path = "memory_agent_provider_settings_repo_tests.rs"]
mod tests;
