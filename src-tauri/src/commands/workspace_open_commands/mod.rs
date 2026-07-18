use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::Serialize;
use tauri::State;

use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::AppState;
use crate::domain::entities::ChatConversationId;
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::{
    find_launchable_cli_path, find_launchable_cli_path_without_shell,
    find_launchable_cli_paths_with_login_shell, is_safe_launchable_binary_path,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceOpenTargetKind {
    Editor,
    Terminal,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceOpenPathKind {
    Directory,
    File,
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
    launch_commands: &'static [WorkspaceOpenCommandDefinition],
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
const ITERM2_APP_COMMANDS: &[WorkspaceOpenCommandDefinition] = &[WorkspaceOpenCommandDefinition {
    name: "iterm2-app",
    fixed_candidates: &[
        "/Applications/iTerm.app/Contents/MacOS/iTerm2",
        "/Applications/iTerm2.app/Contents/MacOS/iTerm2",
    ],
    base_args: &[],
}];
const TERMINAL_APP_COMMANDS: &[WorkspaceOpenCommandDefinition] =
    &[WorkspaceOpenCommandDefinition {
        name: "terminal-app",
        fixed_candidates: &[
            "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
            "/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
        ],
        base_args: &[],
    }];
const ITERM2_LAUNCH_COMMANDS: &[WorkspaceOpenCommandDefinition] =
    &[WorkspaceOpenCommandDefinition {
        name: "open",
        fixed_candidates: &["/usr/bin/open"],
        base_args: &["-b", "com.googlecode.iterm2"],
    }];
const TERMINAL_LAUNCH_COMMANDS: &[WorkspaceOpenCommandDefinition] =
    &[WorkspaceOpenCommandDefinition {
        name: "open",
        fixed_candidates: &["/usr/bin/open"],
        base_args: &["-b", "com.apple.Terminal"],
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

macro_rules! editor_target {
    ($id:expr, $label:expr, $commands:expr $(,)?) => {
        WorkspaceOpenTargetDefinition {
            id: $id,
            label: $label,
            kind: WorkspaceOpenTargetKind::Editor,
            launch_style: WorkspaceOpenLaunchStyle::Path,
            commands: $commands,
            launch_commands: $commands,
        }
    };
}

const EDITOR_TARGETS: &[WorkspaceOpenTargetDefinition] = &[
    editor_target!("cursor", "Cursor", CURSOR_COMMANDS),
    editor_target!("trae", "Trae", TRAE_COMMANDS),
    editor_target!("kiro", "Kiro", KIRO_COMMANDS),
    editor_target!("vscode", "VS Code", VSCODE_COMMANDS),
    editor_target!(
        "vscode-insiders",
        "VS Code Insiders",
        VSCODE_INSIDERS_COMMANDS,
    ),
    editor_target!("vscodium", "VSCodium", VSCODIUM_COMMANDS),
    editor_target!("zed", "Zed", ZED_COMMANDS),
    editor_target!("antigravity", "Antigravity", ANTIGRAVITY_COMMANDS),
    editor_target!("idea", "IntelliJ IDEA", IDEA_COMMANDS),
    editor_target!("aqua", "Aqua", AQUA_COMMANDS),
    editor_target!("clion", "CLion", CLION_COMMANDS),
    editor_target!("datagrip", "DataGrip", DATAGRIP_COMMANDS),
    editor_target!("dataspell", "DataSpell", DATASPELL_COMMANDS),
    editor_target!("goland", "GoLand", GOLAND_COMMANDS),
    editor_target!("phpstorm", "PhpStorm", PHPSTORM_COMMANDS),
    editor_target!("pycharm", "PyCharm", PYCHARM_COMMANDS),
    editor_target!("rider", "Rider", RIDER_COMMANDS),
    editor_target!("rubymine", "RubyMine", RUBYMINE_COMMANDS),
    editor_target!("rustrover", "RustRover", RUSTROVER_COMMANDS),
    editor_target!("webstorm", "WebStorm", WEBSTORM_COMMANDS),
];

const MACOS_TERMINAL_TARGETS: &[WorkspaceOpenTargetDefinition] = &[
    WorkspaceOpenTargetDefinition {
        id: "iterm2",
        label: "iTerm2",
        kind: WorkspaceOpenTargetKind::Terminal,
        launch_style: WorkspaceOpenLaunchStyle::Path,
        commands: ITERM2_APP_COMMANDS,
        launch_commands: ITERM2_LAUNCH_COMMANDS,
    },
    WorkspaceOpenTargetDefinition {
        id: "terminal",
        label: "Terminal",
        kind: WorkspaceOpenTargetKind::Terminal,
        launch_style: WorkspaceOpenLaunchStyle::Path,
        commands: TERMINAL_APP_COMMANDS,
        launch_commands: TERMINAL_LAUNCH_COMMANDS,
    },
];

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
        launch_commands: commands,
    }
}

#[cfg(target_os = "windows")]
fn current_platform() -> &'static str {
    "windows"
}

#[cfg(target_os = "linux")]
fn current_platform() -> &'static str {
    "linux"
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn current_platform() -> &'static str {
    "macos"
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
    let include_macos_terminals = platform == "macos";
    EDITOR_TARGETS
        .iter()
        .copied()
        .chain(
            MACOS_TERMINAL_TARGETS
                .iter()
                .copied()
                .filter(move |_| include_macos_terminals),
        )
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
    workspace_open_target_definitions(platform).find(|target| target.id == target_id)
}

fn is_fixed_binary_command(command: &WorkspaceOpenCommandDefinition) -> bool {
    matches!(command.name, "iterm2-app" | "terminal-app")
        || (command.name == "open" && command.fixed_candidates == ["/usr/bin/open"])
}

fn find_fixed_launchable_binary(command: &WorkspaceOpenCommandDefinition) -> Option<PathBuf> {
    command
        .fixed_candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| is_safe_launchable_binary_path(path))
}

