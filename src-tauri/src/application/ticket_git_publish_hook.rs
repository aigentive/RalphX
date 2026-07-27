use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{
    hook_failure, validate_frozen_commit_rule, ResolvedTicketGitPublishPolicy,
    TicketGitPublishFailure,
};
use crate::application::git_service::{git_cmd, GitService};
use crate::domain::entities::TicketGitConventionSnapshot;

const MANAGED_HOOK_DIR: &str = "ralphx-hooks";
const MANAGED_HOOK_NAME: &str = "commit-msg";

pub async fn install_ticket_git_commit_hook(
    worktree_path: &Path,
    frozen: &TicketGitConventionSnapshot,
) -> Result<(), TicketGitPublishFailure> {
    install_ticket_git_commit_hook_for_rule(worktree_path, &frozen.commit_subject_rule).await
}

pub async fn install_resolved_ticket_git_commit_hook(
    worktree_path: &Path,
    policy: &ResolvedTicketGitPublishPolicy,
) -> Result<(), TicketGitPublishFailure> {
    install_ticket_git_commit_hook_for_rule(worktree_path, &policy.commit_subject_rule).await
}

async fn install_ticket_git_commit_hook_for_rule(
    worktree_path: &Path,
    commit_subject_rule: &str,
) -> Result<(), TicketGitPublishFailure> {
    validate_frozen_commit_rule(commit_subject_rule)?;
    let branch = GitService::get_current_branch(worktree_path)
        .await
        .map_err(|error| hook_failure("read hook worktree branch", error))?;
    let identity = GitService::canonical_target_identity(worktree_path, &branch)
        .await
        .map_err(|error| hook_failure("resolve hook Git directory", error))?;
    let canonical_worktree = tokio::fs::canonicalize(worktree_path)
        .await
        .map_err(|error| hook_failure("canonicalize hook worktree", error))?;
    let worktree_key = Sha256::digest(canonical_worktree.as_os_str().as_encoded_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let hook_dir = ensure_managed_hook_dir(identity.git_common_dir(), &worktree_key).await?;

    let previous_managed = git_config_value(
        worktree_path,
        &[
            "config",
            "--worktree",
            "--get",
            "ralphx.previousCommitMsgHook",
        ],
    )
    .await?;
    let current_hooks = git_config_value(
        worktree_path,
        &["config", "--path", "--get", "core.hooksPath"],
    )
    .await?;
    let managed_text = hook_dir.to_string_lossy();
    let previous_hooks = if current_hooks.as_deref() == Some(managed_text.as_ref()) {
        previous_managed
    } else {
        current_hooks.and_then(|path| resolve_existing_hook_path(worktree_path, &path))
    };

    let hook_path = hook_dir.join(MANAGED_HOOK_NAME);
    let temporary_hook = hook_dir.join("commit-msg.tmp");
    // The hook directory was canonicalized and proven contained by the canonical Git common dir.
    // codeql[rust/path-injection]
    tokio::fs::write(&temporary_hook, managed_commit_msg_hook())
        .await
        .map_err(|error| hook_failure("write managed commit-msg hook", error))?;
    set_executable(&temporary_hook).await?;
    // Both fixed filenames are children of the validated canonical hook directory.
    // codeql[rust/path-injection]
    tokio::fs::rename(&temporary_hook, &hook_path)
        .await
        .map_err(|error| hook_failure("activate managed commit-msg hook", error))?;

    run_git_config(
        worktree_path,
        &["config", "extensions.worktreeConfig", "true"],
    )
    .await?;
    if let Some(previous_hooks) = previous_hooks {
        run_git_config(
            worktree_path,
            &[
                "config",
                "--worktree",
                "ralphx.previousCommitMsgHook",
                &previous_hooks,
            ],
        )
        .await?;
    } else {
        let _ = run_git_config(
            worktree_path,
            &[
                "config",
                "--worktree",
                "--unset-all",
                "ralphx.previousCommitMsgHook",
            ],
        )
        .await;
    }
    run_git_config(
        worktree_path,
        &[
            "config",
            "--worktree",
            "ralphx.ticketCommitRule",
            commit_subject_rule,
        ],
    )
    .await?;
    run_git_config(
        worktree_path,
        &[
            "config",
            "--worktree",
            "core.hooksPath",
            managed_text.as_ref(),
        ],
    )
    .await
}

async fn ensure_managed_hook_dir(
    git_common_dir: &Path,
    worktree_key: &str,
) -> Result<PathBuf, TicketGitPublishFailure> {
    let root_candidate = git_common_dir.join(MANAGED_HOOK_DIR);
    // `git_common_dir` is canonical authority from `GitService`; the child name is fixed.
    // codeql[rust/path-injection]
    if let Err(error) = tokio::fs::create_dir(&root_candidate).await {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(hook_failure("create managed hook root", error));
        }
    }
    let managed_root = tokio::fs::canonicalize(&root_candidate)
        .await
        .map_err(|error| hook_failure("canonicalize managed hook root", error))?;
    if !managed_root.starts_with(git_common_dir) || !managed_root.is_dir() {
        return Err(hook_failure(
            "validate managed hook root",
            "managed hook root escaped the canonical Git directory",
        ));
    }

    let hook_candidate = managed_root.join(worktree_key);
    // The key is a lowercase SHA-256 digest and the canonical parent was contained above.
    // codeql[rust/path-injection]
    if let Err(error) = tokio::fs::create_dir(&hook_candidate).await {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(hook_failure(
                "create managed worktree hook directory",
                error,
            ));
        }
    }
    let hook_dir = tokio::fs::canonicalize(&hook_candidate)
        .await
        .map_err(|error| hook_failure("canonicalize managed hook directory", error))?;
    if !hook_dir.starts_with(&managed_root) || !hook_dir.is_dir() {
        return Err(hook_failure(
            "validate managed worktree hook directory",
            "managed worktree hook directory escaped its canonical root",
        ));
    }
    Ok(hook_dir)
}

