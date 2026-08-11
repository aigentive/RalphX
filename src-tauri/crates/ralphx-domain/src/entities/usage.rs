use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::agents::AgentHarnessKind;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentRunUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_usd: Option<f64>,
}

impl AgentRunUsage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cache_creation_tokens.is_none()
            && self.cache_read_tokens.is_none()
            && self.estimated_usd.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageProvenance {
    ProviderTurnDelta,
    DerivedCumulativeDelta,
    ProviderSnapshotFallback,
    CumulativeBaselineOnly,
}

impl fmt::Display for UsageProvenance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ProviderTurnDelta => "provider_turn_delta",
            Self::DerivedCumulativeDelta => "derived_cumulative_delta",
            Self::ProviderSnapshotFallback => "provider_snapshot_fallback",
            Self::CumulativeBaselineOnly => "cumulative_baseline_only",
        })
    }
}

impl FromStr for UsageProvenance {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "provider_turn_delta" => Ok(Self::ProviderTurnDelta),
            "derived_cumulative_delta" => Ok(Self::DerivedCumulativeDelta),
            "provider_snapshot_fallback" => Ok(Self::ProviderSnapshotFallback),
            "cumulative_baseline_only" => Ok(Self::CumulativeBaselineOnly),
            other => Err(format!("Invalid usage provenance: {other}")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsageSnapshot {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
}

impl ProviderUsageSnapshot {
    pub fn from_usage(usage: AgentRunUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_creation_tokens: usage.cache_creation_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            estimated_usd: usage.estimated_usd,
        }
    }

    pub fn as_usage(&self) -> AgentRunUsage {
        AgentRunUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            cache_read_tokens: self.cache_read_tokens,
            estimated_usd: self.estimated_usd,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageCapture {
    pub normalized: AgentRunUsage,
    pub provenance: UsageProvenance,
    pub raw_snapshot: Option<ProviderUsageSnapshot>,
}

impl UsageCapture {
    pub fn normalized(normalized: AgentRunUsage, provenance: UsageProvenance) -> Self {
        debug_assert_ne!(provenance, UsageProvenance::CumulativeBaselineOnly);
        Self {
            normalized,
            provenance,
            raw_snapshot: None,
        }
    }

    pub fn with_raw_snapshot(mut self, raw_snapshot: ProviderUsageSnapshot) -> Self {
        self.raw_snapshot = Some(raw_snapshot);
        self
    }

    pub fn cumulative_baseline(raw_snapshot: ProviderUsageSnapshot) -> Self {
        Self {
            normalized: AgentRunUsage::default(),
            provenance: UsageProvenance::CumulativeBaselineOnly,
            raw_snapshot: Some(raw_snapshot),
        }
    }
}

pub fn processed_tokens(
    harness: Option<AgentHarnessKind>,
    usage: &AgentRunUsage,
    provenance: Option<UsageProvenance>,
) -> Option<u64> {
    if matches!(provenance, Some(UsageProvenance::CumulativeBaselineOnly))
        || usage.input_tokens.is_none()
            && usage.output_tokens.is_none()
            && usage.cache_creation_tokens.is_none()
            && usage.cache_read_tokens.is_none()
    {
        return None;
    }

    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    let base = input.checked_add(output)?;

    match harness? {
        AgentHarnessKind::Codex => Some(base),
        AgentHarnessKind::Claude => base
            .checked_add(usage.cache_creation_tokens.unwrap_or(0))?
            .checked_add(usage.cache_read_tokens.unwrap_or(0)),
    }
}
