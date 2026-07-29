use super::*;
use crate::domain::entities::{ProjectId, Task};
use crate::domain::services::MemoryRunningAgentRegistry;

fn create_test_validator() -> ResumeValidator {
    let registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    ResumeValidator::new(registry)
}

fn create_test_task() -> Task {
    Task::new(ProjectId::new(), "Test Task".to_string())
}

fn create_test_project() -> Project {
    Project::new("Test Project".to_string(), "/tmp/test".to_string())
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should start");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_validation_result_new_is_valid() {
    let result = ResumeValidationResult::new();
    assert!(result.is_valid);
    assert!(result.warnings.is_empty());
    assert!(result.errors.is_empty());
}

#[test]
fn test_validation_result_with_warning() {
    let result = ResumeValidationResult::new().with_warning("Test warning");
    assert!(result.is_valid);
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0], "Test warning");
}

#[test]
fn test_validation_result_with_error() {
    let result = ResumeValidationResult::new().with_error("Test error");
    assert!(!result.is_valid);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0], "Test error");
}

#[test]
fn test_validation_result_merge() {
    let mut result1 = ResumeValidationResult::new().with_warning("Warning 1");
    let result2 = ResumeValidationResult::new()
        .with_warning("Warning 2")
        .with_error("Error 1");

    result1.merge(&result2);

    assert!(!result1.is_valid);
    assert_eq!(result1.warnings.len(), 2);
    assert_eq!(result1.errors.len(), 1);
}

#[tokio::test]
async fn test_validate_task_without_branch() {
    let validator = create_test_validator();
    let task = create_test_task();
    let project = create_test_project();

    let result = validator.validate(&task, &project, None).await.unwrap();

    // Task without branch should validate (no git isolation)
    assert!(result.is_valid);
}

#[tokio::test]
async fn validate_checks_both_existing_branches_and_rejects_a_missing_task_branch() {
    let temp = tempfile::tempdir().expect("resume validation repository");
    run_git(temp.path(), &["init", "-b", "main"]);
    run_git(temp.path(), &["config", "user.email", "test@example.com"]);
    run_git(temp.path(), &["config", "user.name", "RalphX Test"]);
    run_git(temp.path(), &["commit", "--allow-empty", "-m", "base"]);
    run_git(temp.path(), &["branch", "ralphx/resume-validation"]);
    run_git(
        temp.path(),
        &["commit", "--allow-empty", "-m", "base ahead"],
    );

    let validator = create_test_validator();
    let mut task = create_test_task();
    task.task_branch = Some("ralphx/resume-validation".to_string());
    let mut project = Project::new(
        "Resume validation".to_string(),
        temp.path().to_string_lossy().into_owned(),
    );
    project.base_branch = Some("main".to_string());

    let moved_base = validator
        .validate(&task, &project, None)
        .await
        .expect("existing branch validation");
    assert!(moved_base.is_valid);
    assert!(
        moved_base
            .warnings
            .iter()
            .any(|warning| warning.contains("has new commits")),
        "the exact base/task branches should drive the ahead warning"
    );

    task.task_branch = Some("ralphx/missing-resume-validation".to_string());
    let missing = validator
        .validate(&task, &project, None)
        .await
        .expect("missing branch is a validation result");
    assert!(!missing.is_valid);
    assert!(missing
        .errors
        .iter()
        .any(|error| error.contains("does not exist")));
}

#[tokio::test]
async fn test_cleanup_orphan_agents_no_agents() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let validator = ResumeValidator::new(registry.clone());
    let task = create_test_task();

    let result = validator.cleanup_orphan_agents(&task).await;

    assert!(result.is_valid);
    assert!(result.warnings.is_empty());
}

#[tokio::test]
async fn test_cleanup_orphan_agents_with_running_agent() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let validator = ResumeValidator::new(registry.clone());
    let task = create_test_task();

    // Use the test helper's pid=0 placeholder so cleanup never targets a real OS process.
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    registry.set_running(key.clone()).await;

    let result = validator.cleanup_orphan_agents(&task).await;

    assert!(result.is_valid);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("Stopped 1 orphan agent"));

    // Agent should be unregistered
    assert!(!registry.is_running(&key).await);
}

