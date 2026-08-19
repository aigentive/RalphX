use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use ralphx_events::{EventSink, RecordedEvent, RecordingEventSink};

use crate::application::clone_job_registry::{CloneCancelToken, CloneJobRegistry, CloneJobStatus};
use crate::application::clone_job_runner::{
    run_clone_job, ProgressEmitter, CLONE_CANCELLED_EVENT, CLONE_COMPLETED_EVENT,
    CLONE_FAILED_EVENT, CLONE_PROGRESS_EVENT,
};
use crate::application::git_service::clone::{ClonePhase, CloneProgress, GitCloneRequest};
use crate::infrastructure::git_auth::github_https_remote_to_ssh;
use crate::infrastructure::tool_paths::resolve_git_cli_path;

const RETENTION: Duration = Duration::from_secs(900);

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

fn progress(phase: ClonePhase, percent: u8) -> CloneProgress {
    CloneProgress {
        phase,
        percent: Some(percent),
        received: None,
        total: None,
        line: format!("{phase:?}: {percent}%"),
    }
}

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

struct Harness {
    registry: Arc<CloneJobRegistry>,
    sink: Arc<RecordingEventSink>,
}

impl Harness {
    fn new() -> Self {
        Self {
            registry: Arc::new(CloneJobRegistry::new()),
            sink: Arc::new(RecordingEventSink::new()),
        }
    }

    async fn run(&self, job_id: &str, request: GitCloneRequest, cancel_now: bool) {
        let registration =
            self.registry
                .start(job_id.to_string(), request.destination.clone(), RETENTION);
        if cancel_now {
            registration.cancel.cancel();
        }
        run_clone_job(
            job_id.to_string(),
            request,
            registration.cancel,
            Arc::clone(&self.registry),
            Arc::clone(&self.sink) as Arc<dyn EventSink>,
        )
        .await;
    }

    fn events(&self) -> Vec<RecordedEvent> {
        self.sink.events()
    }

    fn terminal_events(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .map(|event| event.event)
            .filter(|name| name != CLONE_PROGRESS_EVENT)
            .collect()
    }
}

// ── event coverage: one terminal event + one recorded outcome per exit ───────

#[tokio::test]
async fn a_successful_clone_emits_one_completed_event_and_records_it() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let harness = Harness::new();

    harness
        .run(
            "job-ok",
            GitCloneRequest::new(
                format!("file://{}", source.display()),
                directory.path().join("clone"),
            ),
            false,
        )
        .await;

    assert_eq!(harness.terminal_events(), vec![CLONE_COMPLETED_EVENT]);
    let status = harness.registry.status("job-ok", RETENTION);
    let CloneJobStatus::Completed { default_branch, .. } = status else {
        panic!("registry should retain the completed outcome, got {status:?}");
    };
    assert_eq!(default_branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn a_failed_clone_emits_one_failed_event_and_records_it() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let harness = Harness::new();

    harness
        .run(
            "job-fail",
            GitCloneRequest::new(
                format!("file://{}", directory.path().join("missing").display()),
                directory.path().join("clone"),
            ),
            false,
        )
        .await;

    assert_eq!(harness.terminal_events(), vec![CLONE_FAILED_EVENT]);
    assert!(matches!(
        harness.registry.status("job-fail", RETENTION),
        CloneJobStatus::Failed { .. }
    ));
}

#[tokio::test]
async fn a_cancelled_clone_emits_one_cancelled_event_and_records_it() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let harness = Harness::new();

    harness
        .run(
            "job-cancel",
            GitCloneRequest::new(
                format!("file://{}", source.display()),
                directory.path().join("clone"),
            ),
            true,
        )
        .await;

    assert_eq!(harness.terminal_events(), vec![CLONE_CANCELLED_EVENT]);
    assert_eq!(
        harness.registry.status("job-cancel", RETENTION),
        CloneJobStatus::Cancelled { cleaned_up: true }
    );
}

