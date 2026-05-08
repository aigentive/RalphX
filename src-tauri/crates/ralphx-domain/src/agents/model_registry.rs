use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AgentHarnessKind, LogicalEffort};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentModelSource {
    BuiltIn,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelDefinition {
    pub provider: AgentHarnessKind,
    pub model_id: String,
    pub label: String,
    pub menu_label: String,
    pub description: Option<String>,
    pub supported_efforts: Vec<LogicalEffort>,
    pub default_effort: LogicalEffort,
    pub source: AgentModelSource,
    pub enabled: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl AgentModelDefinition {
    pub fn built_in(
        provider: AgentHarnessKind,
        model_id: impl Into<String>,
        label: impl Into<String>,
        menu_label: impl Into<String>,
        description: Option<&str>,
        supported_efforts: Vec<LogicalEffort>,
        default_effort: LogicalEffort,
    ) -> Self {
        Self {
            provider,
            model_id: model_id.into(),
            label: label.into(),
            menu_label: menu_label.into(),
            description: description.map(str::to_string),
            supported_efforts,
            default_effort,
            source: AgentModelSource::BuiltIn,
            enabled: true,
            created_at: None,
            updated_at: None,
        }
    }

    pub fn custom(
        provider: AgentHarnessKind,
        model_id: impl Into<String>,
        label: impl Into<String>,
        menu_label: impl Into<String>,
        description: Option<String>,
        supported_efforts: Vec<LogicalEffort>,
        default_effort: LogicalEffort,
        enabled: bool,
    ) -> Self {
        Self {
            provider,
            model_id: model_id.into(),
            label: label.into(),
            menu_label: menu_label.into(),
            description,
            supported_efforts,
            default_effort,
            source: AgentModelSource::Custom,
            enabled,
            created_at: None,
            updated_at: None,
        }
    }

    pub fn normalized(mut self) -> Self {
        self.model_id = self.model_id.trim().to_string();
        self.label = self.label.trim().to_string();
        self.menu_label = self.menu_label.trim().to_string();
        self.description = self
            .description
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.supported_efforts
            .sort_by_key(|effort| effort_order(*effort));
        self.supported_efforts.dedup();
        if self.supported_efforts.is_empty() {
            self.supported_efforts = default_efforts_for_provider(self.provider).to_vec();
        }
        if !self.supported_efforts.contains(&self.default_effort) {
            self.default_effort = default_effort_for_provider(self.provider);
            if !self.supported_efforts.contains(&self.default_effort) {
                self.default_effort = self.supported_efforts[0];
            }
        }
        if self.label.is_empty() {
            self.label = self.model_id.clone();
        }
        if self.menu_label.is_empty() {
            self.menu_label = self.label.clone();
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelRegistrySnapshot {
    pub models: Vec<AgentModelDefinition>,
}

impl AgentModelRegistrySnapshot {
    pub fn merged(custom_models: Vec<AgentModelDefinition>) -> Self {
        let mut models = built_in_agent_models();
        let mut custom_additions = Vec::new();
        for custom in custom_models {
            let custom = custom.normalized();
            if let Some(existing) = models.iter_mut().find(|model| {
                model.provider == custom.provider && model.model_id == custom.model_id
            }) {
                *existing = custom;
            } else {
                custom_additions.push(custom);
            }
        }
        custom_additions.sort_by_key(|model| {
            (
                provider_order(model.provider),
                model.menu_label.to_ascii_lowercase(),
            )
        });
        models.extend(custom_additions);
        Self { models }
    }

    pub fn enabled_for_provider(
        &self,
        provider: AgentHarnessKind,
    ) -> impl Iterator<Item = &AgentModelDefinition> {
        self.models
            .iter()
            .filter(move |model| model.provider == provider && model.enabled)
    }

    pub fn find_enabled(
        &self,
        provider: AgentHarnessKind,
        model_id: &str,
    ) -> Option<&AgentModelDefinition> {
        self.enabled_for_provider(provider)
            .find(|model| model.model_id == model_id)
    }

    pub fn default_for_provider(
        &self,
        provider: AgentHarnessKind,
    ) -> Option<&AgentModelDefinition> {
        self.enabled_for_provider(provider).next()
    }
}

pub fn built_in_agent_models() -> Vec<AgentModelDefinition> {
    vec![
        AgentModelDefinition::built_in(
            AgentHarnessKind::Claude,
            "sonnet",
            "sonnet",
            "sonnet",
            Some("Claude Sonnet model alias."),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::Max,
            ],
            LogicalEffort::Medium,
        ),
        AgentModelDefinition::built_in(
            AgentHarnessKind::Claude,
            "opus",
            "opus",
            "opus",
            Some("Claude Opus model alias."),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::XHigh,
                LogicalEffort::Max,
            ],
            LogicalEffort::XHigh,
        ),
        AgentModelDefinition::built_in(
            AgentHarnessKind::Claude,
            "haiku",
            "haiku",
            "haiku",
            Some("Claude Haiku model alias."),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
            ],
            LogicalEffort::Medium,
        ),
        AgentModelDefinition::built_in(
            AgentHarnessKind::Codex,
            "gpt-5.5",
            "gpt-5.5 - Frontier model for complex coding, research, and real-world work.",
            "gpt-5.5 (Current)",
            Some("Frontier model for complex coding, research, and real-world work."),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::XHigh,
            ],
            LogicalEffort::XHigh,
        ),
        AgentModelDefinition::built_in(
            AgentHarnessKind::Codex,
            "gpt-5.4",
            "gpt-5.4 - Strong model for everyday coding.",
            "gpt-5.4",
            Some("Strong model for everyday coding."),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::XHigh,
            ],
            LogicalEffort::XHigh,
        ),
        AgentModelDefinition::built_in(
            AgentHarnessKind::Codex,
            "gpt-5.4-mini",
            "gpt-5.4-mini - Small, fast, and cost-efficient model for simpler coding tasks.",
            "gpt-5.4-mini",
            Some("Small, fast, and cost-efficient model for simpler coding tasks."),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
            ],
            LogicalEffort::Medium,
        ),
        AgentModelDefinition::built_in(
            AgentHarnessKind::Codex,
            "gpt-5.3-codex",
            "gpt-5.3-codex - Coding-optimized model.",
            "gpt-5.3-codex",
            Some("Coding-optimized model."),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
            ],
            LogicalEffort::High,
        ),
        AgentModelDefinition::built_in(
            AgentHarnessKind::Codex,
            "gpt-5.3-codex-spark",
            "gpt-5.3-codex-spark - Ultra-fast coding model.",
            "gpt-5.3-codex-spark",
            Some("Ultra-fast coding model."),
            vec![LogicalEffort::Low, LogicalEffort::Medium],
            LogicalEffort::Medium,
        ),
    ]
}

