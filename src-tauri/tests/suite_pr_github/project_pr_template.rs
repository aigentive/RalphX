use ralphx_lib::application::project_pr_template;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::project_commands::{
    read_pr_template_for_state, write_pr_template_for_state,
};
use ralphx_lib::domain::entities::Project;

fn template_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".github").join("pull_request_template.md")
}

fn uppercase_template_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".github").join("PULL_REQUEST_TEMPLATE.md")
}

fn actual_template_names(root: &std::path::Path) -> Vec<String> {
    let github_dir = root.join(".github");
    if !github_dir.exists() {
        return Vec::new();
    }
    let mut names: Vec<String> = std::fs::read_dir(github_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.eq_ignore_ascii_case("pull_request_template.md"))
        .collect();
    names.sort();
    names
}

async fn state_with_project(root: &std::path::Path) -> (AppState, String) {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Template Project".to_string(),
            root.display().to_string(),
        ))
        .await
        .unwrap();
    (state, project.id.as_str().to_string())
}

#[tokio::test]
async fn read_pr_template_returns_none_when_file_is_absent() {
    let root = tempfile::tempdir().unwrap();
    let (state, project_id) = state_with_project(root.path()).await;

    let content = read_pr_template_for_state(&project_id, &state)
        .await
        .unwrap();

    assert_eq!(content, None);
}

#[test]
fn direct_read_returns_none_when_github_directory_is_absent() {
    let root = tempfile::tempdir().unwrap();

    let content = project_pr_template::read_pr_template(root.path()).unwrap();

    assert_eq!(content, None);
}

#[tokio::test]
async fn read_pr_template_preserves_empty_and_exact_content() {
    let root = tempfile::tempdir().unwrap();
    let (state, project_id) = state_with_project(root.path()).await;
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(template_path(root.path()), "").unwrap();

    let empty = read_pr_template_for_state(&project_id, &state)
        .await
        .unwrap();
    assert_eq!(empty, Some(String::new()));

    let exact = "## Summary\n\n- Keep trailing newline\n";
    std::fs::write(template_path(root.path()), exact).unwrap();

    let content = read_pr_template_for_state(&project_id, &state)
        .await
        .unwrap();
    assert_eq!(content.as_deref(), Some(exact));
}

#[tokio::test]
async fn read_pr_template_supports_existing_uppercase_filename() {
    let root = tempfile::tempdir().unwrap();
    let (state, project_id) = state_with_project(root.path()).await;
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(uppercase_template_path(root.path()), "Uppercase\n").unwrap();

    let content = read_pr_template_for_state(&project_id, &state)
        .await
        .unwrap();

    assert_eq!(content.as_deref(), Some("Uppercase\n"));
}

#[test]
fn direct_read_prefers_lowercase_template_when_both_exist() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(template_path(root.path()), "Lowercase\n").unwrap();
    std::fs::write(uppercase_template_path(root.path()), "Uppercase\n").unwrap();

    let content = project_pr_template::read_pr_template(root.path()).unwrap();

    if actual_template_names(root.path()).len() == 2 {
        assert_eq!(content.as_deref(), Some("Lowercase\n"));
    } else {
        assert_eq!(content.as_deref(), Some("Uppercase\n"));
    }
}

#[tokio::test]
async fn write_pr_template_creates_parent_and_overwrites_exact_content() {
    let root = tempfile::tempdir().unwrap();
    let (state, project_id) = state_with_project(root.path()).await;
    let first = "# First\n\n";

    write_pr_template_for_state(&project_id, first, &state)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(template_path(root.path())).unwrap(),
        first
    );
    assert_eq!(
        actual_template_names(root.path()),
        ["pull_request_template.md"]
    );

    let second = "";
    write_pr_template_for_state(&project_id, second, &state)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(template_path(root.path())).unwrap(),
        second
    );
}

#[tokio::test]
async fn write_pr_template_overwrites_existing_uppercase_filename() {
    let root = tempfile::tempdir().unwrap();
    let (state, project_id) = state_with_project(root.path()).await;
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(uppercase_template_path(root.path()), "Old\n").unwrap();

    write_pr_template_for_state(&project_id, "New\n", &state)
        .await
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(uppercase_template_path(root.path())).unwrap(),
        "New\n"
    );
    assert_eq!(
        actual_template_names(root.path()),
        ["PULL_REQUEST_TEMPLATE.md"]
    );
}

#[test]
fn direct_write_prefers_existing_lowercase_template_when_both_exist() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(template_path(root.path()), "Lowercase old\n").unwrap();
    std::fs::write(uppercase_template_path(root.path()), "Uppercase old\n").unwrap();
    let distinct_template_entries = actual_template_names(root.path()).len() == 2;

    project_pr_template::write_pr_template(root.path(), "Lowercase new\n").unwrap();

    assert_eq!(
        std::fs::read_to_string(template_path(root.path())).unwrap(),
        "Lowercase new\n"
    );
    if distinct_template_entries {
        assert_eq!(
            std::fs::read_to_string(uppercase_template_path(root.path())).unwrap(),
            "Uppercase old\n"
        );
    } else {
        assert_eq!(
            std::fs::read_to_string(uppercase_template_path(root.path())).unwrap(),
            "Lowercase new\n"
        );
    }
}

#[tokio::test]
async fn unknown_project_returns_error_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::new_test();

    let result = write_pr_template_for_state("missing-project", "content", &state).await;

    assert!(result.unwrap_err().contains("Project not found"));
    assert!(!template_path(root.path()).exists());
}

#[tokio::test]
async fn project_root_must_be_absolute_non_root_and_existing() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new("Unsafe".to_string(), "/".to_string()))
        .await
        .unwrap();

    let result = read_pr_template_for_state(project.id.as_str(), &state).await;

    assert!(result.is_err());
}

#[test]
fn direct_project_root_must_be_absolute_and_existing_directory() {
    let relative = std::path::Path::new("relative-project");
    let relative_error = project_pr_template::read_pr_template(relative).unwrap_err();
    assert!(relative_error
        .to_string()
        .contains("absolute non-root path"));

    let missing = std::env::current_dir()
        .unwrap()
        .join(".artifacts")
        .join("specs")
        .join("pr-template-coverage")
        .join("missing-project-root");
    let missing_error = project_pr_template::read_pr_template(&missing).unwrap_err();
    assert!(missing_error.to_string().contains("Failed to canonicalize"));
}

#[test]
fn github_entry_must_be_directory() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".github"), "not a directory").unwrap();

    let error = project_pr_template::write_pr_template(root.path(), "content").unwrap_err();

    assert!(error
        .to_string()
        .contains(".github must be a regular directory"));
}

#[test]
fn template_entry_must_be_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::create_dir(template_path(root.path())).unwrap();

    let error = project_pr_template::read_pr_template(root.path()).unwrap_err();

    assert!(error
        .to_string()
        .contains("PR template path must be a regular file"));
}

#[test]
fn write_rejects_existing_template_directory() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::create_dir(template_path(root.path())).unwrap();

    let error = project_pr_template::write_pr_template(root.path(), "content").unwrap_err();

    assert!(error
        .to_string()
        .contains("PR template path must be a regular file"));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_github_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".github")).unwrap();

    let error = project_pr_template::write_pr_template(root.path(), "content").unwrap_err();

    assert!(error.to_string().contains(".github must not be a symlink"));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_template_file() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::os::unix::fs::symlink(outside.path(), template_path(root.path())).unwrap();

    let error = project_pr_template::read_pr_template(root.path()).unwrap_err();

    assert!(error
        .to_string()
        .contains("PR template path must not be a symlink"));
}
