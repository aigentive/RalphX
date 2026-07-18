use super::*;
use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind, Project,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::process::Command;
use tauri::Manager;
use tokio::sync::Mutex;

static ENV_MUTEX: Mutex<()> = Mutex::const_new(());

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

fn fake_command_path(command: &WorkspaceOpenCommandDefinition) -> PathBuf {
    PathBuf::from(format!("/tools/{}", command.name))
}

fn target_ids(targets: &[WorkspaceOpenTargetResponse]) -> Vec<&str> {
    targets.iter().map(|target| target.id.as_str()).collect()
}

fn write_fake_command(bin_dir: &Path, command_name: &str, body: &str) -> PathBuf {
    let command = bin_dir.join(command_name);
    std::fs::write(&command, format!("#!/bin/sh\n{body}\n")).expect("write fake command");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&command)
            .expect("fake command metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&command, permissions).expect("mark fake command executable");
    }
    command
}

fn write_fake_cursor(bin_dir: &Path, body: &str) -> PathBuf {
    write_fake_command(bin_dir, "cursor", body)
}

fn static_fixed_candidates(path: &Path) -> &'static [&'static str] {
    let candidate = Box::leak(path.to_string_lossy().into_owned().into_boxed_str());
    Box::leak(vec![candidate as &'static str].into_boxed_slice())
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_workspace_open_repo(repo_path: &Path) {
    std::fs::create_dir_all(repo_path).expect("create repo");
    git(repo_path, &["init", "-b", "main"]);
    git(repo_path, &["config", "user.email", "test@example.com"]);
    git(repo_path, &["config", "user.name", "Test User"]);
    std::fs::write(repo_path.join("README.md"), "workspace open\n").expect("write readme");
    git(repo_path, &["add", "README.md"]);
    git(repo_path, &["commit", "-m", "initial"]);
}

fn workspace_open_command_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

async fn seed_workspace_open_state(
    repo_path: &Path,
    worktree_parent: &Path,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    ChatConversation,
    PathBuf,
) {
    let state = AppState::new_test();
    setup_workspace_open_repo(repo_path);
    let mut project = Project::new(
        "Workspace open project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("conversation should persist");
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should prepare");
    let workspace_path = PathBuf::from(&workspace.worktree_path);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    (
        workspace_open_command_app(state),
        conversation,
        workspace_path,
    )
}

#[test]
fn available_targets_include_only_resolved_tools() {
    let targets = available_workspace_open_targets_with_resolver("macos", |command| {
        matches!(command.name, "cursor" | "open").then(|| fake_command_path(command))
    });

    assert_eq!(
        targets,
        vec![
            WorkspaceOpenTargetResponse {
                id: "cursor".to_string(),
                label: "Cursor".to_string(),
                kind: WorkspaceOpenTargetKind::Editor,
            },
            WorkspaceOpenTargetResponse {
                id: "file-manager".to_string(),
                label: "Finder".to_string(),
                kind: WorkspaceOpenTargetKind::FileManager,
            },
        ]
    );
}

#[test]
fn platform_file_manager_targets_use_native_labels_and_commands() {
    let linux = file_manager_target("linux");
    assert_eq!(linux.id, "file-manager");
    assert_eq!(linux.label, "Files");
    assert_eq!(linux.kind, WorkspaceOpenTargetKind::FileManager);
    assert_eq!(linux.commands[0].name, "xdg-open");

    let macos = file_manager_target("macos");
    assert_eq!(macos.label, "Finder");
    assert_eq!(macos.commands[0].name, "open");

    let windows = file_manager_target("windows");
    assert_eq!(windows.label, "Explorer");
    assert_eq!(windows.commands[0].name, "explorer.exe");
}

#[test]
fn target_definition_iterators_include_editor_aliases_and_file_manager() {
    let target_ids = workspace_open_target_definitions("macos")
        .map(|target| target.id)
        .collect::<Vec<_>>();
    assert!(target_ids.contains(&"cursor"));
    assert!(target_ids.contains(&"zed"));
    assert!(target_ids.contains(&"file-manager"));

    let command_names = workspace_open_command_definitions("macos")
        .map(|command| command.name)
        .collect::<Vec<_>>();
    assert!(command_names.contains(&"zed"));
    assert!(command_names.contains(&"zeditor"));
    assert!(command_names.contains(&"open"));
}

#[test]
fn macos_terminal_targets_are_ordered_before_file_manager_and_platform_isolated() {
    let macos_ids = workspace_open_target_definitions("macos")
        .map(|target| target.id)
        .collect::<Vec<_>>();
    assert_eq!(
        &macos_ids[macos_ids.len() - 3..],
        &["iterm2", "terminal", "file-manager"]
    );

    for platform in ["linux", "windows"] {
        let ids = workspace_open_target_definitions(platform)
            .map(|target| target.id)
            .collect::<Vec<_>>();
        assert!(!ids.contains(&"iterm2"), "unexpected iTerm2 on {platform}");
        assert!(
            !ids.contains(&"terminal"),
            "unexpected Terminal on {platform}"
        );
    }
}

#[test]
fn macos_terminal_availability_requires_independent_app_probes() {
    let launcher_only = available_workspace_open_targets_with_resolver("macos", |command| {
        (command.name == "open").then(|| fake_command_path(command))
    });
    assert_eq!(target_ids(&launcher_only), vec!["file-manager"]);

    let with_apps = available_workspace_open_targets_with_resolver("macos", |command| {
        matches!(command.name, "iterm2-app" | "terminal-app" | "open")
            .then(|| fake_command_path(command))
    });
    assert_eq!(
        target_ids(&with_apps),
        vec!["iterm2", "terminal", "file-manager"]
    );
    assert_eq!(with_apps[0].kind, WorkspaceOpenTargetKind::Terminal);
    assert_eq!(with_apps[1].kind, WorkspaceOpenTargetKind::Terminal);
}

#[test]
fn first_available_command_uses_later_alias_when_primary_is_missing() {
    let zed = find_target("zed", "macos").expect("zed target");
    let (command, path) = first_available_command(zed, &mut |command| {
        (command.name == "zeditor").then(|| fake_command_path(command))
    })
    .expect("zeditor alias should resolve");

    assert_eq!(command.name, "zeditor");
    assert_eq!(path, PathBuf::from("/tools/zeditor"));
}

#[test]
fn build_launch_adds_editor_base_args() {
    let temp = tempfile::tempdir().expect("tempdir");
    let launch = build_workspace_open_launch("kiro", temp.path(), "macos", |command| {
        (command.name == "kiro").then(|| fake_command_path(command))
    })
    .expect("launch should build");

    assert_eq!(launch.command, PathBuf::from("/tools/kiro"));
    assert_eq!(
        launch.args,
        vec![
            OsString::from("ide"),
            temp.path().as_os_str().to_os_string(),
        ]
    );
}

#[test]
fn build_launch_uses_bundle_identifier_for_external_terminals() {
    let temp = tempfile::tempdir().expect("tempdir");

    let iterm = build_workspace_open_launch("iterm2", temp.path(), "macos", |command| {
        matches!(command.name, "iterm2-app" | "open").then(|| fake_command_path(command))
    })
    .expect("iTerm2 launch should build");
    assert_eq!(iterm.command, PathBuf::from("/tools/open"));
    assert_eq!(
        iterm.args,
        vec![
            OsString::from("-b"),
            OsString::from("com.googlecode.iterm2"),
            temp.path().as_os_str().to_os_string(),
        ]
    );

    let terminal = build_workspace_open_launch("terminal", temp.path(), "macos", |command| {
        matches!(command.name, "terminal-app" | "open").then(|| fake_command_path(command))
    })
    .expect("Terminal launch should build");
    assert_eq!(terminal.command, PathBuf::from("/tools/open"));
    assert_eq!(
        terminal.args,
        vec![
            OsString::from("-b"),
            OsString::from("com.apple.Terminal"),
            temp.path().as_os_str().to_os_string(),
        ]
    );
}

#[test]
fn build_terminal_item_launch_opens_file_parent_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("src");
    std::fs::create_dir(&src_dir).expect("create src dir");
    let file_path = src_dir.join("lib.rs");
    std::fs::write(&file_path, "pub fn f() {}\n").expect("write file");

    let launch = build_workspace_open_item_launch("terminal", &file_path, "macos", |command| {
        matches!(command.name, "terminal-app" | "open").then(|| fake_command_path(command))
    })
    .expect("Terminal item launch should build");

    assert_eq!(launch.command, PathBuf::from("/tools/open"));
    assert_eq!(
        launch.args,
        vec![
            OsString::from("-b"),
            OsString::from("com.apple.Terminal"),
            src_dir.as_os_str().to_os_string(),
        ]
    );
}

#[test]
fn build_terminal_launch_rejects_missing_launcher_after_app_probe_succeeds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let error = build_workspace_open_launch("iterm2", temp.path(), "macos", |command| {
        (command.name == "iterm2-app").then(|| fake_command_path(command))
    })
    .expect_err("missing launcher should be rejected");

    assert!(error
        .to_string()
        .contains("Workspace open target launcher is not available: iTerm2"));
}

#[test]
fn build_item_launch_opens_files_with_editor_targets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("src.rs");
    std::fs::write(&file_path, "fn main() {}\n").expect("write file");

    let launch = build_workspace_open_item_launch("cursor", &file_path, "macos", |command| {
        (command.name == "cursor").then(|| fake_command_path(command))
    })
    .expect("launch should build");

    assert_eq!(launch.command, PathBuf::from("/tools/cursor"));
    assert_eq!(launch.args, vec![file_path.as_os_str().to_os_string()]);
}

#[test]
fn build_item_launch_opens_directories_with_file_manager_targets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("src");
    std::fs::create_dir(&src_dir).expect("create src dir");

    let launch = build_workspace_open_item_launch("file-manager", &src_dir, "macos", |command| {
        (command.name == "open").then(|| fake_command_path(command))
    })
    .expect("launch should build");

    assert_eq!(launch.command, PathBuf::from("/tools/open"));
    assert_eq!(launch.args, vec![src_dir.as_os_str().to_os_string()]);
}

