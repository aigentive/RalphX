use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;
use tauri::State;

use super::root::resolve_composer_root;
use super::types::{
    AgentComposerSkillResponse, ListAgentComposerSkillsInput, ListAgentComposerSkillsResponse,
};
use crate::application::agent_conversation_workspace::agent_name_for_workspace_mode;
use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{AgentConversationWorkspaceMode, ProjectId};
use crate::infrastructure::agents::harness_agent_catalog::load_canonical_agent_definition;
use crate::infrastructure::agents::internal_skills::list_internal_skill_summaries_for_agent;
use crate::utils::path_safety::validate_absolute_non_root_path;

const SKILL_FILE_NAME: &str = "SKILL.md";
const CLAUDE_DIR_NAME: &str = ".claude";
const CLAUDE_SKILLS_DIR_NAME: &str = "skills";
const CLAUDE_COMMANDS_DIR_NAME: &str = "commands";
const CLAUDE_SETTINGS_FILE_NAME: &str = "settings.json";
const CLAUDE_PLUGINS_DIR_NAME: &str = "plugins";
const CLAUDE_INSTALLED_PLUGINS_FILE_NAME: &str = "installed_plugins.json";
const CODEX_AGENTS_DIR_NAME: &str = ".agents";
const CODEX_SKILLS_DIR_NAME: &str = "skills";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const CODEX_CONFIG_FILE_NAME: &str = "config.toml";
const CODEX_PLUGINS_CACHE_DIR: &str = "plugins/cache";
const CODEX_PLUGIN_MANIFEST_PATH: &str = ".codex-plugin/plugin.json";

#[tauri::command]
pub async fn list_agent_composer_skills(
    input: ListAgentComposerSkillsInput,
    state: State<'_, AppState>,
) -> Result<ListAgentComposerSkillsResponse, String> {
    let project_id = ProjectId::from_string(input.project_id);
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id))?;
    let root = resolve_composer_root(&project, input.conversation_id.as_deref(), &state).await?;
    let mode = input
        .mode
        .as_deref()
        .unwrap_or("edit")
        .parse::<AgentConversationWorkspaceMode>()
        .unwrap_or(AgentConversationWorkspaceMode::Edit);
    let agent_name = agent_name_for_workspace_mode(mode);
    let harness = input
        .provider_harness
        .as_deref()
        .and_then(|value| AgentHarnessKind::from_str(value).ok());

    tokio::task::spawn_blocking(move || {
        let mut skills = list_internal_composer_skills(&root, agent_name)?;
        match harness {
            Some(AgentHarnessKind::Claude) => {
                skills.extend(list_claude_native_skills(&root, agent_name)?);
            }
            Some(AgentHarnessKind::Codex) => {
                skills.extend(list_codex_native_skills(&root)?);
            }
            _ => {}
        }
        skills.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.source.cmp(&right.source))
                .then_with(|| left.id.cmp(&right.id))
        });
        skills.dedup_by(|left, right| left.id == right.id);
        Ok(ListAgentComposerSkillsResponse { skills })
    })
    .await
    .map_err(|error| format!("Agent composer skill catalog failed: {error}"))?
}

fn list_internal_composer_skills(
    root: &Path,
    agent_name: &str,
) -> Result<Vec<AgentComposerSkillResponse>, String> {
    list_internal_skill_summaries_for_agent(root, agent_name).map(|skills| {
        skills
            .into_iter()
            .filter(|skill| skill.user_invocable)
            .map(|skill| AgentComposerSkillResponse {
                id: format!("internal:{}", skill.name),
                name: skill.name.clone(),
                display_name: None,
                description: skill.description,
                source: "ralphx-internal".to_string(),
                provider_harness: None,
                scope: Some("RalphX".to_string()),
                invocation_kind: "internal-directive".to_string(),
                invocation_value: skill.name,
                enabled: true,
                source_path: Some(skill.source_path),
            })
            .collect()
    })
}

fn list_claude_native_skills(
    root: &Path,
    agent_name: &str,
) -> Result<Vec<AgentComposerSkillResponse>, String> {
    let mut skills = BTreeMap::<String, AgentComposerSkillResponse>::new();
    if let Some(definition) = load_canonical_agent_definition(root, agent_name) {
        for skill_name in definition.harnesses.claude.skills {
            let key = format!("claude:canonical:{skill_name}");
            skills.insert(
                key.clone(),
                AgentComposerSkillResponse {
                    id: key,
                    name: skill_name.clone(),
                    display_name: None,
                    description: Some(
                        "Claude Code skill declared by the agent profile.".to_string(),
                    ),
                    source: "harness-native".to_string(),
                    provider_harness: Some("claude".to_string()),
                    scope: Some("agent".to_string()),
                    invocation_kind: "harness-native-token".to_string(),
                    invocation_value: format!("/{skill_name}"),
                    enabled: true,
                    source_path: None,
                },
            );
        }
    }

    for candidate in claude_skill_roots(root) {
        for skill in read_claude_skill_dir(
            &candidate.path,
            &candidate.scope,
            candidate.name_prefix.as_deref(),
        )? {
            skills.entry(skill.id.clone()).or_insert(skill);
        }
    }
    Ok(skills.into_values().collect())
}

#[derive(Debug, Clone)]
struct ClaudeSkillRoot {
    path: PathBuf,
    scope: String,
    name_prefix: Option<String>,
}

