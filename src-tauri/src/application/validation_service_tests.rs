use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::validation_service::{
    configure_validation_shell_command_for_test, RunTaskValidationRequest, TaskValidationService,
    ValidationCommandRequest,
};
use crate::application::AppState;
use crate::domain::entities::{Project, Task, ValidationRunStatus};

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn path_index(entries: &[PathBuf], path: impl AsRef<Path>) -> usize {
    entries
        .iter()
        .position(|entry| entry == path.as_ref())
        .unwrap_or_else(|| panic!("PATH entry missing: {}", path.as_ref().display()))
}

fn create_validation_git_repo() -> tempfile::TempDir {
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let run_git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(tmp_dir.path())
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_git(&["init", "-b", "main"]);
    std::fs::write(tmp_dir.path().join("README.md"), "validation\n").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "init"]);
    tmp_dir
}

#[test]
fn validation_shell_command_preserves_user_shims_while_ensuring_node_bin() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin");
    let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");

    let mut command = tokio::process::Command::new("/usr/bin/env");
    configure_validation_shell_command_for_test(&mut command);

    let path_value = command
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
        })
        .expect("PATH env");
    let path_entries = std::env::split_paths(&path_value).collect::<Vec<_>>();

    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        assert!(
            path_index(&path_entries, &cargo_bin) < path_index(&path_entries, "/tmp/fake-node-bin"),
            "user cargo shim should stay before inserted Node bin: {path_value:?}"
        );
    }
    assert!(
        path_index(&path_entries, "/tmp/fake-node-bin") < path_index(&path_entries, "/usr/bin")
    );
}

#[tokio::test]
async fn run_task_validation_settles_run_when_command_cwd_is_invalid() {
    let tmp_dir = create_validation_git_repo();
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Validation".to_string(),
            tmp_dir.path().to_string_lossy().to_string(),
        ))
        .await
        .unwrap();
    let task = state
        .task_repo
        .create(Task::new(project.id, "Validate cwd".to_string()))
        .await
        .unwrap();

    let result = TaskValidationService::run_task_validation(
        &state,
        RunTaskValidationRequest {
            task_id: task.id.as_str().to_string(),
            purpose: None,
            mode: None,
            context_type: None,
            caller_agent: Some("ralphx-execution-worker".to_string()),
            analysis_fingerprint: None,
            commands: vec![ValidationCommandRequest {
                command: "echo should-not-run".to_string(),
                cwd: Some("missing-dir".to_string()),
                label: None,
                category: None,
                reason: None,
                related_files: Vec::new(),
                command_ref: None,
                source: None,
            }],
        },
    )
    .await;

    assert!(result.is_err());
    let latest = state
        .validation_run_repo
        .latest_run_with_results_for_task(&task.id)
        .await
        .unwrap()
        .expect("run should be created before command validation error");
    assert_eq!(latest.run.status, ValidationRunStatus::Error);
    assert!(
        latest.run.completed_at.is_some(),
        "failed validation run must be terminal"
    );
}
