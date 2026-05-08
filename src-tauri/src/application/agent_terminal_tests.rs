use std::ffi::OsStr;

use super::agent_terminal::{
    build_terminal_command_for_test, terminal_env_path_from_parts_for_test,
};

#[test]
fn terminal_command_sets_real_pty_environment() {
    let command = build_terminal_command_for_test(std::path::Path::new("/tmp/project"));

    assert_eq!(command.get_env("TERM"), Some(OsStr::new("xterm-256color")));
    assert_eq!(command.get_env("COLORTERM"), Some(OsStr::new("truecolor")));
    assert_eq!(command.get_env("PWD"), Some(OsStr::new("/tmp/project")));
}

#[cfg(target_os = "macos")]
#[test]
fn terminal_command_launches_zsh_as_login_shell() {
    let command = build_terminal_command_for_test(std::path::Path::new("/tmp/project"));
    let argv = command.get_argv();

    assert_eq!(
        argv.first().and_then(|value| value.to_str()),
        Some("/bin/zsh")
    );
    assert_eq!(argv.get(1).and_then(|value| value.to_str()), Some("-l"));
}

#[test]
fn terminal_path_preserves_existing_path_and_adds_common_dev_bins() {
    let path = terminal_env_path_from_parts_for_test(
        Some(OsStr::new("/existing/bin:/usr/bin")),
        Some(std::path::Path::new("/Users/example")),
    );
    let path = path.to_string_lossy();

    assert!(path.contains("/existing/bin"));
    assert!(path.contains("/opt/homebrew/bin"));
    assert!(path.contains("/Users/example/.rbenv/bin"));
    assert!(path.contains("/Users/example/.asdf/shims"));
}