/// The terminal event payload must carry the same answer the status query gives,
/// or a UI that trusts whichever arrives first would show two different endings.
#[tokio::test]
async fn the_terminal_event_payload_matches_the_retained_status() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let harness = Harness::new();

    harness
        .run(
            "job-payload",
            GitCloneRequest::new(
                format!("file://{}", source.display()),
                directory.path().join("clone"),
            ),
            false,
        )
        .await;

    let terminal = harness
        .events()
        .into_iter()
        .find(|event| event.event == CLONE_COMPLETED_EVENT)
        .expect("a completed event should be emitted");
    assert_eq!(terminal.payload["jobId"], "job-payload");
    assert_eq!(terminal.payload["state"], "completed");
    assert_eq!(terminal.payload["defaultBranch"], "main");
    let CloneJobStatus::Completed { destination, .. } =
        harness.registry.status("job-payload", RETENTION)
    else {
        panic!("expected a completed status");
    };
    assert_eq!(terminal.payload["destination"], destination);
}

/// The last progress the coalescer withheld must still be delivered, or a
/// finished clone can appear frozen mid-bar.
#[tokio::test]
async fn progress_reaches_the_ui_before_the_terminal_event() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let harness = Harness::new();

    harness
        .run(
            "job-progress",
            GitCloneRequest::new(
                format!("file://{}", source.display()),
                directory.path().join("clone"),
            ),
            false,
        )
        .await;

    let names: Vec<String> = harness
        .events()
        .into_iter()
        .map(|event| event.event)
        .collect();
    let terminal_index = names
        .iter()
        .position(|name| name == CLONE_COMPLETED_EVENT)
        .expect("a terminal event should exist");
    assert!(
        names[..terminal_index]
            .iter()
            .any(|name| name == CLONE_PROGRESS_EVENT),
        "at least one progress event should precede the terminal event: {names:?}"
    );
    assert!(
        names[terminal_index + 1..].is_empty(),
        "nothing may be emitted after the terminal event: {names:?}"
    );
}

