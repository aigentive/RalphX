use crate::{
    personas::skill_markdown::{split_frontmatter, trusted_slug},
    AppError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub const PERSONA_BODY_MAX_BYTES: usize = 10_240;
pub const PERSONA_BODY_MAX_LINES: usize = 150;
const STRUCTURAL_TAGS: [&str; 8] = [
    "ralphx_agent_persona",
    "persona_precedence",
    "ralphx_internal_skills",
    "internal_skill",
    "internal_skill_metadata",
    "agent_runtime_profile",
    "ralphx_agent_instructions",
    "agent_task_ledger_contract",
];
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PersonaFrontmatter {
    pub name: String,
    pub kind: Option<String>,
    pub description: String,
}

/// Compose canonical SKILL.md-shaped persona content from structured fields.
pub fn compose_persona_content(slug: &str, description: &str, body: &str) -> String {
    let frontmatter = PersonaFrontmatter {
        name: slug.to_string(),
        kind: Some("persona".to_string()),
        description: normalize_description(description),
    };
    let yaml = serde_yaml::to_string(&frontmatter)
        .expect("PersonaFrontmatter serialization should not fail");
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);

    format!("---\n{yaml}---\n\n{}\n", body.trim())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPersona {
    pub frontmatter: PersonaFrontmatter,
    pub frontmatter_raw: String,
    pub body: String,
    pub content_hash: String,
}
pub fn validate_persona_content(slug: &str, content: &str) -> Result<ParsedPersona, AppError> {
    let (frontmatter_raw, body) = split_frontmatter(content)
        .ok_or_else(|| persona_validation_error(slug, "missing YAML frontmatter"))?;
    let frontmatter = serde_yaml::from_str::<PersonaFrontmatter>(frontmatter_raw)
        .map_err(|_| persona_validation_error(slug, "invalid YAML frontmatter"))?;
    if frontmatter.name != slug {
        return Err(persona_validation_error(
            slug,
            "frontmatter name does not match slug",
        ));
    }
    if trusted_slug(slug).is_none() {
        return Err(persona_validation_error(slug, "invalid slug"));
    }
    if body.len() > PERSONA_BODY_MAX_BYTES {
        return Err(persona_validation_error(slug, "body exceeds byte limit"));
    }
    if body.lines().count() > PERSONA_BODY_MAX_LINES {
        return Err(persona_validation_error(slug, "body exceeds line limit"));
    }
    reject_structural_tags(body)
        .map_err(|_| persona_validation_error(slug, "body contains blocked structural tag"))?;
    if frontmatter.kind.as_deref() != Some("persona") {
        return Err(persona_validation_error(slug, "kind must be persona"));
    }

    Ok(ParsedPersona {
        frontmatter,
        frontmatter_raw: frontmatter_raw.to_string(),
        body: body.to_string(),
        content_hash: compute_content_hash(frontmatter_raw, body),
    })
}
pub fn reject_structural_tags(body: &str) -> Result<(), AppError> {
    let bytes = body.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] != b'<' {
            continue;
        }
        let mut tag_start = index + 1;
        if bytes.get(tag_start) == Some(&b'/') {
            tag_start += 1;
        }
        while bytes.get(tag_start).is_some_and(u8::is_ascii_whitespace) {
            tag_start += 1;
        }
        if STRUCTURAL_TAGS.iter().any(|tag| {
            bytes
                .get(tag_start..tag_start + tag.len())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(tag.as_bytes()))
        }) {
            return Err(AppError::Validation(
                "Persona body contains a blocked structural tag".to_string(),
            ));
        }
    }
    Ok(())
}
pub fn compute_content_hash(frontmatter: &str, body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(frontmatter.as_bytes());
    hasher.update(body.as_bytes());
    format!("{:x}", hasher.finalize())
}
fn persona_validation_error(slug: &str, reason: &str) -> AppError {
    AppError::Validation(format!("Invalid persona `{slug}`: {reason}"))
}

fn normalize_description(description: &str) -> String {
    description
        .trim()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}