fn claude_skill_roots(project_root: &Path) -> Vec<ClaudeSkillRoot> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    for ancestor in project_root.ancestors() {
        push_claude_root(
            &mut roots,
            &mut seen,
            ancestor.join(CLAUDE_DIR_NAME).join(CLAUDE_SKILLS_DIR_NAME),
            "project",
            None,
        );
        push_claude_root(
            &mut roots,
            &mut seen,
            ancestor
                .join(CLAUDE_DIR_NAME)
                .join(CLAUDE_COMMANDS_DIR_NAME),
            "project-command",
            None,
        );
    }
    if let Some(home) = dirs::home_dir() {
        let claude_home = home.join(CLAUDE_DIR_NAME);
        push_claude_root(
            &mut roots,
            &mut seen,
            claude_home.join(CLAUDE_SKILLS_DIR_NAME),
            "global",
            None,
        );
        push_claude_root(
            &mut roots,
            &mut seen,
            claude_home.join(CLAUDE_COMMANDS_DIR_NAME),
            "global-command",
            None,
        );
        push_claude_plugin_roots(&mut roots, &mut seen, &claude_home);
    }
    roots
}

fn push_claude_root(
    roots: &mut Vec<ClaudeSkillRoot>,
    seen: &mut BTreeSet<PathBuf>,
    path: PathBuf,
    scope: &str,
    name_prefix: Option<String>,
) {
    let Ok(safe) = validate_absolute_non_root_path(&path, "Claude skill root") else {
        return;
    };
    let Ok(canonical) = safe.canonicalize() else {
        return;
    };
    if !canonical.is_dir() || !seen.insert(canonical.clone()) {
        return;
    }
    roots.push(ClaudeSkillRoot {
        path: canonical,
        scope: scope.to_string(),
        name_prefix,
    });
}

fn push_claude_plugin_roots(
    roots: &mut Vec<ClaudeSkillRoot>,
    seen: &mut BTreeSet<PathBuf>,
    claude_home: &Path,
) {
    let Ok(safe_home) = validate_absolute_non_root_path(claude_home, "Claude home") else {
        return;
    };
    let Ok(canonical_home) = safe_home.canonicalize() else {
        return;
    };
    let plugins_dir = canonical_home.join(CLAUDE_PLUGINS_DIR_NAME);
    let Ok(canonical_plugins_dir) = plugins_dir.canonicalize() else {
        return;
    };
    if !canonical_plugins_dir.is_dir() {
        return;
    }

    let enabled_plugins = read_enabled_claude_plugins(&canonical_home);
    if enabled_plugins.is_empty() {
        return;
    }
    let installed_plugins = read_installed_claude_plugins(&canonical_home);

    for (plugin_key, installs) in installed_plugins.plugins {
        if !enabled_plugins.contains(&plugin_key) {
            continue;
        }
        let Some(plugin_name) = plugin_key
            .split('@')
            .next()
            .filter(|name| is_safe_skill_token(name))
            .map(str::to_string)
        else {
            continue;
        };
        for install in installs {
            let Some(install_path) = install.install_path else {
                continue;
            };
            let install_path = PathBuf::from(install_path);
            let Ok(safe_install_path) =
                validate_absolute_non_root_path(&install_path, "Claude plugin install path")
            else {
                continue;
            };
            let Ok(canonical_install_path) = safe_install_path.canonicalize() else {
                continue;
            };
            if !canonical_install_path.starts_with(&canonical_plugins_dir)
                || !canonical_install_path.is_dir()
            {
                continue;
            }
            push_claude_root(
                roots,
                seen,
                canonical_install_path.join(CLAUDE_SKILLS_DIR_NAME),
                "plugin",
                Some(plugin_name.clone()),
            );
            push_claude_root(
                roots,
                seen,
                canonical_install_path.join(CLAUDE_COMMANDS_DIR_NAME),
                "plugin-command",
                Some(plugin_name.clone()),
            );
        }
    }
}

fn read_enabled_claude_plugins(claude_home: &Path) -> BTreeSet<String> {
    let settings_path = claude_home.join(CLAUDE_SETTINGS_FILE_NAME);
    // codeql[rust/path-injection]
    let Ok(raw) = std::fs::read_to_string(settings_path) else {
        return BTreeSet::new();
    };
    serde_json::from_str::<ClaudeSettings>(&raw)
        .map(|settings| {
            settings
                .enabled_plugins
                .into_iter()
                .filter_map(|(name, enabled)| enabled.then_some(name))
                .collect()
        })
        .unwrap_or_default()
}

fn read_installed_claude_plugins(claude_home: &Path) -> ClaudeInstalledPlugins {
    let installed_path = claude_home
        .join(CLAUDE_PLUGINS_DIR_NAME)
        .join(CLAUDE_INSTALLED_PLUGINS_FILE_NAME);
    // codeql[rust/path-injection]
    let Ok(raw) = std::fs::read_to_string(installed_path) else {
        return ClaudeInstalledPlugins::default();
    };
    serde_json::from_str::<ClaudeInstalledPlugins>(&raw).unwrap_or_default()
}

