//! Ticket Git-convention template rendering and branch safety primitives.

use sha2::{Digest, Sha256};

use super::ticket_git_convention::{
    Placeholder, TemplatePart, TicketGitConventionContext, TicketGitConventionError,
    TicketGitConventionTemplateKind, MAX_TICKET_BRANCH_BYTES, SHORT_HASH_BYTES, SHORT_HASH_HEX_LEN,
};

/// Append a deterministic hash only when persistence detects a true normalized
/// branch-name collision. The caller supplies stable ticket identity, never a
/// conversation or concurrency token.
pub fn disambiguate_branch_name(
    branch_name: &str,
    collision_identity: &str,
) -> Result<String, TicketGitConventionError> {
    validate_branch_name(branch_name)?;
    if collision_identity.trim().is_empty() {
        return Err(TicketGitConventionError::InvalidBranch {
            reason: "collision identity cannot be empty".to_string(),
        });
    }
    let mut hasher = Sha256::new();
    hasher.update(branch_name.as_bytes());
    hasher.update([0]);
    hasher.update(collision_identity.as_bytes());
    append_hash_suffix(branch_name, &short_hash(hasher.finalize()))
}

pub(super) fn validate_template(
    kind: TicketGitConventionTemplateKind,
    template: &str,
) -> Result<(), TicketGitConventionError> {
    if template.trim().is_empty() {
        return Err(TicketGitConventionError::InvalidTemplate {
            kind,
            reason: "template cannot be empty".to_string(),
        });
    }
    if template
        .chars()
        .any(|character| character == '\n' || character == '\r' || character.is_control())
    {
        return Err(TicketGitConventionError::InvalidTemplate {
            kind,
            reason: "template must be a single printable line".to_string(),
        });
    }
    let parts = parse_template(kind, template)?;
    if !parts
        .iter()
        .any(|part| matches!(part, TemplatePart::Placeholder(Placeholder::TaskId)))
    {
        return Err(TicketGitConventionError::MissingTaskId { kind });
    }
    let summary_count = parts
        .iter()
        .filter(|part| matches!(part, TemplatePart::Placeholder(Placeholder::Summary)))
        .count();
    if kind == TicketGitConventionTemplateKind::Branch && summary_count > 0 {
        return Err(TicketGitConventionError::PlaceholderNotAllowed {
            kind,
            placeholder: Placeholder::Summary.name().to_string(),
        });
    }
    if summary_count > 1 {
        return Err(TicketGitConventionError::InvalidTemplate {
            kind,
            reason: ":summary: may appear at most once".to_string(),
        });
    }
    Ok(())
}

pub(super) fn parse_template(
    kind: TicketGitConventionTemplateKind,
    template: &str,
) -> Result<Vec<TemplatePart>, TicketGitConventionError> {
    let bytes = template.as_bytes();
    let mut parts = Vec::new();
    let mut literal_start = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b':' {
            cursor += 1;
            continue;
        }
        let Some(relative_end) = template[cursor + 1..].find(':') else {
            cursor += 1;
            continue;
        };
        let end = cursor + 1 + relative_end;
        let candidate = &template[cursor + 1..end];
        if candidate.is_empty()
            || !candidate
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            cursor += 1;
            continue;
        }
        if literal_start < cursor {
            parts.push(TemplatePart::Literal(
                template[literal_start..cursor].to_string(),
            ));
        }
        let placeholder = Placeholder::parse(candidate).ok_or_else(|| {
            TicketGitConventionError::UnknownPlaceholder {
                kind,
                placeholder: candidate.to_string(),
            }
        })?;
        parts.push(TemplatePart::Placeholder(placeholder));
        cursor = end + 1;
        literal_start = cursor;
    }
    if literal_start < template.len() {
        parts.push(TemplatePart::Literal(template[literal_start..].to_string()));
    }
    Ok(parts)
}

pub(super) fn render_template(
    kind: TicketGitConventionTemplateKind,
    template: &str,
    context: &TicketGitConventionContext<'_>,
) -> Result<String, TicketGitConventionError> {
    let rendered = render_parts(kind, &parse_template(kind, template)?, context)?;
    if kind != TicketGitConventionTemplateKind::Branch && rendered.trim().is_empty() {
        return Err(TicketGitConventionError::InvalidTemplate {
            kind,
            reason: "rendered value cannot be empty".to_string(),
        });
    }
    Ok(rendered)
}

