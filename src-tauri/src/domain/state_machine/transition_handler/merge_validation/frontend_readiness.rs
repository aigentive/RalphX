use std::path::{Path, PathBuf};

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::infrastructure::tool_paths::resolve_node_cli_path;
use crate::utils::path_safety::{checked_read_to_string, validate_absolute_non_root_path};

#[derive(Debug, Clone)]
pub(super) struct FrontendReadinessFailure {
    issues: Vec<String>,
}

impl FrontendReadinessFailure {
    pub(super) fn message(&self) -> String {
        self.issues.join("; ")
    }
}

pub(super) fn command_cwd(
    base_cwd: &Path,
    resolved_path: &str,
    command: &str,
) -> (PathBuf, String) {
    let default_cwd = if resolved_path == "." {
        base_cwd.to_path_buf()
    } else {
        base_cwd.join(resolved_path)
    };

    if resolved_path == "." && is_node_package_command(command) {
        let nested_frontend = default_cwd.join("frontend");
        if nested_frontend.join("package.json").exists()
            && !default_cwd.join("package.json").exists()
        {
            return (nested_frontend, "frontend".to_string());
        }
    }

    (default_cwd, resolved_path.to_string())
}

pub(super) fn sanitize_frontend_validate_command(command: &str) -> String {
    let mut parts: Vec<&str> = command
        .split_whitespace()
        .filter(|part| {
            let trimmed = part.trim_matches(|ch| ch == '\'' || ch == '"');
            trimmed != "vitest.config.ts" && !trimmed.ends_with("/vitest.config.ts")
        })
        .collect();

    while parts.last().copied() == Some("--") {
        parts.pop();
    }

    let mut sanitized = if parts.is_empty() {
        command.to_string()
    } else {
        parts.join(" ")
    };

    if let Some(rest) = sanitized.strip_prefix("vitest ") {
        sanitized = format!("./node_modules/.bin/vitest {rest}");
    } else if sanitized == "vitest" {
        sanitized = "./node_modules/.bin/vitest".to_string();
    }

    sanitized
}

pub(super) fn requires_frontend_readiness(command: &str, cwd: &Path) -> bool {
    if !is_frontend_validation_command(command) {
        return false;
    }

    is_frontend_package_context(cwd)
}

pub(super) fn is_frontend_package_context(cwd: &Path) -> bool {
    cwd.join("package.json").exists()
        && (cwd.ends_with("frontend") || package_json_mentions_frontend_stack(cwd))
}

pub(super) async fn check_frontend_dependency_readiness(
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<(), FrontendReadinessFailure> {
    let mut issues = Vec::new();
    let vitest_bin = cwd.join("node_modules").join(".bin").join("vitest");
    if !is_executable_file(&vitest_bin) {
        issues.push(format!(
            "{} is missing or not executable",
            vitest_bin.display()
        ));
    }

    for specifier in ["vitest/config", "react", "zod", "@tauri-apps/api"] {
        if let Err(error) = run_node_import_probe(cwd, specifier, cancel).await {
            issues.push(error);
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(FrontendReadinessFailure { issues })
    }
}

fn is_node_package_command(command: &str) -> bool {
    let trimmed = command.trim_start();
    trimmed.starts_with("npm ")
        || trimmed == "npm"
        || trimmed.starts_with("npx ")
        || trimmed.starts_with("vitest")
        || trimmed.starts_with("./node_modules/.bin/vitest")
}

fn is_frontend_validation_command(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    command.contains("npm run lint")
        || command.contains("npm run typecheck")
        || command.contains("npm run test")
        || command == "npm test"
        || command.contains(" vitest")
        || command.starts_with("vitest")
        || command.starts_with("./node_modules/.bin/vitest")
}

fn package_json_mentions_frontend_stack(cwd: &Path) -> bool {
    let Ok(contents) = checked_read_to_string(&cwd.join("package.json"), "frontend package.json")
    else {
        return false;
    };

    contents.contains("\"react\"")
        || contents.contains("\"vitest\"")
        || contents.contains("\"@tauri-apps/api\"")
        || contents.contains("\"zod\"")
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(safe_path) = validate_absolute_non_root_path(path, "frontend executable") else {
        return false;
    };
    let Ok(metadata) = safe_path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

async fn run_node_import_probe(
    cwd: &Path,
    specifier: &str,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let safe_cwd = validate_absolute_non_root_path(cwd, "frontend dependency probe cwd")
        .map_err(|error| format!("invalid frontend dependency probe cwd: {error}"))?;
    let mut child = tokio::process::Command::new(resolve_node_cli_path())
        .arg("-e")
        .arg(format!("import({specifier:?})"))
        .current_dir(&safe_cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("node import {specifier:?} could not start: {error}"))?;

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_fut = async {
        let mut bytes = Vec::new();
        if let Some(mut stdout) = stdout_handle {
            let _ = stdout.read_to_end(&mut bytes).await;
        }
        bytes
    };
    let stderr_fut = async {
        let mut bytes = Vec::new();
        if let Some(mut stderr) = stderr_handle {
            let _ = stderr.read_to_end(&mut bytes).await;
        }
        bytes
    };

    tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(format!("node import {specifier:?} cancelled"))
        }
        (status, stdout, stderr) = async { tokio::join!(child.wait(), stdout_fut, stderr_fut) } => {
            match status {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => {
                    let stderr = String::from_utf8_lossy(&stderr);
                    let stdout = String::from_utf8_lossy(&stdout);
                    let detail = if stderr.trim().is_empty() {
                        stdout.trim()
                    } else {
                        stderr.trim()
                    };
                    Err(format!(
                        "node import {specifier:?} failed with exit {:?}: {}",
                        status.code(),
                        detail
                    ))
                }
                Err(error) => Err(format!("node import {specifier:?} wait failed: {error}")),
            }
        }
    }
}
