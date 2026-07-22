use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use super::mcp_catalog::{classify_reserved_user_registration, ReservedClaudeUserRegistration};

const RESERVED_MCP_REPAIR_TIMEOUT: Duration = Duration::from_secs(15);

fn repair_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservedMcpRepairFailureCode {
    ConfigRead,
    CommandFailed,
    Timeout,
    PostconditionFailed,
}

impl std::fmt::Display for ReservedMcpRepairFailureCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigRead => write!(f, "config_read_failed"),
            Self::CommandFailed => write!(f, "command_failed"),
            Self::Timeout => write!(f, "timeout"),
            Self::PostconditionFailed => write!(f, "postcondition_failed"),
        }
    }
}

pub(crate) async fn remove_reserved_user_registration(
    cli_path: &Path,
    home_dir: &Path,
    provider_env: &HashMap<String, String>,
) -> Result<bool, ReservedMcpRepairFailureCode> {
    remove_reserved_user_registration_with_timeout(
        cli_path,
        home_dir,
        provider_env,
        RESERVED_MCP_REPAIR_TIMEOUT,
    )
    .await
}

async fn remove_reserved_user_registration_with_timeout(
    cli_path: &Path,
    home_dir: &Path,
    provider_env: &HashMap<String, String>,
    timeout: Duration,
) -> Result<bool, ReservedMcpRepairFailureCode> {
    let _guard = repair_lock().lock().await;
    let home_dir =
        crate::utils::path_safety::validate_absolute_non_root_path(home_dir, "Claude config root")
            .map_err(|_| ReservedMcpRepairFailureCode::ConfigRead)?;
    match classify_reserved_user_registration(&home_dir)
        .map_err(|_| ReservedMcpRepairFailureCode::ConfigRead)?
    {
        ReservedClaudeUserRegistration::NotPresent => return Ok(false),
        ReservedClaudeUserRegistration::ReservedUserEntry => {}
    }

    let mut command = tokio::process::Command::new(cli_path);
    super::apply_common_spawn_env(&mut command);
    command
        .envs(provider_env)
        .env("HOME", &home_dir)
        .args(["mcp", "remove", "ralphx", "-s", "user"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| ReservedMcpRepairFailureCode::CommandFailed)?;
    let command_outcome = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) if status.success() => None,
        Ok(_) => Some(ReservedMcpRepairFailureCode::CommandFailed),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Some(ReservedMcpRepairFailureCode::Timeout)
        }
    };

    let postcondition = classify_reserved_user_registration(&home_dir)
        .map_err(|_| ReservedMcpRepairFailureCode::ConfigRead)?;
    if postcondition == ReservedClaudeUserRegistration::NotPresent {
        return Ok(true);
    }
    Err(command_outcome.unwrap_or(ReservedMcpRepairFailureCode::PostconditionFailed))
}

#[cfg(test)]
pub(crate) async fn remove_reserved_user_registration_for_test(
    cli_path: &Path,
    home_dir: &Path,
    timeout: Duration,
) -> Result<bool, ReservedMcpRepairFailureCode> {
    remove_reserved_user_registration_with_timeout(cli_path, home_dir, &HashMap::new(), timeout)
        .await
}

#[cfg(test)]
pub(crate) async fn remove_reserved_user_registration_with_env_for_test(
    cli_path: &Path,
    home_dir: &Path,
    provider_env: &HashMap<String, String>,
    timeout: Duration,
) -> Result<bool, ReservedMcpRepairFailureCode> {
    remove_reserved_user_registration_with_timeout(cli_path, home_dir, provider_env, timeout).await
}
