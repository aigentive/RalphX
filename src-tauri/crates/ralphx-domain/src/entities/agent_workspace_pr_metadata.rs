use serde::{Deserialize, Serialize};

/// Workspace-only decision for metadata on an existing PR. This deliberately
/// does not replace `AgentWorkspacePrDescription`, which is also used by plan PRs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum AgentWorkspacePrMetadataDecision {
    Preserve,
    Patch {
        title: Option<String>,
        body_markdown: Option<String>,
    },
}

impl AgentWorkspacePrMetadataDecision {
    pub fn preserve() -> Self {
        Self::Preserve
    }

    pub fn patch(title: Option<String>, body_markdown: Option<String>) -> Option<Self> {
        let title =
            title.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_string()));
        let body_markdown =
            body_markdown.and_then(|value| (!value.trim().is_empty()).then_some(value));
        (title.is_some() || body_markdown.is_some()).then_some(Self::Patch {
            title,
            body_markdown,
        })
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Preserve => true,
            Self::Patch {
                title,
                body_markdown,
            } => {
                title.as_ref().is_some_and(|value| !value.trim().is_empty())
                    || body_markdown
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty())
            }
        }
    }
}
