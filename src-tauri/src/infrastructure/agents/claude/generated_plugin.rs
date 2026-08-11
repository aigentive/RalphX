use crate::infrastructure::agents::claude::{
    claude_runtime_config, find_base_plugin_dir, get_agent_config,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    internal_mcp_server_name, list_canonical_agent_names, load_canonical_agent_definition,
    load_harness_agent_prompt, resolve_project_root_from_plugin_dir,
    try_load_canonical_claude_metadata, AgentPromptHarness, CanonicalAgentDefinition,
    CanonicalClaudeAgentMetadata,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tracing::warn;

const GENERATED_PLUGIN_DIR_REL_DEBUG: &str = ".artifacts/generated/claude-plugin";
const GENERATED_PLUGIN_DIR_REL_PROD_ROOT: &str = "generated";
const GENERATED_PLUGIN_DIR_NAME: &str = "claude-plugin";
const GENERATED_AGENTS_DIR: &str = "agents";
const HOOKS_ENTRY_NAME: &str = "hooks";
const HOOKS_CONFIG_FILE: &str = "hooks.json";
const HOOKS_SCRIPTS_DIR: &str = "scripts";
const EMPTY_HOOKS_CONFIG: &str = "{\n  \"hooks\": {}\n}\n";
const INTERNAL_MCP_SERVER_DIR: &str = "ralphx-mcp-server";
const EXTERNAL_MCP_SERVER_DIR: &str = "ralphx-external-mcp";
const FALLBACK_RUNTIME_ENTRY_NAMES: &[&str] = &[INTERNAL_MCP_SERVER_DIR, EXTERNAL_MCP_SERVER_DIR];
const GENERATED_PLUGIN_ENTRY_NAMES: &[&str] = &[
    ".claude-plugin",
    HOOKS_ENTRY_NAME,
    "memory-framework.md",
    "skills",
    INTERNAL_MCP_SERVER_DIR,
    EXTERNAL_MCP_SERVER_DIR,
];

fn generated_plugin_materialization_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn generated_plugin_dir_cache() -> &'static Mutex<HashMap<PathBuf, PathBuf>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn generated_plugin_dir_override() -> &'static Mutex<Option<PathBuf>> {
    static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

pub(super) fn replace_generated_plugin_dir_override(next: Option<PathBuf>) -> Option<PathBuf> {
    let mut guard = generated_plugin_dir_override()
        .lock()
        .expect("generated plugin dir override lock poisoned");
    std::mem::replace(&mut *guard, next)
}

fn configured_generated_plugin_dir() -> Option<PathBuf> {
    generated_plugin_dir_override()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

fn generated_plugin_cache_key(base_plugin_dir: &Path) -> PathBuf {
    base_plugin_dir
        .canonicalize()
        .unwrap_or_else(|_| base_plugin_dir.to_path_buf())
}

fn cached_generated_plugin_dir(
    base_plugin_dir: &Path,
    runtime_source_plugin_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let cache_key = generated_plugin_cache_key(base_plugin_dir);
    let cached = {
        let cache = generated_plugin_dir_cache()
            .lock()
            .map_err(|_| "Generated Claude plugin cache lock poisoned".to_string())?;
        cache.get(&cache_key).cloned()
    };

    let Some(generated_dir) = cached else {
        return Ok(None);
    };

    if generated_plugin_dir_is_current(base_plugin_dir, runtime_source_plugin_dir, &generated_dir)?
    {
        return Ok(Some(generated_dir));
    }

    let mut cache = generated_plugin_dir_cache()
        .lock()
        .map_err(|_| "Generated Claude plugin cache lock poisoned".to_string())?;
    cache.remove(&cache_key);
    Ok(None)
}

fn cache_generated_plugin_dir(
    base_plugin_dir: &Path,
    generated_plugin_dir: &Path,
) -> Result<(), String> {
    let mut cache = generated_plugin_dir_cache()
        .lock()
        .map_err(|_| "Generated Claude plugin cache lock poisoned".to_string())?;
    cache.insert(
        generated_plugin_cache_key(base_plugin_dir),
        generated_plugin_dir.to_path_buf(),
    );
    Ok(())
}

pub(crate) fn materialize_generated_plugin_dir(base_plugin_dir: &Path) -> Result<PathBuf, String> {
    materialize_generated_plugin_dir_with_runtime_source(
        base_plugin_dir,
        find_base_plugin_dir().as_deref(),
    )
}

pub(crate) fn materialize_generated_plugin_dir_with_runtime_source(
    base_plugin_dir: &Path,
    fallback_runtime_plugin_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let runtime_source_plugin_dir =
        resolve_runtime_entries_source_plugin_dir(base_plugin_dir, fallback_runtime_plugin_dir);

    // Generated Claude assets are process-local runtime bootstrap outputs.
    // Reuse a clean first materialization, but repair the dir if another
    // process mutates managed symlinks or leaves unmanaged top-level entries.
    if let Some(cached_dir) =
        cached_generated_plugin_dir(base_plugin_dir, &runtime_source_plugin_dir)?
    {
        return Ok(cached_dir);
    }

    let _guard = generated_plugin_materialization_lock()
        .lock()
        .map_err(|_| "Generated Claude plugin materialization lock poisoned".to_string())?;
    if let Some(cached_dir) =
        cached_generated_plugin_dir(base_plugin_dir, &runtime_source_plugin_dir)?
    {
        return Ok(cached_dir);
    }

    let project_root = resolve_project_root_from_plugin_dir(base_plugin_dir);
    let generated_plugin_dir = generated_plugin_dir_for_base(base_plugin_dir);

    // codeql[rust/path-injection]
    fs::create_dir_all(&generated_plugin_dir).map_err(|error| {
        format!(
            "Failed to create generated Claude plugin dir {}: {error}",
            generated_plugin_dir.display()
        )
    })?;

    prune_unmanaged_generated_plugin_entries(&generated_plugin_dir)?;
    sync_runtime_entries(
        base_plugin_dir,
        &runtime_source_plugin_dir,
        &generated_plugin_dir,
    )?;
    sync_generated_agent_prompts(&generated_plugin_dir, &project_root)?;
    cache_generated_plugin_dir(base_plugin_dir, &generated_plugin_dir)?;

    Ok(generated_plugin_dir)
}

pub(crate) fn resolve_runtime_entries_source_plugin_dir(
    base_plugin_dir: &Path,
    fallback_runtime_plugin_dir: Option<&Path>,
) -> PathBuf {
    if let Some(fallback_runtime_plugin_dir) =
        fallback_runtime_plugin_dir.filter(|candidate| *candidate != base_plugin_dir)
    {
        warn!(
            base_plugin_dir = %base_plugin_dir.display(),
            fallback_runtime_plugin_dir = %fallback_runtime_plugin_dir.display(),
            "Local plugin dir is missing runnable MCP runtime dependencies; falling back to canonical runtime bundle"
        );
        return fallback_runtime_plugin_dir.to_path_buf();
    }

    base_plugin_dir.to_path_buf()
}

fn generated_plugin_dir_for_base(base_plugin_dir: &Path) -> PathBuf {
    generated_plugin_dir_for_base_with_override(
        base_plugin_dir,
        configured_generated_plugin_dir().as_deref(),
    )
}

fn generated_plugin_dir_for_base_with_override(
    base_plugin_dir: &Path,
    override_dir: Option<&Path>,
) -> PathBuf {
    if let Some(override_dir) = override_dir {
        return override_dir.to_path_buf();
    }

    let repo_root = resolve_project_root_from_plugin_dir(base_plugin_dir);
    if cfg!(debug_assertions) {
        repo_root.join(GENERATED_PLUGIN_DIR_REL_DEBUG)
    } else {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("com.ralphx.app")
            .join(GENERATED_PLUGIN_DIR_REL_PROD_ROOT)
            .join(generated_plugin_runtime_profile_component())
            .join(GENERATED_PLUGIN_DIR_NAME)
    }
}

fn generated_plugin_runtime_profile_component() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn sync_runtime_entries(
    base_plugin_dir: &Path,
    runtime_source_plugin_dir: &Path,
    generated_plugin_dir: &Path,
) -> Result<(), String> {
    for file_name in GENERATED_PLUGIN_ENTRY_NAMES {
        if *file_name == HOOKS_ENTRY_NAME {
            sync_hooks_entry(base_plugin_dir, generated_plugin_dir)?;
            continue;
        }

        let target = generated_plugin_dir.join(file_name);
        let source =
            expected_runtime_entry_source(base_plugin_dir, runtime_source_plugin_dir, file_name);

        ensure_symlink(&source, &target)?;
    }
    Ok(())
}

fn expected_runtime_entry_source(
    base_plugin_dir: &Path,
    runtime_source_plugin_dir: &Path,
    file_name: &str,
) -> PathBuf {
    let preferred_runtime_source = runtime_source_plugin_dir.join(file_name);
    if FALLBACK_RUNTIME_ENTRY_NAMES.contains(&file_name)
        && runtime_source_plugin_dir != base_plugin_dir
        // codeql[rust/path-injection]
        && preferred_runtime_source.exists()
    {
        preferred_runtime_source
    } else {
        base_plugin_dir.join(file_name)
    }
}

fn generated_plugin_dir_is_current(
    base_plugin_dir: &Path,
    runtime_source_plugin_dir: &Path,
    generated_plugin_dir: &Path,
) -> Result<bool, String> {
    // codeql[rust/path-injection]
    if !generated_plugin_dir.exists() {
        return Ok(false);
    }

    if !generated_plugin_dir_has_only_managed_entries(generated_plugin_dir)? {
        return Ok(false);
    }

    let generated_agents_dir = generated_plugin_dir.join(GENERATED_AGENTS_DIR);
    // codeql[rust/path-injection]
    match fs::symlink_metadata(&generated_agents_dir) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to inspect generated Claude agents dir {}: {error}",
                generated_agents_dir.display()
            ));
        }
    }

    for file_name in GENERATED_PLUGIN_ENTRY_NAMES {
        if *file_name == HOOKS_ENTRY_NAME {
            if !generated_hooks_entry_is_current(base_plugin_dir, generated_plugin_dir)? {
                return Ok(false);
            }
            continue;
        }

        let target = generated_plugin_dir.join(file_name);
        let expected_source =
            expected_runtime_entry_source(base_plugin_dir, runtime_source_plugin_dir, file_name);
        // codeql[rust/path-injection]
        match fs::read_link(&target) {
            Ok(existing) if existing == expected_source => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(format!(
                    "Failed to inspect generated Claude plugin symlink {}: {error}",
                    target.display()
                ));
            }
        }
    }

    Ok(true)
}

