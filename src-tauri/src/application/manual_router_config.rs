use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use crate::domain::agents::{
    AgentHarnessKind, AtlassianMcpAccess, LogicalEffort, ManualRoleDefault, ManualServiceTier,
    RoutingRole,
};
use crate::domain::entities::{CoordinationMode, PersonaId};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[derive(Debug, Default)]
pub struct ManualRouterSnapshot {
    pub entries: HashMap<RoutingRole, Result<ManualRoleDefault, String>>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManualRoleDefault {
    #[serde(alias = "harness")]
    provider: AgentHarnessKind,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<LogicalEffort>,
    #[serde(default, alias = "speed")]
    service_tier: ManualServiceTier,
    #[serde(default, alias = "capability")]
    coordination_mode: Option<CoordinationMode>,
    #[serde(default)]
    persona_id: Option<PersonaId>,
    #[serde(default)]
    approval_policy: Option<String>,
    #[serde(default)]
    sandbox_mode: Option<String>,
    #[serde(default)]
    atlassian_access: Option<AtlassianMcpAccess>,
}

impl TryFrom<RawManualRoleDefault> for ManualRoleDefault {
    type Error = String;

    fn try_from(raw: RawManualRoleDefault) -> Result<Self, Self::Error> {
        Ok(Self {
            harness: raw.provider,
            model: raw.model,
            effort: raw.effort,
            service_tier: raw.service_tier,
            coordination_mode: raw.coordination_mode,
            persona_id: raw.persona_id,
            approval_policy: raw.approval_policy,
            sandbox_mode: raw.sandbox_mode,
            atlassian_access: raw.atlassian_access,
        })
    }
}

pub fn load_manual_router_file(
    owned_root: &Path,
    router_path: &Path,
) -> AppResult<ManualRouterSnapshot> {
    let (owned_root, router_path) = validate_router_path(owned_root, router_path)?;

    // codeql[rust/path-injection]
    if !router_path.exists() {
        return Ok(ManualRouterSnapshot::default());
    }

    let canonical_root = owned_root.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to resolve router config root {}: {error}",
            owned_root.display()
        ))
    })?;
    let canonical_router = router_path.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to resolve router config {}: {error}",
            router_path.display()
        ))
    })?;
    if !canonical_router.starts_with(&canonical_root) {
        return Err(AppError::Validation(format!(
            "Router config escapes its owned root: {}",
            router_path.display()
        )));
    }

    let contents = fs::read_to_string(&canonical_router).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to read router config {}: {error}",
            router_path.display()
        ))
    })?;
    parse_manual_router(&contents)
}

fn validate_router_path(owned_root: &Path, router_path: &Path) -> AppResult<(PathBuf, PathBuf)> {
    let owned_root = validate_absolute_non_root_path(owned_root, "router config root")?;
    let router_path = validate_absolute_non_root_path(router_path, "router config")?;
    let relative_path = router_path.strip_prefix(&owned_root).map_err(|_| {
        AppError::Validation(format!(
            "Router config escapes its owned root: {}",
            router_path.display()
        ))
    })?;
    if relative_path != Path::new("router.yaml")
        && relative_path != Path::new(".ralphx").join("router.yaml")
    {
        return Err(AppError::Validation(format!(
            "Router config must use a supported path under its owned root: {}",
            router_path.display()
        )));
    }

    Ok((owned_root, router_path))
}

fn parse_manual_router(contents: &str) -> AppResult<ManualRouterSnapshot> {
    let document = serde_yaml::from_str::<Value>(contents).map_err(|error| {
        AppError::Validation(format!("Failed to parse router config YAML: {error}"))
    })?;
    let mut snapshot = ManualRouterSnapshot::default();
    let Some(root) = document.as_mapping() else {
        return Err(AppError::Validation(
            "Router config must be a YAML mapping".into(),
        ));
    };
    record_unknown_keys(root, &["manual"], "router", &mut snapshot.diagnostics);

    let Some(manual) = mapping_child(root, "manual")? else {
        return Ok(snapshot);
    };
    record_unknown_keys(manual, &["defaults"], "manual", &mut snapshot.diagnostics);
    let Some(defaults) = mapping_child(manual, "defaults")? else {
        return Ok(snapshot);
    };
    record_unknown_keys(
        defaults,
        &["roles"],
        "manual.defaults",
        &mut snapshot.diagnostics,
    );
    let Some(roles) = mapping_child(defaults, "roles")? else {
        return Ok(snapshot);
    };

    for (role_key, raw_value) in roles {
        let Some(role_key) = role_key.as_str() else {
            snapshot
                .diagnostics
                .push("manual.defaults.roles keys must be strings".into());
            continue;
        };
        let Ok(role) = role_key.parse::<RoutingRole>() else {
            snapshot
                .diagnostics
                .push(format!("Unknown routing role '{role_key}'"));
            continue;
        };
        let parsed = serde_yaml::from_value::<RawManualRoleDefault>(raw_value.clone())
            .map_err(|error| error.to_string())
            .and_then(ManualRoleDefault::try_from);
        snapshot.entries.insert(role, parsed);
    }

    Ok(snapshot)
}

fn mapping_child<'a>(mapping: &'a Mapping, key: &str) -> AppResult<Option<&'a Mapping>> {
    let Some(value) = mapping.get(Value::String(key.to_string())) else {
        return Ok(None);
    };
    value
        .as_mapping()
        .map(Some)
        .ok_or_else(|| AppError::Validation(format!("Router config '{key}' must be a mapping")))
}

fn record_unknown_keys(
    mapping: &Mapping,
    allowed: &[&str],
    path: &str,
    diagnostics: &mut Vec<String>,
) {
    for key in mapping.keys().filter_map(Value::as_str) {
        if !allowed.contains(&key) {
            diagnostics.push(format!("Unknown router config key '{path}.{key}'"));
        }
    }
}