fn resolve_workspace_open_command_without_shell(
    command: &WorkspaceOpenCommandDefinition,
) -> Option<PathBuf> {
    if is_fixed_binary_command(command) {
        find_fixed_launchable_binary(command)
    } else {
        find_launchable_cli_path_without_shell(command.name, command.fixed_candidates)
    }
}

fn resolve_workspace_open_command(command: &WorkspaceOpenCommandDefinition) -> Option<PathBuf> {
    if is_fixed_binary_command(command) {
        find_fixed_launchable_binary(command)
    } else {
        find_launchable_cli_path(command.name, command.fixed_candidates)
    }
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

fn validate_workspace_open_item_path(path: &Path) -> AppResult<(PathBuf, WorkspaceOpenPathKind)> {
    let safe_path = validate_absolute_non_root_path(path, "workspace open path target")?;
    if safe_path.is_dir() {
        return Ok((safe_path, WorkspaceOpenPathKind::Directory));
    }
    if safe_path.is_file() {
        return Ok((safe_path, WorkspaceOpenPathKind::File));
    }
    Err(AppError::Validation(format!(
        "Workspace open path target must be an existing file or directory: {}",
        safe_path.display()
    )))
}

fn resolve_workspace_open_item_path(
    workspace_path: &Path,
    requested_path: &Path,
) -> AppResult<PathBuf> {
    let safe_workspace = validate_workspace_open_path(workspace_path)?;
    let canonical_workspace = safe_workspace.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to canonicalize workspace open root {}: {error}",
            safe_workspace.display()
        ))
    })?;
    let candidate = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        safe_workspace.join(requested_path)
    };
    let safe_candidate = validate_absolute_non_root_path(&candidate, "workspace open path target")?;
    let canonical_candidate = safe_candidate.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to canonicalize workspace open path target {}: {error}",
            safe_candidate.display()
        ))
    })?;
    if !canonical_candidate.starts_with(&canonical_workspace) {
        return Err(AppError::Validation(format!(
            "Workspace open path target is outside the conversation workspace: {}",
            requested_path.display()
        )));
    }
    Ok(canonical_candidate)
}

fn build_workspace_open_args(
    target: WorkspaceOpenTargetDefinition,
    command: &WorkspaceOpenCommandDefinition,
    path: &Path,
    path_kind: WorkspaceOpenPathKind,
    platform: &str,
) -> Vec<OsString> {
    let mut args = command
        .base_args
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    match (target.launch_style, target.kind, path_kind, platform) {
        (
            WorkspaceOpenLaunchStyle::Path,
            WorkspaceOpenTargetKind::FileManager,
            WorkspaceOpenPathKind::File,
            "macos",
        ) => {
            args.push(OsString::from("-R"));
            args.push(path.as_os_str().to_os_string());
        }
        (
            WorkspaceOpenLaunchStyle::Path,
            WorkspaceOpenTargetKind::FileManager,
            WorkspaceOpenPathKind::File,
            "windows",
        ) => {
            let mut select_arg = OsString::from("/select,");
            select_arg.push(path.as_os_str());
            args.push(select_arg);
        }
        (
            WorkspaceOpenLaunchStyle::Path,
            WorkspaceOpenTargetKind::FileManager,
            WorkspaceOpenPathKind::File,
            "linux",
        ) => {
            args.push(path.parent().unwrap_or(path).as_os_str().to_os_string());
        }
        (
            WorkspaceOpenLaunchStyle::Path,
            WorkspaceOpenTargetKind::Terminal,
            WorkspaceOpenPathKind::File,
            _,
        ) => {
            args.push(path.parent().unwrap_or(path).as_os_str().to_os_string());
        }
        (WorkspaceOpenLaunchStyle::Path, _, _, _) => {
            args.push(path.as_os_str().to_os_string());
        }
    }
    args
}