#[test]
fn build_item_launch_rejects_missing_item_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing_path = temp.path().join("missing.rs");

    let error = build_workspace_open_item_launch("cursor", &missing_path, "macos", |command| {
        (command.name == "cursor").then(|| fake_command_path(command))
    })
    .expect_err("missing item should be rejected");

    assert!(error.to_string().contains("existing file or directory"));
}

#[test]
fn build_item_launch_reveals_files_with_macos_file_manager() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("src.rs");
    std::fs::write(&file_path, "fn main() {}\n").expect("write file");

    let launch = build_workspace_open_item_launch("file-manager", &file_path, "macos", |command| {
        (command.name == "open").then(|| fake_command_path(command))
    })
    .expect("launch should build");

    assert_eq!(launch.command, PathBuf::from("/tools/open"));
    assert_eq!(
        launch.args,
        vec![OsString::from("-R"), file_path.as_os_str().to_os_string()]
    );
}

#[test]
fn build_item_launch_opens_parent_for_linux_file_manager_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("src.rs");
    std::fs::write(&file_path, "fn main() {}\n").expect("write file");

    let launch = build_workspace_open_item_launch("file-manager", &file_path, "linux", |command| {
        (command.name == "xdg-open").then(|| fake_command_path(command))
    })
    .expect("launch should build");

    assert_eq!(launch.command, PathBuf::from("/tools/xdg-open"));
    assert_eq!(launch.args, vec![temp.path().as_os_str().to_os_string()]);
}

