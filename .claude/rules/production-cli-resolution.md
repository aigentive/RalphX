> **Maintainer note:** Keep this compact. Add binaries here only when production app runtime launches them.

# Production CLI Resolution

## Non-Negotiables

- macOS app launches from Finder/Homebrew have a stripped PATH; production subprocesses MUST NOT depend on terminal PATH.
- Production runtime CLI launches MUST resolve through `src-tauri/src/infrastructure/tool_paths.rs` or a helper that delegates to it.
- Do not add bare production spawns like `Command::new("git")`, `Command::new("gh")`, `Command::new("claude")`, `Command::new("codex")`, `Command::new("node")`, `Command::new("sh")`, `Command::new("rm")`, `Command::new("ps")`, `Command::new("lsof")`, `Command::new("pgrep")`, or `Command::new("pkill")`.
- Resolver order: current PATH/`which` → fixed Homebrew/system paths → login-shell `command -v` for user-managed installs → bare name fallback only as last resort.
- Shell/env/config-derived executable paths must be shape-validated before process launch; pair changes with focused resolver tests.

## Current Production Binary Inventory

| Binary | Runtime Use |
|---|---|
| `claude` | Claude harness chat/execution and MCP registration |
| `codex` | Codex harness chat/execution and capability probes |
| `gh` | GitHub auth, PR polling, PR/release operations |
| `git` | repository state, diffs, worktrees, merge/cleanup |
| `node` | bundled RalphX MCP servers |
| `sh` | project setup/validation command execution |
| `rm` | worktree cleanup fallback |
| `ps`, `lsof`, `pgrep`, `pkill` | process inspection and cleanup |
| `tasklist`, `taskkill` | Windows process inspection and cleanup |
