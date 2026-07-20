use std::fs;
use std::path::PathBuf;

use ralphx_lib::utils::path_safety::{checked_exists, validate_absolute_non_root_path};

use super::env::EnvVarGuard;

pub(crate) struct FakeClaude {
    _spawn_permission: EnvVarGuard,
    _temp_dir: tempfile::TempDir,
    pub(crate) cli_path: PathBuf,
    invocation_path: PathBuf,
}

impl FakeClaude {
    pub(crate) fn new() -> Self {
        let spawn_permission = EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
        let temp_dir = tempfile::tempdir().expect("fake Claude directory should be created");
        let cli_path = temp_dir.path().join("claude");
        let invocation_path = temp_dir.path().join("claude.invoked");
        validate_absolute_non_root_path(temp_dir.path(), "fake Claude root")
            .expect("fake Claude root should be a safe process-owned path");
        validate_absolute_non_root_path(&cli_path, "fake Claude CLI")
            .expect("fake Claude CLI should be a safe process-owned path");
        validate_absolute_non_root_path(&invocation_path, "fake Claude invocation marker")
            .expect("fake Claude invocation marker should be a safe process-owned path");
        fs::write(
            &cli_path,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '%s\n' '2.1.0 (Claude Code)'
  exit 0
fi
if [ "$1" = "--help" ]; then
  printf '%s\n' 'Options: --output-format --input-format --model --permission-mode --resume --agent'
  exit 0
fi
: > "$0.invoked"
cat > /dev/null 2>&1
printf '%s\n' '{"type":"result","session_id":"standalone-claude-test","is_error":false,"result":"Done.","cost_usd":0.0}'
"#,
        )
        .expect("fake Claude CLI should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&cli_path)
                .expect("fake Claude CLI metadata should load")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&cli_path, permissions)
                .expect("fake Claude CLI should be executable");
        }

        Self {
            _spawn_permission: spawn_permission,
            _temp_dir: temp_dir,
            cli_path,
            invocation_path,
        }
    }

    pub(crate) fn was_invoked(&self) -> bool {
        checked_exists(&self.invocation_path, "fake Claude invocation marker")
            .expect("fake Claude invocation marker should remain a safe process-owned path")
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
}

impl Default for FakeClaude {
    fn default() -> Self {
        Self::new()
    }
}