pub(super) fn read_claude_skill_dir(
    skills_root: &Path,
    scope: &str,
    name_prefix: Option<&str>,
) -> Result<Vec<AgentComposerSkillResponse>, String> {
    let safe_root = validate_absolute_non_root_path(skills_root, "Claude skill directory")
        .map_err(|error| error.to_string())?;
    let safe_root = safe_root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Claude skill directory: {error}"))?;
    let mut skills = Vec::new();
    // codeql[rust/path-injection]
    let read_dir = match std::fs::read_dir(&safe_root) {
        Ok(read_dir) => read_dir,
        // An absent directory legitimately means "no skills live here". Any OTHER error —
        // permissions, not-a-directory, IO — is a failure to LOOK, and reporting that as an
        // empty skill list tells the user this project has no skills when nobody knows.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(skills),
        Err(error) => {
            return Err(format!(
                "Failed to read Claude skill directory {}: {error}",
                safe_root.display()
            ));
        }
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let skill_file = path.join(SKILL_FILE_NAME);
            if let Some(skill) =
                read_claude_skill_file(&safe_root, &skill_file, scope, name_prefix)?
            {
                skills.push(skill);
            }
        } else if file_type.is_file()
            && safe_root.file_name() == Some(OsStr::new(CLAUDE_COMMANDS_DIR_NAME))
            && path.extension() == Some(OsStr::new("md"))
        {
            if let Some(skill) = read_claude_command_file(&safe_root, &path, scope, name_prefix)? {
                skills.push(skill);
            }
        }
    }
    Ok(skills)
}

fn read_claude_skill_file(
    skills_root: &Path,
    skill_file: &Path,
    scope: &str,
    name_prefix: Option<&str>,
) -> Result<Option<AgentComposerSkillResponse>, String> {
    let Ok(canonical) = skill_file.canonicalize() else {
        return Ok(None);
    };
    if !canonical.starts_with(skills_root)
        || canonical.file_name() != Some(OsStr::new(SKILL_FILE_NAME))
    {
        return Ok(None);
    }
    let Some(skill_dir) = canonical.parent() else {
        return Ok(None);
    };
    let Some(fallback_name) = skill_dir.file_name().and_then(OsStr::to_str) else {
        return Ok(None);
    };
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(&canonical).map_err(|error| {
        format!(
            "Failed to read Claude skill {}: {error}",
            canonical.display()
        )
    })?;
    let (frontmatter, _body) = split_frontmatter(&raw).unwrap_or(("", raw.as_str()));
    let metadata = serde_yaml::from_str::<SkillFrontmatter>(frontmatter).unwrap_or_default();
    let name = metadata
        .name
        .as_deref()
        .filter(|name| is_safe_skill_token(name))
        .unwrap_or(fallback_name);
    let name = format_skill_name(name_prefix, name);
    Ok(Some(AgentComposerSkillResponse {
        id: format!("claude:{scope}:{name}"),
        name: name.clone(),
        display_name: metadata.display_name,
        description: metadata.description.or(metadata.when_to_use),
        source: "harness-native".to_string(),
        provider_harness: Some("claude".to_string()),
        scope: Some(scope.to_string()),
        invocation_kind: "harness-native-token".to_string(),
        invocation_value: format!("/{name}"),
        enabled: metadata.user_invocable.unwrap_or(true),
        source_path: Some(canonical.display().to_string()),
    }))
}

fn read_claude_command_file(
    commands_root: &Path,
    command_file: &Path,
    scope: &str,
    name_prefix: Option<&str>,
) -> Result<Option<AgentComposerSkillResponse>, String> {
    let Ok(canonical) = command_file.canonicalize() else {
        return Ok(None);
    };
    if !canonical.starts_with(commands_root) || canonical.extension() != Some(OsStr::new("md")) {
        return Ok(None);
    }
    let Some(stem) = canonical.file_stem().and_then(OsStr::to_str) else {
        return Ok(None);
    };
    if !is_safe_skill_token(stem) {
        return Ok(None);
    }
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(&canonical).unwrap_or_default();
    let (frontmatter, body) = split_frontmatter(&raw).unwrap_or(("", raw.as_str()));
    let metadata = serde_yaml::from_str::<SkillFrontmatter>(frontmatter).unwrap_or_default();
    let first_line = body.lines().map(str::trim).find(|line| !line.is_empty());
    let name = format_skill_name(name_prefix, stem);
    Ok(Some(AgentComposerSkillResponse {
        id: format!("claude:{scope}:{name}"),
        name: name.clone(),
        display_name: metadata.display_name,
        description: metadata
            .description
            .or_else(|| first_line.map(str::to_string)),
        source: "harness-native".to_string(),
        provider_harness: Some("claude".to_string()),
        scope: Some(scope.to_string()),
        invocation_kind: "harness-native-token".to_string(),
        invocation_value: format!("/{name}"),
        enabled: metadata.user_invocable.unwrap_or(true),
        source_path: Some(canonical.display().to_string()),
    }))
}

fn format_skill_name(name_prefix: Option<&str>, name: &str) -> String {
    match name_prefix {
        Some(prefix) => format!("{prefix}:{name}"),
        None => name.to_string(),
    }
}

fn list_codex_native_skills(root: &Path) -> Result<Vec<AgentComposerSkillResponse>, String> {
    let disabled_paths = codex_disabled_skill_paths();
    let mut skills = BTreeMap::<String, AgentComposerSkillResponse>::new();
    for candidate in codex_skill_roots(root) {
        for skill in read_codex_skill_dir(
            &candidate.path,
            &candidate.scope,
            candidate.name_prefix.as_deref(),
            &disabled_paths,
        )? {
            skills.entry(skill.id.clone()).or_insert(skill);
        }
    }
    Ok(skills.into_values().collect())
}

#[derive(Debug, Clone)]
struct CodexSkillRoot {
    path: PathBuf,
    scope: String,
    name_prefix: Option<String>,
}