fn sync_hooks_entry(base_plugin_dir: &Path, generated_plugin_dir: &Path) -> Result<(), String> {
    let source_hooks_dir = base_plugin_dir.join(HOOKS_ENTRY_NAME);
    let target_hooks_dir = generated_plugin_dir.join(HOOKS_ENTRY_NAME);
    remove_existing_path(&target_hooks_dir)?;

    // codeql[rust/path-injection]
    fs::create_dir_all(&target_hooks_dir).map_err(|error| {
        format!(
            "Failed to create generated Claude hooks dir {}: {error}",
            target_hooks_dir.display()
        )
    })?;

    let hooks_config = read_valid_hooks_config(&source_hooks_dir.join(HOOKS_CONFIG_FILE))?
        .unwrap_or_else(|| EMPTY_HOOKS_CONFIG.to_string());
    // codeql[rust/path-injection]
    fs::write(target_hooks_dir.join(HOOKS_CONFIG_FILE), hooks_config).map_err(|error| {
        format!(
            "Failed to write generated Claude hooks config {}: {error}",
            target_hooks_dir.join(HOOKS_CONFIG_FILE).display()
        )
    })?;

    let source_scripts_dir = source_hooks_dir.join(HOOKS_SCRIPTS_DIR);
    // codeql[rust/path-injection]
    if source_scripts_dir.exists() {
        ensure_symlink(
            &source_scripts_dir,
            &target_hooks_dir.join(HOOKS_SCRIPTS_DIR),
        )?;
    }

    Ok(())
}

