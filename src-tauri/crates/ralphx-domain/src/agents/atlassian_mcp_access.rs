//! Role-tiered access model for the built-in Atlassian MCP tools.
//!
//! The tier is never persisted: it is derived at spawn time (tool visibility) and
//! re-derived per request (enforcement), so lowering a role or disabling the
//! Atlassian integration takes effect immediately for enforcement.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use super::RoutingRole;

/// How much of the Atlassian MCP tool surface a routing role may use.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AtlassianMcpAccess {
    /// No Atlassian tools at all. Also the fail-closed result of every
    /// resolution error.
    #[default]
    None,
    /// Read-only Jira/Confluence tools.
    Read,
    /// Read-only tools plus mutating Jira/Confluence tools.
    ReadWrite,
}

impl AtlassianMcpAccess {
    pub const fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Read => "read",
            Self::ReadWrite => "read_write",
        }
    }

    /// Whether this tier may call read-only tools.
    pub const fn allows_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    /// Whether this tier may call mutating tools.
    pub const fn allows_write(self) -> bool {
        matches!(self, Self::ReadWrite)
    }

    /// Bare snake_case MCP tool names this tier grants.
    pub fn granted_tools(self) -> Vec<&'static str> {
        match self {
            Self::None => Vec::new(),
            Self::Read => ATLASSIAN_READ_TOOLS.to_vec(),
            Self::ReadWrite => ATLASSIAN_READ_TOOLS
                .iter()
                .chain(ATLASSIAN_WRITE_TOOLS.iter())
                .copied()
                .collect(),
        }
    }
}

impl fmt::Display for AtlassianMcpAccess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

impl FromStr for AtlassianMcpAccess {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "read" => Ok(Self::Read),
            "read_write" => Ok(Self::ReadWrite),
            other => Err(format!("Invalid Atlassian MCP access '{other}'")),
        }
    }
}

/// Read-only Atlassian MCP tools, bare snake_case as used in `--allowed-tools`
/// and Codex `enabled_tools`.
pub const ATLASSIAN_READ_TOOLS: &[&str] = &[
    "jira_search_issues",
    "jira_get_issue",
    "jira_list_projects",
    "jira_list_transitions",
    "confluence_search_pages",
    "confluence_get_page",
    // Method-gated: read tier may issue GET/HEAD only. The backend enforces the
    // method-to-tier mapping per request.
    "atlassian_api_request",
];

/// Mutating Atlassian MCP tools, granted only at [`AtlassianMcpAccess::ReadWrite`].
pub const ATLASSIAN_WRITE_TOOLS: &[&str] = &[
    "jira_create_issue",
    "jira_update_issue",
    "jira_add_comment",
    "jira_transition_issue",
    "jira_assign_issue",
    "confluence_create_page",
    "confluence_update_page",
];

/// Built-in Atlassian access tier for a routing role, before any
/// `manual_role_defaults` override and before the integration gate.
///
/// The match is exhaustive on purpose: a new routing role must make an explicit
/// read/write decision rather than inherit a catch-all.
pub const fn default_atlassian_access(role: RoutingRole) -> AtlassianMcpAccess {
    use AtlassianMcpAccess::{Read, ReadWrite};

    match role {
        // Roles that may be asked to write back to Jira/Confluence as part of
        // the work they are doing.
        RoutingRole::WorkspaceEdit
        | RoutingRole::WorkspacePrFixer
        | RoutingRole::ExecutionWorker
        | RoutingRole::ExecutionReexecutor => ReadWrite,

        RoutingRole::WorkspaceChat
        | RoutingRole::WorkspacePlan
        | RoutingRole::WorkspaceIdeation
        | RoutingRole::WorkspaceReviewPr
        | RoutingRole::WorkspaceAutomation
        | RoutingRole::AutomationPlanJudge
        | RoutingRole::AutomationResultJudge
        | RoutingRole::WorkspaceReviewer
        | RoutingRole::WorkspaceRepair
        | RoutingRole::WorkspaceMergeRepair
        | RoutingRole::IdeationPrimary
        | RoutingRole::IdeationVerifier
        | RoutingRole::IdeationSubagent
        | RoutingRole::IdeationVerifierSubagent
        | RoutingRole::DelegatedSubagent
        | RoutingRole::ExecutionQaPrep
        | RoutingRole::ExecutionQaRefiner
        | RoutingRole::ExecutionQaTester
        | RoutingRole::ExecutionReviewer
        | RoutingRole::ExecutionMerger
        | RoutingRole::UtilityLightweight
        | RoutingRole::UtilityPrDescriber
        | RoutingRole::UtilityProjectAnalyzer
        | RoutingRole::MemoryCapture
        | RoutingRole::MemoryMaintainer => Read,
    }
}
