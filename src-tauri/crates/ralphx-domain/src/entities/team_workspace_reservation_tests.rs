use super::normalize_team_writable_path;

#[test]
fn team_workspace_paths_must_be_normalized_and_contained() {
    assert_eq!(
        normalize_team_writable_path("src/team.rs").unwrap(),
        "src/team.rs"
    );
    assert!(normalize_team_writable_path("../secret").is_err());
    assert!(normalize_team_writable_path("/absolute").is_err());
    assert!(normalize_team_writable_path("src//team.rs").is_err());
}
