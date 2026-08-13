use serde_json::Value;

use super::message_queue::ComposerIntegrationReference;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerJiraReferenceMetadata {
    pub issue_key: String,
    pub issue_id: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
}

pub fn primary_jira_key_from_composer_metadata(metadata: Option<&str>) -> Option<String> {
    primary_jira_reference_from_composer_metadata(metadata).map(|reference| reference.issue_key)
}

pub fn primary_jira_reference_from_composer_metadata(
    metadata: Option<&str>,
) -> Option<ComposerJiraReferenceMetadata> {
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
        let issue_key = normalize_jira_key(raw_key)?;
        let issue_id = reference
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let title = reference
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let url = reference
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Some(ComposerJiraReferenceMetadata {
            issue_key,
            issue_id,
            title,
            url,
        })
    })
}

pub fn jira_reference_from_composer_reference(
    reference: &ComposerIntegrationReference,
) -> Option<ComposerJiraReferenceMetadata> {
    if reference.provider != "atlassian" || reference.kind != "jira" {
        return None;
    }
    let raw_key = reference.key.as_deref().unwrap_or(reference.id.as_str());
    Some(ComposerJiraReferenceMetadata {
        issue_key: normalize_jira_key(raw_key)?,
        issue_id: Some(reference.id.trim().to_string()).filter(|value| !value.is_empty()),
        title: reference
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        url: reference
            .url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

pub fn primary_jira_reference_from_composer_references(
    references: &[ComposerIntegrationReference],
) -> Option<ComposerJiraReferenceMetadata> {
    references
        .iter()
        .find_map(jira_reference_from_composer_reference)
}

pub fn primary_linear_issue_from_composer_metadata(
    metadata: Option<&str>,
) -> Option<(String, Option<String>, Option<String>)> {
    let value = serde_json::from_str::<Value>(metadata?).ok()?;
    let references = value.get("composer_integration_references")?.as_array()?;
    references.iter().find_map(|reference| {
        let provider = reference.get("provider")?.as_str()?;
        let kind = reference.get("kind")?.as_str()?;
        if provider != "linear" || kind != "linear" {
            return None;
        }
        let id = reference.get("id")?.as_str()?.trim();
        if id.is_empty() || id.contains('\0') || id.contains('\n') || id.contains('\r') {
            return None;
        }
        let key = reference
            .get("key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let url = reference
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Some((id.to_string(), key, url))
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
    let (project, number) = trimmed.split_once('-')?;
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
    fn extracts_primary_jira_reference_details_from_metadata() {
        let metadata = r#"{
            "composer_integration_references": [
                {
                    "provider": "atlassian",
                    "kind": "jira",
                    "id": "10042",
                    "key": "rx-42",
                    "title": " Fix composer ",
                    "url": "https://example.atlassian.net/browse/RX-42"
                }
            ]
        }"#;

        let reference = primary_jira_reference_from_composer_metadata(Some(metadata)).unwrap();
        assert_eq!(reference.issue_key, "RX-42");
        assert_eq!(reference.issue_id.as_deref(), Some("10042"));
        assert_eq!(reference.title.as_deref(), Some("Fix composer"));
        assert_eq!(
            reference.url.as_deref(),
            Some("https://example.atlassian.net/browse/RX-42")
        );
    }

    #[test]
    fn extracts_first_jira_reference_from_multi_reference_metadata() {
        let metadata = r#"{
            "composer_integration_references": [
                { "provider": "atlassian", "kind": "confluence", "id": "RX-1" },
                { "provider": "atlassian", "kind": "jira", "id": "RX-42" },
                { "provider": "atlassian", "kind": "jira", "id": "RX-77" }
            ]
        }"#;

        let reference = primary_jira_reference_from_composer_metadata(Some(metadata)).unwrap();
        assert_eq!(reference.issue_key, "RX-42");
    }

    #[test]
    fn extracts_primary_jira_reference_from_structured_references() {
        let references = vec![
            ComposerIntegrationReference {
                provider: "atlassian".to_string(),
                kind: "confluence".to_string(),
                id: "RX-1".to_string(),
                key: None,
                title: None,
                url: None,
                summary_excerpt: None,
                include_transcript: None,
                selected_excerpt: None,
                selected_source_path: None,
                selected_range_label: None,
            },
            ComposerIntegrationReference {
                provider: "atlassian".to_string(),
                kind: "jira".to_string(),
                id: "10042".to_string(),
                key: Some("rx-42".to_string()),
                title: Some(" Fix composer ".to_string()),
                url: Some("https://jira.test/browse/RX-42".to_string()),
                summary_excerpt: None,
                include_transcript: None,
                selected_excerpt: None,
                selected_source_path: None,
                selected_range_label: None,
            },
        ];

        let reference = primary_jira_reference_from_composer_references(&references).unwrap();
        assert_eq!(reference.issue_key, "RX-42");
        assert_eq!(reference.issue_id.as_deref(), Some("10042"));
        assert_eq!(reference.title.as_deref(), Some("Fix composer"));
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

    #[test]
    fn ignores_non_jira_composer_references() {
        let metadata = r#"{
            "composer_integration_references": [
                { "provider": "github", "kind": "jira", "key": "RX-42" },
                { "provider": "atlassian", "kind": "confluence", "id": "RX-43" }
            ]
        }"#;

        assert_eq!(
            primary_jira_key_from_composer_metadata(Some(metadata)),
            None
        );
    }

    #[test]
    fn extracts_primary_linear_issue_from_metadata() {
        let metadata = r#"{
            "composer_integration_references": [
                {
                    "provider": "linear",
                    "kind": "linear",
                    "id": "539068e2-ae88-4d09-bd75-22eb4a59612f",
                    "key": "LIN-123",
                    "url": "https://linear.app/acme/issue/LIN-123/example"
                }
            ]
        }"#;

        assert_eq!(
            primary_linear_issue_from_composer_metadata(Some(metadata)),
            Some((
                "539068e2-ae88-4d09-bd75-22eb4a59612f".to_string(),
                Some("LIN-123".to_string()),
                Some("https://linear.app/acme/issue/LIN-123/example".to_string())
            ))
        );
    }

    #[test]
    fn accepts_unclosed_bracket_title_key() {
        assert_eq!(
            primary_jira_key_from_title("[rx-42").as_deref(),
            Some("RX-42")
        );
    }

    #[test]
    fn leaves_title_unchanged_for_invalid_jira_key() {
        assert_eq!(
            normalize_title_with_jira_key("  Existing title  ", "not-a-key"),
            "Existing title"
        );
    }

    #[test]
    fn collapses_title_that_only_contains_jira_key() {
        assert_eq!(normalize_title_with_jira_key("[RX-42]", "RX-42"), "RX-42");
        assert_eq!(normalize_title_with_jira_key("RX-42", "RX-42"), "RX-42");
    }

    #[test]
    fn rejects_invalid_title_key() {
        assert_eq!(primary_jira_key_from_title("abc"), None);
    }
}
