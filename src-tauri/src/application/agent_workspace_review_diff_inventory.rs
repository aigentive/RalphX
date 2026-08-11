use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use crate::application::agent_workspace_review::{
    AgentWorkspaceReviewChangedFile, AgentWorkspaceReviewTarget,
};
use crate::application::agent_workspace_review_diff::AgentWorkspaceReviewDiffSource;
use crate::domain::entities::AgentWorkspaceReviewTargetScope;
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::resolve_git_cli_path;

pub(super) fn full_changed_file_inventory(
    target: &AgentWorkspaceReviewTarget,
) -> AppResult<Vec<AgentWorkspaceReviewChangedFile>> {
    let sources = match target.scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => vec![(
            AgentWorkspaceReviewDiffSource::SelectedSource,
            source_file_statuses(target, AgentWorkspaceReviewDiffSource::SelectedSource)?,
        )],
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => vec![
            (
                AgentWorkspaceReviewDiffSource::Committed,
                source_file_statuses(target, AgentWorkspaceReviewDiffSource::Committed)?,
            ),
            (
                AgentWorkspaceReviewDiffSource::Staged,
                source_file_statuses(target, AgentWorkspaceReviewDiffSource::Staged)?,
            ),
            (
                AgentWorkspaceReviewDiffSource::Unstaged,
                source_file_statuses(target, AgentWorkspaceReviewDiffSource::Unstaged)?,
            ),
        ],
    };

    let mut files = BTreeMap::<String, (String, BTreeSet<String>)>::new();
    for (source, statuses) in sources {
        for (path, status) in statuses {
            merge_file_status(&mut files, path, source, &status);
        }
    }
    Ok(files
        .into_iter()
        .map(
            |(path, (status, sources))| AgentWorkspaceReviewChangedFile {
                path,
                status,
                sources: sources.into_iter().collect(),
            },
        )
        .collect())
}

fn merge_file_status(
    files: &mut BTreeMap<String, (String, BTreeSet<String>)>,
    path: String,
    source: AgentWorkspaceReviewDiffSource,
    status: &str,
) {
    let entry = files
        .entry(path)
        .or_insert_with(|| (status.to_string(), BTreeSet::new()));
    if status_rank(status) > status_rank(&entry.0) {
        entry.0 = status.to_string();
    }
    entry.1.insert(source.as_str().to_string());
}

fn status_rank(status: &str) -> u8 {
    match status {
        "deleted" => 4,
        "added" => 3,
        "renamed" => 2,
        "modified" => 1,
        _ => 0,
    }
}

fn source_file_statuses(
    target: &AgentWorkspaceReviewTarget,
    source: AgentWorkspaceReviewDiffSource,
) -> AppResult<BTreeMap<String, String>> {
    let mut command = Command::new(resolve_git_cli_path());
    command.current_dir(&target.working_directory).arg("diff");
    match source {
        AgentWorkspaceReviewDiffSource::SelectedSource => {
            command.args([
                "--name-status",
                "-z",
                "--find-renames",
                &target.base_ref,
                &target.head_ref,
                "--",
            ]);
        }
        AgentWorkspaceReviewDiffSource::Committed => {
            command.args([
                "--name-status",
                "-z",
                "--find-renames",
                &target.base_ref,
                "HEAD",
                "--",
            ]);
        }
        AgentWorkspaceReviewDiffSource::Staged => {
            command.args(["--cached", "--name-status", "-z", "--find-renames", "--"]);
        }
        AgentWorkspaceReviewDiffSource::Unstaged => {
            command.args(["--name-status", "-z", "--find-renames", "--"]);
        }
    }
    let output = command.output().map_err(|error| {
        AppError::GitOperation(format!(
            "Failed to read Workspace Review file statuses: {error}"
        ))
    })?;
    if !output.status.success() {
        return Err(AppError::GitOperation(format!(
            "Failed to read Workspace Review file statuses: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let mut statuses = parse_name_status_z(&output.stdout)?;
    if source == AgentWorkspaceReviewDiffSource::Unstaged {
        let output = Command::new(resolve_git_cli_path())
            .current_dir(&target.working_directory)
            .args(["ls-files", "--others", "--exclude-standard", "-z", "--"])
            .output()
            .map_err(|error| {
                AppError::GitOperation(format!(
                    "Failed to read untracked Workspace Review files: {error}"
                ))
            })?;
        if !output.status.success() {
            return Err(AppError::GitOperation(format!(
                "Failed to read untracked Workspace Review files: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        for field in output.stdout.split(|byte| *byte == 0) {
            if field.is_empty() {
                continue;
            }
            let path = std::str::from_utf8(field).map_err(|_| {
                AppError::Validation(
                    "Workspace Review untracked path is not valid UTF-8".to_string(),
                )
            })?;
            statuses.insert(path.to_string(), "added".to_string());
        }
    }
    Ok(statuses)
}

fn parse_name_status_z(stdout: &[u8]) -> AppResult<BTreeMap<String, String>> {
    let fields = stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut statuses = BTreeMap::new();
    let mut index = 0usize;
    while index < fields.len() {
        let status_token = std::str::from_utf8(fields[index]).map_err(|_| {
            AppError::Validation("Workspace Review git status is not valid UTF-8".to_string())
        })?;
        index += 1;
        let status_code = status_token.chars().next().unwrap_or('M');
        if matches!(status_code, 'R' | 'C') {
            if index + 1 >= fields.len() {
                return Err(AppError::GitOperation(
                    "Workspace Review git rename status was incomplete".to_string(),
                ));
            }
            index += 1;
        } else if index >= fields.len() {
            return Err(AppError::GitOperation(
                "Workspace Review git file status was incomplete".to_string(),
            ));
        }
        let path = std::str::from_utf8(fields[index]).map_err(|_| {
            AppError::Validation("Workspace Review file path is not valid UTF-8".to_string())
        })?;
        index += 1;
        let status = match status_code {
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            _ => "modified",
        };
        statuses.insert(path.to_string(), status.to_string());
    }
    Ok(statuses)
}