#[test]
fn build_item_launch_selects_files_with_windows_file_manager() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("src.rs");
    std::fs::write(&file_path, "fn main() {}\n").expect("write file");
    let mut select_arg = OsString::from("/select,");
    select_arg.push(file_path.as_os_str());

    let launch =
        build_workspace_open_item_launch("file-manager", &file_path, "windows", |command| {
            (command.name == "explorer.exe").then(|| fake_command_path(command))
        })
        .expect("launch should build");

    assert_eq!(launch.command, PathBuf::from("/tools/explorer.exe"));
    assert_eq!(launch.args, vec![select_arg]);
}

#[test]
fn resolve_workspace_open_item_accepts_relative_paths_inside_workspace() {
    let temp = tempfile::tempdir().expect("tempdir");
    let src_dir = temp.path().join("src");
    std::fs::create_dir(&src_dir).expect("create src dir");
    let file_path = src_dir.join("lib.rs");
    std::fs::write(&file_path, "pub fn f() {}\n").expect("write file");

    let resolved = resolve_workspace_open_item_path(temp.path(), Path::new("src/lib.rs"))
        .expect("relative path should resolve");

    assert_eq!(resolved, file_path.canonicalize().expect("canonical file"));
}

#[test]
fn resolve_workspace_open_item_rejects_absolute_paths_outside_workspace() {
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    let file_path = outside.path().join("lib.rs");
    std::fs::write(&file_path, "pub fn f() {}\n").expect("write file");

    let error = resolve_workspace_open_item_path(workspace.path(), &file_path)
        .expect_err("outside path should be rejected");

    assert!(error
        .to_string()
        .contains("outside the conversation workspace"));
}