pub(super) fn render_parts(
    kind: TicketGitConventionTemplateKind,
    parts: &[TemplatePart],
    context: &TicketGitConventionContext<'_>,
) -> Result<String, TicketGitConventionError> {
    let mut rendered = String::new();
    for part in parts {
        match part {
            TemplatePart::Literal(literal) => rendered.push_str(literal),
            TemplatePart::Placeholder(placeholder) => {
                let value = placeholder_value(*placeholder, context)?;
                if kind == TicketGitConventionTemplateKind::Branch {
                    rendered.push_str(&normalize_branch_value(placeholder.name(), value)?);
                } else {
                    rendered.push_str(value.trim());
                }
            }
        }
    }
    Ok(rendered)
}

fn placeholder_value<'a>(
    placeholder: Placeholder,
    context: &'a TicketGitConventionContext<'a>,
) -> Result<&'a str, TicketGitConventionError> {
    let value = match placeholder {
        Placeholder::TaskId => Some(context.task_id),
        Placeholder::TaskName => Some(context.task_name),
        Placeholder::Username => context.username,
        Placeholder::Summary => context.summary,
    }
    .map(str::trim)
    .filter(|value| !value.is_empty());
    value.ok_or_else(|| TicketGitConventionError::MissingPlaceholderValue {
        placeholder: placeholder.name().to_string(),
    })
}

fn normalize_branch_value(
    placeholder: &str,
    value: &str,
) -> Result<String, TicketGitConventionError> {
    let mut normalized = String::new();
    let mut last_was_dash = false;
    for character in value.chars() {
        if character.is_alphanumeric() {
            for lowercase in character.to_lowercase() {
                normalized.push(lowercase);
            }
            last_was_dash = false;
        } else if !last_was_dash && !normalized.is_empty() {
            normalized.push('-');
            last_was_dash = true;
        }
    }
    while normalized.ends_with('-') {
        normalized.pop();
    }
    if normalized.is_empty() {
        Err(TicketGitConventionError::MissingPlaceholderValue {
            placeholder: placeholder.to_string(),
        })
    } else {
        Ok(normalized)
    }
}

pub(super) fn bound_branch_name(branch_name: &str) -> Result<String, TicketGitConventionError> {
    validate_branch_name(branch_name)?;
    if branch_name.len() <= MAX_TICKET_BRANCH_BYTES {
        return Ok(branch_name.to_string());
    }
    let hash = short_hash(Sha256::digest(branch_name.as_bytes()));
    append_hash_suffix(branch_name, &hash)
}

fn append_hash_suffix(branch_name: &str, hash: &str) -> Result<String, TicketGitConventionError> {
    let max_prefix_bytes = MAX_TICKET_BRANCH_BYTES - 1 - SHORT_HASH_HEX_LEN;
    let mut prefix_end = branch_name.len().min(max_prefix_bytes);
    while !branch_name.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }
    let prefix = branch_name[..prefix_end].trim_end_matches(['/', '.', '-']);
    if prefix.is_empty() {
        return Err(TicketGitConventionError::InvalidBranch {
            reason: "branch has no safe prefix for deterministic hash".to_string(),
        });
    }
    let disambiguated = format!("{prefix}-{hash}");
    validate_branch_name(&disambiguated)?;
    Ok(disambiguated)
}

fn short_hash(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref()[..SHORT_HASH_BYTES]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn validate_branch_name(branch_name: &str) -> Result<(), TicketGitConventionError> {
    let invalid_character = branch_name.chars().find(|character| {
        character.is_control()
            || character.is_whitespace()
            || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    });
    let invalid_shape = branch_name.is_empty()
        || branch_name == "@"
        || branch_name.starts_with('-')
        || branch_name.starts_with('/')
        || branch_name.starts_with('.')
        || branch_name.ends_with('/')
        || branch_name.ends_with('.')
        || branch_name.contains("//")
        || branch_name.contains("..")
        || branch_name.contains("@{")
        || branch_name.split('/').any(|component| {
            component.is_empty() || component.starts_with('.') || component.ends_with(".lock")
        });
    if invalid_shape || invalid_character.is_some() {
        return Err(TicketGitConventionError::InvalidBranch {
            reason: format!("'{branch_name}' is not a safe Git branch name"),
        });
    }
    Ok(())
}
