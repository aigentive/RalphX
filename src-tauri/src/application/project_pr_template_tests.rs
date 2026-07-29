use std::path::{Path, PathBuf};

use super::project_pr_template;

fn template_path(root: &Path) -> PathBuf {
    root.join(".github").join("pull_request_template.md")
}

fn uppercase_template_path(root: &Path) -> PathBuf {
    root.join(".github").join("PULL_REQUEST_TEMPLATE.md")
}

fn actual_template_names(root: &Path) -> Vec<String> {
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

#[test]
fn read_returns_none_when_github_directory_is_absent() {
    let root = tempfile::tempdir().unwrap();

    let content = project_pr_template::read_pr_template(root.path()).unwrap();

    assert_eq!(content, None);
}

#[test]
fn read_preserves_empty_and_exact_content() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(template_path(root.path()), "").unwrap();

    let empty = project_pr_template::read_pr_template(root.path()).unwrap();
    assert_eq!(empty, Some(String::new()));

    let exact = "## Summary\n\n- Keep trailing newline\n";
    std::fs::write(template_path(root.path()), exact).unwrap();

    let content = project_pr_template::read_pr_template(root.path()).unwrap();
    assert_eq!(content.as_deref(), Some(exact));
}

#[test]
fn read_supports_existing_uppercase_filename() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(uppercase_template_path(root.path()), "Uppercase\n").unwrap();

    let content = project_pr_template::read_pr_template(root.path()).unwrap();

    assert_eq!(content.as_deref(), Some("Uppercase\n"));
}

#[test]
fn read_prefers_lowercase_template_when_both_exist() {
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

#[test]
fn write_creates_parent_and_overwrites_exact_content() {
    let root = tempfile::tempdir().unwrap();

    project_pr_template::write_pr_template(root.path(), "# First\n\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(template_path(root.path())).unwrap(),
        "# First\n\n"
    );
    assert_eq!(actual_template_names(root.path()), ["pull_request_template.md"]);

    project_pr_template::write_pr_template(root.path(), "").unwrap();
    assert_eq!(std::fs::read_to_string(template_path(root.path())).unwrap(), "");
}

#[test]
fn write_overwrites_existing_uppercase_filename() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".github")).unwrap();
    std::fs::write(uppercase_template_path(root.path()), "Old\n").unwrap();

    project_pr_template::write_pr_template(root.path(), "New\n").unwrap();

    assert_eq!(
        std::fs::read_to_string(uppercase_template_path(root.path())).unwrap(),
        "New\n"
    );
    assert_eq!(actual_template_names(root.path()), ["PULL_REQUEST_TEMPLATE.md"]);
}

#[test]
fn write_prefers_existing_lowercase_template_when_both_exist() {
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

#[test]
fn project_root_must_be_absolute_and_existing_directory() {
    let relative = Path::new("relative-project");
    let relative_error = project_pr_template::read_pr_template(relative).unwrap_err();
    assert!(relative_error
        .to_string()
        .contains("absolute non-root path"));

    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing-project-root");
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
