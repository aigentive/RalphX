use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::Serialize;
use tauri::State;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path_for_send;
use crate::application::AppState;
use crate::domain::entities::ChatConversationId;
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::{
    find_launchable_cli_path, find_launchable_cli_path_without_shell,
    find_launchable_cli_paths_with_login_shell,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceOpenTargetKind {
    Editor,
    FileManager,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOpenTargetResponse {
    pub id: String,
    pub label: String,
    pub kind: WorkspaceOpenTargetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceOpenLaunchStyle {
    Path,
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceOpenCommandDefinition {
    name: &'static str,
    fixed_candidates: &'static [&'static str],
    base_args: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceOpenTargetDefinition {
    id: &'static str,
    label: &'static str,
    kind: WorkspaceOpenTargetKind,
    launch_style: WorkspaceOpenLaunchStyle,
    commands: &'static [WorkspaceOpenCommandDefinition],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceOpenLaunch {
    command: PathBuf,
    args: Vec<OsString>,
}

static WORKSPACE_OPEN_TARGET_CACHE: OnceLock<Mutex<Option<Vec<WorkspaceOpenTargetResponse>>>> =
    OnceLock::new();
static WORKSPACE_OPEN_TARGET_REFRESH_IN_FLIGHT: OnceLock<Mutex<bool>> = OnceLock::new();

const CURSOR_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "cursor",
    fixed_candidates: &["/opt/homebrew/bin/cursor", "/usr/local/bin/cursor"],
    base_args: &[],
}];
const TRAE_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "trae",
    fixed_candidates: &["/opt/homebrew/bin/trae", "/usr/local/bin/trae"],
    base_args: &[],
}];
const KIRO_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "kiro",
    fixed_candidates: &["/opt/homebrew/bin/kiro", "/usr/local/bin/kiro"],
    base_args: &["ide"],
}];
const VSCODE_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "code",
    fixed_candidates: &[
        "/opt/homebrew/bin/code",
        "/usr/local/bin/code",
        "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
    ],
    base_args: &[],
}];
const VSCODE_INSIDERS_COMMANDS: &[WorkspaceOpenCommandDefinition] =
    &[WorkspaceOpenCommandDefinition {
        name: "code-insiders",
        fixed_candidates: &[
            "/opt/homebrew/bin/code-insiders",
            "/usr/local/bin/code-insiders",
            "/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/bin/code-insiders",
        ],
        base_args: &[],
    }];
const VSCODIUM_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "codium",
    fixed_candidates: &[
        "/opt/homebrew/bin/codium",
        "/usr/local/bin/codium",
        "/Applications/VSCodium.app/Contents/Resources/app/bin/codium",
    ],
    base_args: &[],
}];
const ZED_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[
    WorkspaceOpenCommandDefinition {
        name: "zed",
        fixed_candidates: &["/opt/homebrew/bin/zed", "/usr/local/bin/zed"],
        base_args: &[],
    },
    WorkspaceOpenCommandDefinition {
        name: "zeditor",
        fixed_candidates: &["/opt/homebrew/bin/zeditor", "/usr/local/bin/zeditor"],
        base_args: &[],
    },
];
const ANTIGRAVITY_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "agy",
    fixed_candidates: &["/opt/homebrew/bin/agy", "/usr/local/bin/agy"],
    base_args: &[],
}];
const IDEA_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "idea",
    fixed_candidates: &["/opt/homebrew/bin/idea", "/usr/local/bin/idea"],
    base_args: &[],
}];
const AQUA_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "aqua",
    fixed_candidates: &["/opt/homebrew/bin/aqua", "/usr/local/bin/aqua"],
    base_args: &[],
}];
const CLION_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "clion",
    fixed_candidates: &["/opt/homebrew/bin/clion", "/usr/local/bin/clion"],
    base_args: &[],
}];
const DATAGRIP_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "datagrip",
    fixed_candidates: &["/opt/homebrew/bin/datagrip", "/usr/local/bin/datagrip"],
    base_args: &[],
}];
const DATASPELL_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "dataspell",
    fixed_candidates: &["/opt/homebrew/bin/dataspell", "/usr/local/bin/dataspell"],
    base_args: &[],
}];
const GOLAND_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "goland",
    fixed_candidates: &["/opt/homebrew/bin/goland", "/usr/local/bin/goland"],
    base_args: &[],
}];
const PHPSTORM_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "phpstorm",
    fixed_candidates: &["/opt/homebrew/bin/phpstorm", "/usr/local/bin/phpstorm"],
    base_args: &[],
}];
const PYCHARM_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "pycharm",
    fixed_candidates: &["/opt/homebrew/bin/pycharm", "/usr/local/bin/pycharm"],
    base_args: &[],
}];
const RIDER_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "rider",
    fixed_candidates: &["/opt/homebrew/bin/rider", "/usr/local/bin/rider"],
    base_args: &[],
}];
const RUBYMINE_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "rubymine",
    fixed_candidates: &["/opt/homebrew/bin/rubymine", "/usr/local/bin/rubymine"],
    base_args: &[],
}];
const RUSTROVER_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "rustrover",
    fixed_candidates: &["/opt/homebrew/bin/rustrover", "/usr/local/bin/rustrover"],
    base_args: &[],
}];
const WEBSTORM_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "webstorm",
    fixed_candidates: &["/opt/homebrew/bin/webstorm", "/usr/local/bin/webstorm"],
    base_args: &[],
}];
const FINDER_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "open",
    fixed_candidates: &["/usr/bin/open"],
    base_args: &[],
}];
const EXPLORER_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "explorer.exe",
    fixed_candidates: &[
        "C:\\Windows\\explorer.exe",
        "C:\\Windows\\System32\\explorer.exe",
    ],
    base_args: &[],
}];
const FILES_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "xdg-open",
    fixed_candidates: &["/usr/bin/xdg-open", "/bin/xdg-open"],
    base_args: &[],
}];

