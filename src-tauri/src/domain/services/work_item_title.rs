use serde_json::Value;

pub fn primary_jira_key_from_composer_metadata(metadata: Option<&str>) -> Option<String> {
    let value = serde_json::from_str::<Value>(metadata?).ok()?;
    let references = value.get("composer_integration_references")?.as_array()?;
    references.iter().find_map(|reference| {
        let provider = reference.get("provider")?.as_str()?;
        let kind = reference.get("kind")?.as_str()?;
        if provider != "atlassian" || kind != "jira" {
            return None;
        }
        let raw_key = reference
            .get("key")
            .and_then(Value::as_str)
            .or_else(|| reference.get("id").and_then(Value::as_str))?;
        normalize_jira_key(raw_key)
    })
}

pub fn primary_jira_key_from_title(title: &str) -> Option<String> {
    let trimmed = title.trim_start();
    let candidate = if let Some(rest) = trimmed.strip_prefix('[') {
        rest.split_once(']').map(|(key, _)| key).unwrap_or(rest)
    } else {
        trimmed
            .split(|ch: char| ch == ':' || ch.is_whitespace())
            .next()
            .unwrap_or(trimmed)
    };
    normalize_jira_key(candidate)
}

pub fn normalize_title_with_jira_key(title: &str, key: &str) -> String {
    let Some(key) = normalize_jira_key(key) else {
        return title.trim().to_string();
    };
    let mut remaining = title.trim();
    loop {
        let Some(next) = strip_leading_jira_key(remaining, &key) else {
            break;
        };
        if next == remaining {
            break;
        }
        remaining = next.trim_start();
    }
    if remaining.is_empty() {
        key
    } else {
        format!("{key}: {}", remaining.trim())
    }
}

fn strip_leading_jira_key<'a>(title: &'a str, key: &str) -> Option<&'a str> {
    let trimmed = title.trim_start();
    let upper = trimmed.to_ascii_uppercase();
    let bracket_prefix = format!("[{key}]");
    if upper.starts_with(&bracket_prefix) {
        return Some(trim_leading_title_separators(
            &trimmed[bracket_prefix.len()..],
        ));
    }
    if upper.starts_with(key) {
        let rest = &trimmed[key.len()..];
        if rest
            .chars()
            .next()
            .is_none_or(|ch| ch == ':' || ch == '-' || ch.is_whitespace())
        {
            return Some(trim_leading_title_separators(rest));
        }
    }
    None
}

fn trim_leading_title_separators(value: &str) -> &str {
    value.trim_start_matches(|ch: char| ch == ':' || ch == '-' || ch.is_whitespace())
}

fn normalize_jira_key(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches(|ch| ch == '[' || ch == ']');
    let mut parts = trimmed.splitn(2, '-');
    let project = parts.next()?;
    let number = parts.next()?;
    let project = project.to_ascii_uppercase();
    if project.is_empty()
        || number.is_empty()
        || project.len() > 32
        || !project.chars().all(|ch| ch.is_ascii_alphanumeric())
        || !number.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    Some(format!("{project}-{number}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_primary_jira_key_from_metadata() {
        let metadata = r#"{
            "composer_integration_references": [
                { "provider": "atlassian", "kind": "jira", "id": "RX-42", "title": "Fix" }
            ]
        }"#;

        assert_eq!(
            primary_jira_key_from_composer_metadata(Some(metadata)).as_deref(),
            Some("RX-42")
        );
    }

    #[test]
    fn normalizes_duplicate_jira_prefixes() {
        assert_eq!(
            normalize_title_with_jira_key("RX-42: [RX-42] RX-42 - Fix composer", "RX-42"),
            "RX-42: Fix composer"
        );
    }

    #[test]
    fn extracts_key_from_normalized_title() {
        assert_eq!(
            primary_jira_key_from_title("RX-42: Fix composer").as_deref(),
            Some("RX-42")
        );
    }
}