fn read_valid_hooks_config(path: &Path) -> Result<Option<String>, String> {
    // codeql[rust/path-injection]
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Failed to read Claude hooks config {}: {error}",
                path.display()
            ));
        }
    };

    if hooks_config_has_record(&raw) {
        Ok(Some(raw))
    } else {
        warn!(
            path = %path.display(),
            "Normalizing generated Claude hooks config to an empty hooks record"
        );
        Ok(None)
    }
}

fn hooks_config_has_record(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.get("hooks").map(serde_json::Value::is_object))
        .unwrap_or(false)
}

fn generated_hooks_entry_is_current(
    base_plugin_dir: &Path,
    generated_plugin_dir: &Path,
) -> Result<bool, String> {
    let target_hooks_dir = generated_plugin_dir.join(HOOKS_ENTRY_NAME);
    // codeql[rust/path-injection]
    match fs::symlink_metadata(&target_hooks_dir) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to inspect generated Claude hooks dir {}: {error}",
                target_hooks_dir.display()
            ));
        }
    }

    // codeql[rust/path-injection]
    let raw = match fs::read_to_string(target_hooks_dir.join(HOOKS_CONFIG_FILE)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to read generated Claude hooks config {}: {error}",
                target_hooks_dir.join(HOOKS_CONFIG_FILE).display()
            ));
        }
    };
    if !hooks_config_has_record(&raw) {
        return Ok(false);
    }

    let source_scripts_dir = base_plugin_dir
        .join(HOOKS_ENTRY_NAME)
        .join(HOOKS_SCRIPTS_DIR);
    let target_scripts_dir = target_hooks_dir.join(HOOKS_SCRIPTS_DIR);
    // codeql[rust/path-injection]
    if source_scripts_dir.exists() {
        // codeql[rust/path-injection]
        match fs::read_link(&target_scripts_dir) {
            Ok(existing) if existing == source_scripts_dir => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(format!(
                    "Failed to inspect generated Claude hooks scripts symlink {}: {error}",
                    target_scripts_dir.display()
                ));
            }
        }
    } else {
        // codeql[rust/path-injection]
        match fs::symlink_metadata(&target_scripts_dir) {
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Failed to inspect generated Claude hooks scripts path {}: {error}",
                    target_scripts_dir.display()
                ));
            }
        }
    }

    Ok(true)
}

fn generated_plugin_dir_has_only_managed_entries(
    generated_plugin_dir: &Path,
) -> Result<bool, String> {
    // codeql[rust/path-injection]
    let entries = fs::read_dir(generated_plugin_dir).map_err(|error| {
        format!(
            "Failed to list generated Claude plugin dir {}: {error}",
            generated_plugin_dir.display()
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to read generated Claude plugin dir entry {}: {error}",
                generated_plugin_dir.display()
            )
        })?;
        if !is_managed_generated_plugin_entry(&entry.file_name()) {
            return Ok(false);
        }
    }

    Ok(true)
}

fn prune_unmanaged_generated_plugin_entries(generated_plugin_dir: &Path) -> Result<(), String> {
    let generated_root = generated_plugin_dir.canonicalize().map_err(|error| {
        format!(
            "Failed to canonicalize generated Claude plugin dir {}: {error}",
            generated_plugin_dir.display()
        )
    })?;
    // codeql[rust/path-injection]
    let entries = fs::read_dir(&generated_root).map_err(|error| {
        format!(
            "Failed to list generated Claude plugin dir {}: {error}",
            generated_root.display()
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to read generated Claude plugin dir entry {}: {error}",
                generated_root.display()
            )
        })?;
        if is_managed_generated_plugin_entry(&entry.file_name()) {
            continue;
        }

        let path =
            trusted_existing_generated_plugin_top_level_path(&generated_root, &entry.path())?;
        warn!(
            path = %path.display(),
            "Removing unmanaged generated Claude plugin entry"
        );
        remove_existing_path(&path)?;
    }

    Ok(())
}

fn is_managed_generated_plugin_entry(file_name: &std::ffi::OsStr) -> bool {
    file_name == GENERATED_AGENTS_DIR
        || GENERATED_PLUGIN_ENTRY_NAMES
            .iter()
            .any(|name| file_name == *name)
}

fn sync_generated_agent_prompts(
    generated_plugin_dir: &Path,
    project_root: &Path,
) -> Result<(), String> {
    let generated_agents_dir = generated_plugin_dir.join("agents");
    // codeql[rust/path-injection]
    if let Err(error) = fs::remove_dir_all(&generated_agents_dir) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(format!(
                "Failed to clear generated Claude agents dir {}: {error}",
                generated_agents_dir.display()
            ));
        }
    }
    // codeql[rust/path-injection]
    fs::create_dir_all(&generated_agents_dir).map_err(|error| {
        format!(
            "Failed to create generated Claude agents dir {}: {error}",
            generated_agents_dir.display()
        )
    })?;

    for short_name in list_canonical_agent_names(project_root) {
        let Some(definition) = load_canonical_agent_definition(project_root, &short_name) else {
            continue;
        };

        let relative_output = claude_output_relative_path(&definition, &short_name)?;

        let Some(prompt_body) =
            load_harness_agent_prompt(project_root, &short_name, AgentPromptHarness::Claude)
        else {
            continue;
        };
        let claude_metadata = try_load_canonical_claude_metadata(project_root, &short_name)?;

        let generated_target =
            trusted_generated_plugin_child_path(generated_plugin_dir, &relative_output)?;
        let rendered = render_generated_agent_markdown(
            &short_name,
            &definition,
            &claude_metadata,
            &prompt_body,
        )?;
        // codeql[rust/path-injection]
        fs::write(&generated_target, rendered).map_err(|error| {
            format!(
                "Failed to write generated Claude agent prompt {}: {error}",
                generated_target.display()
            )
        })?;
    }

    Ok(())
}

fn claude_output_relative_path(
    _definition: &CanonicalAgentDefinition,
    short_name: &str,
) -> Result<PathBuf, String> {
    Ok(PathBuf::from("agents").join(format!("{short_name}.md")))
}

fn relative_path_has_only_trusted_components(relative_path: &Path) -> bool {
    relative_path.components().all(|component| match component {
        std::path::Component::Normal(part) => {
            let part = part.to_string_lossy();
            !part.is_empty() && !part.contains("..")
        }
        _ => false,
    })
}

