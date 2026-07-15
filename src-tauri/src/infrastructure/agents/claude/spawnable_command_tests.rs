use super::SpawnableCommand;
use std::path::PathBuf;
use tokio::process::Command;

#[test]
fn spawnable_command_debug_redacts_persona_bearing_arguments_and_stdin() {
    const PERSONA_BODY: &str = "PRIVATE PERSONA BODY: always answer as the moon librarian";

    let mut command = Command::new("/fake/claude");
    command.arg("--append-system-prompt");
    command.arg(PERSONA_BODY);
    let spawnable = SpawnableCommand::new(command, Some(PERSONA_BODY.to_string()))
        .with_prompt_arg_debug_redaction(1, PathBuf::from("/tmp/ralphx-prompt.log"));

    let debug = format!("{spawnable:?}");

    assert!(debug.contains("args_count"));
    assert!(debug.contains("has_prompt_artifact: true"));
    assert!(debug.contains("/tmp/ralphx-prompt.log"));
    assert!(debug.contains("stdin_prompt_redacted: true"));
    assert!(!debug.contains(PERSONA_BODY));
    assert!(!debug.contains("stdin_prompt_preview"));
}
