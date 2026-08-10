use std::path::Path;
use std::process::Command;

mod bootstrap_tests;
mod branch_tests;
mod commit_tests;
mod identity_tests;
mod merge_tests;
mod query_tests;
mod rebase_tests;
mod state_query_tests;
mod worktree_restart_tests;
mod worktree_tests;

fn init_test_repo(dir: &Path) {
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .unwrap();
}