fn trusted_generated_plugin_child_path(
    generated_plugin_dir: &Path,
    relative_output: &Path,
) -> Result<PathBuf, String> {
    if !relative_path_has_only_trusted_components(relative_output) {
        return Err(format!(
            "Refusing generated Claude plugin output outside trusted relative path: {}",
            relative_output.display()
        ));
    }

    let generated_root = generated_plugin_dir.canonicalize().map_err(|error| {
        format!(
            "Failed to canonicalize generated Claude plugin dir {}: {error}",
            generated_plugin_dir.display()
        )
    })?;
    let target = generated_root.join(relative_output);
    let Some(parent) = target.parent() else {
        return Err(format!(
            "Generated Claude plugin output has no parent: {}",
            target.display()
        ));
    };

    // codeql[rust/path-injection]
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Failed to create generated Claude plugin output parent dir {}: {error}",
            parent.display()
        )
    })?;
    let canonical_parent = parent.canonicalize().map_err(|error| {
        format!(
            "Failed to canonicalize generated Claude plugin output parent dir {}: {error}",
            parent.display()
        )
    })?;
    if !canonical_parent.starts_with(&generated_root) {
        return Err(format!(
            "Refusing generated Claude plugin output outside {}: {}",
            generated_root.display(),
            canonical_parent.display()
        ));
    }

    let Some(file_name) = target.file_name() else {
        return Err(format!(
            "Generated Claude plugin output has no file name: {}",
            target.display()
        ));
    };
    Ok(canonical_parent.join(file_name))
}

fn trusted_existing_generated_plugin_top_level_path(
    generated_root: &Path,
    existing_path: &Path,
) -> Result<PathBuf, String> {
    let Some(parent) = existing_path.parent() else {
        return Err(format!(
            "Generated Claude plugin entry has no parent: {}",
            existing_path.display()
        ));
    };
    let canonical_parent = parent.canonicalize().map_err(|error| {
        format!(
            "Failed to canonicalize generated Claude plugin entry parent {}: {error}",
            parent.display()
        )
    })?;
    if canonical_parent != generated_root {
        return Err(format!(
            "Refusing to remove generated Claude plugin entry outside {}: {}",
            generated_root.display(),
            existing_path.display()
        ));
    }

    let Some(file_name) = existing_path.file_name() else {
        return Err(format!(
            "Generated Claude plugin entry has no file name: {}",
            existing_path.display()
        ));
    };
    Ok(canonical_parent.join(file_name))
}

fn render_generated_agent_markdown(
    agent_name: &str,
    definition: &CanonicalAgentDefinition,
    claude_metadata: &CanonicalClaudeAgentMetadata,
    prompt_body: &str,
) -> Result<String, String> {
    let frontmatter = build_claude_frontmatter(agent_name, definition, claude_metadata)?;
    Ok(format!("{frontmatter}\n\n{prompt_body}\n"))
}

fn build_claude_frontmatter(
    agent_name: &str,
    definition: &CanonicalAgentDefinition,
    claude_metadata: &CanonicalClaudeAgentMetadata,
) -> Result<String, String> {
    let agent_config = get_agent_config(agent_name).ok_or_else(|| {
        format!(
            "Canonical Claude agent {} is missing runtime config in config/ralphx.yaml",
            agent_name
        )
    })?;
    let mcp_server_name = &claude_runtime_config().mcp_server_name;
    let tools = build_claude_frontmatter_tools(agent_config, claude_metadata, mcp_server_name);
    let primary_mcp_tools = resolved_claude_primary_mcp_tools(agent_config, claude_metadata);
    let internal_sidecar_tools = resolved_claude_internal_sidecar_tools(claude_metadata);

    let mut lines = vec![
        "---".to_string(),
        format!("name: {}", yaml_scalar(&definition.name)?),
    ];

    if let Some(description) = definition.description.as_deref() {
        lines.push(format!("description: {}", yaml_scalar(description)?));
    }

    if !tools.is_empty() {
        lines.push("tools:".to_string());
        for tool in tools {
            lines.push(format!("  - {}", yaml_scalar(&tool)?));
        }
    }

    if !primary_mcp_tools.is_empty() || !internal_sidecar_tools.is_empty() {
        lines.push("mcpServers:".to_string());
        if !primary_mcp_tools.is_empty() {
            lines.push(format!("  - {}:", yaml_scalar(mcp_server_name)?));
        }
        if claude_metadata.mcp_transport.as_deref() == Some("external")
            && !primary_mcp_tools.is_empty()
        {
            lines.push("      type: http".to_string());
            lines.push(format!(
                "      url: {}",
                yaml_scalar("http://127.0.0.1:3848/mcp")?
            ));
            lines.push("      headers:".to_string());
            lines.push(format!(
                "        Authorization: {}",
                yaml_scalar("Bearer ${RALPHX_TAURI_MCP_BYPASS_TOKEN}")?
            ));
        } else if !primary_mcp_tools.is_empty() {
            push_stdio_mcp_server_frontmatter(&mut lines, agent_name, None)?;
        }
        if !internal_sidecar_tools.is_empty() {
            lines.push(format!(
                "  - {}:",
                yaml_scalar(&internal_mcp_server_name(mcp_server_name))?
            ));
            push_stdio_mcp_server_frontmatter(
                &mut lines,
                agent_name,
                Some(internal_sidecar_tools),
            )?;
        }
    }

    if !claude_metadata.disallowed_tools.is_empty() {
        lines.push("disallowedTools:".to_string());
        for tool in &claude_metadata.disallowed_tools {
            lines.push(format!("  - {}", yaml_scalar(tool)?));
        }
    }

    if let Some(model) = agent_config.model.as_deref() {
        lines.push(format!("model: {}", yaml_scalar(model)?));
    }

    if let Some(max_turns) = claude_metadata.max_turns {
        lines.push(format!("maxTurns: {max_turns}"));
    }

    if !claude_metadata.skills.is_empty() {
        lines.push("skills:".to_string());
        for skill in &claude_metadata.skills {
            lines.push(format!("  - {}", yaml_scalar(skill)?));
        }
    }

    lines.push("---".to_string());
    Ok(lines.join("\n"))
}

