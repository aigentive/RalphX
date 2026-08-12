//! Pending question records shared by the agent question bridge and the
//! question repositories.
//!
//! These are the wire records the MCP question bridge persists and the UI
//! answers. The live coordination container (`QuestionState`, claims, watch
//! channels) stays in the application layer; only the records live here.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Answer provided by the user in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnswer {
    pub selected_options: Vec<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

/// Metadata for a pending question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQuestionInfo {
    pub request_id: String,
    pub session_id: String,
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
    #[serde(default = "default_allow_skip")]
    pub allow_skip: bool,
    pub batch_index: Option<u32>,
    pub batch_total: Option<u32>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default = "default_created_at")]
    pub created_at: String,
}

fn default_allow_skip() -> bool {
    true
}

fn default_created_at() -> String {
    Utc::now().to_rfc3339()
}

/// A single option in a question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}
