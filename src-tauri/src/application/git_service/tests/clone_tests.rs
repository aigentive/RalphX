use std::path::{Path, PathBuf};
use std::process::Command;

use crate::application::git_service::clone::{
    build_clone_args, classify_clone_failure, clone_subprocess_env, CloneOutcome, ClonePhase,
    GitCloneRequest, CLONE_AUTH_FAILED, CLONE_DEST_INVALID, CLONE_DEST_NOT_EMPTY, CLONE_NETWORK,
    CLONE_NOT_FOUND, CLONE_UNKNOWN,
};
use crate::application::git_service::clone_progress::parse_clone_progress;
use crate::application::GitService;
use crate::infrastructure::tool_paths::resolve_git_cli_path;

fn git(path: &Path, args: &[&str]) {
    let output = Command::new(resolve_git_cli_path())
        .args(args)
        .current_dir(path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A local source repository with one commit, usable as a clone source through a
/// `file://` URL — which the *normalizer* rejects, so these tests call the
/// process layer directly where a local fixture is legitimate.
fn source_repo(root: &Path) -> PathBuf {
    let source = root.join("source");
    std::fs::create_dir(&source).expect("fixture directory should create");
    git(&source, &["init", "--initial-branch", "main"]);
    git(&source, &["config", "user.name", "RalphX Test"]);
    git(&source, &["config", "user.email", "test@localhost"]);
    std::fs::write(source.join("README.md"), "hello\n").expect("fixture file should write");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial", "--no-gpg-sign"]);
    source
}

fn request(url: String, destination: PathBuf) -> GitCloneRequest {
    GitCloneRequest::new(url, destination)
}

// ── argument mapping ─────────────────────────────────────────────────────────

#[test]
fn default_options_emit_no_extra_flags_and_always_terminate_options() {
    let request = request("https://host/o/r.git".to_string(), PathBuf::from("/tmp/r"));

    let args = build_clone_args(&request, "https://host/o/r.git", "/tmp/r", None);

    assert_eq!(
        args,
        vec![
            "clone",
            "--progress",
            "--",
            "https://host/o/r.git",
            "/tmp/r"
        ]
    );
}

#[test]
fn advanced_options_map_to_flags_before_the_separator() {
    let mut request = request("https://host/o/r.git".to_string(), PathBuf::from("/tmp/r"));
    request.depth = Some(1);
    request.single_branch = true;
    request.recurse_submodules = true;

    let args = build_clone_args(&request, "https://host/o/r.git", "/tmp/r", Some("dev"));

    assert_eq!(
        args,
        vec![
            "clone",
            "--progress",
            "--branch",
            "dev",
            "--depth",
            "1",
            "--single-branch",
            "--recurse-submodules",
            "--",
            "https://host/o/r.git",
            "/tmp/r"
        ]
    );
    let separator = args.iter().position(|arg| arg == "--").expect("separator");
    assert!(
        args[..separator]
            .iter()
            .all(|arg| arg != "https://host/o/r.git"),
        "the URL must sit after the option terminator"
    );
}

// ── environment (proof obligation 9) ─────────────────────────────────────────

#[test]
fn clone_env_disables_every_interactive_prompt_path() {
    let env = clone_subprocess_env();
    let lookup = |key: &str| {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };

    assert_eq!(
        lookup("GIT_ASKPASS").as_deref(),
        Some(""),
        "an empty GIT_ASKPASS stops HTTPS falling back to a GUI prompt"
    );
    assert_eq!(lookup("SSH_ASKPASS").as_deref(), Some(""));
    let ssh = lookup("GIT_SSH_COMMAND").expect("GIT_SSH_COMMAND should be set");
    assert!(
        ssh.contains("BatchMode=yes"),
        "SSH must fail fast instead of waiting on a passphrase: {ssh}"
    );
    assert!(ssh.contains("StrictHostKeyChecking=accept-new"));
    assert!(
        lookup("GIT_TERMINAL_PROMPT").is_none(),
        "GIT_TERMINAL_PROMPT is already applied to every git subprocess and must not be duplicated"
    );
}

// ── failure taxonomy ─────────────────────────────────────────────────────────

#[test]
fn failure_classification_puts_auth_ahead_of_broad_patterns() {
    assert_eq!(
        classify_clone_failure(
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
        )
        .0,
        CLONE_AUTH_FAILED
    );
    // Auth text that also mentions a not-found style phrase is still auth.
    assert_eq!(
        classify_clone_failure("remote: Repository not found. fatal: Authentication failed").0,
        CLONE_AUTH_FAILED
    );
    assert_eq!(
        classify_clone_failure("remote: Repository not found.").0,
        CLONE_NOT_FOUND
    );
    assert_eq!(
        classify_clone_failure("fatal: unable to access: Could not resolve host: github.com").0,
        CLONE_NETWORK
    );
    assert_eq!(
        classify_clone_failure("fatal: something entirely new").0,
        CLONE_UNKNOWN
    );
}

// ── progress parsing ─────────────────────────────────────────────────────────

#[test]
fn progress_lines_map_to_phases_percentages_and_counts() {
    let cases: &[(&str, ClonePhase, Option<u8>, Option<u64>, Option<u64>)] = &[
        (
            "Cloning into 'repo'...",
            ClonePhase::Connecting,
            None,
            None,
            None,
        ),
        (
            "remote: Enumerating objects: 120, done.",
            ClonePhase::Counting,
            None,
            None,
            None,
        ),
        (
            "remote: Compressing objects:  46% (12/26)",
            ClonePhase::Compressing,
            Some(46),
            Some(12),
            Some(26),
        ),
        (
            "Receiving objects:  73% (438/600), 1.20 MiB | 2.00 MiB/s",
            ClonePhase::Receiving,
            Some(73),
            Some(438),
            Some(600),
        ),
        (
            "Resolving deltas: 100% (150/150), done.",
            ClonePhase::Resolving,
            Some(100),
            Some(150),
            Some(150),
        ),
        (
            "Updating files:  12% (30/250)",
            ClonePhase::CheckingOut,
            Some(12),
            Some(30),
            Some(250),
        ),
    ];

    for (line, phase, percent, received, total) in cases {
        let progress = parse_clone_progress(line).unwrap_or_else(|| panic!("{line} should parse"));
        assert_eq!(progress.phase, *phase, "phase for {line}");
        assert_eq!(progress.percent, *percent, "percent for {line}");
        assert_eq!(progress.received, *received, "received for {line}");
        assert_eq!(progress.total, *total, "total for {line}");
    }
}

#[test]
fn unrecognized_lines_are_not_progress() {
    assert!(parse_clone_progress("warning: something unrelated").is_none());
    assert!(parse_clone_progress("").is_none());
}

/// `--recurse-submodules` replays the whole phase sequence per submodule, so the
/// parser must accept a percentage going back down rather than treating it as an
/// error.
#[test]
fn repeated_phase_sequences_are_accepted_without_percent_regression_errors() {
    let replay = [
        "Receiving objects: 100% (10/10), done.",
        "Cloning into 'vendor/lib'...",
        "Receiving objects:   4% (1/25)",
    ];

    let phases: Vec<ClonePhase> = replay
        .iter()
        .filter_map(|line| parse_clone_progress(line))
        .map(|progress| progress.phase)
        .collect();

    assert_eq!(
        phases,
        vec![
            ClonePhase::Receiving,
            ClonePhase::Connecting,
            ClonePhase::Receiving
        ]
    );
}

// ── path safety (proof obligation 4) ─────────────────────────────────────────

#[tokio::test]
async fn traversal_and_relative_destinations_fail_closed() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let url = format!("file://{}", source.display());

    for destination in [
        directory.path().join("..").join("escape"),
        PathBuf::from("relative/destination"),
    ] {
        let outcome = GitService::clone_repository(
            request(url.clone(), destination.clone()),
            std::future::pending::<()>(),
            |_progress| {},
        )
        .await;

        let CloneOutcome::Failed { code, .. } = outcome else {
            panic!("destination {} should fail closed", destination.display());
        };
        assert_eq!(code, CLONE_DEST_INVALID, "for {}", destination.display());
    }
    assert!(
        !directory.path().parent().unwrap().join("escape").exists(),
        "nothing may be created outside the chosen parent"
    );
}

#[tokio::test]
async fn a_populated_destination_is_refused_before_spawn_and_never_deleted() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let occupied = directory.path().join("occupied");
    std::fs::create_dir(&occupied).expect("fixture directory should create");
    std::fs::write(occupied.join("important.txt"), "user data").expect("fixture file should write");

    let outcome = GitService::clone_repository(
        request(format!("file://{}", source.display()), occupied.clone()),
        std::future::pending::<()>(),
        |_progress| {},
    )
    .await;

    let CloneOutcome::Failed { code, .. } = outcome else {
        panic!("a populated destination must be refused");
    };
    assert_eq!(code, CLONE_DEST_NOT_EMPTY);
    assert!(
        occupied.join("important.txt").exists(),
        "a refused destination must never be cleaned up"
    );
}