fn build_claude_frontmatter_tools(
    agent_config: &crate::infrastructure::agents::claude::AgentConfig,
    claude_metadata: &CanonicalClaudeAgentMetadata,
    mcp_server_name: &str,
) -> Vec<String> {
    let mut tools = Vec::new();
    if !agent_config.mcp_only {
        tools.extend(agent_config.resolved_cli_tools.iter().cloned());
    }

    tools.extend(resolved_claude_frontmatter_mcp_tools(
        agent_config,
        claude_metadata,
        mcp_server_name,
    ));
    tools.extend(agent_config.preapproved_cli_tools.iter().cloned());

    let mut seen = HashSet::new();
    tools.retain(|tool| seen.insert(tool.clone()));
    tools
}

fn resolved_claude_primary_mcp_tools<'a>(
    agent_config: &'a crate::infrastructure::agents::claude::AgentConfig,
    claude_metadata: &'a CanonicalClaudeAgentMetadata,
) -> &'a [String] {
    if claude_metadata.mcp_transport.as_deref() == Some("external")
        && !claude_metadata.mcp_tools.is_empty()
    {
        &claude_metadata.mcp_tools
    } else {
        &agent_config.allowed_mcp_tools
    }
}

fn resolved_claude_internal_sidecar_tools(
    claude_metadata: &CanonicalClaudeAgentMetadata,
) -> &[String] {
    if claude_metadata.mcp_transport.as_deref() == Some("external") {
        &claude_metadata.internal_mcp_tools
    } else {
        &[]
    }
}

fn resolved_claude_frontmatter_mcp_tools(
    agent_config: &crate::infrastructure::agents::claude::AgentConfig,
    claude_metadata: &CanonicalClaudeAgentMetadata,
    mcp_server_name: &str,
) -> Vec<String> {
    let mut tools = resolved_claude_primary_mcp_tools(agent_config, claude_metadata)
        .iter()
        .map(|tool| normalize_frontmatter_mcp_tool(tool, mcp_server_name))
        .collect::<Vec<_>>();
    let internal_server_name = internal_mcp_server_name(mcp_server_name);
    tools.extend(
        resolved_claude_internal_sidecar_tools(claude_metadata)
            .iter()
            .map(|tool| normalize_frontmatter_mcp_tool(tool, &internal_server_name)),
    );
    tools
}

fn push_stdio_mcp_server_frontmatter(
    lines: &mut Vec<String>,
    agent_name: &str,
    allowed_tools: Option<&[String]>,
) -> Result<(), String> {
    lines.push("      type: stdio".to_string());
    lines.push("      command: node".to_string());
    lines.push("      args:".to_string());
    lines.push(format!(
        "        - {}",
        yaml_scalar("${CLAUDE_PLUGIN_ROOT}/ralphx-mcp-server/build/index.js")?
    ));
    lines.push(format!("        - {}", yaml_scalar("--agent-type")?));
    lines.push(format!("        - {}", yaml_scalar(agent_name)?));
    if let Some(tools) = allowed_tools {
        lines.push(format!(
            "        - {}",
            yaml_scalar(&format!("--allowed-tools={}", allowed_tools_arg(tools)))?
        ));
    }
    Ok(())
}

fn allowed_tools_arg(tools: &[String]) -> String {
    if tools.is_empty() {
        "__NONE__".to_string()
    } else {
        tools.join(",")
    }
}

fn normalize_frontmatter_mcp_tool(tool: &str, mcp_server_name: &str) -> String {
    if tool.starts_with("mcp__") {
        tool.to_string()
    } else {
        format!("mcp__{mcp_server_name}__{tool}")
    }
}

fn yaml_scalar(value: &str) -> Result<String, String> {
    serde_yaml::to_string(value)
        .map(|rendered| rendered.trim().to_string())
        .map_err(|error| format!("Failed to render YAML scalar {value:?}: {error}"))
}

fn ensure_symlink(source: &Path, target: &Path) -> Result<(), String> {
    // codeql[rust/path-injection]
    if let Ok(existing) = fs::read_link(target) {
        if existing == source {
            return Ok(());
        }
    }

    remove_existing_path(target)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create generated Claude plugin parent dir {}: {error}",
                parent.display()
            )
        })?;
    }

    symlink_path(source, target)
}

fn remove_existing_path(path: &Path) -> Result<(), String> {
    // codeql[rust/path-injection]
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Failed to inspect existing generated Claude plugin path {}: {error}",
                path.display()
            ));
        }
    };

    if metadata.file_type().is_symlink() || metadata.is_file() {
        // codeql[rust/path-injection]
        fs::remove_file(path).map_err(|error| {
            format!(
                "Failed to remove generated Claude plugin file {}: {error}",
                path.display()
            )
        })
    } else {
        // codeql[rust/path-injection]
        fs::remove_dir_all(path).map_err(|error| {
            format!(
                "Failed to remove generated Claude plugin directory {}: {error}",
                path.display()
            )
        })
    }
}

#[cfg(unix)]
fn symlink_path(source: &Path, target: &Path) -> Result<(), String> {
    // codeql[rust/path-injection]
    std::os::unix::fs::symlink(source, target).map_err(|error| {
        format!(
            "Failed to symlink generated Claude plugin path {} -> {}: {error}",
            target.display(),
            source.display()
        )
    })
}