const EDITOR_TARGETS: &[WorkspaceOpenTargetDefinition] = &[
    target("cursor", "Cursor", CURSOR_COMMANDS),
    target("trae", "Trae", TRAE_COMMANDS),
    target("kiro", "Kiro", KIRO_COMMANDS),
    target("vscode", "VS Code", VSCODE_COMMANDS),
    target(
        "vscode-insiders",
        "VS Code Insiders",
        VSCODE_INSIDERS_COMMANDS,
    ),
    target("vscodium", "VSCodium", VSCODIUM_COMMANDS),
    target("zed", "Zed", ZED_COMMANDS),
    target("antigravity", "Antigravity", ANTIGRAVITY_COMMANDS),
    target("idea", "IntelliJ IDEA", IDEA_COMMANDS),
    target("aqua", "Aqua", AQUA_COMMANDS),
    target("clion", "CLion", CLION_COMMANDS),
    target("datagrip", "DataGrip", DATAGRIP_COMMANDS),
    target("dataspell", "DataSpell", DATASPELL_COMMANDS),
    target("goland", "GoLand", GOLAND_COMMANDS),
    target("phpstorm", "PhpStorm", PHPSTORM_COMMANDS),
    target("pycharm", "PyCharm", PYCHARM_COMMANDS),
    target("rider", "Rider", RIDER_COMMANDS),
    target("rubymine", "RubyMine", RUBYMINE_COMMANDS),
    target("rustrover", "RustRover", RUSTROVER_COMMANDS),
    target("webstorm", "WebStorm", WEBSTORM_COMMANDS),
];

const fn target(
    id: &'static str,
    label: &'static str,
    commands: &'static [WorkspaceOpenCommandDefinition],
) -> WorkspaceOpenTargetDefinition {
    WorkspaceOpenTargetDefinition {
        id,
        label,
        kind: WorkspaceOpenTargetKind::Editor,
        launch_style: WorkspaceOpenLaunchStyle::Path,
        commands,
    }
}

fn file_manager_target(platform: &str) -> WorkspaceOpenTargetDefinition {
    let (label, commands): (&'static str, &'static [WorkspaceOpenCommandDefinition]) =
        match platform {
            "windows" => ("Explorer", EXPLORER_COMMANDS),
            "linux" => ("Files", FILES_COMMANDS),
            _ => ("Finder", FINDER_COMMANDS),
        };

    WorkspaceOpenTargetDefinition {
        id: "file-manager",
        label,
        kind: WorkspaceOpenTargetKind::FileManager,
        launch_style: WorkspaceOpenLaunchStyle::Path,
        commands,
    }
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "macos"
    }
}

fn available_workspace_open_targets_with_resolver(
    platform: &str,
    mut resolve_command: impl FnMut(&WorkspaceOpenCommandDefinition) -> Option<PathBuf>,
) -> Vec<WorkspaceOpenTargetResponse> {
    workspace_open_target_definitions(platform)
        .filter(|target| first_available_command(*target, &mut resolve_command).is_some())
        .map(workspace_open_target_response)
        .collect()
}

fn workspace_open_target_definitions(
    platform: &str,
) -> impl Iterator<Item = WorkspaceOpenTargetDefinition> {
    EDITOR_TARGETS
        .iter()
        .copied()
        .chain(std::iter::once(file_manager_target(platform)))
}

fn workspace_open_command_definitions(
    platform: &str,
) -> impl Iterator<Item = &'static WorkspaceOpenCommandDefinition> {
    workspace_open_target_definitions(platform).flat_map(|target| target.commands.iter())
}