#[tokio::test]
async fn a_symlinked_parent_resolves_to_its_real_target() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let real_parent = directory.path().join("real-parent");
    std::fs::create_dir(&real_parent).expect("fixture directory should create");
    let linked_parent = directory.path().join("linked-parent");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_parent, &linked_parent).expect("symlink should create");

    let outcome = GitService::clone_repository(
        request(
            format!("file://{}", source.display()),
            linked_parent.join("checkout"),
        ),
        std::future::pending::<()>(),
        |_progress| {},
    )
    .await;

    let CloneOutcome::Completed { destination, .. } = outcome else {
        panic!("cloning through a symlinked parent should succeed, got {outcome:?}");
    };
    assert!(
        destination.starts_with(
            real_parent
                .canonicalize()
                .expect("real parent should canonicalize")
        ),
        "clone landed at {} instead of under the real parent",
        destination.display()
    );
    assert!(real_parent.join("checkout").join(".git").is_dir());
}

// ── end-to-end clone lifecycle (proof obligation 5) ──────────────────────────

#[tokio::test]
async fn a_successful_clone_reports_phases_and_lands_a_working_repository() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let destination = directory.path().join("clone");
    let mut phases = Vec::new();

    let outcome = GitService::clone_repository(
        request(format!("file://{}", source.display()), destination.clone()),
        std::future::pending::<()>(),
        |progress| phases.push(progress.phase),
    )
    .await;

    let CloneOutcome::Completed {
        destination: landed,
        default_branch,
    } = outcome
    else {
        panic!("clone should succeed, got {outcome:?}");
    };
    assert!(
        landed.join(".git").is_dir(),
        "a real repository should exist"
    );
    assert!(
        landed.join("README.md").is_file(),
        "the worktree should be checked out"
    );
    assert_eq!(default_branch.as_deref(), Some("main"));
    assert!(
        phases.contains(&ClonePhase::Connecting),
        "at least the connecting phase should be reported, got {phases:?}"
    );
}

