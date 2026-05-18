//! Process-group isolation for spawned agent CLIs (Claude, Codex, future
//! harnesses). Provider-neutral per the multi-harness rules — both harnesses
//! reach the same helper instead of branching `claude|codex` at every spawn
//! site.

/// On Unix, put the spawned CLI (and everything it spawns — Task subagents,
/// the stdio MCP server, etc.) into its own process group via `setsid()`.
/// Two consequences:
///
/// 1. **Whole-tree shutdown**: `killpg(SIGTERM)` from the Tauri exit handler
///    reaches the CLI AND its descendants, giving the MCP server a chance to
///    close keep-alive sockets cleanly instead of being orphaned mid-burst.
///    That's the actual lever against the stuck-TIME_WAIT incident.
/// 2. **Blast-radius isolation**: without setsid, the spawned tree shares
///    Tauri's process group. A group-targeted kill would risk hitting the
///    app itself. Putting each agent in its own group makes group kills safe.
///
/// External MCP supervisor uses the same pattern
/// (`src-tauri/src/infrastructure/external_mcp_supervisor.rs`).
///
/// No-op on Windows — Windows process groups don't compose the same way;
/// `taskkill /T` walks the process tree by PID instead.
pub fn install_setsid_pre_exec(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: pre_exec runs in the forked child between fork() and exec().
        // setsid() is async-signal-safe and does not touch any heap state that
        // could be corrupted by the fork. Failure surfaces as a spawn failure
        // through std::io::Error, which the caller already handles.
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map(|_| ())
                    .map_err(std::io::Error::from)
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = cmd;
    }
}

/// Tokio convenience wrapper: same semantics as
/// [`install_setsid_pre_exec`], but takes a `tokio::process::Command`.
pub fn install_setsid_pre_exec_tokio(cmd: &mut tokio::process::Command) {
    install_setsid_pre_exec(cmd.as_std_mut());
}

/// Send a signal to the process group whose PGID equals `pid`.
///
/// Only meaningful when the child was spawned with [`install_setsid_pre_exec`]
/// so that its PID is also its PGID. Without setsid the negative PID in
/// `kill(2)` would either fail (no group with that PGID exists) or, worse,
/// signal an unrelated group — which is exactly the foot-gun setsid prevents.
///
/// Defensive PID guard rejects PID 0 / 1 even if a caller passes them.
#[cfg(unix)]
pub fn send_signal_to_group(pid: u32, signal: nix::sys::signal::Signal) {
    use nix::sys::signal;
    use nix::unistd::Pid;

    if pid <= 1 {
        tracing::warn!(pid, "refusing to send {signal} to PID {pid} (safety guard)");
        return;
    }

    match signal::kill(Pid::from_raw(-(pid as i32)), signal) {
        Ok(()) => {}
        Err(nix::errno::Errno::ESRCH) => {
            // Group already gone — race vs. natural exit, harmless.
        }
        Err(e) => {
            tracing::warn!(pid, %signal, error = %e, "killpg failed");
        }
    }
}

/// No-op stub on Windows so callers can stay platform-neutral.
#[cfg(not(unix))]
pub fn send_signal_to_group(_pid: u32, _signal: ()) {}

#[cfg(test)]
#[path = "spawn_isolation_tests.rs"]
mod tests;