fn workspace_open_target_response(
    target: WorkspaceOpenTargetDefinition,
) -> WorkspaceOpenTargetResponse {
    WorkspaceOpenTargetResponse {
        id: target.id.to_string(),
        label: target.label.to_string(),
        kind: target.kind,
    }
}

fn first_available_command(
    target: WorkspaceOpenTargetDefinition,
    resolve_command: &mut impl FnMut(&WorkspaceOpenCommandDefinition) -> Option<PathBuf>,
) -> Option<(&'static WorkspaceOpenCommandDefinition, PathBuf)> {
    target
        .commands
        .iter()
        .find_map(|command| resolve_command(command).map(|path| (command, path)))
}

fn find_target(target_id: &str, platform: &str) -> Option<WorkspaceOpenTargetDefinition> {
    EDITOR_TARGETS
        .iter()
        .copied()
        .chain(std::iter::once(file_manager_target(platform)))
        .find(|target| target.id == target_id)
}

fn validate_workspace_open_path(path: &Path) -> AppResult<PathBuf> {
    let safe_path = validate_absolute_non_root_path(path, "workspace open target")?;
    if !safe_path.is_dir() {
        return Err(AppError::Validation(format!(
            "Workspace open target must be an existing directory: {}",
            safe_path.display()
        )));
    }
    Ok(safe_path)
}

fn build_workspace_open_launch(
    target_id: &str,
    workspace_path: &Path,
    platform: &str,
    mut resolve_command: impl FnMut(&WorkspaceOpenCommandDefinition) -> Option<PathBuf>,
) -> AppResult<WorkspaceOpenLaunch> {
    let safe_path = validate_workspace_open_path(workspace_path)?;
    let target = find_target(target_id, platform).ok_or_else(|| {
        AppError::Validation(format!("Unknown workspace open target: {target_id}"))
    })?;
    let (command, command_path) = first_available_command(target, &mut resolve_command)
        .ok_or_else(|| {
            AppError::Validation(format!(
                "Workspace open target is not available: {}",
                target.label
            ))
        })?;

    let mut args = command
        .base_args
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    match target.launch_style {
        WorkspaceOpenLaunchStyle::Path => args.push(safe_path.into_os_string()),
    }

    Ok(WorkspaceOpenLaunch {
        command: command_path,
        args,
    })
}

fn probe_workspace_open_targets(
    platform: &str,
    direct_resolve_command: impl FnMut(&WorkspaceOpenCommandDefinition) -> Option<PathBuf>,
    shell_resolve_commands: impl FnOnce(
        &[&'static str],
    ) -> std::collections::HashMap<&'static str, PathBuf>,
) -> Vec<WorkspaceOpenTargetResponse> {
    let mut direct_resolve_command = direct_resolve_command;
    let mut resolved_commands = std::collections::HashMap::new();
    let mut unresolved_commands = Vec::new();

    for command in workspace_open_command_definitions(platform) {
        if resolved_commands.contains_key(command.name)
            || unresolved_commands.contains(&command.name)
        {
            continue;
        }
        if let Some(path) = direct_resolve_command(command) {
            resolved_commands.insert(command.name, path);
        } else {
            unresolved_commands.push(command.name);
        }
    }

    for (name, path) in shell_resolve_commands(&unresolved_commands) {
        resolved_commands.entry(name).or_insert(path);
    }

    available_workspace_open_targets_with_resolver(platform, |command| {
        resolved_commands.get(command.name).cloned()
    })
}

fn probe_workspace_open_targets_fast(platform: &str) -> Vec<WorkspaceOpenTargetResponse> {
    probe_workspace_open_targets(
        platform,
        |command| find_launchable_cli_path_without_shell(command.name, command.fixed_candidates),
        |_| std::collections::HashMap::new(),
    )
}

fn probe_workspace_open_targets_full(platform: &str) -> Vec<WorkspaceOpenTargetResponse> {
    probe_workspace_open_targets(
        platform,
        |command| find_launchable_cli_path_without_shell(command.name, command.fixed_candidates),
        find_launchable_cli_paths_with_login_shell,
    )
}

fn cached_workspace_open_targets() -> Option<Vec<WorkspaceOpenTargetResponse>> {
    WORKSPACE_OPEN_TARGET_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|cache| cache.clone())
}

fn store_workspace_open_target_cache(targets: Vec<WorkspaceOpenTargetResponse>) {
    if let Ok(mut cache) = WORKSPACE_OPEN_TARGET_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *cache = Some(targets);
    }
}