pub fn default_model_for_provider(provider: AgentHarnessKind) -> &'static str {
    match provider {
        AgentHarnessKind::Claude => "sonnet",
        AgentHarnessKind::Codex => "gpt-5.5",
    }
}

pub fn lightweight_model_for_provider(provider: AgentHarnessKind) -> &'static str {
    match provider {
        AgentHarnessKind::Claude => "haiku",
        AgentHarnessKind::Codex => "gpt-5.4-mini",
    }
}

pub fn default_effort_for_provider(provider: AgentHarnessKind) -> LogicalEffort {
    match provider {
        AgentHarnessKind::Claude => LogicalEffort::Medium,
        AgentHarnessKind::Codex => LogicalEffort::XHigh,
    }
}

pub fn default_efforts_for_provider(provider: AgentHarnessKind) -> &'static [LogicalEffort] {
    match provider {
        AgentHarnessKind::Claude => &[
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
        ],
        AgentHarnessKind::Codex => &[
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::XHigh,
        ],
    }
}

fn effort_order(effort: LogicalEffort) -> u8 {
    match effort {
        LogicalEffort::Low => 0,
        LogicalEffort::Medium => 1,
        LogicalEffort::High => 2,
        LogicalEffort::XHigh => 3,
        LogicalEffort::Max => 4,
    }
}

fn provider_order(provider: AgentHarnessKind) -> u8 {
    match provider {
        AgentHarnessKind::Claude => 0,
        AgentHarnessKind::Codex => 1,
    }
}

#[cfg(test)]
#[path = "model_registry_tests.rs"]
mod tests;
