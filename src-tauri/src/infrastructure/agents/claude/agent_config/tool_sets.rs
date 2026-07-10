use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const EMBEDDED_CLAUDE_HARNESS_CONFIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../config/harnesses/claude.yaml"
));

#[derive(Debug, Deserialize)]
struct ClaudeHarnessToolSetsConfig {
    #[serde(default)]
    tool_sets: HashMap<String, Vec<String>>,
}

static CANONICAL_CLAUDE_TOOL_SETS: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();

pub fn canonical_claude_tool_sets() -> &'static HashMap<String, Vec<String>> {
    CANONICAL_CLAUDE_TOOL_SETS.get_or_init(|| {
        let parsed: ClaudeHarnessToolSetsConfig =
            serde_yaml::from_str(EMBEDDED_CLAUDE_HARNESS_CONFIG)
                .expect("embedded config/harnesses/claude.yaml should parse");
        parsed.tool_sets
    })
}