pub(crate) fn warm_workspace_open_target_cache() {
    let in_flight = WORKSPACE_OPEN_TARGET_REFRESH_IN_FLIGHT.get_or_init(|| Mutex::new(false));
    {
        let Ok(mut refresh_in_flight) = in_flight.lock() else {
            return;
        };
        if *refresh_in_flight {
            return;
        }
        *refresh_in_flight = true;
    }

    if let Err(error) = std::thread::Builder::new()
        .name("workspace-open-target-probe".to_string())
        .spawn(|| {
            let started = Instant::now();
            let platform = current_platform();
            let result = std::panic::catch_unwind(|| probe_workspace_open_targets_full(platform));
            match result {
                Ok(targets) => {
                    tracing::info!(
                        targets = targets.len(),
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "Workspace open target probe cache refreshed"
                    );
                    store_workspace_open_target_cache(targets);
                }
                Err(_) => {
                    tracing::warn!("Workspace open target probe cache refresh panicked");
                }
            }

            if let Some(in_flight) = WORKSPACE_OPEN_TARGET_REFRESH_IN_FLIGHT.get() {
                if let Ok(mut refresh_in_flight) = in_flight.lock() {
                    *refresh_in_flight = false;
                }
            }
        })
    {
        tracing::warn!(%error, "Failed to spawn workspace open target probe");
        if let Ok(mut refresh_in_flight) = in_flight.lock() {
            *refresh_in_flight = false;
        }
    }
}

fn launch_workspace_open_target(target_id: &str, workspace_path: &Path) -> AppResult<()> {
    let launch =
        build_workspace_open_launch(target_id, workspace_path, current_platform(), |command| {
            find_launchable_cli_path(command.name, command.fixed_candidates)
        })?;

    tracing::info!(
        target_id,
        command = %launch.command.display(),
        workspace_path = %workspace_path.display(),
        "Launching workspace open target"
    );

    // Command path is produced by `find_launchable_cli_path`, which accepts only safe absolute
    // launchable files from fixed candidates, validated user tool dirs, or login-shell `command -v`.
    // codeql[rust/path-injection]
    let mut child = Command::new(&launch.command)
        .args(&launch.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            tracing::warn!(
                target_id,
                command = %launch.command.display(),
                workspace_path = %workspace_path.display(),
                %error,
                "Failed to launch workspace open target"
            );
            AppError::Infrastructure(format!(
                "Failed to launch workspace open target {}: {error}",
                target_id
            ))
        })?;

    match child.try_wait() {
        Ok(Some(status)) if !status.success() => {
            tracing::warn!(
                target_id,
                command = %launch.command.display(),
                workspace_path = %workspace_path.display(),
                exit_status = %status,
                "Workspace open target exited immediately with failure"
            );
            return Err(AppError::Infrastructure(format!(
                "Workspace open target {} exited immediately with {status}",
                target_id
            )));
        }
        Ok(Some(status)) => {
            tracing::info!(
                target_id,
                command = %launch.command.display(),
                exit_status = %status,
                "Workspace open target completed immediately"
            );
        }
        Ok(None) => {
            tracing::info!(
                target_id,
                command = %launch.command.display(),
                pid = child.id(),
                "Workspace open target launched"
            );
        }
        Err(error) => {
            tracing::warn!(
                target_id,
                command = %launch.command.display(),
                %error,
                "Unable to inspect workspace open target status after launch"
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub fn list_workspace_open_targets() -> Vec<WorkspaceOpenTargetResponse> {
    if let Some(targets) = cached_workspace_open_targets() {
        return targets;
    }

    let started = Instant::now();
    let targets = probe_workspace_open_targets_fast(current_platform());
    store_workspace_open_target_cache(targets.clone());
    warm_workspace_open_target_cache();
    tracing::info!(
        targets = targets.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Workspace open target probe cache initialized from fast path"
    );
    targets
}

#[tauri::command]
pub async fn open_agent_conversation_workspace(
    conversation_id: String,
    target_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Agent conversation workspace not found".to_string())?;
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Workspace project not found".to_string())?;
    let workspace_path = resolve_agent_conversation_workspace_path_for_send(&project, &workspace)
        .map_err(|error| error.to_string())?;

    launch_workspace_open_target(&target_id, &workspace_path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    fn fake_command_path(command: &WorkspaceOpenCommandDefinition) -> PathBuf {
        PathBuf::from(format!("/tools/{}", command.name))
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
    fn build_launch_rejects_relative_workspace_paths() {
        let error =
            build_workspace_open_launch("cursor", Path::new("relative"), "macos", |command| {
                (command.name == "cursor").then(|| fake_command_path(command))
            })
            .expect_err("relative path should be rejected");

        assert!(error.to_string().contains("absolute"));
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
    fn full_probe_uses_one_batched_shell_fallback_for_unresolved_commands() {
        let shell_called = Cell::new(false);
        let targets = probe_workspace_open_targets(
            "macos",
            |command| (command.name == "open").then(|| fake_command_path(command)),
            |command_names| {
                shell_called.set(true);
                assert!(command_names.contains(&"cursor"));
                assert!(command_names.contains(&"webstorm"));
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
}
