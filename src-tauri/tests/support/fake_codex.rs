use std::fs;
use std::path::PathBuf;

use ralphx_lib::utils::path_safety::{checked_exists, validate_absolute_non_root_path};

pub(crate) struct FakeCodex {
    _temp_dir: tempfile::TempDir,
    pub(crate) cli_path: PathBuf,
    invocation_path: PathBuf,
    invocation_args_path: PathBuf,
}

impl FakeCodex {
    pub(crate) fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("fake Codex directory should be created");
        let cli_path = temp_dir.path().join("codex");
        let invocation_path = temp_dir.path().join("codex.invoked");
        let invocation_args_path = temp_dir.path().join("codex.args");
        validate_absolute_non_root_path(temp_dir.path(), "fake Codex root")
            .expect("fake Codex root should be a safe process-owned path");
        validate_absolute_non_root_path(&cli_path, "fake Codex CLI")
            .expect("fake Codex CLI should be a safe process-owned path");
        validate_absolute_non_root_path(&invocation_path, "fake Codex invocation marker")
            .expect("fake Codex invocation marker should be a safe process-owned path");
        validate_absolute_non_root_path(&invocation_args_path, "fake Codex invocation arguments")
            .expect("fake Codex invocation arguments should be a safe process-owned path");
        fs::write(
            &cli_path,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '%s\n' 'codex-cli 0.116.0'
  exit 0
fi
if [ "$1" = "--help" ]; then
  printf '%s\n' 'Commands: exec mcp resume' 'Options: -c --config -m --model -s --sandbox --search --add-dir'
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Usage: codex exec [OPTIONS]' 'Options: -c --config -m --model -s --sandbox --add-dir --json -C --cd --skip-git-repo-check'
  exit 0
fi
printf '%s\n' "$@" > "$0.args"
: > "$0.invoked"
printf '%s\n' '{"type":"thread.started","thread_id":"standalone-codex-test"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"Done."}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}'
"#,
        )
        .expect("fake Codex CLI should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&cli_path)
                .expect("fake Codex CLI metadata should load")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&cli_path, permissions)
                .expect("fake Codex CLI should be executable");
        }

        Self {
            _temp_dir: temp_dir,
            cli_path,
            invocation_path,
            invocation_args_path,
        }
    }

    pub(crate) fn was_invoked(&self) -> bool {
        checked_exists(&self.invocation_path, "fake Codex invocation marker")
            .expect("fake Codex invocation marker should remain a safe process-owned path")
    }

    pub(crate) async fn wait_until_invoked(&self) -> bool {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if self.was_invoked() {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        self.was_invoked()
    }

    pub(crate) fn invocation_args(&self) -> String {
        fs::read_to_string(&self.invocation_args_path)
            .expect("fake Codex invocation arguments should be readable")
    }
}

impl Default for FakeCodex {
    fn default() -> Self {
        Self::new()
    }
}
