use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{ChatConversationId, ProjectId};

pub const AGENT_CONVERSATION_ISSUE_STATUS_OPEN: &str = "open";
pub const AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED: &str = "resolved";
pub const AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED: &str = "dismissed";
pub const AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED: &str = "created";
pub const AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED: &str = "exact_attached";
pub const AGENT_CONVERSATION_ISSUE_DEDUPE_CANDIDATE_ATTACHED: &str = "candidate_attached";
pub const AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW: &str = "confirmed_new";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationIssue {
    pub id: String,
    pub project_id: ProjectId,
    pub conversation_id: ChatConversationId,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub issue_kind: String,
    pub severity: String,
    pub status: String,
    pub blocking_scope: String,
    pub title: String,
    pub summary: String,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub canonical_fingerprint: Option<String>,
    pub canonical_scope_kind: Option<String>,
    pub canonical_scope_subject: Option<String>,
    pub canonical_family: Option<String>,
    pub superseded_by_issue_id: Option<String>,
    pub followup_title: Option<String>,
    pub followup_prompt: Option<String>,
    pub auto_followup_eligible: bool,
    pub linked_followup_conversation_id: Option<ChatConversationId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl AgentConversationIssue {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: ProjectId,
        conversation_id: ChatConversationId,
        source_task_id: Option<String>,
        source_context_type: Option<String>,
        source_context_id: Option<String>,
        source_agent_name: Option<String>,
        issue_kind: String,
        severity: String,
        blocking_scope: String,
        title: String,
        summary: String,
        evidence: Option<String>,
        recommendation: Option<String>,
        blocker_fingerprint: Option<String>,
        followup_title: Option<String>,
        followup_prompt: Option<String>,
        auto_followup_eligible: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            project_id,
            conversation_id,
            source_task_id,
            source_context_type,
            source_context_id,
            source_agent_name,
            issue_kind,
            severity,
            status: AGENT_CONVERSATION_ISSUE_STATUS_OPEN.to_string(),
            blocking_scope,
            title,
            summary,
            evidence,
            recommendation,
            blocker_fingerprint,
            canonical_fingerprint: None,
            canonical_scope_kind: None,
            canonical_scope_subject: None,
            canonical_family: None,
            superseded_by_issue_id: None,
            followup_title,
            followup_prompt,
            auto_followup_eligible,
            linked_followup_conversation_id: None,
            created_at: now,
            updated_at: now,
            resolved_at: None,
        }
    }

    pub fn refresh_from(&mut self, incoming: Self) {
        self.source_task_id = incoming.source_task_id;
        self.source_context_type = incoming.source_context_type;
        self.source_context_id = incoming.source_context_id;
        self.source_agent_name = incoming.source_agent_name;
        self.severity = incoming.severity;
        self.blocking_scope = incoming.blocking_scope;
        self.title = incoming.title;
        self.summary = incoming.summary;
        self.evidence = incoming.evidence;
        self.recommendation = incoming.recommendation;
        self.blocker_fingerprint = incoming.blocker_fingerprint;
        if incoming.canonical_fingerprint.is_some() {
            self.canonical_fingerprint = incoming.canonical_fingerprint;
            self.canonical_scope_kind = incoming.canonical_scope_kind;
            self.canonical_scope_subject = incoming.canonical_scope_subject;
            self.canonical_family = incoming.canonical_family;
        }
        self.followup_title = incoming.followup_title;
        self.followup_prompt = incoming.followup_prompt;
        self.auto_followup_eligible = incoming.auto_followup_eligible;
        self.status = AGENT_CONVERSATION_ISSUE_STATUS_OPEN.to_string();
        self.resolved_at = None;
        self.updated_at = Utc::now();
    }

    pub fn apply_canonical_identity(&mut self, identity: &AgentConversationIssueCanonicalIdentity) {
        self.canonical_fingerprint = Some(identity.fingerprint.clone());
        self.canonical_scope_kind = Some(identity.scope_kind.clone());
        self.canonical_scope_subject = Some(identity.scope_subject.clone());
        self.canonical_family = Some(identity.family.clone());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationIssueOccurrence {
    pub id: String,
    pub issue_id: String,
    pub project_id: ProjectId,
    pub conversation_id: ChatConversationId,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub issue_kind: String,
    pub severity: String,
    pub blocking_scope: String,
    pub title: String,
    pub summary: String,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
    pub raw_blocker_fingerprint: Option<String>,
    pub canonical_fingerprint: Option<String>,
    pub dedupe_decision: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl AgentConversationIssueOccurrence {
    pub fn from_issue(issue: &AgentConversationIssue, dedupe_decision: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            issue_id: issue.id.clone(),
            project_id: issue.project_id.clone(),
            conversation_id: issue.conversation_id,
            source_task_id: issue.source_task_id.clone(),
            source_context_type: issue.source_context_type.clone(),
            source_context_id: issue.source_context_id.clone(),
            source_agent_name: issue.source_agent_name.clone(),
            issue_kind: issue.issue_kind.clone(),
            severity: issue.severity.clone(),
            blocking_scope: issue.blocking_scope.clone(),
            title: issue.title.clone(),
            summary: issue.summary.clone(),
            evidence: issue.evidence.clone(),
            recommendation: issue.recommendation.clone(),
            raw_blocker_fingerprint: issue.blocker_fingerprint.clone(),
            canonical_fingerprint: issue.canonical_fingerprint.clone(),
            dedupe_decision: Some(dedupe_decision.into()),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConversationIssueCanonicalIdentity {
    pub fingerprint: String,
    pub scope_kind: String,
    pub scope_subject: String,
    pub family: String,
    pub candidate_match_eligible: bool,
}

#[derive(Debug, Clone)]
pub struct AgentConversationIssueCanonicalInput<'a> {
    pub issue_kind: &'a str,
    pub blocking_scope: &'a str,
    pub title: &'a str,
    pub summary: &'a str,
    pub evidence: Option<&'a str>,
    pub recommendation: Option<&'a str>,
    pub blocker_fingerprint: Option<&'a str>,
    pub source_task_id: Option<&'a str>,
}

pub fn canonicalize_agent_conversation_issue(
    input: &AgentConversationIssueCanonicalInput<'_>,
) -> AgentConversationIssueCanonicalIdentity {
    let raw_text = [
        Some(input.title),
        Some(input.summary),
        input.evidence,
        input.recommendation,
        input.blocker_fingerprint,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let text = normalize_signature_text(&raw_text);
    let issue_kind = normalized_token(input.issue_kind, "unknown");

    if is_frontend_dependency_setup(&text) {
        return known_identity(
            "setup",
            "project",
            "frontend-package",
            "missing-frontend-dependency",
        );
    }
    if is_package_lock_drift(&text) {
        return known_identity(
            "setup",
            "project",
            "ralphx-plugin-mcp",
            "package-lock-drift",
        );
    }
    if is_rails_test_database_setup(&text) {
        return known_identity(
            "setup",
            "project",
            "rails-test-database",
            "schema-unavailable",
        );
    }
    if text.contains("cargo clippy") || text.contains(" clippy ") || text.contains("clippy:") {
        return known_identity(
            "validation",
            "project",
            "backend-clippy",
            "preexisting-baseline",
        );
    }
    if text.contains("runtime-index") || text.contains("runtime index") {
        return known_identity(
            "prerequisite",
            "project",
            "runtime-index",
            "missing-runtime-surface",
        );
    }
    if text.contains("merge hook") && has_environment_failure_signal(&text) {
        return known_identity(
            "merge-hook",
            "project",
            "merge-hook-environment",
            "environment-failure",
        );
    }
    if is_scope_drift(input.blocker_fingerprint, &text) {
        let task = input
            .source_task_id
            .map(|value| slug_token(value, "unknown-task"))
            .unwrap_or_else(|| "unknown-task".to_string());
        let hash = short_hash(&text);
        return AgentConversationIssueCanonicalIdentity {
            fingerprint: format!("v1:scope-drift:task:{task}:files:{hash}"),
            scope_kind: "task".to_string(),
            scope_subject: task,
            family: "scope-drift".to_string(),
            candidate_match_eligible: true,
        };
    }

    if let Some(blocker_fingerprint) = input.blocker_fingerprint {
        let trimmed = blocker_fingerprint.trim();
        if !trimmed.is_empty() {
            let (scope_kind, scope_subject) =
                fallback_scope(input.blocking_scope, input.source_task_id);
            return AgentConversationIssueCanonicalIdentity {
                fingerprint: trimmed.to_string(),
                scope_kind,
                scope_subject,
                family: issue_kind,
                candidate_match_eligible: false,
            };
        }
    }

    let (scope_kind, scope_subject) = fallback_scope(input.blocking_scope, input.source_task_id);
    let title = slug_token(input.title, "untitled");
    AgentConversationIssueCanonicalIdentity {
        fingerprint: format!("v1:unknown:{scope_kind}:{scope_subject}:{issue_kind}:{title}"),
        scope_kind,
        scope_subject,
        family: issue_kind,
        candidate_match_eligible: false,
    }
}

fn known_identity(
    family: &str,
    scope_kind: &str,
    scope_subject: &str,
    failure: &str,
) -> AgentConversationIssueCanonicalIdentity {
    AgentConversationIssueCanonicalIdentity {
        fingerprint: format!("v1:{family}:{scope_kind}:{scope_subject}:{failure}"),
        scope_kind: scope_kind.to_string(),
        scope_subject: scope_subject.to_string(),
        family: family.to_string(),
        candidate_match_eligible: true,
    }
}

fn is_frontend_dependency_setup(text: &str) -> bool {
    let mentions_frontend = text.contains("frontend")
        || text.contains("node_modules")
        || text.contains("package lookup")
        || text.contains("repo-root setup");
    let mentions_tsc = text.contains(" tsc")
        || text.contains("tsc ")
        || text.contains("tsc-not-found")
        || text.contains("cannot find tsc")
        || text.contains("find tsc");
    let dependency_signal = text.contains("node_modules")
        || text.contains("missing")
        || text.contains("not found")
        || text.contains("cannot find")
        || text.contains("path");
    (mentions_frontend || mentions_tsc) && dependency_signal
}

fn is_package_lock_drift(text: &str) -> bool {
    text.contains("package-lock")
        && (text.contains("ralphx-plugin")
            || text.contains("ralphx-mcp-server")
            || text.contains("mcp"))
}

fn is_rails_test_database_setup(text: &str) -> bool {
    let rails_or_spec = text.contains("rails")
        || text.contains("rspec")
        || text.contains("db:schema:load")
        || text.contains("database.yml");
    let test_database = text.contains("test database")
        || text.contains("test db")
        || text.contains("task worktree");
    let schema_failure = text.contains("pending migration")
        || text.contains("pending migrations")
        || text.contains("schema")
        || text.contains("pg::undefinedtable")
        || text.contains("relation ")
        || text.contains("table");
    let setup_failure = text.contains("missing")
        || text.contains("unavailable")
        || text.contains("cannot run")
        || text.contains("blocked")
        || text.contains("fails")
        || text.contains("does not exist");

    rails_or_spec
        && (test_database || text.contains("db:schema:load"))
        && schema_failure
        && setup_failure
}

fn has_environment_failure_signal(text: &str) -> bool {
    text.contains("environment")
        || text.contains("path")
        || text.contains("not found")
        || text.contains("cannot find")
        || text.contains("missing")
}

fn is_scope_drift(blocker_fingerprint: Option<&str>, text: &str) -> bool {
    blocker_fingerprint
        .map(|value| {
            let trimmed = value.trim().to_ascii_lowercase();
            trimmed.starts_with("scope-drift:") || trimmed.starts_with("ood:")
        })
        .unwrap_or(false)
        || text.contains("scope drift")
        || text.contains("out-of-scope")
}

fn fallback_scope(blocking_scope: &str, source_task_id: Option<&str>) -> (String, String) {
    let scope = normalized_token(blocking_scope, "none");
    if matches!(scope.as_str(), "current_task" | "review_decision" | "merge") {
        let subject = source_task_id
            .map(|value| slug_token(value, "unknown-task"))
            .unwrap_or_else(|| "unknown-task".to_string());
        return ("task".to_string(), subject);
    }
    ("project".to_string(), "project".to_string())
}

fn normalize_signature_text(value: &str) -> String {
    collapse_whitespace(&strip_ansi_and_controls(value).to_ascii_lowercase())
}

fn strip_ansi_and_controls(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_escape = false;
    for ch in value.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if ch == '\u{1b}' {
            in_escape = true;
            continue;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            output.push(' ');
        } else {
            output.push(ch);
        }
    }
    output
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalized_token(value: &str, default_value: &str) -> String {
    let token = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if token.is_empty() {
        default_value.to_string()
    } else {
        token
    }
}

fn slug_token(value: &str, default_value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        default_value.to_string()
    } else {
        slug
    }
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
#[path = "agent_conversation_issue_tests.rs"]
mod tests;