#[tokio::test]
async fn a_shallow_clone_succeeds_against_a_local_fixture() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let destination = directory.path().join("shallow");
    let mut request = request(format!("file://{}", source.display()), destination.clone());
    request.depth = Some(1);

    let outcome =
        GitService::clone_repository(request, std::future::pending::<()>(), |_progress| {}).await;

    assert!(
        matches!(outcome, CloneOutcome::Completed { .. }),
        "a --depth 1 clone should succeed, got {outcome:?}"
    );
    assert!(destination.join(".git").is_dir());
}

#[tokio::test]
async fn cancelling_removes_a_destination_ralphx_created() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let destination = directory.path().join("cancelled");

    let outcome = GitService::clone_repository(
        request(format!("file://{}", source.display()), destination.clone()),
        std::future::ready(()),
        |_progress| {},
    )
    .await;

    let CloneOutcome::Cancelled { cleaned_up } = outcome else {
        panic!("an already-cancelled clone should report Cancelled, got {outcome:?}");
    };
    assert!(cleaned_up, "cancellation must clean up what it created");
    assert!(
        !destination.exists(),
        "a destination RalphX created must not survive cancellation"
    );
}

#[tokio::test]
async fn cancelling_empties_but_keeps_a_pre_existing_destination() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let destination = directory.path().join("users-own-empty-folder");
    std::fs::create_dir(&destination).expect("fixture directory should create");

    let outcome = GitService::clone_repository(
        request(format!("file://{}", source.display()), destination.clone()),
        std::future::ready(()),
        |_progress| {},
    )
    .await;

    assert!(matches!(
        outcome,
        CloneOutcome::Cancelled { cleaned_up: true }
    ));
    assert!(
        destination.is_dir(),
        "a folder the user already had must survive"
    );
    assert!(
        std::fs::read_dir(&destination)
            .expect("destination should read")
            .next()
            .is_none(),
        "but its contents should be gone"
    );
}

#[tokio::test]
async fn a_failed_clone_cleans_up_and_classifies_the_failure() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let destination = directory.path().join("missing-source");
    let missing = directory.path().join("no-such-repo");

    let outcome = GitService::clone_repository(
        request(format!("file://{}", missing.display()), destination.clone()),
        std::future::pending::<()>(),
        |_progress| {},
    )
    .await;

    let CloneOutcome::Failed {
        code, cleaned_up, ..
    } = outcome
    else {
        panic!("cloning a missing source should fail, got {outcome:?}");
    };
    assert_eq!(code, CLONE_NOT_FOUND);
    assert!(cleaned_up);
    assert!(
        !destination.exists(),
        "a failed clone must not leave a partial destination behind"
    );
}
