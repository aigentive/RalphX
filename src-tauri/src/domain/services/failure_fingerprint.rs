use lazy_static::lazy_static;
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::domain::entities::learned_skill::{
    RECURRENCE_KEY_FIELD, RECURRENCE_KEY_PREFIX, RECURRENCE_SESSION_FIELD,
};
use crate::domain::entities::TaskOutcomeClass;

lazy_static! {
    static ref ANSI_ESCAPE_RE: Regex =
        Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("valid ansi escape regex");
    static ref ABSOLUTE_PATH_RE: Regex =
        Regex::new(r#"(?m)(^|[\s=])(?:/[^\s:'"`]+)+"#).expect("valid path regex");
    static ref WINDOWS_PATH_RE: Regex =
        Regex::new(r#"(?i)[a-z]:\\[^\s:'"`]+(?:\\[^\s:'"`]+)*"#).expect("valid windows path regex");
    static ref UUID_RE: Regex =
        Regex::new(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
            .expect("valid uuid regex");
    static ref SHA_RE: Regex = Regex::new(r"\b[0-9a-f]{12,40}\b").expect("valid sha regex");
    static ref TIMESTAMP_RE: Regex =
        Regex::new(r"\b\d{4}-\d{2}-\d{2}[t ][0-9:.+-z]+\b").expect("valid timestamp regex");
}

pub(crate) fn normalize_failure_evidence(text: &str) -> String {
    let mut normalized = ANSI_ESCAPE_RE
        .replace_all(text, "")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .to_lowercase();
    normalized = TIMESTAMP_RE
        .replace_all(&normalized, "<timestamp>")
        .into_owned();
    normalized = UUID_RE.replace_all(&normalized, "<uuid>").into_owned();
    normalized = SHA_RE.replace_all(&normalized, "<sha>").into_owned();
    normalized = WINDOWS_PATH_RE
        .replace_all(&normalized, "<path>")
        .into_owned();
    normalized = ABSOLUTE_PATH_RE
        .replace_all(&normalized, |captures: &regex::Captures<'_>| {
            format!(
                "{}<path>",
                captures.get(1).map(|value| value.as_str()).unwrap_or("")
            )
        })
        .into_owned();
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn failure_fingerprint(class: &TaskOutcomeClass, evidence: &str) -> String {
    let normalized = normalize_failure_evidence(evidence);
    format!(
        "{:x}",
        Sha256::digest(format!("{}\n{normalized}", class.as_str()).as_bytes())
    )
}

pub(crate) fn recurrence_key(evidence: &str) -> Option<String> {
    let tokens = evidence
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if tokens.is_empty() {
        return None;
    }
    let canonical = tokens.into_iter().collect::<Vec<_>>().join("\n");
    Some(format!(
        "{RECURRENCE_KEY_PREFIX}{:x}",
        Sha256::digest(canonical.as_bytes())
    ))
}

pub(crate) fn attach_recurrence_evidence(
    evidence: &mut Value,
    recurrence_text: &str,
    trusted_session: Option<&str>,
) -> Option<String> {
    let key = recurrence_key(recurrence_text)?;
    let object = evidence.as_object_mut()?;
    object.insert(RECURRENCE_KEY_FIELD.to_string(), Value::String(key.clone()));
    if let Some(session) = trusted_session
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            RECURRENCE_SESSION_FIELD.to_string(),
            Value::String(session.to_string()),
        );
    }
    Some(key)
}