#[cfg(unix)]
#[test]
fn resolve_workspace_open_item_rejects_symlink_escape() {
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    let outside_file = outside.path().join("lib.rs");
    std::fs::write(&outside_file, "pub fn f() {}\n").expect("write file");
    let symlink_path = workspace.path().join("linked.rs");
    std::os::unix::fs::symlink(&outside_file, &symlink_path).expect("create symlink");

    let error = resolve_workspace_open_item_path(workspace.path(), Path::new("linked.rs"))
        .expect_err("symlink escape should be rejected");

    assert!(error
        .to_string()
        .contains("outside the conversation workspace"));
}

#[test]
fn build_launch_rejects_relative_workspace_paths() {
    let error = build_workspace_open_launch("cursor", Path::new("relative"), "macos", |command| {
        (command.name == "cursor").then(|| fake_command_path(command))
    })
    .expect_err("relative path should be rejected");

    assert!(error.to_string().contains("absolute"));
}

#[test]
fn build_launch_rejects_existing_file_workspace_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let file_path = temp.path().join("not-a-dir");
    std::fs::write(&file_path, "workspace").expect("write file");

    let error = build_workspace_open_launch("cursor", &file_path, "macos", |command| {
        (command.name == "cursor").then(|| fake_command_path(command))
    })
    .expect_err("file path should be rejected");

    assert!(error.to_string().contains("existing directory"));
}

#[test]
fn build_launch_rejects_unknown_targets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let error = build_workspace_open_launch("unknown", temp.path(), "macos", |_| None)
        .expect_err("unknown target should be rejected");

    assert!(error.to_string().contains("Unknown workspace open target"));
}

#[test]
fn build_launch_rejects_unavailable_targets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let error = build_workspace_open_launch("cursor", temp.path(), "macos", |_| None)
        .expect_err("unavailable target should be rejected");

    assert!(error
        .to_string()
        .contains("Workspace open target is not available: Cursor"));
}

#[test]
fn windows_file_manager_uses_explorer_exe_command_name() {
    let targets = available_workspace_open_targets_with_resolver("windows", |command| {
        (command.name == "explorer.exe").then(|| fake_command_path(command))
    });

    assert_eq!(
        targets,
        vec![WorkspaceOpenTargetResponse {
            id: "file-manager".to_string(),
            label: "Explorer".to_string(),
            kind: WorkspaceOpenTargetKind::FileManager,
        }]
    );
}

#[test]
fn shell_probe_results_do_not_override_direct_resolution() {
    let targets = probe_workspace_open_targets(
        "macos",
        |command| {
            matches!(command.name, "cursor" | "open")
                .then(|| PathBuf::from(format!("/direct/{}", command.name)))
        },
        |_| HashMap::from([("cursor", PathBuf::from("/shell/cursor"))]),
    );

    assert_eq!(target_ids(&targets), vec!["cursor", "file-manager"]);
}

