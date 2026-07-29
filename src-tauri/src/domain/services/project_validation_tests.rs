use super::*;

#[test]
fn test_validate_project_path_rejects_system_dirs() {
    for blocked in &["/etc/foo", "/usr/local/bar", "/var/db", "/System/Library", "/Library/Application Support"] {
        let result = validate_project_path(blocked);
        assert!(result.is_err(), "Expected error for blocked path: {blocked}");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("restricted system directory") || msg.contains("home directory"),
            "Unexpected error for {blocked}: {msg}"
        );
    }
}

#[test]
fn test_validate_project_path_rejects_outside_home() {
    // /opt is not in BLOCKED_PREFIXES but is outside home
    let result = validate_project_path("/opt/some/project");
    // May fail with "cannot canonicalize" (if /opt doesn't exist) or "home directory"
    // Either way it should fail
    assert!(result.is_err());
}

#[test]
fn test_validate_project_path_accepts_home_subpath() {
    let home = std::env::var("HOME").expect("HOME must be set in test env");
    // Use a path under home that likely doesn't exist (validation should handle it)
    let test_path = format!("{home}/ralphx_test_project_xyz_nonexistent");
    let result = validate_project_path(&test_path);
    // Should succeed — path is under home even if it doesn't exist
    assert!(result.is_ok(), "Expected ok for home subpath, got: {result:?}");
}

#[test]
fn test_validate_project_path_traversal_attack() {
    let home = std::env::var("HOME").expect("HOME must be set in test env");
    // Path traversal: start from home, go to /etc via ..
    let traversal = format!("{home}/../../../etc/passwd");
    let result = validate_project_path(&traversal);
    // After canonicalization, this resolves to /etc/passwd → blocked
    assert!(result.is_err(), "Path traversal should be blocked");
}
