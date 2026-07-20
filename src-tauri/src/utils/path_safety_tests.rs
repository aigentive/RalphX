use super::*;

#[test]
fn rejects_relative_path() {
    let err = validate_absolute_non_root_path(Path::new("relative/path"), "test")
        .expect_err("relative path should be rejected");
    assert!(err.to_string().contains("absolute"));
}

#[test]
fn rejects_parent_components() {
    let err = validate_absolute_non_root_path(Path::new("/tmp/../etc"), "test")
        .expect_err("parent path should be rejected");
    assert!(err.to_string().contains("unsafe components"));
}

#[test]
fn accepts_absolute_child_path() {
    let path = validate_absolute_non_root_path(Path::new("/tmp/ralphx-child"), "test")
        .expect("normal absolute child path should be accepted");
    assert_eq!(path, PathBuf::from("/tmp/ralphx-child"));
}