fn codex_skill_roots(project_root: &Path) -> Vec<CodexSkillRoot> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    let repo_root = git_repo_root(project_root);
    for ancestor in project_root.ancestors() {
        push_codex_root(
            &mut roots,
            &mut seen,
            ancestor
                .join(CODEX_AGENTS_DIR_NAME)
                .join(CODEX_SKILLS_DIR_NAME),
            "repo",
            None,
        );
        if ancestor == repo_root {
            break;
        }
    }
    if let Some(home) = dirs::home_dir() {
        push_codex_root(
            &mut roots,
            &mut seen,
            home.join(CODEX_AGENTS_DIR_NAME).join(CODEX_SKILLS_DIR_NAME),
            "user",
            None,
        );
    }
    push_codex_root(
        &mut roots,
        &mut seen,
        PathBuf::from("/etc/codex").join(CODEX_SKILLS_DIR_NAME),
        "admin",
        None,
    );
    if let Some(codex_home) = codex_home_dir() {
        push_codex_root(
            &mut roots,
            &mut seen,
            codex_home.join(CODEX_SKILLS_DIR_NAME),
            "codex-home",
            None,
        );
        push_codex_root(
            &mut roots,
            &mut seen,
            codex_home.join(CODEX_SKILLS_DIR_NAME).join(".system"),
            "system",
            None,
        );
        push_codex_plugin_roots(&mut roots, &mut seen, &codex_home);
    }
    roots
}

fn push_codex_root(
    roots: &mut Vec<CodexSkillRoot>,
    seen: &mut BTreeSet<PathBuf>,
    path: PathBuf,
    scope: &str,
    name_prefix: Option<String>,
) {
    let Ok(safe) = validate_absolute_non_root_path(&path, "Codex skill root") else {
        return;
    };
    let Ok(canonical) = safe.canonicalize() else {
        return;
    };
    if !canonical.is_dir() || !seen.insert(canonical.clone()) {
        return;
    }
    roots.push(CodexSkillRoot {
        path: canonical,
        scope: scope.to_string(),
        name_prefix,
    });
}

fn push_codex_plugin_roots(
    roots: &mut Vec<CodexSkillRoot>,
    seen: &mut BTreeSet<PathBuf>,
    codex_home: &Path,
) {
    let cache_dir = codex_home.join(CODEX_PLUGINS_CACHE_DIR);
    let Ok(safe_cache_dir) = validate_absolute_non_root_path(&cache_dir, "Codex plugin cache")
    else {
        return;
    };
    let Ok(canonical_cache_dir) = safe_cache_dir.canonicalize() else {
        return;
    };
    if !canonical_cache_dir.is_dir() {
        return;
    }

    // codeql[rust/path-injection]
    let Ok(marketplaces) = std::fs::read_dir(&canonical_cache_dir) else {
        return;
    };
    for marketplace in marketplaces.flatten() {
        let Ok(file_type) = marketplace.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        // codeql[rust/path-injection]
        let Ok(plugin_names) = std::fs::read_dir(marketplace.path()) else {
            continue;
        };
        for plugin_name in plugin_names.flatten() {
            let Ok(file_type) = plugin_name.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            // codeql[rust/path-injection]
            let Ok(plugin_versions) = std::fs::read_dir(plugin_name.path()) else {
                continue;
            };
            for plugin_version in plugin_versions.flatten() {
                let Ok(file_type) = plugin_version.file_type() else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }
                let plugin_root = plugin_version.path();
                let Some((plugin_display_name, skills_dir)) =
                    read_codex_plugin_skill_root(&plugin_root)
                else {
                    continue;
                };
                push_codex_root(roots, seen, skills_dir, "plugin", Some(plugin_display_name));
            }
        }
    }
}

fn read_codex_plugin_skill_root(plugin_root: &Path) -> Option<(String, PathBuf)> {
    let safe_plugin_root = validate_absolute_non_root_path(plugin_root, "Codex plugin root")
        .ok()?
        .canonicalize()
        .ok()?;
    let manifest_path = safe_plugin_root.join(CODEX_PLUGIN_MANIFEST_PATH);
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest = serde_json::from_str::<CodexPluginManifest>(&raw).ok()?;
    let plugin_name = manifest.name.filter(|name| is_safe_skill_token(name))?;
    let skills_path = manifest.skills.unwrap_or_else(|| "./skills/".to_string());
    let relative_skills_path = Path::new(skills_path.trim());
    if relative_skills_path.is_absolute() {
        return None;
    }
    let skills_dir = safe_plugin_root.join(relative_skills_path);
    let canonical_skills_dir = skills_dir.canonicalize().ok()?;
    if !canonical_skills_dir.starts_with(&safe_plugin_root) || !canonical_skills_dir.is_dir() {
        return None;
    }
    Some((plugin_name, canonical_skills_dir))
}

pub(super) fn read_codex_skill_dir(
    skills_root: &Path,
    scope: &str,
    name_prefix: Option<&str>,
    disabled_paths: &BTreeSet<PathBuf>,
) -> Result<Vec<AgentComposerSkillResponse>, String> {
    let safe_root = validate_absolute_non_root_path(skills_root, "Codex skill directory")
        .map_err(|error| error.to_string())?;
    let safe_root = safe_root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Codex skill directory: {error}"))?;
    let mut skills = Vec::new();
    // codeql[rust/path-injection]
    let read_dir = match std::fs::read_dir(&safe_root) {
        Ok(read_dir) => read_dir,
        // An absent directory legitimately means "no skills live here". Any OTHER error —
        // permissions, not-a-directory, IO — is a failure to LOOK, and reporting that as an
        // empty skill list tells the user this project has no skills when nobody knows.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(skills),
        Err(error) => {
            return Err(format!(
                "Failed to read Codex skill directory {}: {error}",
                safe_root.display()
            ));
        }
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let skill_file = path.join(SKILL_FILE_NAME);
        if let Some(skill) =
            read_codex_skill_file(&safe_root, &skill_file, scope, name_prefix, disabled_paths)?
        {
            skills.push(skill);
        }
    }
    Ok(skills)
}

