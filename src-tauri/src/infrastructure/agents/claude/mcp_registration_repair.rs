use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use super::mcp_catalog::{classify_legacy_user_registration, LegacyClaudeRegistration};

const LEGACY_MCP_REPAIR_TIMEOUT: Duration = Duration::from_secs(15);

fn repair_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyMcpRepairFailureCode {
    ConfigRead,
    NoExactHistoricalMatch,
    CommandFailed,
    Timeout,
    PostconditionFailed,
}

impl std::fmt::Display for LegacyMcpRepairFailureCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigRead => write!(f, "config_read_failed"),
            Self::NoExactHistoricalMatch => write!(f, "no_exact_historical_match"),
            Self::CommandFailed => write!(f, "command_failed"),
            Self::Timeout => write!(f, "timeout"),
            Self::PostconditionFailed => write!(f, "postcondition_failed"),
        }
    }
}

pub(crate) async fn retire_exact_legacy_user_registration(
    cli_path: &Path,
    home_dir: &Path,
    app_data_dir: &Path,
    provider_env: &HashMap<String, String>,
) -> Result<bool, LegacyMcpRepairFailureCode> {
    retire_exact_legacy_user_registration_with_timeout(
        cli_path,
        home_dir,
        app_data_dir,
        provider_env,
        LEGACY_MCP_REPAIR_TIMEOUT,
    )
    .await
}

async fn retire_exact_legacy_user_registration_with_timeout(
    cli_path: &Path,
    home_dir: &Path,
    app_data_dir: &Path,
    provider_env: &HashMap<String, String>,
    timeout: Duration,
) -> Result<bool, LegacyMcpRepairFailureCode> {
    let _guard = repair_lock().lock().await;
    match classify_legacy_user_registration(home_dir, app_data_dir)
        .map_err(|_| LegacyMcpRepairFailureCode::ConfigRead)?
    {
        LegacyClaudeRegistration::NotPresent => return Ok(false),
        LegacyClaudeRegistration::AmbiguousCollision => {
            return Err(LegacyMcpRepairFailureCode::NoExactHistoricalMatch)
        }
        LegacyClaudeRegistration::ExactHistorical => {}
    }

    let mut command = tokio::process::Command::new(cli_path);
    super::apply_common_spawn_env(&mut command);
    command
        .envs(provider_env)
        .args(["mcp", "remove", "ralphx", "-s", "user"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(timeout, command.status())
        .await
        .map_err(|_| LegacyMcpRepairFailureCode::Timeout)?
        .map_err(|_| LegacyMcpRepairFailureCode::CommandFailed)?;

    let postcondition = classify_legacy_user_registration(home_dir, app_data_dir)
        .map_err(|_| LegacyMcpRepairFailureCode::ConfigRead)?;
    if postcondition == LegacyClaudeRegistration::NotPresent {
        return Ok(true);
    }
    if !status.success() {
        return Err(LegacyMcpRepairFailureCode::CommandFailed);
    }
    Err(LegacyMcpRepairFailureCode::PostconditionFailed)
}

#[cfg(test)]
pub(crate) async fn retire_exact_legacy_user_registration_for_test(
    cli_path: &Path,
    home_dir: &Path,
    app_data_dir: &Path,
    timeout: Duration,
) -> Result<bool, LegacyMcpRepairFailureCode> {
    retire_exact_legacy_user_registration_with_timeout(
        cli_path,
        home_dir,
        app_data_dir,
        &HashMap::new(),
        timeout,
    )
    .await
}