#[cfg(windows)]
fn symlink_path(source: &Path, target: &Path) -> Result<(), String> {
    // codeql[rust/path-injection]
    let result = if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target)
    } else {
        std::os::windows::fs::symlink_file(source, target)
    };
    result.map_err(|error| {
        format!(
            "Failed to symlink generated Claude plugin path {} -> {}: {error}",
            target.display(),
            source.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_symlink, expected_runtime_entry_source, generated_hooks_entry_is_current,
        generated_plugin_dir_for_base_with_override, generated_plugin_dir_has_only_managed_entries,
        generated_plugin_dir_is_current, generated_plugin_runtime_profile_component,
        prune_unmanaged_generated_plugin_entries, read_valid_hooks_config,
        render_generated_agent_markdown, sync_runtime_entries,
        trusted_existing_generated_plugin_top_level_path, trusted_generated_plugin_child_path,
        EMPTY_HOOKS_CONFIG, EXTERNAL_MCP_SERVER_DIR, GENERATED_AGENTS_DIR, HOOKS_CONFIG_FILE,
        HOOKS_ENTRY_NAME, HOOKS_SCRIPTS_DIR, INTERNAL_MCP_SERVER_DIR,
    };
    use crate::infrastructure::agents::harness_agent_catalog::{
        CanonicalAgentDefinition, CanonicalClaudeAgentMetadata,
    };
    use crate::utils::path_safety::{checked_exists, validate_absolute_non_root_path};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn generated_plugin_dir_uses_override_when_present() {
        let override_dir = PathBuf::from("/tmp/custom-generated-plugin-dir");
        let resolved = generated_plugin_dir_for_base_with_override(
            Path::new("/tmp/ralphx/plugins/app"),
            Some(&override_dir),
        );

        assert_eq!(resolved, PathBuf::from(&override_dir));
    }

    #[test]
    fn generated_plugin_dir_uses_debug_repo_artifact_path_without_override() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let root = dir.path();
        let base_plugin_dir = root.join("plugins/app");
        fs::create_dir_all(&base_plugin_dir).expect("create base plugin dir");

        let resolved = generated_plugin_dir_for_base_with_override(&base_plugin_dir, None);

        assert_eq!(resolved, root.join(".artifacts/generated/claude-plugin"));
    }

    #[test]
    fn generated_plugin_runtime_profile_uses_debug_for_test_builds() {
        assert_eq!(generated_plugin_runtime_profile_component(), "debug");
    }

    #[test]
    fn expected_runtime_entry_source_uses_fallback_only_for_runnable_entries() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let fallback_plugin_dir = dir.path().join("fallback/plugins/app");
        fs::create_dir_all(fallback_plugin_dir.join(INTERNAL_MCP_SERVER_DIR))
            .expect("create fallback internal mcp dir");

        assert_eq!(
            expected_runtime_entry_source(
                &base_plugin_dir,
                &fallback_plugin_dir,
                INTERNAL_MCP_SERVER_DIR,
            ),
            fallback_plugin_dir.join(INTERNAL_MCP_SERVER_DIR)
        );
        assert_eq!(
            expected_runtime_entry_source(
                &base_plugin_dir,
                &fallback_plugin_dir,
                EXTERNAL_MCP_SERVER_DIR,
            ),
            base_plugin_dir.join(EXTERNAL_MCP_SERVER_DIR)
        );
    }

    fn seed_generated_runtime_links(base_plugin_dir: &Path, generated_dir: &Path) {
        fs::create_dir_all(generated_dir.join(GENERATED_AGENTS_DIR))
            .expect("create generated agents dir");
        sync_runtime_entries(base_plugin_dir, base_plugin_dir, generated_dir)
            .expect("seed generated runtime entries");
    }

    fn create_test_dir_all(path: impl AsRef<Path>) {
        let path = validate_absolute_non_root_path(path.as_ref(), "generated plugin test dir")
            .expect("validate generated plugin test dir");

        // codeql[rust/path-injection]
        fs::create_dir_all(path).expect("create generated plugin test dir");
    }

    fn write_test_file(path: impl AsRef<Path>, contents: &str) {
        let path = validate_absolute_non_root_path(path.as_ref(), "generated plugin test file")
            .expect("validate generated plugin test file");

        // codeql[rust/path-injection]
        fs::write(path, contents).expect("write generated plugin test file");
    }

    fn remove_test_file(path: impl AsRef<Path>) {
        let path = validate_absolute_non_root_path(path.as_ref(), "generated plugin test removal")
            .expect("validate generated plugin test removal path");

        // codeql[rust/path-injection]
        fs::remove_file(path).expect("remove generated plugin test file");
    }

    fn remove_test_dir_all(path: impl AsRef<Path>) {
        let path = validate_absolute_non_root_path(path.as_ref(), "generated plugin test removal")
            .expect("validate generated plugin test removal path");

        // codeql[rust/path-injection]
        fs::remove_dir_all(path).expect("remove generated plugin test dir");
    }

    fn test_path_exists(path: impl AsRef<Path>) -> bool {
        checked_exists(path.as_ref(), "generated plugin test path")
            .expect("inspect generated plugin test path")
    }

    #[test]
    fn generated_plugin_runtime_entries_normalize_empty_hooks_config() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        let base_hooks_dir = base_plugin_dir.join(HOOKS_ENTRY_NAME);
        fs::create_dir_all(base_hooks_dir.join(HOOKS_SCRIPTS_DIR))
            .expect("create base hooks scripts dir");
        fs::create_dir_all(&generated_dir).expect("create generated plugin dir");
        fs::write(base_hooks_dir.join(HOOKS_CONFIG_FILE), "{}").expect("write stale hooks config");

        sync_runtime_entries(&base_plugin_dir, &base_plugin_dir, &generated_dir)
            .expect("sync runtime entries");

        assert_eq!(
            fs::read_to_string(generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_CONFIG_FILE))
                .expect("read generated hooks config"),
            EMPTY_HOOKS_CONFIG
        );
        assert_eq!(
            fs::read_link(generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_SCRIPTS_DIR))
                .expect("read generated hooks scripts symlink"),
            base_hooks_dir.join(HOOKS_SCRIPTS_DIR)
        );
    }

    #[test]
    fn generated_plugin_runtime_entries_preserve_valid_hooks_config_without_scripts() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        let base_hooks_dir = base_plugin_dir.join(HOOKS_ENTRY_NAME);
        let valid_hooks_config = "{\n  \"hooks\": {\n    \"PreToolUse\": []\n  }\n}\n";
        fs::create_dir_all(&base_hooks_dir).expect("create base hooks dir");
        fs::create_dir_all(&generated_dir).expect("create generated plugin dir");
        fs::write(base_hooks_dir.join(HOOKS_CONFIG_FILE), valid_hooks_config)
            .expect("write valid hooks config");

        sync_runtime_entries(&base_plugin_dir, &base_plugin_dir, &generated_dir)
            .expect("sync runtime entries");

        assert_eq!(
            fs::read_to_string(generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_CONFIG_FILE))
                .expect("read generated hooks config"),
            valid_hooks_config
        );
        assert!(!test_path_exists(
            generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_SCRIPTS_DIR)
        ));
    }

    #[test]
    fn generated_plugin_runtime_entries_use_empty_hooks_config_when_source_hooks_missing() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        create_test_dir_all(&base_plugin_dir);
        create_test_dir_all(&generated_dir);

        sync_runtime_entries(&base_plugin_dir, &base_plugin_dir, &generated_dir)
            .expect("sync runtime entries");

        assert_eq!(
            fs::read_to_string(generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_CONFIG_FILE))
                .expect("read generated hooks config"),
            EMPTY_HOOKS_CONFIG
        );
        assert!(!test_path_exists(
            generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_SCRIPTS_DIR)
        ));
    }

    #[test]
    fn generated_plugin_dir_current_validation_rejects_dirty_or_stale_shapes() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        create_test_dir_all(&base_plugin_dir);
        create_test_dir_all(&generated_dir);

        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("missing generated agents dir should be inspectable"));

        seed_generated_runtime_links(&base_plugin_dir, &generated_dir);
        assert!(generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("clean generated dir should be inspectable"));

        fs::write(generated_dir.join(".cache"), "stale dev cache").expect("write unmanaged entry");
        assert!(
            !generated_plugin_dir_has_only_managed_entries(&generated_dir)
                .expect("unmanaged entry should be inspectable")
        );
        prune_unmanaged_generated_plugin_entries(&generated_dir)
            .expect("prune unmanaged generated entry");
        assert!(
            generated_plugin_dir_has_only_managed_entries(&generated_dir)
                .expect("managed entries should be inspectable after prune")
        );

        fs::write(
            generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_CONFIG_FILE),
            "{}",
        )
        .expect("write stale generated hooks config");
        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("invalid generated hooks config should be inspectable"));
        fs::write(
            generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_CONFIG_FILE),
            EMPTY_HOOKS_CONFIG,
        )
        .expect("repair generated hooks config");

        fs::remove_file(generated_dir.join(INTERNAL_MCP_SERVER_DIR))
            .expect("remove managed runtime symlink");
        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("missing runtime symlink should be inspectable"));
        ensure_symlink(
            &dir.path()
                .join("stale/plugins/app")
                .join(INTERNAL_MCP_SERVER_DIR),
            &generated_dir.join(INTERNAL_MCP_SERVER_DIR),
        )
        .expect("seed stale runtime symlink");
        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("stale runtime symlink should be inspectable"));
    }

    #[test]
    fn generated_plugin_dir_current_validation_rejects_stale_hooks_scripts_entry() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        let base_hooks_dir = base_plugin_dir.join(HOOKS_ENTRY_NAME);
        fs::create_dir_all(base_hooks_dir.join(HOOKS_SCRIPTS_DIR))
            .expect("create base hooks scripts dir");
        fs::create_dir_all(&generated_dir).expect("create generated plugin dir");

        seed_generated_runtime_links(&base_plugin_dir, &generated_dir);
        assert!(generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("generated plugin should start current"));

        fs::remove_file(generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_SCRIPTS_DIR))
            .expect("remove generated scripts symlink");
        ensure_symlink(
            &dir.path().join("stale/hooks/scripts"),
            &generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_SCRIPTS_DIR),
        )
        .expect("seed stale hooks scripts symlink");

        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("stale hooks scripts symlink should be inspectable"));
    }

    #[test]
    fn generated_plugin_dir_current_validation_rejects_missing_or_file_hooks_entry() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        create_test_dir_all(&base_plugin_dir);
        create_test_dir_all(&generated_dir);

        seed_generated_runtime_links(&base_plugin_dir, &generated_dir);
        assert!(generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("generated plugin should start current"));

        remove_test_dir_all(generated_dir.join(HOOKS_ENTRY_NAME));
        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("missing hooks dir should be inspectable"));

        write_test_file(generated_dir.join(HOOKS_ENTRY_NAME), "not a directory");
        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("file hooks path should be inspectable"));
    }

    #[test]
    fn generated_plugin_dir_current_validation_rejects_missing_hooks_config() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        create_test_dir_all(&base_plugin_dir);
        create_test_dir_all(&generated_dir);

        seed_generated_runtime_links(&base_plugin_dir, &generated_dir);
        remove_test_file(generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_CONFIG_FILE));

        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("missing hooks config should be inspectable"));
    }

    #[test]
    fn generated_plugin_dir_current_validation_rejects_unexpected_hooks_scripts_entry() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        create_test_dir_all(&base_plugin_dir);
        create_test_dir_all(&generated_dir);

        seed_generated_runtime_links(&base_plugin_dir, &generated_dir);
        write_test_file(
            generated_dir.join(HOOKS_ENTRY_NAME).join(HOOKS_SCRIPTS_DIR),
            "unexpected scripts file",
        );

        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("unexpected hooks scripts entry should be inspectable"));
    }

    #[test]
    fn generated_plugin_hooks_helpers_report_unreadable_shapes() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        create_test_dir_all(&base_plugin_dir);

        let unreadable_config_path = dir.path().join("hooks-config-dir");
        create_test_dir_all(&unreadable_config_path);
        let error = read_valid_hooks_config(&unreadable_config_path)
            .expect_err("directory hooks config path should fail");
        assert!(error.contains("Failed to read Claude hooks config"));

        let generated_file = dir.path().join("generated-file");
        write_test_file(&generated_file, "not a directory");
        let error = generated_hooks_entry_is_current(&base_plugin_dir, &generated_file)
            .expect_err("file generated root should fail hooks metadata");
        assert!(error.contains("Failed to inspect generated Claude hooks dir"));

        let generated_bad_config = dir.path().join("generated-bad-config");
        create_test_dir_all(generated_bad_config.join(HOOKS_ENTRY_NAME));
        create_test_dir_all(
            generated_bad_config
                .join(HOOKS_ENTRY_NAME)
                .join(HOOKS_CONFIG_FILE),
        );
        let error = generated_hooks_entry_is_current(&base_plugin_dir, &generated_bad_config)
            .expect_err("directory hooks config should fail read");
        assert!(error.contains("Failed to read generated Claude hooks config"));

        let generated_bad_scripts = dir.path().join("generated-bad-scripts");
        create_test_dir_all(
            base_plugin_dir
                .join(HOOKS_ENTRY_NAME)
                .join(HOOKS_SCRIPTS_DIR),
        );
        create_test_dir_all(generated_bad_scripts.join(HOOKS_ENTRY_NAME));
        write_test_file(
            generated_bad_scripts
                .join(HOOKS_ENTRY_NAME)
                .join(HOOKS_CONFIG_FILE),
            EMPTY_HOOKS_CONFIG,
        );
        create_test_dir_all(
            generated_bad_scripts
                .join(HOOKS_ENTRY_NAME)
                .join(HOOKS_SCRIPTS_DIR),
        );
        let error = generated_hooks_entry_is_current(&base_plugin_dir, &generated_bad_scripts)
            .expect_err("directory scripts path should fail read_link");
        assert!(error.contains("Failed to inspect generated Claude hooks scripts symlink"));
    }

    #[test]
    fn generated_agent_markdown_includes_internal_sidecar_stdio_tools() {
        let definition = CanonicalAgentDefinition {
            name: "ralphx-general-explorer".to_string(),
            role: "explorer".to_string(),
            description: None,
            capabilities: Default::default(),
            harnesses: Default::default(),
            delegation: Default::default(),
            profiles: Default::default(),
        };
        let claude_metadata = CanonicalClaudeAgentMetadata {
            mcp_transport: Some("external".to_string()),
            internal_mcp_tools: vec!["list_agent_tasks".to_string()],
            ..Default::default()
        };

        let markdown = render_generated_agent_markdown(
            "ralphx-general-explorer",
            &definition,
            &claude_metadata,
            "Explorer prompt",
        )
        .expect("render generated markdown");

        assert!(markdown.contains("ralphx_internal"));
        assert!(markdown.contains("--allowed-tools=list_agent_tasks"));
    }

    #[test]
    fn generated_plugin_dir_current_validation_reports_invalid_filesystem_shapes() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let base_plugin_dir = dir.path().join("base/plugins/app");
        let generated_dir = dir.path().join("generated/claude-plugin");
        fs::create_dir_all(&base_plugin_dir).expect("create base plugin dir");

        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("missing generated dir should be inspectable"));

        fs::create_dir_all(&generated_dir).expect("create generated plugin dir");
        ensure_symlink(
            &dir.path().join("stale/agents"),
            &generated_dir.join(GENERATED_AGENTS_DIR),
        )
        .expect("seed generated agents symlink");
        assert!(!generated_plugin_dir_is_current(
            &base_plugin_dir,
            &base_plugin_dir,
            &generated_dir
        )
        .expect("symlinked generated agents dir should be inspectable"));

        fs::remove_file(generated_dir.join(GENERATED_AGENTS_DIR))
            .expect("remove generated agents symlink");
        fs::create_dir_all(generated_dir.join(GENERATED_AGENTS_DIR))
            .expect("create generated agents dir");
        fs::create_dir_all(generated_dir.join(".claude-plugin"))
            .expect("create managed entry as directory");
        let error =
            generated_plugin_dir_is_current(&base_plugin_dir, &base_plugin_dir, &generated_dir)
                .expect_err("managed directory in symlink slot should be reported");
        assert!(error.contains("Failed to inspect generated Claude plugin symlink"));
    }

    #[test]
    fn generated_plugin_dir_helpers_report_containment_errors() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let generated_root = dir.path().canonicalize().expect("canonical temp dir");
        let file_path = generated_root.join("not-a-dir");
        fs::write(&file_path, "not a directory").expect("write file");

        let error = generated_plugin_dir_has_only_managed_entries(&file_path)
            .expect_err("file path should not be listed as generated dir");
        assert!(error.contains("Failed to list generated Claude plugin dir"));

        let missing_dir = generated_root.join("missing-generated-dir");
        let error = prune_unmanaged_generated_plugin_entries(&missing_dir)
            .expect_err("missing generated dir should not be pruned");
        assert!(error.contains("Failed to canonicalize generated Claude plugin dir"));

        let error = prune_unmanaged_generated_plugin_entries(&file_path)
            .expect_err("file path should not be pruned as generated dir");
        assert!(error.contains("Failed to list generated Claude plugin dir"));

        let error = trusted_existing_generated_plugin_top_level_path(
            &generated_root,
            &generated_root.join("missing-parent").join(".cache"),
        )
        .expect_err("entry with missing parent should be rejected");
        assert!(error.contains("Failed to canonicalize generated Claude plugin entry parent"));

        let error =
            trusted_existing_generated_plugin_top_level_path(&generated_root, Path::new(""))
                .expect_err("empty generated entry path should be rejected");
        assert!(error.contains("Generated Claude plugin entry has no parent"));
    }

    #[test]
    fn trusted_existing_generated_plugin_top_level_path_rejects_nested_entries() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let generated_root = dir.path().canonicalize().expect("canonical temp dir");
        let top_level = generated_root.join(".cache");
        let nested_parent = generated_root.join("agents");
        fs::create_dir_all(&nested_parent).expect("create nested parent");

        assert_eq!(
            trusted_existing_generated_plugin_top_level_path(&generated_root, &top_level)
                .expect("top-level generated entry should be accepted"),
            top_level
        );
        assert!(trusted_existing_generated_plugin_top_level_path(
            &generated_root,
            &nested_parent.join("nested.md"),
        )
        .expect_err("nested generated entry should be rejected")
        .contains("outside"));
    }

    #[test]
    fn trusted_generated_plugin_child_path_rejects_parent_traversal() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let err = trusted_generated_plugin_child_path(
            dir.path(),
            Path::new("agents").join("..").join("escape.md").as_path(),
        )
        .expect_err("parent traversal must be rejected");

        assert!(
            err.contains("Refusing generated Claude plugin output outside trusted relative path"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn trusted_generated_plugin_child_path_allows_normal_relative_output() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let target =
            trusted_generated_plugin_child_path(dir.path(), Path::new("agents/ralphx-test.md"))
                .expect("trusted relative path should be accepted");

        assert!(target.starts_with(dir.path().canonicalize().unwrap()));
        assert_eq!(
            target.file_name().and_then(|name| name.to_str()),
            Some("ralphx-test.md")
        );
    }
}