fn read_codex_skill_file(
    skills_root: &Path,
    skill_file: &Path,
    scope: &str,
    name_prefix: Option<&str>,
    disabled_paths: &BTreeSet<PathBuf>,
) -> Result<Option<AgentComposerSkillResponse>, String> {
    let Ok(canonical) = skill_file.canonicalize() else {
        return Ok(None);
    };
    if !canonical.starts_with(skills_root)
        || canonical.file_name() != Some(OsStr::new(SKILL_FILE_NAME))
    {
        return Ok(None);
    }
    let Some(skill_dir) = canonical.parent() else {
        return Ok(None);
    };
    let Some(fallback_name) = skill_dir.file_name().and_then(OsStr::to_str) else {
        return Ok(None);
    };
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(&canonical).map_err(|error| {
        format!(
            "Failed to read Codex skill {}: {error}",
            canonical.display()
        )
    })?;
    let (frontmatter, body) = split_frontmatter(&raw).unwrap_or(("", raw.as_str()));
    let metadata = serde_yaml::from_str::<SkillFrontmatter>(frontmatter).unwrap_or_default();
    let base_name = metadata
        .name
        .as_deref()
        .filter(|name| is_safe_skill_token(name))
        .unwrap_or(fallback_name);
    let name = match name_prefix {
        Some(prefix) => format!("{prefix}:{base_name}"),
        None => base_name.to_string(),
    };
    let first_line = body.lines().map(str::trim).find(|line| !line.is_empty());
    Ok(Some(AgentComposerSkillResponse {
        id: format!("codex:{scope}:{name}"),
        name: name.clone(),
        display_name: metadata.display_name,
        description: metadata
            .description
            .or_else(|| first_line.map(str::to_string)),
        source: "harness-native".to_string(),
        provider_harness: Some("codex".to_string()),
        scope: Some(scope.to_string()),
        invocation_kind: "harness-native-token".to_string(),
        invocation_value: format!("${name}"),
        enabled: !disabled_paths.contains(&canonical),
        source_path: Some(canonical.display().to_string()),
    }))
}

fn codex_disabled_skill_paths() -> BTreeSet<PathBuf> {
    let Some(codex_home) = codex_home_dir() else {
        return BTreeSet::new();
    };
    let config_path = codex_home.join(CODEX_CONFIG_FILE_NAME);
    let Ok(safe_config_path) = validate_absolute_non_root_path(&config_path, "Codex config") else {
        return BTreeSet::new();
    };
    // codeql[rust/path-injection]
    let Ok(raw) = std::fs::read_to_string(&safe_config_path) else {
        return BTreeSet::new();
    };
    parse_disabled_codex_skill_paths(&raw)
}

