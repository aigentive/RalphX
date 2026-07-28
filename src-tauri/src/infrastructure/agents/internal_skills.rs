use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::domain::services::learned_skill_adapters::{
    build_constraint_bundle_skill_citations, select_pre_execution_learned_skills,
    select_pre_execution_learned_skills_multi, LearnedSkillConstraintCitation,
    LearnedSkillMultiSelectionRequest, LearnedSkillRecord, LearnedSkillSelectionRequest,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    load_canonical_agent_definition, load_canonical_agent_definition_for_profile,
};
use ralphx_domain::personas::skill_markdown::{
    split_frontmatter, trusted_slug as trusted_skill_name,
};

const SKILL_FILE_NAME: &str = "SKILL.md";
const APP_SKILLS_DIR: &[&str] = &["plugins", "app", "skills"];
const SHARED_SKILLS_DIR: &[&str] = &["plugins", "shared", "skills"];
pub(super) const DEFAULT_PRE_EXECUTION_LEARNED_SKILL_LIMIT: usize = 4;
const DEFAULT_PRE_EXECUTION_LEARNED_SKILL_TOTAL_CHARS: usize = 6_000;
const DEFAULT_PRE_EXECUTION_LEARNED_SKILL_GUIDANCE_CHARS: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalSkillInjection {
    pub system_prompt: String,
    pub injected_skill_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreExecutionLearnedSkillContext {
    pub request: LearnedSkillMultiSelectionRequest,
    pub available_skills: Vec<LearnedSkillRecord>,
    pub max_total_chars: usize,
    pub max_guidance_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalSkillSummary {
    pub name: String,
    pub description: Option<String>,
    pub user_invocable: bool,
    pub source_path: String,
}

#[derive(Debug, Clone)]
pub(super) struct InternalSkill {
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
    inject_internal_skills_into_system_prompt_for_profile(
        project_root,
        agent_name,
        None,
        system_prompt,
        match_text,
    )
}

pub fn inject_internal_skills_into_system_prompt_for_profile(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
    system_prompt: &str,
    match_text: &str,
) -> Result<InternalSkillInjection, String> {
    let Some(definition) =
        load_canonical_agent_definition_for_profile(project_root, agent_name, profile_name)
    else {
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

pub fn list_internal_skill_summaries_for_agent(
    project_root: &Path,
    agent_name: &str,
) -> Result<Vec<InternalSkillSummary>, String> {
    let Some(definition) = load_canonical_agent_definition(project_root, agent_name) else {
        return Ok(Vec::new());
    };
    let policy = definition.capabilities.internal_skills;
    validate_policy_skill_names(&policy.allowed)?;
    let mut summaries = Vec::new();
    for skill_name in &policy.allowed {
        let skill = load_internal_skill(project_root, skill_name)?;
        summaries.push(InternalSkillSummary {
            name: skill.name,
            description: skill.description,
            user_invocable: skill.user_invocable,
            source_path: display_skill_path(project_root, &skill.file_path),
        });
    }
    summaries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(summaries)
}

pub fn inject_learned_skill_citations_into_system_prompt(
    system_prompt: &str,
    citations: &[LearnedSkillConstraintCitation],
) -> InternalSkillInjection {
    inject_bounded_learned_skill_citations_into_system_prompt(
        system_prompt,
        citations,
        DEFAULT_PRE_EXECUTION_LEARNED_SKILL_LIMIT,
        DEFAULT_PRE_EXECUTION_LEARNED_SKILL_TOTAL_CHARS,
        DEFAULT_PRE_EXECUTION_LEARNED_SKILL_GUIDANCE_CHARS,
    )
}

pub fn inject_bounded_learned_skill_citations_into_system_prompt(
    system_prompt: &str,
    citations: &[LearnedSkillConstraintCitation],
    max_skills: usize,
    max_total_chars: usize,
    max_guidance_chars: usize,
) -> InternalSkillInjection {
    let max_skills = max_skills.min(DEFAULT_PRE_EXECUTION_LEARNED_SKILL_LIMIT);
    let max_total_chars = max_total_chars.min(DEFAULT_PRE_EXECUTION_LEARNED_SKILL_TOTAL_CHARS);
    let max_guidance_chars =
        max_guidance_chars.min(DEFAULT_PRE_EXECUTION_LEARNED_SKILL_GUIDANCE_CHARS);
    if max_skills == 0 || max_total_chars == 0 || max_guidance_chars == 0 {
        return InternalSkillInjection {
            system_prompt: system_prompt.to_string(),
            injected_skill_names: Vec::new(),
        };
    }

    let mut learned_skill_names = Vec::with_capacity(max_skills);
    let mut lines = vec![
        "<ralphx_learned_skill_citations>".to_string(),
        "RalphX selected the following approved learned project skill citations for this turn. Treat them as compact constraints with cited provenance.".to_string(),
    ];
    let closing = "</ralphx_learned_skill_citations>";
    for citation in citations
        .iter()
        .filter(|citation| is_safe_learned_skill_id(&citation.skill_id))
        .take(max_skills)
    {
        let guidance = citation
            .compact_guidance
            .chars()
            .take(max_guidance_chars)
            .collect::<String>();
        let mut citation_lines = vec![format!(
            "<learned_skill_citation id=\"{}\">",
            escape_prompt_text(&citation.skill_id)
        )];
        citation_lines.push(format!("title: {}", escape_prompt_text(&citation.title)));
        citation_lines.push(format!(
            "predicted_effect: {}",
            escape_prompt_text(&citation.predicted_effect)
        ));
        citation_lines.push(format!("guidance: {}", escape_prompt_text(&guidance)));
        if !citation.provenance_refs.is_empty() {
            citation_lines.push(format!(
                "provenance_refs: {}",
                escape_prompt_text(&citation.provenance_refs.join(", "))
            ));
        }
        citation_lines.push("</learned_skill_citation>".to_string());
        let mut candidate_lines = lines.clone();
        candidate_lines.extend(citation_lines.iter().cloned());
        candidate_lines.push(closing.to_string());
        if candidate_lines.join("\n").chars().count() > max_total_chars {
            break;
        }
        learned_skill_names.push(format!("learned:{}", citation.skill_id));
        lines.extend(citation_lines);
    }
    if learned_skill_names.is_empty() {
        return InternalSkillInjection {
            system_prompt: system_prompt.to_string(),
            injected_skill_names: Vec::new(),
        };
    }
    lines.push(closing.to_string());

    let mut enriched = system_prompt.trim().to_string();
    enriched.push_str("\n\n");
    enriched.push_str(&lines.join("\n"));
    InternalSkillInjection {
        system_prompt: enriched,
        injected_skill_names: learned_skill_names,
    }
}

pub fn inject_pre_execution_learned_skills_into_system_prompt(
    system_prompt: &str,
    request: LearnedSkillSelectionRequest,
    skills: &[LearnedSkillRecord],
) -> InternalSkillInjection {
    let selected = select_pre_execution_learned_skills(request, skills);
    let citations = build_constraint_bundle_skill_citations(&selected);
    inject_learned_skill_citations_into_system_prompt(system_prompt, &citations)
}

pub fn inject_pre_execution_learned_skills_into_existing_injection(
    mut injection: InternalSkillInjection,
    context: Option<&PreExecutionLearnedSkillContext>,
) -> InternalSkillInjection {
    let Some(context) = context else {
        return injection;
    };
    let selected = select_pre_execution_learned_skills_multi(
        context.request.clone(),
        &context.available_skills,
    );
    let learned_injection = inject_bounded_learned_skill_citations_into_system_prompt(
        &injection.system_prompt,
        &build_constraint_bundle_skill_citations(&selected),
        context.request.max_skills,
        context.max_total_chars,
        context.max_guidance_chars,
    );
    if learned_injection.injected_skill_names.is_empty() {
        return injection;
    }

    injection.system_prompt = learned_injection.system_prompt;
    injection
        .injected_skill_names
        .extend(learned_injection.injected_skill_names);
    injection
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

fn is_safe_learned_skill_id(skill_id: &str) -> bool {
    !skill_id.is_empty()
        && skill_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn escape_prompt_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}
pub(super) fn load_internal_skill(
    project_root: &Path,
    skill_name: &str,
) -> Result<InternalSkill, String> {
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
    let raw = crate::utils::path_safety::checked_read_to_string(skill_file, "internal skill")
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

pub(super) fn extract_internal_skill_directives(text: &str) -> Vec<String> {
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

pub(super) fn is_manual_invocation(text: &str, skill_name: &str) -> bool {
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

pub(super) fn split_match_terms(text: &str) -> Vec<String> {
    text.split(|char: char| !char.is_ascii_alphanumeric() && char != '-')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}
