// Provider-neutral logical process to canonical agent mapping.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const EMBEDDED_PROCESS_CONFIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../config/processes.yaml"
));
// ── Core types ──────────────────────────────────────────────────────────

/// A single process slot with a default agent and optional named variants.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct ProcessSlot {
    pub default: String,
    #[serde(flatten)]
    pub variants: HashMap<String, String>,
}

/// Maps logical process names to their agent slots.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct ProcessMapping {
    #[serde(flatten)]
    pub slots: HashMap<String, ProcessSlot>,
}

#[derive(Debug, Clone, Deserialize)]
struct EmbeddedProcessConfig {
    process_mapping: ProcessMapping,
}

static CANONICAL_PROCESS_CONFIG: OnceLock<EmbeddedProcessConfig> = OnceLock::new();

fn embedded_process_config() -> &'static EmbeddedProcessConfig {
    CANONICAL_PROCESS_CONFIG.get_or_init(|| {
        serde_yaml::from_str(EMBEDDED_PROCESS_CONFIG)
            .expect("embedded config/processes.yaml should parse")
    })
}

pub fn canonical_process_mapping() -> ProcessMapping {
    embedded_process_config().process_mapping.clone()
}

pub fn resolve_canonical_process_mapping(raw: &ProcessMapping) -> ProcessMapping {
    let mut resolved = canonical_process_mapping();

    for (process, yaml_slot) in &raw.slots {
        match resolved.slots.get(process) {
            Some(canonical_slot) => {
                if canonical_slot != yaml_slot {
                    tracing::warn!(
                        process = %process,
                        yaml_slot = ?yaml_slot,
                        canonical_slot = ?canonical_slot,
                        "Canonical process mapping overrides divergent runtime YAML slot"
                    );
                }
            }
            None => {
                resolved.slots.insert(process.clone(), yaml_slot.clone());
            }
        }
    }

    resolved
}
// ── Process agent resolution ────────────────────────────────────────────

/// Resolve which agent to use for a process + variant combination.
///
/// Fallback chain: process_mapping variant → process_mapping default → None.
pub fn resolve_process_agent(
    mapping: &ProcessMapping,
    process: &str,
    variant: &str,
) -> Option<String> {
    let slot = mapping.slots.get(process)?;

    if variant == "default" {
        return Some(slot.default.clone());
    }

    slot.variants
        .get(variant)
        .cloned()
        .or_else(|| Some(slot.default.clone()))
}