fn parse_disabled_codex_skill_paths(raw: &str) -> BTreeSet<PathBuf> {
    let mut disabled_paths = BTreeSet::new();
    let mut in_skill_config = false;
    let mut current_path: Option<PathBuf> = None;
    let mut current_enabled: Option<bool> = None;

    for line in raw.lines() {
        let trimmed = line.split('#').next().unwrap_or("").trim();
        if trimmed == "[[skills.config]]" {
            flush_codex_skill_config(&mut disabled_paths, &mut current_path, &mut current_enabled);
            in_skill_config = true;
            continue;
        }
        if trimmed.starts_with('[') {
            flush_codex_skill_config(&mut disabled_paths, &mut current_path, &mut current_enabled);
            in_skill_config = false;
            continue;
        }
        if !in_skill_config {
            continue;
        }
        if let Some(value) = parse_toml_key_value(trimmed, "path").and_then(parse_toml_string) {
            current_path = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = parse_toml_key_value(trimmed, "enabled") {
            current_enabled = match value.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    flush_codex_skill_config(&mut disabled_paths, &mut current_path, &mut current_enabled);
    disabled_paths
}

fn flush_codex_skill_config(
    disabled_paths: &mut BTreeSet<PathBuf>,
    current_path: &mut Option<PathBuf>,
    current_enabled: &mut Option<bool>,
) {
    if *current_enabled == Some(false) {
        if let Some(path) = current_path.take() {
            if let Ok(safe) = validate_absolute_non_root_path(&path, "disabled Codex skill path") {
                if let Ok(canonical) = safe.canonicalize() {
                    disabled_paths.insert(canonical);
                }
            }
        }
    } else {
        current_path.take();
    }
    *current_enabled = None;
}

fn parse_toml_key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?.trim_start();
    rest.strip_prefix('=').map(str::trim)
}

fn parse_toml_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 2 {
        return None;
    }
    let quote = value.as_bytes()[0];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let end = value[1..].find(quote as char)? + 1;
    Some(value[1..end].to_string())
}

fn codex_home_dir() -> Option<PathBuf> {
    env::var_os(CODEX_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

fn git_repo_root(start: &Path) -> PathBuf {
    for ancestor in start.ancestors() {
        if ancestor.join(".git").exists() {
            return ancestor.to_path_buf();
        }
    }
    start.to_path_buf()
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "display-name")]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "when_to_use")]
    when_to_use: Option<String>,
    #[serde(default, rename = "user-invocable")]
    user_invocable: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeSettings {
    #[serde(default, rename = "enabledPlugins")]
    enabled_plugins: BTreeMap<String, bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeInstalledPlugins {
    #[serde(default)]
    plugins: BTreeMap<String, Vec<ClaudeInstalledPlugin>>,
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeInstalledPlugin {
    #[serde(default, rename = "installPath")]
    install_path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CodexPluginManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    skills: Option<String>,
}

fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let frontmatter = &rest[..end];
    let closing = &rest[end + 1..];
    let body = closing
        .strip_prefix("---\r\n")
        .or_else(|| closing.strip_prefix("---\n"))
        .or_else(|| closing.strip_prefix("---"))?;
    Some((frontmatter, body))
}

fn is_safe_skill_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn claude_native_skill_discovery_reads_project_skills_and_commands() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join(".claude/skills/ship-it")).expect("skill dir");
        fs::write(
            temp.path().join(".claude/skills/ship-it/SKILL.md"),
            r#"---
name: ship-it
description: Ship focused changes
---
# Ship It
"#,
        )
        .expect("skill");
        fs::create_dir_all(temp.path().join(".claude/commands")).expect("commands dir");
        fs::write(
            temp.path().join(".claude/commands/review.md"),
            "Review the current work.",
        )
        .expect("command");

        let skills = list_claude_native_skills(temp.path(), "missing-agent").expect("skills");

        assert!(skills.iter().any(|skill| {
            skill.id == "claude:project:ship-it"
                && skill.provider_harness.as_deref() == Some("claude")
        }));
        assert!(skills
            .iter()
            .any(|skill| skill.id == "claude:project-command:review"));
    }

    #[test]
    fn codex_native_skill_discovery_reads_repo_and_plugin_skills() {
        let temp = tempdir().expect("tempdir");
        let disabled_paths = BTreeSet::new();
        fs::create_dir_all(temp.path().join(".agents/skills/ship-it")).expect("skill dir");
        fs::write(
            temp.path().join(".agents/skills/ship-it/SKILL.md"),
            r#"---
name: ship-it
description: Ship focused changes
---
# Ship It
"#,
        )
        .expect("repo skill");
        let repo_skills = read_codex_skill_dir(
            &temp.path().join(".agents/skills"),
            "repo",
            None,
            &disabled_paths,
        )
        .expect("repo skills");

        assert!(repo_skills.iter().any(|skill| {
            skill.id == "codex:repo:ship-it"
                && skill.name == "ship-it"
                && skill.invocation_value == "$ship-it"
        }));

        fs::create_dir_all(temp.path().join("plugins/github/skills/yeet")).expect("plugin skill");
        fs::create_dir_all(temp.path().join("plugins/github/.codex-plugin"))
            .expect("plugin metadata");
        fs::write(
            temp.path().join("plugins/github/.codex-plugin/plugin.json"),
            r#"{"name":"github","skills":"./skills/"}"#,
        )
        .expect("plugin manifest");
        fs::write(
            temp.path().join("plugins/github/skills/yeet/SKILL.md"),
            r#"---
name: yeet
description: Publish through GitHub
---
# Yeet
"#,
        )
        .expect("plugin skill");
        let (plugin_name, plugin_skills_dir) =
            read_codex_plugin_skill_root(&temp.path().join("plugins/github")).expect("plugin root");
        let plugin_skills = read_codex_skill_dir(
            &plugin_skills_dir,
            "plugin",
            Some(&plugin_name),
            &disabled_paths,
        )
        .expect("plugin skills");

        assert!(plugin_skills.iter().any(|skill| {
            skill.id == "codex:plugin:github:yeet"
                && skill.name == "github:yeet"
                && skill.invocation_value == "$github:yeet"
        }));
    }

    #[test]
    fn codex_disabled_skill_config_marks_canonical_skill_path_disabled() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("skills/local")).expect("skill dir");
        let skill_file = temp.path().join("skills/local/SKILL.md");
        fs::write(
            &skill_file,
            r#"---
name: local
description: Local skill
---
Body
"#,
        )
        .expect("skill");
        let config = format!(
            r#"
[[skills.config]]
path = "{}"
enabled = false
"#,
            skill_file.display()
        );
        let disabled_paths = parse_disabled_codex_skill_paths(&config);
        let skills =
            read_codex_skill_dir(&temp.path().join("skills"), "user", None, &disabled_paths)
                .expect("skills");

        assert_eq!(skills.len(), 1);
        assert!(!skills[0].enabled);
    }

    fn create_agent(root: &Path, agent_yaml: &str) {
        fs::create_dir_all(root.join("agents/test-agent")).expect("agent dir");
        fs::write(root.join("agents/test-agent/agent.yaml"), agent_yaml).expect("agent yaml");
    }

    fn create_internal_skill(root: &Path, name: &str, frontmatter: &str, body: &str) {
        fs::create_dir_all(root.join(format!("plugins/app/skills/{name}"))).expect("skill dir");
        fs::write(
            root.join(format!("plugins/app/skills/{name}/SKILL.md")),
            format!("---\n{frontmatter}\n---\n{body}\n"),
        )
        .expect("skill file");
    }

    #[test]
    fn internal_composer_skills_exposes_only_user_invocable_allowed_skills() {
        let temp = tempdir().expect("tempdir");
        create_agent(
            temp.path(),
            r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - quick-fix
      - hidden-bridge
"#,
        );
        create_internal_skill(
            temp.path(),
            "quick-fix",
            "name: quick-fix\ndescription: Apply focused fixes",
            "Quick fix instructions.",
        );
        create_internal_skill(
            temp.path(),
            "hidden-bridge",
            "name: hidden-bridge\nuser-invocable: false",
            "Hidden bridge instructions.",
        );

        let skills =
            list_internal_composer_skills(temp.path(), "test-agent").expect("internal skills");

        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill.id, "internal:quick-fix");
        assert_eq!(skill.name, "quick-fix");
        assert_eq!(skill.description.as_deref(), Some("Apply focused fixes"));
        assert_eq!(skill.source, "ralphx-internal");
        assert_eq!(skill.invocation_kind, "internal-directive");
        assert_eq!(skill.invocation_value, "quick-fix");
        assert!(skill.enabled);
        assert!(skill
            .source_path
            .as_deref()
            .is_some_and(|path| path.ends_with("plugins/app/skills/quick-fix/SKILL.md")));
    }

    #[test]
    fn claude_skill_reader_uses_metadata_fallbacks_and_filters_commands() {
        let temp = tempdir().expect("tempdir");
        let skill_root = temp.path().join(".claude/skills");
        fs::create_dir_all(skill_root.join("fallback-skill")).expect("skill dir");
        fs::write(
            skill_root.join("fallback-skill/SKILL.md"),
            r#"---
name: ../unsafe
display-name: Fallback Skill
when_to_use: Use when metadata name is unsafe
user-invocable: false
---
Skill body.
"#,
        )
        .expect("skill file");

        let skills = read_claude_skill_dir(&skill_root, "project", None).expect("skills");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "claude:project:fallback-skill");
        assert_eq!(skills[0].name, "fallback-skill");
        assert_eq!(skills[0].display_name.as_deref(), Some("Fallback Skill"));
        assert_eq!(
            skills[0].description.as_deref(),
            Some("Use when metadata name is unsafe")
        );
        assert!(!skills[0].enabled);

        let commands_root = temp.path().join(".claude/commands");
        fs::create_dir_all(&commands_root).expect("commands dir");
        fs::write(
            commands_root.join("review.md"),
            "\nReview the current branch.\n",
        )
        .expect("command file");
        fs::write(commands_root.join("bad.name.md"), "Should be ignored.")
            .expect("unsafe command file");
        fs::write(commands_root.join("notes.txt"), "Should also be ignored.")
            .expect("non-command file");

        let commands =
            read_claude_skill_dir(&commands_root, "project-command", None).expect("commands");

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].id, "claude:project-command:review");
        assert_eq!(
            commands[0].description.as_deref(),
            Some("Review the current branch.")
        );
        assert_eq!(commands[0].invocation_value, "/review");
    }

    #[test]
    fn claude_plugin_roots_use_enabled_installed_plugins_and_namespace_invocations() {
        let temp = tempdir().expect("tempdir");
        let claude_home = temp.path().join(".claude");
        let plugins_dir = claude_home.join("plugins");
        let active_root = plugins_dir.join("cache/claude-plugins-official/figma/2.2.50");
        let stale_root = plugins_dir.join("cache/claude-plugins-official/figma/2.1.30");
        let disabled_root = plugins_dir.join("cache/claude-plugins-official/microsoft-docs/0.3.1");

        fs::create_dir_all(active_root.join("skills/figma-use")).expect("active skill dir");
        fs::write(
            active_root.join("skills/figma-use/SKILL.md"),
            r#"---
name: figma-use
description: Use Figma
---
Body
"#,
        )
        .expect("active skill");
        fs::create_dir_all(active_root.join("commands")).expect("active commands dir");
        fs::write(
            active_root.join("commands/sync.md"),
            "Synchronize Figma assets.",
        )
        .expect("active command");
        fs::create_dir_all(stale_root.join("skills/old-only")).expect("stale skill dir");
        fs::write(
            stale_root.join("skills/old-only/SKILL.md"),
            "---\nname: old-only\n---\nBody\n",
        )
        .expect("stale skill");
        fs::create_dir_all(disabled_root.join("skills/microsoft-docs"))
            .expect("disabled skill dir");
        fs::write(
            disabled_root.join("skills/microsoft-docs/SKILL.md"),
            "---\nname: microsoft-docs\n---\nBody\n",
        )
        .expect("disabled skill");
        fs::write(
            claude_home.join(CLAUDE_SETTINGS_FILE_NAME),
            serde_json::json!({
                "enabledPlugins": {
                    "figma@claude-plugins-official": true,
                    "microsoft-docs@claude-plugins-official": false
                }
            })
            .to_string(),
        )
        .expect("settings");
        fs::write(
            plugins_dir.join(CLAUDE_INSTALLED_PLUGINS_FILE_NAME),
            serde_json::json!({
                "version": 2,
                "plugins": {
                    "figma@claude-plugins-official": [
                        { "scope": "user", "installPath": active_root }
                    ],
                    "microsoft-docs@claude-plugins-official": [
                        { "scope": "user", "installPath": disabled_root }
                    ]
                }
            })
            .to_string(),
        )
        .expect("installed plugins");

        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();
        push_claude_plugin_roots(&mut roots, &mut seen, &claude_home);
        let mut skills = Vec::new();
        for root in roots {
            skills.extend(
                read_claude_skill_dir(&root.path, &root.scope, root.name_prefix.as_deref())
                    .expect("plugin skills"),
            );
        }

        assert!(skills.iter().any(|skill| {
            skill.id == "claude:plugin:figma:figma-use"
                && skill.name == "figma:figma-use"
                && skill.invocation_value == "/figma:figma-use"
                && skill
                    .source_path
                    .as_deref()
                    .is_some_and(|path| path.ends_with("figma/2.2.50/skills/figma-use/SKILL.md"))
        }));
        assert!(skills.iter().any(|skill| {
            skill.id == "claude:plugin-command:figma:sync"
                && skill.name == "figma:sync"
                && skill.invocation_value == "/figma:sync"
        }));
        assert!(!skills.iter().any(|skill| skill.name == "figma:old-only"));
        assert!(!skills
            .iter()
            .any(|skill| skill.name == "microsoft-docs:microsoft-docs"));
    }

    #[test]
    fn claude_canonical_agent_skill_entries_are_included() {
        let temp = tempdir().expect("tempdir");
        create_agent(
            temp.path(),
            r#"name: test-agent
role: test
harnesses:
  claude:
    skills:
      - ship-it
"#,
        );

        let skills = list_claude_native_skills(temp.path(), "test-agent").expect("skills");

        assert!(skills.iter().any(|skill| {
            skill.id == "claude:canonical:ship-it"
                && skill.invocation_value == "/ship-it"
                && skill.scope.as_deref() == Some("agent")
        }));
    }

    #[test]
    fn codex_skill_reader_uses_body_description_prefix_and_disabled_set() {
        let temp = tempdir().expect("tempdir");
        let skills_root = temp.path().join("skills");
        fs::create_dir_all(skills_root.join("local")).expect("skill dir");
        let skill_file = skills_root.join("local/SKILL.md");
        fs::write(
            &skill_file,
            r#"---
name: bad/name
display-name: Local Skill
---
First body line is the description.

More body.
"#,
        )
        .expect("skill file");
        let disabled_paths = BTreeSet::from([skill_file.canonicalize().expect("canonical")]);

        let skills = read_codex_skill_dir(&skills_root, "plugin", Some("github"), &disabled_paths)
            .expect("skills");

        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill.id, "codex:plugin:github:local");
        assert_eq!(skill.name, "github:local");
        assert_eq!(skill.display_name.as_deref(), Some("Local Skill"));
        assert_eq!(
            skill.description.as_deref(),
            Some("First body line is the description.")
        );
        assert_eq!(skill.invocation_value, "$github:local");
        assert!(!skill.enabled);
    }

    #[test]
    fn codex_config_parser_handles_quotes_comments_and_table_boundaries() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("skills/disabled")).expect("disabled skill dir");
        fs::create_dir_all(temp.path().join("skills/enabled")).expect("enabled skill dir");
        let disabled = temp.path().join("skills/disabled/SKILL.md");
        let enabled = temp.path().join("skills/enabled/SKILL.md");
        fs::write(&disabled, "---\nname: disabled\n---\nBody").expect("disabled skill");
        fs::write(&enabled, "---\nname: enabled\n---\nBody").expect("enabled skill");
        let config = format!(
            r#"
[[skills.config]]
path = '{}' # single quotes are accepted
enabled = false

[[skills.config]]
path = "{}"
enabled = true

[other]
path = "/tmp/ignored"
enabled = false
"#,
            disabled.display(),
            enabled.display()
        );

        let disabled_paths = parse_disabled_codex_skill_paths(&config);

        assert!(disabled_paths.contains(&disabled.canonicalize().expect("canonical disabled")));
        assert!(!disabled_paths.contains(&enabled.canonicalize().expect("canonical enabled")));
    }

    #[test]
    fn codex_plugin_roots_reject_unsafe_manifests_and_accept_cache_plugins() {
        let temp = tempdir().expect("tempdir");
        let unsafe_plugin = temp.path().join("unsafe-plugin");
        fs::create_dir_all(unsafe_plugin.join(".codex-plugin")).expect("unsafe metadata");
        fs::create_dir_all(temp.path().join("outside-skills")).expect("outside skills");
        fs::write(
            unsafe_plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"unsafe","skills":"../outside-skills"}"#,
        )
        .expect("unsafe manifest");

        assert!(read_codex_plugin_skill_root(&unsafe_plugin).is_none());

        let plugin_root = temp
            .path()
            .join("plugins/cache/openai-curated/github/version-1");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("plugin metadata");
        fs::create_dir_all(plugin_root.join("skills/yeet")).expect("plugin skill");
        fs::write(
            plugin_root.join(".codex-plugin/plugin.json"),
            r#"{"name":"github","skills":"./skills"}"#,
        )
        .expect("plugin manifest");
        fs::write(
            plugin_root.join("skills/yeet/SKILL.md"),
            "---\nname: yeet\n---\nBody",
        )
        .expect("plugin skill file");

        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();
        push_codex_plugin_roots(&mut roots, &mut seen, temp.path());

        assert!(roots.iter().any(|root| {
            root.scope == "plugin"
                && root.name_prefix.as_deref() == Some("github")
                && root
                    .path
                    .ends_with("plugins/cache/openai-curated/github/version-1/skills")
        }));
    }

    #[test]
    fn skill_helpers_ignore_invalid_inputs() {
        assert!(split_frontmatter("No frontmatter").is_none());
        assert!(is_safe_skill_token("safe-name_1"));
        assert!(!is_safe_skill_token("bad/name"));
        assert_eq!(
            parse_toml_key_value("enabled = false", "enabled"),
            Some("false")
        );
        assert_eq!(
            parse_toml_string("\"hello\" # ignored"),
            Some("hello".to_string())
        );
        assert_eq!(parse_toml_string("unquoted"), None);
    }
}
