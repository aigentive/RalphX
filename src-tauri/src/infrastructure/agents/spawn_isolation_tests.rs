use super::*;

// ── send_signal_to_group safety guards ───────────────────────────────────

#[cfg(unix)]
#[test]
fn send_signal_to_group_refuses_pid_zero() {
    use nix::sys::signal::Signal;
    // PID 0 in killpg(2) targets the caller's own process group — would
    // signal Tauri itself. The safety guard MUST short-circuit before the
    // syscall is issued. Reaching here without a panic and without the test
    // process receiving SIGTERM is the assertion.
    send_signal_to_group(0, Signal::SIGTERM);
}

#[cfg(unix)]
#[test]
fn send_signal_to_group_refuses_pid_one() {
    use nix::sys::signal::Signal;
    // PID 1 is init / launchd — signalling its group would be catastrophic.
    send_signal_to_group(1, Signal::SIGTERM);
}

#[cfg(unix)]
#[test]
fn send_signal_to_group_handles_nonexistent_group_gracefully() {
    use nix::sys::signal::Signal;
    // Pick a PID astronomically unlikely to exist on a healthy system —
    // macOS PID_MAX is < 100k by default, so a value near u32::MAX cannot
    // be a live PGID. Exercises the ESRCH match arm: silent return, no
    // panic, no warning escalation.
    let definitely_dead_pid = 999_999_999u32;
    send_signal_to_group(definitely_dead_pid, Signal::SIGTERM);
}

// ── install_setsid_pre_exec smoke ────────────────────────────────────────

#[test]
fn install_setsid_pre_exec_does_not_panic_on_std_command() {
    // The pre_exec closure only runs in the forked child after spawn — here
    // we just verify the wiring registers cleanly on the parent-side
    // Command builder. Body coverage for the unsafe block.
    let mut cmd = std::process::Command::new("/bin/true");
    install_setsid_pre_exec(&mut cmd);
}

#[test]
fn install_setsid_pre_exec_tokio_delegates_to_std() {
    // Confirms the tokio wrapper compiles and reaches the std helper. The
    // delegation goes through Command::as_std_mut so this exercises the
    // tokio-specific code path explicitly.
    let mut cmd = tokio::process::Command::new("/bin/true");
    install_setsid_pre_exec_tokio(&mut cmd);
}

// ── Capability test: real subprocess + real group kill ────────────────────

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires subprocess spawn + signal capability"]
async fn setsid_pre_exec_isolates_child_and_killpg_reaps_it() {
    use nix::sys::signal::Signal;
    use std::time::Duration;
    use tokio::time::timeout;

    // Spawn a long-running child with setsid → child's PID is its PGID.
    let mut cmd = tokio::process::Command::new("/bin/sleep");
    cmd.arg("60");
    install_setsid_pre_exec_tokio(&mut cmd);
    let mut child = cmd.spawn().expect("spawn sleep");
    let pid = child.id().expect("child has pid");

    // Send SIGTERM to the child's process group. With setsid this targets
    // ONLY the spawned tree, not the test process's group.
    send_signal_to_group(pid, Signal::SIGTERM);

    // Child must exit within a short window.
    let exit = timeout(Duration::from_secs(2), child.wait())
        .await
        .expect("child should be reaped after killpg(SIGTERM)")
        .expect("wait failed");

    // SIGTERM normally produces signal exit (no exit code). The exact code
    // varies by platform; the assertion is just "process is no longer alive."
    assert!(
        !exit.success(),
        "sleep should not have exited successfully after SIGTERM"
    );
}
