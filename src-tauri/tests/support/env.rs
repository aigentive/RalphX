use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn assert_nextest() {
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
    );
}

pub struct EnvVarGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    pub fn set(key: &'static str, value: impl Into<OsString>) -> Self {
        assert_nextest();
        let original = std::env::var_os(key);
        std::env::set_var(key, value.into());
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original.as_ref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

pub fn prepend_to_path(dir: &Path) -> EnvVarGuard {
    let previous_path = std::env::var_os("PATH");
    let mut paths = vec![PathBuf::from(dir)];
    if let Some(existing) = previous_path.as_ref() {
        paths.extend(std::env::split_paths(existing));
    }
    let joined_path = std::env::join_paths(paths).expect("PATH entries should join");
    EnvVarGuard::set("PATH", joined_path)
}