fn build_workspace_open_launch_for_valid_path(
    target_id: &str,
    safe_path: &Path,
    path_kind: WorkspaceOpenPathKind,
    platform: &str,
    mut resolve_command: impl FnMut(&WorkspaceOpenCommandDefinition) -> Option<PathBuf>,
) -> AppResult<WorkspaceOpenLaunch> {
    let target = find_target(target_id, platform).ok_or_else(|| {
        AppError::Validation(format!("Unknown workspace open target: {target_id}"))
    })?;
    first_available_command(target, &mut resolve_command).ok_or_else(|| {
        AppError::Validation(format!(
            "Workspace open target is not available: {}",
            target.label
        ))
    })?;
    let (command, command_path) = target
        .launch_commands
        .iter()
        .find_map(|command| resolve_command(command).map(|path| (command, path)))
        .ok_or_else(|| {
            AppError::Validation(format!(
                "Workspace open target launcher is not available: {}",
                target.label
            ))
        })?;
    let args = build_workspace_open_args(target, command, safe_path, path_kind, platform);

    Ok(WorkspaceOpenLaunch {
        command: command_path,
        args,
    })
}

fn build_workspace_open_launch(
    target_id: &str,
    workspace_path: &Path,
    platform: &str,
    mut resolve_command: impl FnMut(&WorkspaceOpenCommandDefinition) -> Option<PathBuf>,
) -> AppResult<WorkspaceOpenLaunch> {
    let safe_path = validate_workspace_open_path(workspace_path)?;
    build_workspace_open_launch_for_valid_path(
        target_id,
        &safe_path,
        WorkspaceOpenPathKind::Directory,
        platform,
        &mut resolve_command,
    )
}

fn build_workspace_open_item_launch(
    target_id: &str,
    item_path: &Path,
    platform: &str,
    mut resolve_command: impl FnMut(&WorkspaceOpenCommandDefinition) -> Option<PathBuf>,
) -> AppResult<WorkspaceOpenLaunch> {
    let (safe_path, path_kind) = validate_workspace_open_item_path(item_path)?;
    build_workspace_open_launch_for_valid_path(
        target_id,
        &safe_path,
        path_kind,
        platform,
        &mut resolve_command,
    )
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
        } else if !is_fixed_binary_command(command) {
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
        resolve_workspace_open_command_without_shell,
        |_| std::collections::HashMap::new(),
    )
}

fn probe_workspace_open_targets_full(platform: &str) -> Vec<WorkspaceOpenTargetResponse> {
    probe_workspace_open_targets(
        platform,
        resolve_workspace_open_command_without_shell,
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
    let launch = build_workspace_open_launch(
        target_id,
        workspace_path,
        current_platform(),
        resolve_workspace_open_command,
    )?;

    tracing::info!(
        target_id,
        command = %launch.command.display(),
        workspace_path = %workspace_path.display(),
        "Launching workspace open target"
    );

    // Command path is resolver-produced from either validated fixed binaries or safe absolute CLI
    // paths from fixed candidates, validated user tool dirs, or login-shell `command -v`.
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

fn launch_workspace_open_item_target(target_id: &str, item_path: &Path) -> AppResult<()> {
    let launch = build_workspace_open_item_launch(
        target_id,
        item_path,
        current_platform(),
        resolve_workspace_open_command,
    )?;

    tracing::info!(
        target_id,
        command = %launch.command.display(),
        item_path = %item_path.display(),
        "Launching workspace open path target"
    );

    // Command path is resolver-produced from either validated fixed binaries or safe absolute CLI
    // paths from fixed candidates, validated user tool dirs, or login-shell `command -v`.
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
                item_path = %item_path.display(),
                %error,
                "Failed to launch workspace open path target"
            );
            AppError::Infrastructure(format!(
                "Failed to launch workspace open path target {}: {error}",
                target_id
            ))
        })?;

    match child.try_wait() {
        Ok(Some(status)) if !status.success() => {
            tracing::warn!(
                target_id,
                command = %launch.command.display(),
                item_path = %item_path.display(),
                exit_status = %status,
                "Workspace open path target exited immediately with failure"
            );
            return Err(AppError::Infrastructure(format!(
                "Workspace open path target {} exited immediately with {status}",
                target_id
            )));
        }
        Ok(Some(status)) => {
            tracing::info!(
                target_id,
                command = %launch.command.display(),
                exit_status = %status,
                "Workspace open path target completed immediately"
            );
        }
        Ok(None) => {
            tracing::info!(
                target_id,
                command = %launch.command.display(),
                pid = child.id(),
                "Workspace open path target launched"
            );
        }
        Err(error) => {
            tracing::warn!(
                target_id,
                command = %launch.command.display(),
                %error,
                "Unable to inspect workspace open path target status after launch"
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
    let workspace_path = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await
    .map_err(|error| error.to_string())?
    .path;

    launch_workspace_open_target(&target_id, &workspace_path).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_agent_conversation_workspace_path(
    conversation_id: String,
    target_id: String,
    path: String,
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
    let workspace_path = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await
    .map_err(|error| error.to_string())?
    .path;
    let item_path = resolve_workspace_open_item_path(&workspace_path, Path::new(&path))
        .map_err(|error| error.to_string())?;

    launch_workspace_open_item_target(&target_id, &item_path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod workspace_open_commands_tests;
