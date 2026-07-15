pub fn trusted_slug(skill_name: &str) -> Option<&str> {
    let valid = !skill_name.is_empty()
        && !skill_name.contains("..")
        && !skill_name.contains('/')
        && !skill_name.contains('\\')
        && skill_name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    valid.then_some(skill_name)
}

pub fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
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
