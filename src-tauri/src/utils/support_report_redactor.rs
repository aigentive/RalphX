use lazy_static::lazy_static;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::utils::secret_redactor;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportReportRedactionSummary {
    pub replacements: Vec<SupportReportRedactionEntry>,
}

impl SupportReportRedactionSummary {
    pub fn is_empty(&self) -> bool {
        self.replacements.is_empty()
    }

    pub fn merge(&mut self, other: SupportReportRedactionSummary) {
        let mut counts: BTreeMap<String, usize> = self
            .replacements
            .iter()
            .map(|entry| (entry.category.clone(), entry.count))
            .collect();
        for entry in other.replacements {
            *counts.entry(entry.category).or_default() += entry.count;
        }
        self.replacements = counts
            .into_iter()
            .map(|(category, count)| SupportReportRedactionEntry { category, count })
            .collect();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportReportRedactionEntry {
    pub category: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SupportReportRedactionContext {
    pub project_root: Option<PathBuf>,
    pub workspace_root: Option<PathBuf>,
    pub home_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportReportRedactionResult {
    pub text: String,
    pub summary: SupportReportRedactionSummary,
}

lazy_static! {
    static ref EMAIL_RE: regex::Regex =
        regex::Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap();
    static ref HTTPS_URL_RE: regex::Regex = regex::Regex::new(r#"https?://[^\s<>"')\]]+"#).unwrap();
    static ref SSH_GIT_REMOTE_RE: regex::Regex =
        regex::Regex::new(r"\bgit@[A-Za-z0-9._-]+:[^\s]+").unwrap();
    static ref USERS_PATH_RE: regex::Regex = regex::Regex::new(r"/Users/[^\s/]+").unwrap();
}

pub fn redact_support_report_text(
    input: &str,
    context: &SupportReportRedactionContext,
) -> SupportReportRedactionResult {
    let mut text = secret_redactor::redact(input);
    let mut counts = BTreeMap::<String, usize>::new();
    if text != input {
        counts.insert("secret_pattern".to_string(), 1);
    }

    apply_path_replacement(
        &mut text,
        context.project_root.as_deref(),
        "[PROJECT_ROOT]",
        "project_path",
        &mut counts,
    );
    apply_path_replacement(
        &mut text,
        context.workspace_root.as_deref(),
        "[AGENT_WORKSPACE]",
        "workspace_path",
        &mut counts,
    );
    apply_path_replacement(
        &mut text,
        context.home_dir.as_deref(),
        "$HOME",
        "home_path",
        &mut counts,
    );

    apply_regex_replacement(
        &mut text,
        &SSH_GIT_REMOTE_RE,
        "[REDACTED_GIT_REMOTE]",
        "git_remote",
        &mut counts,
    );
    apply_regex_replacement(
        &mut text,
        &HTTPS_URL_RE,
        "[REDACTED_URL]",
        "url",
        &mut counts,
    );
    apply_regex_replacement(
        &mut text,
        &EMAIL_RE,
        "[REDACTED_EMAIL]",
        "email",
        &mut counts,
    );
    apply_regex_replacement(&mut text, &USERS_PATH_RE, "$HOME", "home_path", &mut counts);

    SupportReportRedactionResult {
        text,
        summary: SupportReportRedactionSummary {
            replacements: counts
                .into_iter()
                .map(|(category, count)| SupportReportRedactionEntry { category, count })
                .collect(),
        },
    }
}

fn apply_path_replacement(
    text: &mut String,
    path: Option<&Path>,
    replacement: &str,
    category: &str,
    counts: &mut BTreeMap<String, usize>,
) {
    let Some(path) = path else {
        return;
    };
    let needle = path.to_string_lossy();
    if needle.is_empty() || !text.contains(needle.as_ref()) {
        return;
    }

    let count = text.matches(needle.as_ref()).count();
    *text = text.replace(needle.as_ref(), replacement);
    *counts.entry(category.to_string()).or_default() += count;
}

fn apply_regex_replacement(
    text: &mut String,
    pattern: &regex::Regex,
    replacement: &str,
    category: &str,
    counts: &mut BTreeMap<String, usize>,
) {
    let count = pattern.find_iter(text).count();
    if count == 0 {
        return;
    }
    *text = pattern
        .replace_all(text, regex::NoExpand(replacement))
        .into_owned();
    *counts.entry(category.to_string()).or_default() += count;
}

#[cfg(test)]
#[path = "support_report_redactor_tests.rs"]
mod tests;