fn managed_commit_msg_hook() -> &'static str {
    r#"#!/bin/sh
previous=$(git config --worktree --get ralphx.previousCommitMsgHook 2>/dev/null || true)
if [ -n "$previous" ] && [ -x "$previous/commit-msg" ]; then
  "$previous/commit-msg" "$@" || exit $?
fi
rule=$(git config --worktree --get ralphx.ticketCommitRule 2>/dev/null || true)
if [ -z "$rule" ]; then
  echo "ERROR: RalphX strict ticket commit rule is unavailable." >&2
  exit 1
fi
subject=$(sed -n '1p' "$1")
case "$rule" in
  *:summary:*)
    prefix=${rule%%:summary:*}
    suffix=${rule#*:summary:}
    case "$subject" in
      "$prefix"*"$suffix")
        middle=${subject#"$prefix"}
        if [ -n "$suffix" ]; then middle=${middle%"$suffix"}; fi
        compact=$(printf '%s' "$middle" | tr -d '[:space:]')
        [ -n "$compact" ] && exit 0
        ;;
    esac
    ;;
  *) [ "$subject" = "$rule" ] && exit 0 ;;
esac
echo "ERROR: Commit subject must match the frozen RalphX ticket rule: $rule" >&2
exit 1
"#
}

async fn git_config_value(
    worktree_path: &Path,
    args: &[&str],
) -> Result<Option<String>, TicketGitPublishFailure> {
    let output = git_cmd::run(args, worktree_path)
        .await
        .map_err(|error| hook_failure("read Git hook configuration", error))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8(output.stdout)
        .map_err(|error| hook_failure("decode Git hook configuration", error))?;
    Ok(Some(value.trim().to_string()).filter(|value| !value.is_empty()))
}

async fn run_git_config(
    worktree_path: &Path,
    args: &[&str],
) -> Result<(), TicketGitPublishFailure> {
    let output = git_cmd::run(args, worktree_path)
        .await
        .map_err(|error| hook_failure("write Git hook configuration", error))?;
    if output.status.success() {
        return Ok(());
    }
    Err(hook_failure(
        "write Git hook configuration",
        String::from_utf8_lossy(&output.stderr).trim(),
    ))
}

fn resolve_existing_hook_path(worktree_path: &Path, configured: &str) -> Option<String> {
    let path = Path::new(configured);
    let candidate: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        worktree_path.join(path)
    };
    candidate
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())
        .map(|path| path.to_string_lossy().to_string())
}

#[cfg(unix)]
async fn set_executable(path: &Path) -> Result<(), TicketGitPublishFailure> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = tokio::fs::metadata(path)
        .await
        .map_err(|error| hook_failure("read managed hook permissions", error))?
        .permissions();
    permissions.set_mode(0o755);
    // Callers pass the fixed temporary hook below a canonical contained hook directory.
    // codeql[rust/path-injection]
    tokio::fs::set_permissions(path, permissions)
        .await
        .map_err(|error| hook_failure("set managed hook permissions", error))
}

#[cfg(not(unix))]
async fn set_executable(_path: &Path) -> Result<(), TicketGitPublishFailure> {
    Ok(())
}
