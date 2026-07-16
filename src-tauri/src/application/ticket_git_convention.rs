//! Provider-neutral ticket Git-convention templates and rendering.

use thiserror::Error;

pub use super::ticket_git_convention_render::disambiguate_branch_name;
use super::ticket_git_convention_render::{
    bound_branch_name, parse_template, render_parts, render_template, validate_template,
};
use crate::domain::integrations::{
    DEFAULT_CLICKUP_BRANCH_NAME_TEMPLATE, DEFAULT_CLICKUP_COMMIT_SUBJECT_TEMPLATE,
    DEFAULT_CLICKUP_PR_TITLE_TEMPLATE,
};

/// Conservative byte limit for the full branch name before `refs/heads/`.
///
/// Keeping the rendered name below a filesystem component's common 255-byte
/// limit also leaves room for the deterministic hash suffix used on truncation.
pub const MAX_TICKET_BRANCH_BYTES: usize = 240;
pub(super) const SHORT_HASH_BYTES: usize = 4;
pub(super) const SHORT_HASH_HEX_LEN: usize = SHORT_HASH_BYTES * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketGitConventionTemplateKind {
    Branch,
    CommitSubject,
    PrTitle,
}

impl std::fmt::Display for TicketGitConventionTemplateKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Branch => formatter.write_str("branch"),
            Self::CommitSubject => formatter.write_str("commit subject"),
            Self::PrTitle => formatter.write_str("PR title"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TicketGitConventionError {
    #[error("{kind} template must contain :taskId:")]
    MissingTaskId {
        kind: TicketGitConventionTemplateKind,
    },
    #[error("{kind} template contains unknown placeholder :{placeholder}:")]
    UnknownPlaceholder {
        kind: TicketGitConventionTemplateKind,
        placeholder: String,
    },
    #[error("{kind} template does not allow :{placeholder}:")]
    PlaceholderNotAllowed {
        kind: TicketGitConventionTemplateKind,
        placeholder: String,
    },
    #[error("missing value for ticket Git placeholder :{placeholder}:")]
    MissingPlaceholderValue { placeholder: String },
    #[error("invalid {kind} template: {reason}")]
    InvalidTemplate {
        kind: TicketGitConventionTemplateKind,
        reason: String,
    },
    #[error("invalid rendered ticket branch: {reason}")]
    InvalidBranch { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placeholder {
    TaskId,
    TaskName,
    Username,
    Summary,
}

impl Placeholder {
    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "taskId" => Some(Self::TaskId),
            "taskName" => Some(Self::TaskName),
            "username" => Some(Self::Username),
            "summary" => Some(Self::Summary),
            _ => None,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::TaskId => "taskId",
            Self::TaskName => "taskName",
            Self::Username => "username",
            Self::Summary => "summary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TemplatePart {
    Literal(String),
    Placeholder(Placeholder),
}

#[derive(Debug, Clone, Copy)]
pub struct TicketGitConventionContext<'a> {
    pub task_id: &'a str,
    pub task_name: &'a str,
    pub username: Option<&'a str>,
    pub summary: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTicketGitConvention {
    pub branch_name: String,
    pub commit_subject: String,
    pub pr_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketGitConventionTemplates {
    branch_name_template: String,
    commit_subject_template: String,
    pr_title_template: String,
}

impl TicketGitConventionTemplates {
    /// Build and validate a complete ticket Git-convention template set.
    pub fn new(
        branch_name_template: impl Into<String>,
        commit_subject_template: impl Into<String>,
        pr_title_template: impl Into<String>,
    ) -> Result<Self, TicketGitConventionError> {
        let templates = Self {
            branch_name_template: branch_name_template.into(),
            commit_subject_template: commit_subject_template.into(),
            pr_title_template: pr_title_template.into(),
        };
        validate_template(
            TicketGitConventionTemplateKind::Branch,
            &templates.branch_name_template,
        )?;
        validate_template(
            TicketGitConventionTemplateKind::CommitSubject,
            &templates.commit_subject_template,
        )?;
        validate_template(
            TicketGitConventionTemplateKind::PrTitle,
            &templates.pr_title_template,
        )?;
        let validation_context = TicketGitConventionContext {
            task_id: "task-id",
            task_name: "task-name",
            username: Some("username"),
            summary: Some("summary"),
        };
        let validation_branch = render_template(
            TicketGitConventionTemplateKind::Branch,
            &templates.branch_name_template,
            &validation_context,
        )?;
        bound_branch_name(&validation_branch)?;
        Ok(templates)
    }

    pub fn clickup_defaults() -> Self {
        Self::new(
            DEFAULT_CLICKUP_BRANCH_NAME_TEMPLATE,
            DEFAULT_CLICKUP_COMMIT_SUBJECT_TEMPLATE,
            DEFAULT_CLICKUP_PR_TITLE_TEMPLATE,
        )
        .expect("built-in ClickUp Git convention templates must be valid")
    }

    pub fn branch_name_template(&self) -> &str {
        &self.branch_name_template
    }

    pub fn commit_subject_template(&self) -> &str {
        &self.commit_subject_template
    }

    pub fn pr_title_template(&self) -> &str {
        &self.pr_title_template
    }

    /// Render the concrete branch, commit example, and PR title.
    pub fn render(
        &self,
        context: &TicketGitConventionContext<'_>,
    ) -> Result<RenderedTicketGitConvention, TicketGitConventionError> {
        let branch_name = render_template(
            TicketGitConventionTemplateKind::Branch,
            &self.branch_name_template,
            context,
        )?;
        let branch_name = bound_branch_name(&branch_name)?;
        let commit_subject = render_template(
            TicketGitConventionTemplateKind::CommitSubject,
            &self.commit_subject_template,
            context,
        )?;
        let pr_title = render_template(
            TicketGitConventionTemplateKind::PrTitle,
            &self.pr_title_template,
            context,
        )?;
        Ok(RenderedTicketGitConvention {
            branch_name,
            commit_subject,
            pr_title,
        })
    }

    /// Check a commit subject against the configured rule.
    ///
    /// `:summary:` is the only dynamic portion: when present once, it must
    /// match at least one non-whitespace character between the rendered prefix
    /// and suffix. Templates without it require an exact subject match.
    pub fn commit_subject_matches(
        &self,
        context: &TicketGitConventionContext<'_>,
        actual_subject: &str,
    ) -> Result<bool, TicketGitConventionError> {
        if actual_subject.is_empty()
            || actual_subject
                .chars()
                .any(|character| character.is_control())
        {
            return Ok(false);
        }
        let parts = parse_template(
            TicketGitConventionTemplateKind::CommitSubject,
            &self.commit_subject_template,
        )?;
        let Some(summary_index) = parts
            .iter()
            .position(|part| matches!(part, TemplatePart::Placeholder(Placeholder::Summary)))
        else {
            return Ok(render_parts(
                TicketGitConventionTemplateKind::CommitSubject,
                &parts,
                context,
            )? == actual_subject);
        };

        let prefix = render_parts(
            TicketGitConventionTemplateKind::CommitSubject,
            &parts[..summary_index],
            context,
        )?;
        let suffix = render_parts(
            TicketGitConventionTemplateKind::CommitSubject,
            &parts[summary_index + 1..],
            context,
        )?;
        if actual_subject.len() < prefix.len() + suffix.len()
            || !actual_subject.starts_with(&prefix)
            || !actual_subject.ends_with(&suffix)
        {
            return Ok(false);
        }
        let dynamic_end = actual_subject.len() - suffix.len();
        Ok(!actual_subject[prefix.len()..dynamic_end].trim().is_empty())
    }
}
