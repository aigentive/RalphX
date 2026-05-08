use std::path::PathBuf;

#[cfg(test)]
#[path = "node_utils_tests.rs"]
mod tests;

/// Resolve the Node.js binary path for macOS GUI app contexts.
///
/// macOS apps launched from Finder/Dock have a stripped PATH (/usr/bin:/bin only),
/// so `which::which` may fail. Falls back to common install paths.
///
/// Priority:
/// 1. `RALPHX_NODE_PATH` env var (explicit override)
/// 2. `which::which("node")` (PATH-based lookup)
/// 3. `$NVM_BIN/node` and `$VOLTA_HOME/bin/node`
/// 4. `/opt/homebrew/bin/node` and `/usr/local/bin/node`
/// 5. login shell `command -v node` (covers nvm/asdf/volta shell init)
/// 6. `"node"` (last resort — rely on whatever PATH the process has)
///
/// # Errors
///
/// This function never errors — it always returns a path, falling back to the bare
/// `"node"` string if nothing else is found.
pub fn find_node_binary() -> PathBuf {
    crate::infrastructure::tool_paths::resolve_node_cli_path()
}