#[test]
fn full_probe_uses_one_batched_shell_fallback_for_unresolved_commands() {
    let shell_called = Cell::new(false);
    let targets = probe_workspace_open_targets(
        "macos",
        |command| (command.name == "open").then(|| fake_command_path(command)),
        |command_names| {
            shell_called.set(true);
            assert!(command_names.contains(&"cursor"));
            assert!(command_names.contains(&"webstorm"));
            assert!(!command_names.contains(&"iterm2-app"));
            assert!(!command_names.contains(&"terminal-app"));
            HashMap::from([("cursor", PathBuf::from("/tools/cursor"))])
        },
    );

    assert!(shell_called.get());
    assert_eq!(
        targets,
        vec![
            WorkspaceOpenTargetResponse {
                id: "cursor".to_string(),
                label: "Cursor".to_string(),
                kind: WorkspaceOpenTargetKind::Editor,
            },
            WorkspaceOpenTargetResponse {
                id: "file-manager".to_string(),
                label: "Finder".to_string(),
                kind: WorkspaceOpenTargetKind::FileManager,
            },
        ]
    );
}

#[test]
fn direct_resolution_uses_fixed_binaries_and_cli_paths_without_shell() {
    let _lock = ENV_MUTEX.blocking_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    let terminal_app = write_fake_command(&bin_dir, "terminal-app", "exit 0");
    let cursor = write_fake_cursor(&bin_dir, "exit 0");
    let _path = EnvGuard::set_os("PATH", &bin_dir);

    let fixed_command = WorkspaceOpenCommandDefinition {
        name: "terminal-app",
        fixed_candidates: static_fixed_candidates(&terminal_app),
        base_args: &[],
    };
    assert_eq!(
        resolve_workspace_open_command_without_shell(&fixed_command),
        Some(terminal_app.clone())
    );
    assert_eq!(
        resolve_workspace_open_command(&fixed_command),
        Some(terminal_app)
    );

    let cli_command = WorkspaceOpenCommandDefinition {
        name: "cursor",
        fixed_candidates: &[],
        base_args: &[],
    };
    assert_eq!(
        resolve_workspace_open_command_without_shell(&cli_command),
        Some(cursor)
    );
}

#[test]
fn cached_target_list_is_returned_without_reprobing() {
    let cached = vec![WorkspaceOpenTargetResponse {
        id: "cursor".to_string(),
        label: "Cursor".to_string(),
        kind: WorkspaceOpenTargetKind::Editor,
    }];
    store_workspace_open_target_cache(cached.clone());

    assert_eq!(cached_workspace_open_targets(), Some(cached.clone()));
    assert_eq!(list_workspace_open_targets(), cached);
}

#[test]
fn launch_workspace_open_target_accepts_immediate_success() {
    let _lock = ENV_MUTEX.blocking_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    write_fake_cursor(&bin_dir, "exit 0");
    let _path = EnvGuard::set_os("PATH", &bin_dir);

    launch_workspace_open_target("cursor", temp.path()).expect("launch should succeed");
}

