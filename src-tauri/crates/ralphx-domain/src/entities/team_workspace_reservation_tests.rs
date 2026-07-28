use super::team_workspace_reservation::{team_paths_overlap, TeamWorkspaceReservationId};
use super::{
    normalize_team_writable_path, TeamMemberId, TeamSessionId, TeamWorkClassification,
    TeamWorkspaceReservation,
};
use chrono::Utc;

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

#[test]
fn team_paths_overlap_matches_equal_and_nested_but_not_sibling_prefixes() {
    assert!(team_paths_overlap("src/a", "src/a"));
    assert!(team_paths_overlap("src/a", "src/a/b.rs"));
    assert!(team_paths_overlap("src/a/b.rs", "src/a"));
    assert!(!team_paths_overlap("src/a", "src/ab"));
    assert!(!team_paths_overlap("src/a", "src/b"));
}

fn reservation(paths: &[&str], outputs: &[&str], locks: &[&str]) -> TeamWorkspaceReservation {
    TeamWorkspaceReservation {
        id: TeamWorkspaceReservationId::new(),
        team_id: TeamSessionId::from_string("team-1"),
        team_member_id: TeamMemberId::from_string("member-1"),
        assignment_id: None,
        team_member_generation: 0,
        writable_paths: paths.iter().map(|path| path.to_string()).collect(),
        generated_outputs: outputs.iter().map(|path| path.to_string()).collect(),
        resource_locks: locks.iter().map(|lock| lock.to_string()).collect(),
        work_classification: TeamWorkClassification::Write,
        attempt_number: 1,
        acquired_at: Utc::now(),
        released_at: None,
    }
}

#[test]
fn reservation_conflicts_cover_paths_outputs_and_locks() {
    let base = reservation(&["src/a"], &["dist/bundle"], &["cargo-target"]);

    assert!(base.conflicts_with(&reservation(&["src/a/inner.rs"], &[], &[])));
    assert!(base.conflicts_with(&reservation(&["dist/bundle/app.js"], &[], &[])));
    assert!(base.conflicts_with(&reservation(&["src/other"], &[], &["cargo-target"])));
    assert!(!base.conflicts_with(&reservation(&["src/ab"], &[], &["other-lock"])));
}