#[tokio::test]
async fn test_cleanup_multiple_orphan_agents() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let validator = ResumeValidator::new(registry.clone());
    let task = create_test_task();

    // Use pid=0 placeholder entries so cleanup stays hermetic on shared CI hosts.
    for context_type in &["task_execution", "review"] {
        let key = RunningAgentKey::new(*context_type, task.id.as_str());
        registry.set_running(key).await;
    }

    let result = validator.cleanup_orphan_agents(&task).await;

    assert!(result.is_valid);
    assert!(result.warnings[0].contains("Stopped 2 orphan agent"));
}

#[test]
fn test_truncate_status_output_short() {
    let validator = create_test_validator();
    let status = "M file1.txt\nM file2.txt";
    let truncated = validator.truncate_status_output(status);
    assert_eq!(truncated, status);
}

#[test]
fn test_truncate_status_output_long() {
    let validator = create_test_validator();
    let lines: Vec<String> = (0..20).map(|i| format!("M file{}.txt", i)).collect();
    let status = lines.join("\n");
    let truncated = validator.truncate_status_output(&status);

    assert!(truncated.contains("... and 10 more files"));
    assert!(!truncated.contains("file19.txt"));
}

// ── IPR cleanup tests ──────────────────────────────────────────────────

/// Helper for creating test stdin pipes (real subprocess for IPR testing)
async fn create_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}

/// cleanup_orphan_agents removes IPR entries alongside running agent registry.
/// Verify: after cleanup, both IPR and registry are clean.
#[tokio::test]
async fn test_cleanup_orphan_agents_with_ipr_removes_ipr_entries() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let validator =
        ResumeValidator::new(registry.clone()).with_interactive_process_registry(Arc::clone(&ipr));
    let task = create_test_task();

    // Register agent in both running registry and IPR
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    registry.set_running(key.clone()).await;

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("task_execution", task.id.as_str());
    ipr.register(ipr_key.clone(), stdin).await;
    assert!(
        ipr.has_process(&ipr_key).await,
        "Precondition: IPR has entry"
    );

    let result = validator.cleanup_orphan_agents(&task).await;

    assert!(result.is_valid);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("Stopped 1 orphan agent"));

    // Both must be cleaned up
    assert!(
        !registry.is_running(&key).await,
        "Agent must be unregistered from running registry"
    );
    assert!(
        !ipr.has_process(&ipr_key).await,
        "IPR entry must be removed"
    );
    assert_eq!(ipr.count().await, 0, "IPR must be empty");
}

/// cleanup_orphan_agents removes IPR entries for ALL context types
/// (task_execution, review, merge).
#[tokio::test]
async fn test_cleanup_orphan_agents_with_ipr_handles_multiple_context_types() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let validator =
        ResumeValidator::new(registry.clone()).with_interactive_process_registry(Arc::clone(&ipr));
    let task = create_test_task();

    // Register agents in multiple context types
    let context_types = ["task_execution", "review"];
    for context_type in &context_types {
        let key = RunningAgentKey::new(*context_type, task.id.as_str());
        registry.set_running(key).await;

        let (stdin, _child) = create_test_stdin().await;
        let ipr_key = InteractiveProcessKey::new(*context_type, task.id.as_str());
        ipr.register(ipr_key, stdin).await;
    }

    assert_eq!(ipr.count().await, 2, "Precondition: both IPR entries exist");

    let result = validator.cleanup_orphan_agents(&task).await;

    assert!(result.is_valid);
    assert!(result.warnings[0].contains("Stopped 2 orphan agent"));

    // All IPR entries must be removed
    assert_eq!(ipr.count().await, 0, "All IPR entries must be removed");
    for context_type in &context_types {
        let ipr_key = InteractiveProcessKey::new(*context_type, task.id.as_str());
        assert!(
            !ipr.has_process(&ipr_key).await,
            "IPR entry for {} must be removed",
            context_type
        );
    }
}

/// Without IPR set on validator, cleanup still works (backward compat).
#[tokio::test]
async fn test_cleanup_orphan_agents_without_ipr_still_cleans_registry() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    // No IPR set — validator.interactive_process_registry = None
    let validator = ResumeValidator::new(registry.clone());
    let task = create_test_task();

    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    registry.set_running(key.clone()).await;

    let result = validator.cleanup_orphan_agents(&task).await;

    assert!(result.is_valid);
    assert!(result.warnings[0].contains("Stopped 1 orphan agent"));
    assert!(
        !registry.is_running(&key).await,
        "Agent must still be stopped"
    );
}
