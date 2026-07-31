use std::path::Path;

use super::claude_runtime_config;
use super::cli_capabilities::{
    claude_cli_supports_partial_messages, claude_cli_supports_thinking_display,
};

pub(crate) fn shared_streaming_cli_args(cli_path: &Path) -> Vec<String> {
    let mut args = Vec::new();

    // Optional setting-sources override from config/ralphx.yaml.
    if let Some(sources) = &claude_runtime_config().setting_sources {
        if !sources.is_empty() {
            args.extend(["--setting-sources".to_string(), sources.join(",")]);
        }
    }

    // Temporary hardening: disable slash-command skill loading to avoid
    // startup JSON parse crashes in Claude's skill initialization path.
    args.push("--disable-slash-commands".to_string());

    // Stream structured output; --verbose is required for stream-json with -p.
    args.extend(["--output-format".to_string(), "stream-json".to_string()]);
    args.push("--verbose".to_string());

    if claude_cli_supports_partial_messages(cli_path) {
        args.push("--include-partial-messages".to_string());
    }
    if claude_cli_supports_thinking_display(cli_path) {
        args.extend(["--thinking-display".to_string(), "summarized".to_string()]);
    }

    args
}
