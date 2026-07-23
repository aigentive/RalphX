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
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ProjectId, ProjectSkill, ProjectSkillLifecycleStatus,
};
use crate::domain::repositories::ProjectSkillListOptions;
use crate::infrastructure::agents::harness_agent_catalog::load_canonical_agent_definition;
use crate::infrastructure::agents::internal_skills::list_internal_skill_summaries_for_agent;
use crate::utils::path_safety::validate_absolute_non_root_path;

const SKILL_FILE_NAME: &str = "SKILL.md";
const CLAUDE_DIR_NAME: &str = ".claude";
const CLAUDE_SKILLS_DIR_NAME: &str = "skills";
const CLAUDE_COMMANDS_DIR_NAME: &str = "commands";
pub(super) const CLAUDE_SETTINGS_FILE_NAME: &str = "settings.json";
const CLAUDE_PLUGINS_DIR_NAME: &str = "plugins";
pub(super) const CLAUDE_INSTALLED_PLUGINS_FILE_NAME: &str = "installed_plugins.json";
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
    let learned_skills = state
        .project_skill_repo
        .list_by_project(
            &project_id,
            ProjectSkillListOptions {
                status: Some(ProjectSkillLifecycleStatus::Approved),
                include_archived: false,
                ..ProjectSkillListOptions::default()
            },
        )
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        // Path-scoped learned skills auto-activate (Claude `paths` frontmatter /
        // RalphX path-based injection), so they are intentionally excluded from
        // the manual composer picker; only unscoped skills are manually selectable.
        .filter(|skill| skill.scope_paths.is_empty())
        .map(project_skill_to_composer_skill)
        .collect::<Vec<_>>();

    tokio::task::spawn_blocking(move || {
        let mut skills = list_internal_composer_skills(&root, agent_name)?;
        skills.extend(learned_skills);
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

pub(super) fn project_skill_to_composer_skill(skill: ProjectSkill) -> AgentComposerSkillResponse {
    let id = skill.id.as_str().to_string();
    let name = composer_token_from_project_skill(&skill);
    // Prefer the open-standard `compact_guidance` (third-person what+when) as the
    // surfaced description so the composer matches the exported SKILL.md, falling
    // back to predicted_effect only when guidance is empty.
    let description = {
        let compact = skill.compact_guidance.trim();
        if compact.is_empty() {
            skill
                .predicted_effect
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        } else {
            Some(compact.to_string())
        }
    };
    AgentComposerSkillResponse {
        id: format!("learned:{id}"),
        name,
        display_name: Some(skill.title),
        description,
        source: "learned".to_string(),
        provider_harness: None,
        scope: Some("project".to_string()),
        invocation_kind: "project-skill-directive".to_string(),
        invocation_value: id,
        enabled: true,
        source_path: None,
    }
}

pub(super) fn composer_token_from_project_skill(skill: &ProjectSkill) -> String {
    let mut token = String::new();
    for character in skill.title.chars() {
        if character.is_ascii_alphanumeric() {
            token.push(character.to_ascii_lowercase());
        } else if matches!(character, '-' | '_' | ' ') && !token.ends_with('-') {
            token.push('-');
        }
    }
    let token = token.trim_matches('-');
    if token.is_empty() {
        short_project_skill_token(skill.id.as_str())
    } else {
        token.to_string()
    }
}

pub(super) fn short_project_skill_token(id: &str) -> String {
    id.chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(16)
        .collect::<String>()
}

pub(super) fn list_internal_composer_skills(
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

pub(super) fn list_claude_native_skills(
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
pub(super) struct ClaudeSkillRoot {
    pub(super) path: PathBuf,
    pub(super) scope: String,
    pub(super) name_prefix: Option<String>,
}

pub(super) fn claude_skill_roots(project_root: &Path) -> Vec<ClaudeSkillRoot> {
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

pub(super) fn push_claude_root(
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

pub(super) fn push_claude_plugin_roots(
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

pub(super) fn read_enabled_claude_plugins(claude_home: &Path) -> BTreeSet<String> {
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
        Err(_) => return Ok(skills),
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

pub(super) fn read_claude_skill_file(
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

pub(super) fn read_claude_command_file(
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

pub(super) fn format_skill_name(name_prefix: Option<&str>, name: &str) -> String {
    match name_prefix {
        Some(prefix) => format!("{prefix}:{name}"),
        None => name.to_string(),
    }
}

pub(super) fn list_codex_native_skills(
    root: &Path,
) -> Result<Vec<AgentComposerSkillResponse>, String> {
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
pub(super) struct CodexSkillRoot {
    pub(super) path: PathBuf,
    pub(super) scope: String,
    pub(super) name_prefix: Option<String>,
}

pub(super) fn codex_skill_roots(project_root: &Path) -> Vec<CodexSkillRoot> {
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

pub(super) fn push_codex_root(
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

pub(super) fn push_codex_plugin_roots(
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

pub(super) fn read_codex_plugin_skill_root(plugin_root: &Path) -> Option<(String, PathBuf)> {
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
        Err(_) => return Ok(skills),
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

pub(super) fn read_codex_skill_file(
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

pub(super) fn codex_disabled_skill_paths() -> BTreeSet<PathBuf> {
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

pub(super) fn parse_disabled_codex_skill_paths(raw: &str) -> BTreeSet<PathBuf> {
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

pub(super) fn flush_codex_skill_config(
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

pub(super) fn parse_toml_key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?.trim_start();
    rest.strip_prefix('=').map(str::trim)
}

pub(super) fn parse_toml_string(value: &str) -> Option<String> {
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

pub(super) fn codex_home_dir() -> Option<PathBuf> {
    env::var_os(CODEX_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

pub(super) fn git_repo_root(start: &Path) -> PathBuf {
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

pub(super) fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
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

pub(super) fn is_safe_skill_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}