#[test]
fn launch_workspace_open_target_reports_spawn_failures() {
    let _lock = ENV_MUTEX.blocking_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    let cursor = bin_dir.join("cursor");
    std::fs::write(&cursor, "#!/definitely/missing/ralphx-shell\n").expect("write fake cursor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&cursor)
            .expect("fake cursor metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&cursor, permissions).expect("mark fake cursor executable");
    }
    let _path = EnvGuard::set_os("PATH", &bin_dir);

    let error = launch_workspace_open_target("cursor", temp.path())
        .expect_err("spawn failure should be rejected");

    assert!(error
        .to_string()
        .contains("Failed to launch workspace open target"));
}

#[test]
fn launch_workspace_open_target_accepts_background_process() {
    let _lock = ENV_MUTEX.blocking_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    write_fake_cursor(&bin_dir, "sleep 1\nexit 0");
    let _path = EnvGuard::set_os("PATH", &bin_dir);

    launch_workspace_open_target("cursor", temp.path()).expect("launch should spawn");
}

#[test]
fn launch_workspace_open_target_returns_builder_errors_before_spawning() {
    let error = launch_workspace_open_target("cursor", Path::new("relative"))
        .expect_err("invalid workspace path should be rejected before spawn");

    assert!(error.to_string().contains("absolute"));
}

#[test]
fn launch_workspace_open_item_target_accepts_immediate_success() {
    let _lock = ENV_MUTEX.blocking_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    let file_path = temp.path().join("src.rs");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    std::fs::write(&file_path, "fn main() {}\n").expect("write file");
    write_fake_cursor(
        &bin_dir,
        r#"if [ "$1" != "$RALPHX_EXPECTED_ITEM" ]; then exit 42; fi
exit 0"#,
    );
    let _path = EnvGuard::set_os("PATH", &bin_dir);
    let _expected_item = EnvGuard::set_os("RALPHX_EXPECTED_ITEM", &file_path);

    launch_workspace_open_item_target("cursor", &file_path).expect("launch should succeed");
}

#[test]
fn launch_workspace_open_item_target_reports_spawn_failures() {
    let _lock = ENV_MUTEX.blocking_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    let file_path = temp.path().join("src.rs");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    std::fs::write(&file_path, "fn main() {}\n").expect("write file");
    let cursor = bin_dir.join("cursor");
    std::fs::write(&cursor, "#!/definitely/missing/ralphx-shell\n").expect("write fake cursor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&cursor)
            .expect("fake cursor metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&cursor, permissions).expect("mark fake cursor executable");
    }
    let _path = EnvGuard::set_os("PATH", &bin_dir);

    let error = launch_workspace_open_item_target("cursor", &file_path)
        .expect_err("spawn failure should be rejected");

    assert!(error
        .to_string()
        .contains("Failed to launch workspace open path target"));
}

#[test]
fn launch_workspace_open_item_target_accepts_background_process() {
    let _lock = ENV_MUTEX.blocking_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    let file_path = temp.path().join("src.rs");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    std::fs::write(&file_path, "fn main() {}\n").expect("write file");
    write_fake_cursor(&bin_dir, "sleep 1\nexit 0");
    let _path = EnvGuard::set_os("PATH", &bin_dir);

    launch_workspace_open_item_target("cursor", &file_path).expect("launch should spawn");
}

#[test]
fn launch_workspace_open_item_target_returns_builder_errors_before_spawning() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing_path = temp.path().join("missing.rs");

    let error = launch_workspace_open_item_target("cursor", &missing_path)
        .expect_err("missing item path should be rejected before spawn");

    assert!(error.to_string().contains("existing file or directory"));
}

#[tokio::test]
async fn open_agent_conversation_workspace_command_launches_resolved_workspace() {
    let _lock = ENV_MUTEX.lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    write_fake_cursor(
        &bin_dir,
        r#"if [ "$1" != "$RALPHX_EXPECTED_WORKSPACE" ]; then exit 42; fi
exit 0"#,
    );
    let (app, conversation, workspace_dir) =
        seed_workspace_open_state(&repo_dir, &worktree_parent).await;
    let _path = EnvGuard::set_os("PATH", &bin_dir);
    let _expected_workspace = EnvGuard::set_os("RALPHX_EXPECTED_WORKSPACE", &workspace_dir);

    open_agent_conversation_workspace(
        conversation.id.as_str().to_string(),
        "cursor".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("workspace command should launch fake cursor");
}

#[tokio::test]
async fn open_agent_conversation_workspace_path_command_resolves_relative_items() {
    let _lock = ENV_MUTEX.lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    write_fake_cursor(
        &bin_dir,
        r#"if [ "$1" != "$RALPHX_EXPECTED_ITEM" ]; then exit 42; fi
exit 0"#,
    );
    let (app, conversation, workspace_dir) =
        seed_workspace_open_state(&repo_dir, &worktree_parent).await;
    let src_dir = workspace_dir.join("src");
    let file_path = src_dir.join("main.rs");
    std::fs::create_dir_all(&src_dir).expect("create src dir");
    std::fs::write(&file_path, "fn main() {}\n").expect("write source file");
    let _path = EnvGuard::set_os("PATH", &bin_dir);
    let _expected_item = EnvGuard::set_os("RALPHX_EXPECTED_ITEM", &file_path);

    open_agent_conversation_workspace_path(
        conversation.id.as_str().to_string(),
        "cursor".to_string(),
        "src/main.rs".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("workspace path command should launch fake cursor");
}