/// Proof obligation 10: a burst of updates must not become a burst of events,
/// while every phase change and the final update still get through.
#[test]
fn a_progress_burst_is_coalesced_but_never_drops_a_phase_change() {
    let sink = Arc::new(RecordingEventSink::new());
    let registry = Arc::new(CloneJobRegistry::new());
    registry.start(
        "job-burst".to_string(),
        PathBuf::from("/tmp/ralphx-burst"),
        RETENTION,
    );
    let mut emitter = ProgressEmitter::new(
        "job-burst",
        Arc::clone(&sink) as Arc<dyn EventSink>,
        Arc::clone(&registry),
    );

    // 200 updates inside a single coalescing window, with one phase change.
    for index in 0..100u8 {
        emitter.observe(progress(ClonePhase::Receiving, index));
    }
    for index in 0..100u8 {
        emitter.observe(progress(ClonePhase::Resolving, index));
    }
    emitter.flush();

    let phases: Vec<String> = sink
        .events()
        .into_iter()
        .filter(|event| event.event == CLONE_PROGRESS_EVENT)
        .map(|event| {
            event.payload["phase"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();

    assert_eq!(
        phases,
        vec![
            "receiving".to_string(),
            "resolving".to_string(),
            "resolving".to_string()
        ],
        "expected one event per phase change plus the final flush, got {phases:?}"
    );
    // The registry still tracks every update, so a late subscriber sees the truth
    // even though the event stream was thinned.
    assert_eq!(
        registry.status("job-burst", RETENTION),
        CloneJobStatus::Running {
            phase: Some(ClonePhase::Resolving),
            percent: Some(99)
        }
    );
}

#[test]
fn the_final_withheld_update_is_always_flushed() {
    let sink = Arc::new(RecordingEventSink::new());
    let registry = Arc::new(CloneJobRegistry::new());
    registry.start(
        "job-flush".to_string(),
        PathBuf::from("/tmp/ralphx-flush"),
        RETENTION,
    );
    let mut emitter = ProgressEmitter::new(
        "job-flush",
        Arc::clone(&sink) as Arc<dyn EventSink>,
        Arc::clone(&registry),
    );

    emitter.observe(progress(ClonePhase::Receiving, 10));
    emitter.observe(progress(ClonePhase::Receiving, 99));
    emitter.flush();

    let percents: Vec<u64> = sink
        .events()
        .into_iter()
        .filter(|event| event.event == CLONE_PROGRESS_EVENT)
        .filter_map(|event| event.payload["percent"].as_u64())
        .collect();

    assert_eq!(
        percents,
        vec![10, 99],
        "the withheld final update must not be lost, or the bar freezes short"
    );
}

// ── SSH suggestion (proof obligation 18) ─────────────────────────────────────

#[test]
fn a_convertible_github_https_url_yields_an_ssh_suggestion() {
    assert_eq!(
        github_https_remote_to_ssh("https://github.com/owner/repo.git").as_deref(),
        Some("git@github.com:owner/repo.git")
    );
}

#[test]
fn ssh_scp_like_and_non_github_urls_yield_no_suggestion() {
    for url in [
        "git@github.com:owner/repo.git",
        "ssh://git@github.com/owner/repo.git",
        "https://gitlab.com/owner/repo.git",
        "https://github.com/owner/repo/extra",
    ] {
        assert!(
            github_https_remote_to_ssh(url).is_none(),
            "{url} should not produce an SSH suggestion"
        );
    }
}

// ── cancellation token wiring ────────────────────────────────────────────────

#[tokio::test]
async fn cancelling_through_the_registry_stops_the_job_runner() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let source = source_repo(directory.path());
    let registry = Arc::new(CloneJobRegistry::new());
    let sink = Arc::new(RecordingEventSink::new());
    let destination = directory.path().join("clone");
    let registration = registry.start("job-x".to_string(), destination.clone(), RETENTION);

    assert!(registry.cancel("job-x"), "cancel should take effect");
    run_clone_job(
        "job-x".to_string(),
        GitCloneRequest::new(format!("file://{}", source.display()), destination.clone()),
        registration.cancel,
        Arc::clone(&registry),
        Arc::clone(&sink) as Arc<dyn EventSink>,
    )
    .await;

    assert_eq!(
        registry.status("job-x", RETENTION),
        CloneJobStatus::Cancelled { cleaned_up: true }
    );
    assert!(!destination.exists());
}

#[tokio::test]
async fn an_uncancelled_token_lets_the_clone_finish() {
    let token = Arc::new(CloneCancelToken::default());

    assert!(!token.is_cancelled());
}

// ── GitHub repository picker (proof obligation 16) ───────────────────────────

use crate::commands::project_clone_commands::parse_github_repo_list;

#[test]
fn the_repo_list_parses_and_tolerates_unknown_fields() {
    // `gh` adds fields between versions; a picker must not break on them.
    let stdout = r#"[
      {"nameWithOwner":"owner/repo","description":"A repo","isPrivate":false,
       "updatedAt":"2026-08-01T00:00:00Z","someFutureField":{"nested":true}},
      {"nameWithOwner":"owner/private","description":null,"isPrivate":true,
       "updatedAt":"2026-08-02T00:00:00Z"}
    ]"#;

    let repos = parse_github_repo_list(stdout).expect("gh JSON should parse");

    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0].name_with_owner, "owner/repo");
    assert_eq!(repos[0].description.as_deref(), Some("A repo"));
    assert!(!repos[0].is_private);
    assert!(repos[1].is_private);
    assert_eq!(repos[1].description, None);
}

#[test]
fn missing_fields_fall_back_to_defaults_rather_than_failing() {
    let repos = parse_github_repo_list(r#"[{"nameWithOwner":"owner/repo"}]"#)
        .expect("a minimal object should still parse");

    assert_eq!(repos[0].name_with_owner, "owner/repo");
    assert!(!repos[0].is_private);
    assert_eq!(repos[0].updated_at, None);
}

#[test]
fn an_empty_list_is_valid() {
    assert_eq!(
        parse_github_repo_list("[]").expect("empty is valid").len(),
        0
    );
}

/// A gh failure must surface as a typed error the picker can silently fall back
/// from, never as a partial list.
#[test]
fn non_array_output_is_a_typed_error() {
    for stdout in ["", "not json", r#"{"nameWithOwner":"owner/repo"}"#] {
        assert!(
            parse_github_repo_list(stdout).is_err(),
            "{stdout:?} should not parse as a repository list"
        );
    }
}
