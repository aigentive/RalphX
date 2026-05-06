use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::infrastructure::agents::harness_agent_catalog::load_canonical_agent_definition;

const SKILL_FILE_NAME: &str = "SKILL.md";
const APP_SKILLS_DIR: &[&str] = &["plugins", "app", "skills"];
const SHARED_SKILLS_DIR: &[&str] = &["plugins", "shared", "skills"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalSkillInjection {
    pub system_prompt: String,
    pub injected_skill_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct InternalSkill {
    name: String,
    description: Option<String>,
    trigger: Option<String>,
    disable_model_invocation: bool,
    user_invocable: bool,
    priority: i32,
    body: String,
    file_path: PathBuf,
}

#[derive(Debug, Deserialize, Default)]
struct InternalSkillFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    trigger: Option<String>,
    #[serde(default, rename = "disable-model-invocation")]
    disable_model_invocation: Option<bool>,
    #[serde(default, rename = "user-invocable")]
    user_invocable: Option<bool>,
    #[serde(default)]
    priority: Option<i32>,
}

#[derive(Debug, Clone)]
struct SkillCandidate {
    name: String,
    score: i32,
    priority: i32,
}

pub fn inject_internal_skills_into_system_prompt(
    project_root: &Path,
    agent_name: &str,
    system_prompt: &str,
    match_text: &str,
) -> Result<InternalSkillInjection, String> {
    let Some(definition) = load_canonical_agent_definition(project_root, agent_name) else {
        return Ok(InternalSkillInjection {
            system_prompt: system_prompt.to_string(),
            injected_skill_names: Vec::new(),
        });
    };
    let policy = definition.capabilities.internal_skills;
    if policy.allowed.is_empty() {
        return Ok(InternalSkillInjection {
            system_prompt: system_prompt.to_string(),
            injected_skill_names: Vec::new(),
        });
    }

    validate_policy_skill_names(&policy.allowed)?;
    let allowed = policy.allowed.iter().cloned().collect::<BTreeSet<_>>();

    let mut skills = BTreeMap::new();
    for skill_name in &policy.allowed {
        let skill = load_internal_skill(project_root, skill_name)?;
        skills.insert(skill.name.clone(), skill);
    }

    let mut selected_names = Vec::new();
    let mut selected_set = BTreeSet::new();
    for skill_name in extract_internal_skill_directives(match_text) {
        validate_allowed_skill_reference(&skill_name, &allowed)?;
        if selected_set.insert(skill_name.clone()) {
            selected_names.push(skill_name);
        }
    }

    for skill in skills.values() {
        if !skill.user_invocable {
            continue;
        }
        if is_manual_invocation(match_text, &skill.name) && selected_set.insert(skill.name.clone())
        {
            selected_names.push(skill.name.clone());
        }
    }

    if policy.auto_match {
        let max_auto_loaded = policy.max_auto_loaded.unwrap_or(2);
        let mut candidates = skills
            .values()
            .filter(|skill| !skill.disable_model_invocation)
            .filter(|skill| !selected_set.contains(&skill.name))
            .filter_map(|skill| {
                let score = score_skill_match(skill, match_text);
                (score > 0).then(|| SkillCandidate {
                    name: skill.name.clone(),
                    score,
                    priority: skill.priority,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.priority.cmp(&left.priority))
                .then_with(|| left.name.cmp(&right.name))
        });
        for candidate in candidates.into_iter().take(max_auto_loaded) {
            if selected_set.insert(candidate.name.clone()) {
                selected_names.push(candidate.name);
            }
        }
    }

    if selected_names.is_empty() {
        return Ok(InternalSkillInjection {
            system_prompt: system_prompt.to_string(),
            injected_skill_names: Vec::new(),
        });
    }

    let selected_skills = selected_names
        .iter()
        .map(|name| {
            skills
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Selected internal skill `{name}` was not loaded"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let internal_skill_context = render_internal_skill_context(project_root, &selected_skills);
    let mut enriched = system_prompt.trim().to_string();
    enriched.push_str("\n\n");
    enriched.push_str(&internal_skill_context);

    Ok(InternalSkillInjection {
        system_prompt: enriched,
        injected_skill_names: selected_names,
    })
}

pub fn validate_agent_internal_skills(project_root: &Path, agent_name: &str) -> Result<(), String> {
    let Some(definition) = load_canonical_agent_definition(project_root, agent_name) else {
        return Ok(());
    };
    let policy = definition.capabilities.internal_skills;
    validate_policy_skill_names(&policy.allowed)?;
    for skill_name in &policy.allowed {
        load_internal_skill(project_root, skill_name)?;
    }
    Ok(())
}

fn render_internal_skill_context(project_root: &Path, skills: &[InternalSkill]) -> String {
    let mut lines = vec![
        "<ralphx_internal_skills>".to_string(),
        "RalphX selected the following internal skills for this turn. Follow these instructions as part of your system guidance.".to_string(),
    ];
    for skill in skills {
        lines.push(format!("<internal_skill name=\"{}\">", skill.name));
        lines.push("<internal_skill_metadata>".to_string());
        lines.push(format!(
            "source_file: {}",
            display_skill_path(project_root, &skill.file_path)
        ));
        if let Some(description) = skill.description.as_deref() {
            lines.push(format!("description: {description}"));
        }
        if let Some(trigger) = skill.trigger.as_deref() {
            lines.push(format!("trigger: {trigger}"));
        }
        lines.push("</internal_skill_metadata>".to_string());
        lines.push(skill.body.trim().to_string());
        lines.push("</internal_skill>".to_string());
    }
    lines.push("</ralphx_internal_skills>".to_string());
    lines.join("\n")
}

fn display_skill_path(project_root: &Path, file_path: &Path) -> String {
    file_path
        .strip_prefix(project_root)
        .unwrap_or(file_path)
        .display()
        .to_string()
}

fn validate_policy_skill_names(skill_names: &[String]) -> Result<(), String> {
    for skill_name in skill_names {
        trusted_skill_name(skill_name)
            .ok_or_else(|| format!("Invalid internal skill name `{skill_name}`"))?;
    }
    Ok(())
}

fn validate_allowed_skill_reference(
    skill_name: &str,
    allowed: &BTreeSet<String>,
) -> Result<(), String> {
    trusted_skill_name(skill_name)
        .ok_or_else(|| format!("Invalid internal skill name `{skill_name}`"))?;
    if !allowed.contains(skill_name) {
        return Err(format!(
            "Internal skill `{skill_name}` was requested by directive but is not listed in allowed"
        ));
    }
    Ok(())
}

fn trusted_skill_name(skill_name: &str) -> Option<&str> {
    let valid = !skill_name.is_empty()
        && !skill_name.contains("..")
        && !skill_name.contains('/')
        && !skill_name.contains('\\')
        && skill_name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    valid.then_some(skill_name)
}

fn load_internal_skill(project_root: &Path, skill_name: &str) -> Result<InternalSkill, String> {
    let trusted_name = trusted_skill_name(skill_name)
        .ok_or_else(|| format!("Invalid skill name `{skill_name}`"))?;
    for root_parts in [APP_SKILLS_DIR, SHARED_SKILLS_DIR] {
        if let Some(skill_file) = trusted_skill_file(project_root, trusted_name, root_parts) {
            return read_internal_skill_file(trusted_name, &skill_file);
        }
    }
    Err(format!("Internal skill `{trusted_name}` was not found"))
}

fn trusted_skill_file(
    project_root: &Path,
    skill_name: &str,
    root_parts: &[&str],
) -> Option<PathBuf> {
    let canonical_project_root = project_root.canonicalize().ok()?;
    let skills_root = root_parts
        .iter()
        .fold(canonical_project_root.clone(), |path, part| path.join(part));
    let canonical_skills_root = skills_root.canonicalize().ok()?;
    if !canonical_skills_root.starts_with(&canonical_project_root)
        || canonical_skills_root.file_name() != Some(OsStr::new("skills"))
        || !canonical_skills_root.is_dir()
    {
        return None;
    }
    let candidate = canonical_skills_root.join(skill_name).join(SKILL_FILE_NAME);
    let canonical_candidate = candidate.canonicalize().ok()?;
    if canonical_candidate.starts_with(&canonical_skills_root)
        && canonical_candidate.file_name() == Some(OsStr::new(SKILL_FILE_NAME))
        && canonical_candidate.is_file()
    {
        Some(canonical_candidate)
    } else {
        None
    }
}

fn read_internal_skill_file(skill_name: &str, skill_file: &Path) -> Result<InternalSkill, String> {
    // codeql[rust/path-injection]
    let raw = std::fs::read_to_string(skill_file)
        .map_err(|error| format!("Failed to read internal skill `{skill_name}`: {error}"))?;
    let (frontmatter, body) = split_frontmatter(&raw)
        .ok_or_else(|| format!("Internal skill `{skill_name}` must start with YAML frontmatter"))?;
    let metadata = serde_yaml::from_str::<InternalSkillFrontmatter>(frontmatter)
        .map_err(|error| format!("Failed to parse internal skill `{skill_name}`: {error}"))?;
    let declared_name = metadata.name.as_deref().unwrap_or(skill_name);
    if trusted_skill_name(declared_name) != Some(skill_name) {
        return Err(format!(
            "Internal skill `{skill_name}` declares mismatched name `{declared_name}`"
        ));
    }
    Ok(InternalSkill {
        name: skill_name.to_string(),
        description: metadata.description,
        trigger: metadata.trigger,
        disable_model_invocation: metadata.disable_model_invocation.unwrap_or(false),
        user_invocable: metadata.user_invocable.unwrap_or(true),
        priority: metadata.priority.unwrap_or(0),
        body: body.trim().to_string(),
        file_path: skill_file.to_path_buf(),
    })
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

fn extract_internal_skill_directives(text: &str) -> Vec<String> {
    let mut skill_names = BTreeSet::new();
    for line in text.lines() {
        if let Some(index) = line.find("ralphx_internal_skill=") {
            let raw = &line[index + "ralphx_internal_skill=".len()..];
            if let Some(value) = raw.split_whitespace().next() {
                let skill_name = value
                    .trim_matches(|char| matches!(char, '<' | '>' | '-' | '"' | '\'' | ';' | ','));
                if trusted_skill_name(skill_name).is_some() {
                    skill_names.insert(skill_name.to_string());
                }
            }
        }
        for skill_name in extract_use_skill_directives(line) {
            skill_names.insert(skill_name);
        }
    }
    skill_names.into_iter().collect()
}

fn extract_use_skill_directives(line: &str) -> Vec<String> {
    let lower = line.to_ascii_lowercase();
    let mut skill_names = Vec::new();
    let mut offset = 0;
    while let Some(index) = lower[offset..].find("use /") {
        let start = offset + index + "use /".len();
        let candidate = lower[start..]
            .chars()
            .take_while(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || *char == '-')
            .collect::<String>();
        if candidate.is_empty() {
            offset = start;
            continue;
        }
        let rest = lower[start + candidate.len()..].trim_start();
        if rest.starts_with("skill") && trusted_skill_name(&candidate).is_some() {
            skill_names.push(candidate);
        }
        offset = start + 1;
    }
    skill_names
}

fn is_manual_invocation(text: &str, skill_name: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains(&format!("/{skill_name}"))
}

fn score_skill_match(skill: &InternalSkill, text: &str) -> i32 {
    let lower = text.to_ascii_lowercase();
    let mut score = 0;

    if let Some(trigger) = skill.trigger.as_deref() {
        for trigger_phrase in split_match_terms(trigger) {
            if !trigger_phrase.is_empty() && lower.contains(&trigger_phrase) {
                score += 80;
            }
        }
    }

    for token in split_match_terms(&skill.name) {
        if token.len() > 2 && lower.contains(&token) {
            score += 10;
        }
    }

    if let Some(description) = skill.description.as_deref() {
        let mut hits = 0;
        for token in split_match_terms(description) {
            if token.len() > 4 && lower.contains(&token) {
                hits += 1;
            }
        }
        if hits >= 2 {
            score += hits * 5;
        }
    }

    score + skill.priority
}

fn split_match_terms(text: &str) -> Vec<String> {
    text.split(|char: char| !char.is_ascii_alphanumeric() && char != '-')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_agent(root: &Path, agent_yaml: &str) {
        fs::create_dir_all(root.join("agents/test-agent")).expect("create agent dir");
        fs::write(root.join("agents/test-agent/agent.yaml"), agent_yaml).expect("write agent");
    }

    fn create_skill(root: &Path, name: &str, body: &str) {
        fs::create_dir_all(root.join(format!("plugins/app/skills/{name}")))
            .expect("create skill dir");
        fs::write(
            root.join(format!("plugins/app/skills/{name}/SKILL.md")),
            body,
        )
        .expect("write skill");
    }

    #[test]
    fn explicit_internal_directive_injects_allowlisted_internal_skill() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        create_agent(
            root,
            r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - workspace-swe
"#,
        );
        create_skill(
            root,
            "workspace-swe",
            r#"---
name: workspace-swe
description: Workspace bridge instructions
disable-model-invocation: true
user-invocable: false
---
# Workspace SWE
Report only unless the event payload explicitly asks for intervention.
"#,
        );

        let injected = inject_internal_skills_into_system_prompt(
            root,
            "test-agent",
            "Base prompt",
            "Use /workspace-swe skill for this bridge wake-up.",
        )
        .expect("inject");

        assert_eq!(injected.injected_skill_names, vec!["workspace-swe"]);
        assert!(injected.system_prompt.contains("Base prompt"));
        assert!(injected.system_prompt.contains("# Workspace SWE"));
    }

    #[test]
    fn disallowed_manual_skill_request_is_not_injected() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        create_agent(
            root,
            r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed: []
"#,
        );
        create_skill(
            root,
            "workspace-swe",
            r#"---
name: workspace-swe
description: Workspace bridge instructions
---
# Workspace SWE
This should not load.
"#,
        );

        let injected = inject_internal_skills_into_system_prompt(
            root,
            "test-agent",
            "Base prompt",
            "Please use /workspace-swe.",
        )
        .expect("inject");

        assert!(injected.injected_skill_names.is_empty());
        assert!(!injected.system_prompt.contains("This should not load"));
    }

    #[test]
    fn disabled_skill_does_not_auto_match_but_can_be_directed() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        create_agent(
            root,
            r#"name: test-agent
role: test
capabilities:
  internal_skills:
    auto_match: true
    allowed:
      - workspace-swe
"#,
        );
        create_skill(
            root,
            "workspace-swe",
            r#"---
name: workspace-swe
description: Workspace bridge instructions
trigger: workspace bridge
disable-model-invocation: true
user-invocable: false
---
# Workspace SWE
Forced only.
"#,
        );

        let auto = inject_internal_skills_into_system_prompt(
            root,
            "test-agent",
            "Base prompt",
            "workspace bridge",
        )
        .expect("auto inject");
        assert!(auto.injected_skill_names.is_empty());

        let directed = inject_internal_skills_into_system_prompt(
            root,
            "test-agent",
            "Base prompt",
            "<!-- ralphx_internal_skill=workspace-swe -->",
        )
        .expect("directed inject");
        assert_eq!(directed.injected_skill_names, vec!["workspace-swe"]);
        assert!(directed.system_prompt.contains("Forced only."));
    }

    #[test]
    fn validation_rejects_unknown_allowed_skill() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        create_agent(
            root,
            r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - missing-skill
"#,
        );

        let error = validate_agent_internal_skills(root, "test-agent")
            .expect_err("unknown skill should fail validation");
        assert!(error.contains("missing-skill"));
    }

    #[test]
    fn directive_for_disallowed_skill_fails_closed() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        create_agent(
            root,
            r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - workspace-swe
"#,
        );
        create_skill(
            root,
            "workspace-swe",
            r#"---
name: workspace-swe
description: Workspace bridge instructions
---
# Workspace SWE
"#,
        );

        let error = inject_internal_skills_into_system_prompt(
            root,
            "test-agent",
            "Base prompt",
            "<!-- ralphx_internal_skill=other-skill -->",
        )
        .expect_err("disallowed directive should fail closed");
        assert!(error.contains("other-skill"));
        assert!(error.contains("allowed"));
    }

    #[test]
    fn live_agent_internal_skill_configs_are_valid() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let agent_name = "ralphx-chat-project";
        validate_agent_internal_skills(&root, agent_name)
            .unwrap_or_else(|error| panic!("{agent_name} internal skills invalid: {error}"));
    }

    #[test]
    fn live_general_workspace_agents_do_not_load_workspace_bridge_skill() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        for agent_name in ["ralphx-general-worker", "ralphx-general-explorer"] {
            let injected = inject_internal_skills_into_system_prompt(
                &root,
                agent_name,
                "Base prompt",
                "<!-- ralphx_internal_skill=ralphx-agent-workspace-swe -->",
            )
            .unwrap_or_else(|error| panic!("{agent_name} injection failed: {error}"));
            assert!(
                injected.injected_skill_names.is_empty(),
                "{agent_name} must not load agent workspace bridge skills"
            );
            assert_eq!(injected.system_prompt, "Base prompt");
        }
    }
}
