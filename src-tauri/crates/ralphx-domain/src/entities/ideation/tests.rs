use super::*;
use crate::entities::{ChatMessageId, IdeationSessionId, ProjectId, TaskId, TaskProposalId};

// ===== IdeationSessionStatus Tests =====

#[test]
fn status_default_is_active() {
    assert_eq!(
        IdeationSessionStatus::default(),
        IdeationSessionStatus::Active
    );
}

#[test]
fn status_display_active() {
    assert_eq!(format!("{}", IdeationSessionStatus::Active), "active");
}

#[test]
fn status_display_archived() {
    assert_eq!(format!("{}", IdeationSessionStatus::Archived), "archived");
}

#[test]
fn status_display_accepted() {
    assert_eq!(format!("{}", IdeationSessionStatus::Accepted), "accepted");
}

#[test]
fn status_serializes_to_snake_case() {
    let active_json =
        serde_json::to_string(&IdeationSessionStatus::Active).expect("Should serialize");
    let archived_json =
        serde_json::to_string(&IdeationSessionStatus::Archived).expect("Should serialize");
    let accepted_json =
        serde_json::to_string(&IdeationSessionStatus::Accepted).expect("Should serialize");

    assert_eq!(active_json, "\"active\"");
    assert_eq!(archived_json, "\"archived\"");
    assert_eq!(accepted_json, "\"accepted\"");
}

#[test]
fn status_deserializes_from_snake_case() {
    let active: IdeationSessionStatus =
        serde_json::from_str("\"active\"").expect("Should deserialize");
    let archived: IdeationSessionStatus =
        serde_json::from_str("\"archived\"").expect("Should deserialize");
    let accepted: IdeationSessionStatus =
        serde_json::from_str("\"accepted\"").expect("Should deserialize");

    assert_eq!(active, IdeationSessionStatus::Active);
    assert_eq!(archived, IdeationSessionStatus::Archived);
    assert_eq!(accepted, IdeationSessionStatus::Accepted);
}

#[test]
fn status_from_str_active() {
    let status: IdeationSessionStatus = "active".parse().unwrap();
    assert_eq!(status, IdeationSessionStatus::Active);
}

#[test]
fn status_from_str_archived() {
    let status: IdeationSessionStatus = "archived".parse().unwrap();
    assert_eq!(status, IdeationSessionStatus::Archived);
}

#[test]
fn status_from_str_accepted() {
    let status: IdeationSessionStatus = "accepted".parse().unwrap();
    assert_eq!(status, IdeationSessionStatus::Accepted);
}

#[test]
fn status_from_str_invalid() {
    let result: Result<IdeationSessionStatus, _> = "invalid".parse();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.value, "invalid");
}

#[test]
fn status_parse_error_displays_correctly() {
    let err = ParseIdeationSessionStatusError {
        value: "unknown".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "unknown ideation session status: 'unknown'"
    );
}

#[test]
fn status_clone_works() {
    let status = IdeationSessionStatus::Archived;
    let cloned = status;
    assert_eq!(status, cloned);
}

#[test]
fn status_equality_works() {
    assert_eq!(IdeationSessionStatus::Active, IdeationSessionStatus::Active);
    assert_eq!(
        IdeationSessionStatus::Archived,
        IdeationSessionStatus::Archived
    );
    assert_eq!(
        IdeationSessionStatus::Accepted,
        IdeationSessionStatus::Accepted
    );
    assert_ne!(
        IdeationSessionStatus::Active,
        IdeationSessionStatus::Archived
    );
    assert_ne!(
        IdeationSessionStatus::Active,
        IdeationSessionStatus::Accepted
    );
    assert_ne!(
        IdeationSessionStatus::Archived,
        IdeationSessionStatus::Accepted
    );
}

// ===== IdeationSession Creation Tests =====

#[test]
fn session_new_creates_with_defaults() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());

    assert_eq!(session.project_id, project_id);
    assert_eq!(session.status, IdeationSessionStatus::Active);
    assert!(session.title.is_none());
    assert!(session.archived_at.is_none());
    assert!(session.converted_at.is_none());
}

#[test]
fn session_new_generates_unique_id() {
    let project_id = ProjectId::new();
    let session1 = IdeationSession::new(project_id.clone());
    let session2 = IdeationSession::new(project_id);

    assert_ne!(session1.id, session2.id);
}

#[test]
fn session_new_sets_timestamps() {
    let before = Utc::now();
    let session = IdeationSession::new(ProjectId::new());
    let after = Utc::now();

    assert!(session.created_at >= before);
    assert!(session.created_at <= after);
    assert!(session.updated_at >= before);
    assert!(session.updated_at <= after);
    assert_eq!(session.created_at, session.updated_at);
}

#[test]
fn session_new_with_title() {
    let session = IdeationSession::new_with_title(ProjectId::new(), "Auth Feature");

    assert_eq!(session.title, Some("Auth Feature".to_string()));
    assert_eq!(session.status, IdeationSessionStatus::Active);
}

// ===== Builder Tests =====

#[test]
fn builder_creates_session_with_all_fields() {
    let project_id = ProjectId::new();
    let created = Utc::now();

    let session = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Custom Session")
        .status(IdeationSessionStatus::Active)
        .created_at(created)
        .updated_at(created)
        .build();

    assert_eq!(session.project_id, project_id);
    assert_eq!(session.title, Some("Custom Session".to_string()));
    assert_eq!(session.status, IdeationSessionStatus::Active);
    assert_eq!(session.created_at, created);
}

#[test]
fn builder_uses_defaults_for_optional_fields() {
    let session = IdeationSession::builder()
        .project_id(ProjectId::new())
        .build();

    assert!(session.title.is_none());
    assert_eq!(session.status, IdeationSessionStatus::Active);
    assert!(session.archived_at.is_none());
    assert!(session.converted_at.is_none());
}

#[test]
fn builder_generates_id_if_not_provided() {
    let session = IdeationSession::builder()
        .project_id(ProjectId::new())
        .build();

    assert!(uuid::Uuid::parse_str(session.id.as_str()).is_ok());
}

#[test]
fn builder_uses_provided_id() {
    let id = IdeationSessionId::from_string("custom-id");
    let session = IdeationSession::builder()
        .id(id.clone())
        .project_id(ProjectId::new())
        .build();

    assert_eq!(session.id, id);
}

#[test]
#[should_panic(expected = "project_id is required")]
fn builder_panics_without_project_id() {
    IdeationSession::builder().build();
}

// ===== Session Method Tests =====

#[test]
fn session_is_active_returns_true_for_active() {
    let session = IdeationSession::new(ProjectId::new());
    assert!(session.is_active());
}

#[test]
fn session_is_active_returns_false_for_other_statuses() {
    let mut session = IdeationSession::new(ProjectId::new());
    session.archive();
    assert!(!session.is_active());
}

#[test]
fn session_is_archived_returns_true_for_archived() {
    let mut session = IdeationSession::new(ProjectId::new());
    session.archive();
    assert!(session.is_archived());
}

#[test]
fn session_is_accepted_returns_true_for_accepted() {
    let mut session = IdeationSession::new(ProjectId::new());
    session.mark_accepted();
    assert!(session.is_accepted());
}

#[test]
fn session_archive_sets_status_and_timestamp() {
    let mut session = IdeationSession::new(ProjectId::new());
    let before = Utc::now();

    session.archive();

    assert_eq!(session.status, IdeationSessionStatus::Archived);
    assert!(session.archived_at.is_some());
    assert!(session.archived_at.unwrap() >= before);
    assert!(session.updated_at >= before);
}

#[test]
fn session_mark_accepted_sets_status_and_timestamp() {
    let mut session = IdeationSession::new(ProjectId::new());
    let before = Utc::now();

    session.mark_accepted();

    assert_eq!(session.status, IdeationSessionStatus::Accepted);
    assert!(session.converted_at.is_some());
    assert!(session.converted_at.unwrap() >= before);
    assert!(session.updated_at >= before);
}

#[test]
fn session_touch_updates_timestamp() {
    let mut session = IdeationSession::new(ProjectId::new());
    let original_updated = session.updated_at;
    let original_created = session.created_at;

    std::thread::sleep(std::time::Duration::from_millis(10));

    session.touch();

    assert_eq!(session.created_at, original_created);
    assert!(session.updated_at > original_updated);
}

// ===== Serialization Tests =====

#[test]
fn session_serializes_to_json() {
    let session = IdeationSession::new_with_title(ProjectId::new(), "JSON Test");
    let json = serde_json::to_string(&session).expect("Should serialize");

    assert!(json.contains("\"title\":\"JSON Test\""));
    assert!(json.contains("\"status\":\"active\""));
}

#[test]
fn session_deserializes_from_json() {
    let json = r#"{
            "id": "session-123",
            "project_id": "proj-456",
            "title": "Deserialized",
            "status": "archived",
            "created_at": "2026-01-24T12:00:00Z",
            "updated_at": "2026-01-24T13:00:00Z",
            "archived_at": "2026-01-24T13:00:00Z",
            "converted_at": null
        }"#;

    let session: IdeationSession = serde_json::from_str(json).expect("Should deserialize");

    assert_eq!(session.id.as_str(), "session-123");
    assert_eq!(session.project_id.as_str(), "proj-456");
    assert_eq!(session.title, Some("Deserialized".to_string()));
    assert_eq!(session.status, IdeationSessionStatus::Archived);
    assert!(session.archived_at.is_some());
    assert!(session.converted_at.is_none());
}

#[test]
fn session_deserializes_with_null_optionals() {
    let json = r#"{
            "id": "session-min",
            "project_id": "proj-min",
            "title": null,
            "status": "active",
            "created_at": "2026-01-24T12:00:00Z",
            "updated_at": "2026-01-24T12:00:00Z",
            "archived_at": null,
            "converted_at": null
        }"#;

    let session: IdeationSession = serde_json::from_str(json).expect("Should deserialize");

    assert!(session.title.is_none());
    assert!(session.archived_at.is_none());
    assert!(session.converted_at.is_none());
}

#[test]
fn session_roundtrip_serialization() {
    let mut original = IdeationSession::new_with_title(ProjectId::new(), "Roundtrip");
    original.archive();

    let json = serde_json::to_string(&original).expect("Should serialize");
    let restored: IdeationSession = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(original.id, restored.id);
    assert_eq!(original.project_id, restored.project_id);
    assert_eq!(original.title, restored.title);
    assert_eq!(original.status, restored.status);
}

#[test]
fn session_clone_works() {
    let original = IdeationSession::new_with_title(ProjectId::new(), "Clone Test");
    let cloned = original.clone();

    assert_eq!(original.id, cloned.id);
    assert_eq!(original.project_id, cloned.project_id);
    assert_eq!(original.title, cloned.title);
    assert_eq!(original.status, cloned.status);
}

#[test]
fn session_clone_is_independent() {
    let original = IdeationSession::new(ProjectId::new());
    let mut cloned = original.clone();

    cloned.archive();

    // Original should be unchanged
    assert_eq!(original.status, IdeationSessionStatus::Active);
    assert_eq!(cloned.status, IdeationSessionStatus::Archived);
}

#[test]
fn session_flow_defaults_to_ideation_and_parses_planning() {
    let session = IdeationSession::new(ProjectId::new());
    assert_eq!(session.session_flow, IdeationSessionFlow::Ideation);
    assert_eq!(
        "planning".parse::<IdeationSessionFlow>().unwrap(),
        IdeationSessionFlow::Planning
    );
    assert_eq!(IdeationSessionFlow::Planning.to_string(), "planning");
}

#[test]
fn new_sessions_require_a_complete_v2_plan_pair() {
    let mut session = IdeationSession::new(ProjectId::new());
    session.plan_artifact_id = Some(ArtifactId::from_string("overview-1"));

    assert_eq!(session.plan_contract_version, 2);
    assert!(session.plan_artifact_bundle().is_none());

    session.plan_blueprint_artifact_id = Some(ArtifactId::from_string("blueprint-1"));
    let bundle = session.plan_artifact_bundle().expect("complete pair");
    assert_eq!(
        bundle.action_target_id(),
        "plan_bundle:v2:overview-1:blueprint-1"
    );
}

#[test]
fn legacy_sessions_keep_overview_only_action_authority() {
    let mut session = IdeationSession::new(ProjectId::new());
    session.plan_contract_version = 1;
    session.plan_artifact_id = Some(ArtifactId::from_string("legacy-overview"));

    let bundle = session.plan_artifact_bundle().expect("legacy overview");
    assert_eq!(bundle.action_target_id(), "legacy-overview");
    assert!(bundle.blueprint_id.is_none());
}

#[test]
fn pair_verification_requires_both_exact_current_members() {
    let mut session = IdeationSession::new(ProjectId::new());
    session.plan_artifact_id = Some(ArtifactId::from_string("overview-1"));
    session.plan_blueprint_artifact_id = Some(ArtifactId::from_string("blueprint-2"));
    session.verified_plan_artifact_id = Some(ArtifactId::from_string("overview-1"));
    session.verified_plan_blueprint_artifact_id = Some(ArtifactId::from_string("blueprint-stale"));

    assert!(!session.has_exact_plan_verification());
    session.verified_plan_blueprint_artifact_id = Some(ArtifactId::from_string("blueprint-2"));
    assert!(session.has_exact_plan_verification());
}

// ===== from_row Integration Tests =====

use chrono::Timelike;
use rusqlite::Connection;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        r#"CREATE TABLE ideation_sessions (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                title TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                plan_artifact_id TEXT,
                inherited_plan_artifact_id TEXT,
                seed_task_id TEXT,
                parent_session_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                archived_at TEXT,
                converted_at TEXT
            )"#,
        [],
    )
    .unwrap();
    conn
}

#[test]
fn session_from_row_active() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO ideation_sessions (id, project_id, title, status, created_at, updated_at)
               VALUES ('sess-1', 'proj-1', 'Auth Feature', 'active',
               '2026-01-24T10:00:00Z', '2026-01-24T11:00:00Z')"#,
        [],
    )
    .unwrap();

    let session: IdeationSession = conn
        .query_row(
            "SELECT * FROM ideation_sessions WHERE id = 'sess-1'",
            [],
            |row| IdeationSession::from_row(row),
        )
        .unwrap();

    assert_eq!(session.id.as_str(), "sess-1");
    assert_eq!(session.project_id.as_str(), "proj-1");
    assert_eq!(session.title, Some("Auth Feature".to_string()));
    assert_eq!(session.status, IdeationSessionStatus::Active);
    assert!(session.archived_at.is_none());
    assert!(session.converted_at.is_none());
}

#[test]
fn session_from_row_archived() {
    let conn = setup_test_db();
    conn.execute(
            r#"INSERT INTO ideation_sessions (id, project_id, title, status, created_at, updated_at, archived_at)
               VALUES ('sess-2', 'proj-1', NULL, 'archived',
               '2026-01-24T08:00:00Z', '2026-01-24T12:00:00Z', '2026-01-24T12:00:00Z')"#,
            [],
        )
        .unwrap();

    let session: IdeationSession = conn
        .query_row(
            "SELECT * FROM ideation_sessions WHERE id = 'sess-2'",
            [],
            |row| IdeationSession::from_row(row),
        )
        .unwrap();

    assert_eq!(session.status, IdeationSessionStatus::Archived);
    assert!(session.title.is_none());
    assert!(session.archived_at.is_some());
}

#[test]
fn session_from_row_accepted() {
    let conn = setup_test_db();
    conn.execute(
            r#"INSERT INTO ideation_sessions (id, project_id, title, status, created_at, updated_at, converted_at)
               VALUES ('sess-3', 'proj-1', 'Done Session', 'accepted',
               '2026-01-24T08:00:00Z', '2026-01-24T14:00:00Z', '2026-01-24T14:00:00Z')"#,
            [],
        )
        .unwrap();

    let session: IdeationSession = conn
        .query_row(
            "SELECT * FROM ideation_sessions WHERE id = 'sess-3'",
            [],
            |row| IdeationSession::from_row(row),
        )
        .unwrap();

    assert_eq!(session.status, IdeationSessionStatus::Accepted);
    assert!(session.converted_at.is_some());
}

#[test]
fn session_from_row_unknown_status_defaults_to_active() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO ideation_sessions (id, project_id, title, status, created_at, updated_at)
               VALUES ('sess-unk', 'proj-1', NULL, 'unknown_status',
               '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z')"#,
        [],
    )
    .unwrap();

    let session: IdeationSession = conn
        .query_row(
            "SELECT * FROM ideation_sessions WHERE id = 'sess-unk'",
            [],
            |row| IdeationSession::from_row(row),
        )
        .unwrap();

    // Unknown status should default to Active
    assert_eq!(session.status, IdeationSessionStatus::Active);
}

#[test]
fn session_from_row_sqlite_datetime_format() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO ideation_sessions (id, project_id, title, status, created_at, updated_at)
               VALUES ('sess-sql', 'proj-1', NULL, 'active',
               '2026-01-24 12:30:00', '2026-01-24 14:45:00')"#,
        [],
    )
    .unwrap();

    let session: IdeationSession = conn
        .query_row(
            "SELECT * FROM ideation_sessions WHERE id = 'sess-sql'",
            [],
            |row| IdeationSession::from_row(row),
        )
        .unwrap();

    assert_eq!(session.created_at.hour(), 12);
    assert_eq!(session.created_at.minute(), 30);
    assert_eq!(session.updated_at.hour(), 14);
    assert_eq!(session.updated_at.minute(), 45);
}

#[test]
fn session_from_row_with_all_timestamps() {
    let conn = setup_test_db();
    conn.execute(
            r#"INSERT INTO ideation_sessions (id, project_id, title, status, created_at, updated_at, archived_at, converted_at)
               VALUES ('sess-full', 'proj-1', 'Full', 'accepted',
               '2026-01-24T08:00:00Z', '2026-01-24T16:00:00Z',
               '2026-01-24T12:00:00Z', '2026-01-24T16:00:00Z')"#,
            [],
        )
        .unwrap();

    let session: IdeationSession = conn
        .query_row(
            "SELECT * FROM ideation_sessions WHERE id = 'sess-full'",
            [],
            |row| IdeationSession::from_row(row),
        )
        .unwrap();

    assert!(session.archived_at.is_some());
    assert!(session.converted_at.is_some());
    assert_eq!(session.archived_at.unwrap().hour(), 12);
    assert_eq!(session.converted_at.unwrap().hour(), 16);
}

// ==========================================
// Priority Enum Tests
// ==========================================

#[test]
fn priority_default_is_medium() {
    assert_eq!(Priority::default(), Priority::Medium);
}

#[test]
fn priority_display() {
    assert_eq!(format!("{}", Priority::Critical), "critical");
    assert_eq!(format!("{}", Priority::High), "high");
    assert_eq!(format!("{}", Priority::Medium), "medium");
    assert_eq!(format!("{}", Priority::Low), "low");
}

#[test]
fn priority_from_str() {
    assert_eq!("critical".parse::<Priority>().unwrap(), Priority::Critical);
    assert_eq!("high".parse::<Priority>().unwrap(), Priority::High);
    assert_eq!("medium".parse::<Priority>().unwrap(), Priority::Medium);
    assert_eq!("low".parse::<Priority>().unwrap(), Priority::Low);
}

#[test]
fn priority_from_str_invalid() {
    let result: Result<Priority, _> = "invalid".parse();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().value, "invalid");
}

#[test]
fn priority_serializes() {
    assert_eq!(
        serde_json::to_string(&Priority::Critical).unwrap(),
        "\"critical\""
    );
    assert_eq!(serde_json::to_string(&Priority::Low).unwrap(), "\"low\"");
}

#[test]
fn priority_deserializes() {
    assert_eq!(
        serde_json::from_str::<Priority>("\"high\"").unwrap(),
        Priority::High
    );
}

// ==========================================
// Complexity Enum Tests
// ==========================================

#[test]
fn complexity_default_is_moderate() {
    assert_eq!(Complexity::default(), Complexity::Moderate);
}

#[test]
fn complexity_display() {
    assert_eq!(format!("{}", Complexity::Trivial), "trivial");
    assert_eq!(format!("{}", Complexity::Simple), "simple");
    assert_eq!(format!("{}", Complexity::Moderate), "moderate");
    assert_eq!(format!("{}", Complexity::Complex), "complex");
    assert_eq!(format!("{}", Complexity::VeryComplex), "very_complex");
}

#[test]
fn complexity_from_str() {
    assert_eq!(
        "trivial".parse::<Complexity>().unwrap(),
        Complexity::Trivial
    );
    assert_eq!("simple".parse::<Complexity>().unwrap(), Complexity::Simple);
    assert_eq!(
        "moderate".parse::<Complexity>().unwrap(),
        Complexity::Moderate
    );
    assert_eq!(
        "complex".parse::<Complexity>().unwrap(),
        Complexity::Complex
    );
    assert_eq!(
        "very_complex".parse::<Complexity>().unwrap(),
        Complexity::VeryComplex
    );
}

#[test]
fn complexity_from_str_invalid() {
    let result: Result<Complexity, _> = "unknown".parse();
    assert!(result.is_err());
}

#[test]
fn complexity_serializes() {
    assert_eq!(
        serde_json::to_string(&Complexity::VeryComplex).unwrap(),
        "\"very_complex\""
    );
}

// ==========================================
// ProposalStatus Enum Tests
// ==========================================

#[test]
fn proposal_status_default_is_pending() {
    assert_eq!(ProposalStatus::default(), ProposalStatus::Pending);
}

#[test]
fn proposal_status_display() {
    assert_eq!(format!("{}", ProposalStatus::Pending), "pending");
    assert_eq!(format!("{}", ProposalStatus::Accepted), "accepted");
    assert_eq!(format!("{}", ProposalStatus::Rejected), "rejected");
    assert_eq!(format!("{}", ProposalStatus::Modified), "modified");
}

#[test]
fn proposal_status_from_str() {
    assert_eq!(
        "pending".parse::<ProposalStatus>().unwrap(),
        ProposalStatus::Pending
    );
    assert_eq!(
        "accepted".parse::<ProposalStatus>().unwrap(),
        ProposalStatus::Accepted
    );
    assert_eq!(
        "rejected".parse::<ProposalStatus>().unwrap(),
        ProposalStatus::Rejected
    );
    assert_eq!(
        "modified".parse::<ProposalStatus>().unwrap(),
        ProposalStatus::Modified
    );
}

#[test]
fn proposal_status_from_str_invalid() {
    let result: Result<ProposalStatus, _> = "invalid".parse();
    assert!(result.is_err());
}

// ==========================================
// ProposalCategory Enum Tests
// ==========================================

#[test]
fn proposal_category_default_is_feature() {
    assert_eq!(ProposalCategory::default(), ProposalCategory::Feature);
}

#[test]
fn proposal_category_display() {
    assert_eq!(format!("{}", ProposalCategory::Setup), "setup");
    assert_eq!(format!("{}", ProposalCategory::Feature), "feature");
    assert_eq!(format!("{}", ProposalCategory::Fix), "fix");
    assert_eq!(format!("{}", ProposalCategory::Refactor), "refactor");
    assert_eq!(format!("{}", ProposalCategory::Docs), "docs");
    assert_eq!(format!("{}", ProposalCategory::Test), "test");
    assert_eq!(format!("{}", ProposalCategory::Performance), "performance");
    assert_eq!(format!("{}", ProposalCategory::Security), "security");
    assert_eq!(format!("{}", ProposalCategory::DevOps), "devops");
    assert_eq!(format!("{}", ProposalCategory::Research), "research");
    assert_eq!(format!("{}", ProposalCategory::Design), "design");
    assert_eq!(format!("{}", ProposalCategory::Chore), "chore");
}

#[test]
fn proposal_category_from_str() {
    assert_eq!(
        "setup".parse::<ProposalCategory>().unwrap(),
        ProposalCategory::Setup
    );
    assert_eq!(
        "feature".parse::<ProposalCategory>().unwrap(),
        ProposalCategory::Feature
    );
    assert_eq!(
        "fix".parse::<ProposalCategory>().unwrap(),
        ProposalCategory::Fix
    );
    assert_eq!(
        "devops".parse::<ProposalCategory>().unwrap(),
        ProposalCategory::DevOps
    );
}

#[test]
fn proposal_category_from_str_invalid() {
    let result: Result<ProposalCategory, _> = "invalid".parse();
    assert!(result.is_err());
}

// ==========================================
// PriorityFactors Tests
// ==========================================

#[test]
fn priority_factors_default() {
    let factors = PriorityFactors::default();
    assert_eq!(factors.dependency, 0);
    assert_eq!(factors.business_value, 0);
    assert_eq!(factors.technical_risk, 0);
    assert_eq!(factors.user_demand, 0);
}

#[test]
fn priority_factors_serializes() {
    let factors = PriorityFactors {
        dependency: 25,
        business_value: 30,
        technical_risk: 10,
        user_demand: 15,
    };
    let json = serde_json::to_string(&factors).unwrap();
    assert!(json.contains("\"dependency\":25"));
    assert!(json.contains("\"business_value\":30"));
}

#[test]
fn priority_factors_deserializes() {
    let json = r#"{"dependency":10,"business_value":20,"technical_risk":5,"user_demand":15}"#;
    let factors: PriorityFactors = serde_json::from_str(json).unwrap();
    assert_eq!(factors.dependency, 10);
    assert_eq!(factors.user_demand, 15);
}

#[test]
fn priority_factors_deserializes_with_missing_fields() {
    let json = r#"{"dependency":10}"#;
    let factors: PriorityFactors = serde_json::from_str(json).unwrap();
    assert_eq!(factors.dependency, 10);
    assert_eq!(factors.business_value, 0); // default
}

// ==========================================
// TaskProposal Creation Tests
// ==========================================

#[test]
fn proposal_new_creates_with_defaults() {
    let session_id = IdeationSessionId::new();
    let proposal = TaskProposal::new(
        session_id.clone(),
        "Add authentication",
        ProposalCategory::Feature,
        Priority::High,
    );

    assert_eq!(proposal.session_id, session_id);
    assert_eq!(proposal.title, "Add authentication");
    assert_eq!(proposal.category, ProposalCategory::Feature);
    assert_eq!(proposal.suggested_priority, Priority::High);
    assert_eq!(proposal.priority_score, 50);
    assert_eq!(proposal.estimated_complexity, Complexity::Moderate);
    assert_eq!(proposal.status, ProposalStatus::Pending);
    assert!(!proposal.selected);
    assert!(!proposal.user_modified);
    assert!(proposal.description.is_none());
    assert!(proposal.created_task_id.is_none());
}

#[test]
fn proposal_new_generates_unique_id() {
    let session_id = IdeationSessionId::new();
    let p1 = TaskProposal::new(
        session_id.clone(),
        "Task 1",
        ProposalCategory::Feature,
        Priority::High,
    );
    let p2 = TaskProposal::new(
        session_id,
        "Task 2",
        ProposalCategory::Feature,
        Priority::Low,
    );

    assert_ne!(p1.id, p2.id);
}

#[test]
fn proposal_effective_priority_returns_suggested_when_no_override() {
    let proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::High,
    );

    assert_eq!(proposal.effective_priority(), Priority::High);
}

#[test]
fn proposal_effective_priority_returns_user_override() {
    let mut proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::High,
    );
    proposal.set_user_priority(Priority::Low);

    assert_eq!(proposal.effective_priority(), Priority::Low);
}

// ==========================================
// TaskProposal Method Tests
// ==========================================

#[test]
fn proposal_is_pending() {
    let proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    assert!(proposal.is_pending());
    assert!(!proposal.has_created_task());
}

#[test]
fn proposal_accept() {
    let mut proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.accept();

    assert!(proposal.is_accepted());
    assert!(!proposal.is_pending());
    assert_eq!(proposal.status, ProposalStatus::Accepted);
}

#[test]
fn proposal_reject() {
    let mut proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.reject();

    assert_eq!(proposal.status, ProposalStatus::Rejected);
    assert!(!proposal.selected);
}

#[test]
fn proposal_set_user_priority() {
    let mut proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::Low,
    );
    proposal.set_user_priority(Priority::Critical);

    assert_eq!(proposal.user_priority, Some(Priority::Critical));
    assert!(proposal.user_modified);
    assert_eq!(proposal.status, ProposalStatus::Modified);
}

#[test]
fn proposal_link_to_task() {
    let mut proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let task_id = TaskId::new();
    proposal.link_to_task(task_id.clone());

    assert_eq!(proposal.created_task_id, Some(task_id));
    assert!(proposal.has_created_task());
}

#[test]
fn proposal_toggle_selection() {
    let mut proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    assert!(!proposal.selected);

    proposal.toggle_selection();
    assert!(proposal.selected);

    proposal.toggle_selection();
    assert!(!proposal.selected);
}

#[test]
fn proposal_touch_updates_timestamp() {
    let mut proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "Test",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let original = proposal.updated_at;

    std::thread::sleep(std::time::Duration::from_millis(10));
    proposal.touch();

    assert!(proposal.updated_at > original);
}

// ==========================================
// TaskProposal Serialization Tests
// ==========================================

#[test]
fn proposal_serializes_to_json() {
    let proposal = TaskProposal::new(
        IdeationSessionId::new(),
        "JSON Test",
        ProposalCategory::Fix,
        Priority::Critical,
    );
    let json = serde_json::to_string(&proposal).unwrap();

    assert!(json.contains("\"title\":\"JSON Test\""));
    assert!(json.contains("\"category\":\"fix\""));
    assert!(json.contains("\"suggested_priority\":\"critical\""));
}

#[test]
fn proposal_deserializes_from_json() {
    let json = r#"{
            "id": "prop-123",
            "session_id": "sess-456",
            "title": "Deserialized",
            "description": "A test proposal",
            "category": "refactor",
            "steps": null,
            "acceptance_criteria": null,
            "suggested_priority": "high",
            "priority_score": 75,
            "priority_reason": "Important",
            "priority_factors": null,
            "estimated_complexity": "complex",
            "user_priority": "critical",
            "user_modified": true,
            "status": "modified",
            "selected": true,
            "created_task_id": null,
            "sort_order": 5,
            "created_at": "2026-01-24T12:00:00Z",
            "updated_at": "2026-01-24T13:00:00Z"
        }"#;

    let proposal: TaskProposal = serde_json::from_str(json).unwrap();

    assert_eq!(proposal.id.as_str(), "prop-123");
    assert_eq!(proposal.session_id.as_str(), "sess-456");
    assert_eq!(proposal.title, "Deserialized");
    assert_eq!(proposal.category, ProposalCategory::Refactor);
    assert_eq!(proposal.suggested_priority, Priority::High);
    assert_eq!(proposal.priority_score, 75);
    assert_eq!(proposal.estimated_complexity, Complexity::Complex);
    assert_eq!(proposal.user_priority, Some(Priority::Critical));
    assert!(proposal.user_modified);
    assert_eq!(proposal.status, ProposalStatus::Modified);
    assert_eq!(proposal.sort_order, 5);
}

#[test]
fn proposal_roundtrip_serialization() {
    let mut original = TaskProposal::new(
        IdeationSessionId::new(),
        "Roundtrip",
        ProposalCategory::Security,
        Priority::High,
    );
    original.set_user_priority(Priority::Critical);

    let json = serde_json::to_string(&original).unwrap();
    let restored: TaskProposal = serde_json::from_str(&json).unwrap();

    assert_eq!(original.id, restored.id);
    assert_eq!(original.title, restored.title);
    assert_eq!(original.category, restored.category);
    assert_eq!(original.user_priority, restored.user_priority);
    assert_eq!(original.status, restored.status);
}

// ==========================================
// TaskProposal from_row Integration Tests
// ==========================================

fn setup_proposal_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        r#"CREATE TABLE task_proposals (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                category TEXT NOT NULL,
                steps TEXT,
                acceptance_criteria TEXT,
                suggested_priority TEXT NOT NULL,
                priority_score INTEGER NOT NULL DEFAULT 50,
                priority_reason TEXT,
                priority_factors TEXT,
                estimated_complexity TEXT DEFAULT 'moderate',
                user_priority TEXT,
                user_modified INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                selected INTEGER DEFAULT 1,
                created_task_id TEXT,
                plan_artifact_id TEXT,
                plan_version_at_creation INTEGER,
                affected_paths TEXT,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                archived_at TEXT NULL
            )"#,
        [],
    )
    .unwrap();
    conn
}

#[test]
fn proposal_from_row_basic() {
    let conn = setup_proposal_test_db();
    conn.execute(
            r#"INSERT INTO task_proposals (id, session_id, title, category, suggested_priority,
               priority_score, estimated_complexity, status, selected, sort_order, created_at, updated_at)
               VALUES ('prop-1', 'sess-1', 'Test Proposal', 'feature', 'high',
               75, 'complex', 'pending', 1, 0, '2026-01-24T10:00:00Z', '2026-01-24T11:00:00Z')"#,
            [],
        )
        .unwrap();

    let proposal: TaskProposal = conn
        .query_row(
            "SELECT * FROM task_proposals WHERE id = 'prop-1'",
            [],
            |row| TaskProposal::from_row(row),
        )
        .unwrap();

    assert_eq!(proposal.id.as_str(), "prop-1");
    assert_eq!(proposal.session_id.as_str(), "sess-1");
    assert_eq!(proposal.title, "Test Proposal");
    assert_eq!(proposal.category, ProposalCategory::Feature);
    assert_eq!(proposal.suggested_priority, Priority::High);
    assert_eq!(proposal.priority_score, 75);
    assert_eq!(proposal.estimated_complexity, Complexity::Complex);
    assert!(proposal.selected);
    assert!(!proposal.user_modified);
}

#[test]
fn proposal_from_row_with_user_override() {
    let conn = setup_proposal_test_db();
    conn.execute(
            r#"INSERT INTO task_proposals (id, session_id, title, category, suggested_priority,
               priority_score, user_priority, user_modified, status, selected, sort_order, created_at, updated_at)
               VALUES ('prop-2', 'sess-1', 'Modified', 'fix', 'medium',
               50, 'critical', 1, 'modified', 1, 3, '2026-01-24T10:00:00Z', '2026-01-24T12:00:00Z')"#,
            [],
        )
        .unwrap();

    let proposal: TaskProposal = conn
        .query_row(
            "SELECT * FROM task_proposals WHERE id = 'prop-2'",
            [],
            |row| TaskProposal::from_row(row),
        )
        .unwrap();

    assert_eq!(proposal.user_priority, Some(Priority::Critical));
    assert!(proposal.user_modified);
    assert_eq!(proposal.status, ProposalStatus::Modified);
    assert_eq!(proposal.sort_order, 3);
}

#[test]
fn proposal_from_row_with_priority_factors() {
    let conn = setup_proposal_test_db();
    conn.execute(
            r#"INSERT INTO task_proposals (id, session_id, title, category, suggested_priority,
               priority_score, priority_factors, status, selected, sort_order, created_at, updated_at)
               VALUES ('prop-3', 'sess-1', 'With Factors', 'feature', 'high',
               80, '{"dependency":25,"business_value":30,"technical_risk":10,"user_demand":15}',
               'pending', 1, 0, '2026-01-24T10:00:00Z', '2026-01-24T10:00:00Z')"#,
            [],
        )
        .unwrap();

    let proposal: TaskProposal = conn
        .query_row(
            "SELECT * FROM task_proposals WHERE id = 'prop-3'",
            [],
            |row| TaskProposal::from_row(row),
        )
        .unwrap();

    assert!(proposal.priority_factors.is_some());
    let factors = proposal.priority_factors.unwrap();
    assert_eq!(factors.dependency, 25);
    assert_eq!(factors.business_value, 30);
}

#[test]
fn proposal_from_row_with_created_task() {
    let conn = setup_proposal_test_db();
    conn.execute(
            r#"INSERT INTO task_proposals (id, session_id, title, category, suggested_priority,
               priority_score, status, selected, created_task_id, sort_order, created_at, updated_at)
               VALUES ('prop-4', 'sess-1', 'Has Created Task', 'feature', 'medium',
               50, 'accepted', 1, 'task-abc', 0, '2026-01-24T10:00:00Z', '2026-01-24T14:00:00Z')"#,
            [],
        )
        .unwrap();

    let proposal: TaskProposal = conn
        .query_row(
            "SELECT * FROM task_proposals WHERE id = 'prop-4'",
            [],
            |row| TaskProposal::from_row(row),
        )
        .unwrap();

    assert!(proposal.created_task_id.is_some());
    assert_eq!(
        proposal.created_task_id.as_ref().unwrap().as_str(),
        "task-abc"
    );
    assert!(proposal.has_created_task());
}

#[test]
fn proposal_from_row_unknown_category_defaults_to_feature() {
    let conn = setup_proposal_test_db();
    conn.execute(
        r#"INSERT INTO task_proposals (id, session_id, title, category, suggested_priority,
               priority_score, status, selected, sort_order, created_at, updated_at)
               VALUES ('prop-5', 'sess-1', 'Unknown Cat', 'invalid_category', 'medium',
               50, 'pending', 1, 0, '2026-01-24T10:00:00Z', '2026-01-24T10:00:00Z')"#,
        [],
    )
    .unwrap();

    let proposal: TaskProposal = conn
        .query_row(
            "SELECT * FROM task_proposals WHERE id = 'prop-5'",
            [],
            |row| TaskProposal::from_row(row),
        )
        .unwrap();

    assert_eq!(proposal.category, ProposalCategory::Feature);
}

// ==========================================
// DependencyFactor Tests
// ==========================================

#[test]
fn dependency_factor_default() {
    let factor = DependencyFactor::default();
    assert_eq!(factor.score, 0);
    assert_eq!(factor.blocks_count, 0);
    assert_eq!(factor.reason, "");
}

#[test]
fn dependency_factor_new() {
    let factor = DependencyFactor::new(25, 3, "Blocks 3 tasks");
    assert_eq!(factor.score, 25);
    assert_eq!(factor.blocks_count, 3);
    assert_eq!(factor.reason, "Blocks 3 tasks");
}

#[test]
fn dependency_factor_new_clamps_score() {
    let factor = DependencyFactor::new(50, 5, "Too high");
    assert_eq!(factor.score, 30); // Max is 30
}

#[test]
fn dependency_factor_calculate_zero_blocks() {
    let factor = DependencyFactor::calculate(0);
    assert_eq!(factor.score, 0);
    assert_eq!(factor.blocks_count, 0);
    assert_eq!(factor.reason, "Does not block other tasks");
}

#[test]
fn dependency_factor_calculate_one_block() {
    let factor = DependencyFactor::calculate(1);
    assert_eq!(factor.score, 10);
    assert_eq!(factor.blocks_count, 1);
    assert_eq!(factor.reason, "Blocks 1 other task");
}

#[test]
fn dependency_factor_calculate_two_blocks() {
    let factor = DependencyFactor::calculate(2);
    assert_eq!(factor.score, 18);
    assert_eq!(factor.blocks_count, 2);
}

#[test]
fn dependency_factor_calculate_three_blocks() {
    let factor = DependencyFactor::calculate(3);
    assert_eq!(factor.score, 24);
}

#[test]
fn dependency_factor_calculate_many_blocks() {
    let factor = DependencyFactor::calculate(10);
    assert_eq!(factor.score, 30); // Max score
    assert_eq!(factor.blocks_count, 10);
}

#[test]
fn dependency_factor_serializes() {
    let factor = DependencyFactor::new(20, 2, "Test");
    let json = serde_json::to_string(&factor).unwrap();
    assert!(json.contains("\"score\":20"));
    assert!(json.contains("\"blocks_count\":2"));
}

#[test]
fn dependency_factor_deserializes() {
    let json = r#"{"score":15,"blocks_count":1,"reason":"Blocks 1 task"}"#;
    let factor: DependencyFactor = serde_json::from_str(json).unwrap();
    assert_eq!(factor.score, 15);
    assert_eq!(factor.blocks_count, 1);
}

#[test]
fn dependency_factor_max_score_constant() {
    assert_eq!(DependencyFactor::MAX_SCORE, 30);
}

// ==========================================
// CriticalPathFactor Tests
// ==========================================

#[test]
fn critical_path_factor_default() {
    let factor = CriticalPathFactor::default();
    assert_eq!(factor.score, 0);
    assert!(!factor.is_on_critical_path);
    assert_eq!(factor.path_length, 0);
}

#[test]
fn critical_path_factor_new() {
    let factor = CriticalPathFactor::new(20, true, 3, "On critical path");
    assert_eq!(factor.score, 20);
    assert!(factor.is_on_critical_path);
    assert_eq!(factor.path_length, 3);
}

#[test]
fn critical_path_factor_new_clamps_score() {
    let factor = CriticalPathFactor::new(50, true, 5, "Too high");
    assert_eq!(factor.score, 25); // Max is 25
}

#[test]
fn critical_path_factor_calculate_not_on_path() {
    let factor = CriticalPathFactor::calculate(false, 0);
    assert_eq!(factor.score, 0);
    assert!(!factor.is_on_critical_path);
}

#[test]
fn critical_path_factor_calculate_path_length_1() {
    let factor = CriticalPathFactor::calculate(true, 1);
    assert_eq!(factor.score, 10);
    assert!(factor.is_on_critical_path);
}

#[test]
fn critical_path_factor_calculate_path_length_2() {
    let factor = CriticalPathFactor::calculate(true, 2);
    assert_eq!(factor.score, 15);
}

#[test]
fn critical_path_factor_calculate_path_length_3() {
    let factor = CriticalPathFactor::calculate(true, 3);
    assert_eq!(factor.score, 20);
}

#[test]
fn critical_path_factor_calculate_long_path() {
    let factor = CriticalPathFactor::calculate(true, 10);
    assert_eq!(factor.score, 25); // Max score
}

#[test]
fn critical_path_factor_serializes() {
    let factor = CriticalPathFactor::new(15, true, 2, "On path");
    let json = serde_json::to_string(&factor).unwrap();
    assert!(json.contains("\"is_on_critical_path\":true"));
    assert!(json.contains("\"path_length\":2"));
}

#[test]
fn critical_path_factor_max_score_constant() {
    assert_eq!(CriticalPathFactor::MAX_SCORE, 25);
}

// ==========================================
// BusinessValueFactor Tests
// ==========================================

#[test]
fn business_value_factor_default() {
    let factor = BusinessValueFactor::default();
    assert_eq!(factor.score, 0);
    assert!(factor.keywords.is_empty());
}

#[test]
fn business_value_factor_new() {
    let factor = BusinessValueFactor::new(15, vec!["mvp".to_string()], "Contains MVP");
    assert_eq!(factor.score, 15);
    assert_eq!(factor.keywords.len(), 1);
}

#[test]
fn business_value_factor_new_clamps_score() {
    let factor = BusinessValueFactor::new(50, vec![], "Too high");
    assert_eq!(factor.score, 20); // Max is 20
}

#[test]
fn business_value_factor_calculate_critical_keywords() {
    let factor = BusinessValueFactor::calculate("This is URGENT and blocking other work");
    assert_eq!(factor.score, 20);
    assert!(factor.keywords.contains(&"urgent".to_string()));
    assert!(factor.keywords.contains(&"blocking".to_string()));
}

#[test]
fn business_value_factor_calculate_high_keywords() {
    let factor = BusinessValueFactor::calculate("This is essential for the MVP");
    assert_eq!(factor.score, 15);
    assert!(
        factor.keywords.contains(&"essential".to_string())
            || factor.keywords.contains(&"mvp".to_string())
    );
}

#[test]
fn business_value_factor_calculate_low_keywords() {
    let factor = BusinessValueFactor::calculate("Nice to have feature for later");
    assert_eq!(factor.score, 5);
    assert!(
        factor.keywords.contains(&"nice to have".to_string())
            || factor.keywords.contains(&"later".to_string())
    );
}

#[test]
fn business_value_factor_calculate_no_keywords() {
    let factor = BusinessValueFactor::calculate("Just a regular task description");
    assert_eq!(factor.score, 10);
    assert!(factor.keywords.is_empty());
}

#[test]
fn business_value_factor_serializes() {
    let factor = BusinessValueFactor::new(15, vec!["important".to_string()], "Has important");
    let json = serde_json::to_string(&factor).unwrap();
    assert!(json.contains("\"keywords\":[\"important\"]"));
}

#[test]
fn business_value_factor_max_score_constant() {
    assert_eq!(BusinessValueFactor::MAX_SCORE, 20);
}

#[test]
fn business_value_factor_critical_keywords_exist() {
    #[allow(clippy::const_is_empty)]
    let not_empty = !BusinessValueFactor::CRITICAL_KEYWORDS.is_empty();
    assert!(not_empty);
    assert!(BusinessValueFactor::CRITICAL_KEYWORDS.contains(&"urgent"));
    assert!(BusinessValueFactor::CRITICAL_KEYWORDS.contains(&"blocker"));
}

#[test]
fn business_value_factor_high_keywords_exist() {
    #[allow(clippy::const_is_empty)]
    let not_empty = !BusinessValueFactor::HIGH_KEYWORDS.is_empty();
    assert!(not_empty);
    assert!(BusinessValueFactor::HIGH_KEYWORDS.contains(&"important"));
    assert!(BusinessValueFactor::HIGH_KEYWORDS.contains(&"mvp"));
}

#[test]
fn business_value_factor_low_keywords_exist() {
    #[allow(clippy::const_is_empty)]
    let not_empty = !BusinessValueFactor::LOW_KEYWORDS.is_empty();
    assert!(not_empty);
    assert!(BusinessValueFactor::LOW_KEYWORDS.contains(&"optional"));
    assert!(BusinessValueFactor::LOW_KEYWORDS.contains(&"future"));
}

// ==========================================
// ComplexityFactor Tests
// ==========================================

#[test]
fn complexity_factor_default() {
    let factor = ComplexityFactor::default();
    assert_eq!(factor.score, 0);
    assert_eq!(factor.complexity, Complexity::Moderate);
}

#[test]
fn complexity_factor_new() {
    let factor = ComplexityFactor::new(12, Complexity::Simple, "Simple task");
    assert_eq!(factor.score, 12);
    assert_eq!(factor.complexity, Complexity::Simple);
}

#[test]
fn complexity_factor_new_clamps_score() {
    let factor = ComplexityFactor::new(50, Complexity::Trivial, "Too high");
    assert_eq!(factor.score, 15); // Max is 15
}

#[test]
fn complexity_factor_calculate_trivial() {
    let factor = ComplexityFactor::calculate(Complexity::Trivial);
    assert_eq!(factor.score, 15);
    assert_eq!(factor.complexity, Complexity::Trivial);
    assert!(factor.reason.contains("trivial"));
}

#[test]
fn complexity_factor_calculate_simple() {
    let factor = ComplexityFactor::calculate(Complexity::Simple);
    assert_eq!(factor.score, 12);
}

#[test]
fn complexity_factor_calculate_moderate() {
    let factor = ComplexityFactor::calculate(Complexity::Moderate);
    assert_eq!(factor.score, 9);
}

#[test]
fn complexity_factor_calculate_complex() {
    let factor = ComplexityFactor::calculate(Complexity::Complex);
    assert_eq!(factor.score, 5);
}

#[test]
fn complexity_factor_calculate_very_complex() {
    let factor = ComplexityFactor::calculate(Complexity::VeryComplex);
    assert_eq!(factor.score, 2);
}

#[test]
fn complexity_factor_serializes() {
    let factor = ComplexityFactor::calculate(Complexity::Simple);
    let json = serde_json::to_string(&factor).unwrap();
    assert!(json.contains("\"complexity\":\"simple\""));
}

#[test]
fn complexity_factor_max_score_constant() {
    assert_eq!(ComplexityFactor::MAX_SCORE, 15);
}

// ==========================================
// UserHintFactor Tests
// ==========================================

#[test]
fn user_hint_factor_default() {
    let factor = UserHintFactor::default();
    assert_eq!(factor.score, 0);
    assert!(factor.hints.is_empty());
}

#[test]
fn user_hint_factor_new() {
    let factor = UserHintFactor::new(8, vec!["urgent".to_string()], "User said urgent");
    assert_eq!(factor.score, 8);
    assert_eq!(factor.hints.len(), 1);
}

#[test]
fn user_hint_factor_new_clamps_score() {
    let factor = UserHintFactor::new(50, vec![], "Too high");
    assert_eq!(factor.score, 10); // Max is 10
}

#[test]
fn user_hint_factor_calculate_no_hints() {
    let factor = UserHintFactor::calculate("Just a regular request");
    assert_eq!(factor.score, 0);
    assert!(factor.hints.is_empty());
}

#[test]
fn user_hint_factor_calculate_one_hint() {
    let factor = UserHintFactor::calculate("I need this done ASAP");
    assert_eq!(factor.score, 3);
    assert!(factor.hints.contains(&"asap".to_string()));
}

#[test]
fn user_hint_factor_calculate_multiple_hints() {
    let factor = UserHintFactor::calculate("This is urgent and blocking, do it first");
    assert!(factor.score >= 6);
    assert!(factor.hints.len() >= 2);
}

#[test]
fn user_hint_factor_calculate_max_score() {
    let factor = UserHintFactor::calculate("urgent asap immediately now today deadline blocker");
    assert_eq!(factor.score, 10); // Capped at max
}

#[test]
fn user_hint_factor_serializes() {
    let factor = UserHintFactor::new(
        6,
        vec!["urgent".to_string(), "asap".to_string()],
        "User hints",
    );
    let json = serde_json::to_string(&factor).unwrap();
    assert!(json.contains("\"hints\":["));
}

#[test]
fn user_hint_factor_max_score_constant() {
    assert_eq!(UserHintFactor::MAX_SCORE, 10);
}

#[test]
fn user_hint_factor_urgency_hints_exist() {
    #[allow(clippy::const_is_empty)]
    let not_empty = !UserHintFactor::URGENCY_HINTS.is_empty();
    assert!(not_empty);
    assert!(UserHintFactor::URGENCY_HINTS.contains(&"urgent"));
    assert!(UserHintFactor::URGENCY_HINTS.contains(&"asap"));
}

// ==========================================
// PriorityAssessmentFactors Tests
// ==========================================

#[test]
fn priority_assessment_factors_default() {
    let factors = PriorityAssessmentFactors::default();
    assert_eq!(factors.dependency_factor.score, 0);
    assert_eq!(factors.critical_path_factor.score, 0);
    assert_eq!(factors.business_value_factor.score, 0);
    assert_eq!(factors.complexity_factor.score, 0);
    assert_eq!(factors.user_hint_factor.score, 0);
}

#[test]
fn priority_assessment_factors_total_score() {
    let factors = PriorityAssessmentFactors {
        dependency_factor: DependencyFactor::new(20, 2, ""),
        critical_path_factor: CriticalPathFactor::new(15, true, 2, ""),
        business_value_factor: BusinessValueFactor::new(10, vec![], ""),
        complexity_factor: ComplexityFactor::new(8, Complexity::Moderate, ""),
        user_hint_factor: UserHintFactor::new(5, vec![], ""),
    };
    assert_eq!(factors.total_score(), 58);
}

#[test]
fn priority_assessment_factors_max_total() {
    assert_eq!(PriorityAssessmentFactors::MAX_TOTAL_SCORE, 100);
}

#[test]
fn priority_assessment_factors_serializes() {
    let factors = PriorityAssessmentFactors::default();
    let json = serde_json::to_string(&factors).unwrap();
    assert!(json.contains("\"dependency_factor\""));
    assert!(json.contains("\"critical_path_factor\""));
    assert!(json.contains("\"business_value_factor\""));
    assert!(json.contains("\"complexity_factor\""));
    assert!(json.contains("\"user_hint_factor\""));
}

// ==========================================
// PriorityAssessment Tests
// ==========================================

#[test]
fn priority_assessment_new() {
    let proposal_id = TaskProposalId::new();
    let factors = PriorityAssessmentFactors {
        dependency_factor: DependencyFactor::calculate(2),
        critical_path_factor: CriticalPathFactor::calculate(true, 3),
        business_value_factor: BusinessValueFactor::calculate("This is essential"),
        complexity_factor: ComplexityFactor::calculate(Complexity::Simple),
        user_hint_factor: UserHintFactor::calculate("urgent"),
    };

    let assessment = PriorityAssessment::new(proposal_id.clone(), factors);

    assert_eq!(assessment.proposal_id, proposal_id);
    assert!(assessment.priority_score > 0);
    assert!(!assessment.priority_reason.is_empty());
}

#[test]
fn priority_assessment_score_to_priority_critical() {
    assert_eq!(
        PriorityAssessment::score_to_priority(100),
        Priority::Critical
    );
    assert_eq!(
        PriorityAssessment::score_to_priority(85),
        Priority::Critical
    );
    assert_eq!(
        PriorityAssessment::score_to_priority(80),
        Priority::Critical
    );
}

#[test]
fn priority_assessment_score_to_priority_high() {
    assert_eq!(PriorityAssessment::score_to_priority(79), Priority::High);
    assert_eq!(PriorityAssessment::score_to_priority(70), Priority::High);
    assert_eq!(PriorityAssessment::score_to_priority(60), Priority::High);
}

#[test]
fn priority_assessment_score_to_priority_medium() {
    assert_eq!(PriorityAssessment::score_to_priority(59), Priority::Medium);
    assert_eq!(PriorityAssessment::score_to_priority(50), Priority::Medium);
    assert_eq!(PriorityAssessment::score_to_priority(40), Priority::Medium);
}

#[test]
fn priority_assessment_score_to_priority_low() {
    assert_eq!(PriorityAssessment::score_to_priority(39), Priority::Low);
    assert_eq!(PriorityAssessment::score_to_priority(20), Priority::Low);
    assert_eq!(PriorityAssessment::score_to_priority(0), Priority::Low);
}

#[test]
fn priority_assessment_neutral() {
    let proposal_id = TaskProposalId::new();
    let assessment = PriorityAssessment::neutral(proposal_id.clone());

    assert_eq!(assessment.proposal_id, proposal_id);
    assert_eq!(assessment.priority_score, 0);
    assert_eq!(assessment.suggested_priority, Priority::Low);
}

#[test]
fn priority_assessment_serializes() {
    let proposal_id = TaskProposalId::new();
    let assessment = PriorityAssessment::neutral(proposal_id);
    let json = serde_json::to_string(&assessment).unwrap();

    assert!(json.contains("\"proposal_id\""));
    assert!(json.contains("\"suggested_priority\""));
    assert!(json.contains("\"priority_score\""));
    assert!(json.contains("\"priority_reason\""));
    assert!(json.contains("\"factors\""));
}

#[test]
fn priority_assessment_deserializes() {
    let json = r#"{
            "proposal_id": "prop-123",
            "suggested_priority": "high",
            "priority_score": 75,
            "priority_reason": "Important task",
            "factors": {
                "dependency_factor": {"score": 20, "blocks_count": 2, "reason": "Blocks 2 tasks"},
                "critical_path_factor": {"score": 15, "is_on_critical_path": true, "path_length": 2, "reason": "On path"},
                "business_value_factor": {"score": 15, "keywords": ["important"], "reason": "High value"},
                "complexity_factor": {"score": 12, "complexity": "simple", "reason": "Simple"},
                "user_hint_factor": {"score": 6, "hints": ["urgent"], "reason": "User urgent"}
            }
        }"#;

    let assessment: PriorityAssessment = serde_json::from_str(json).unwrap();

    assert_eq!(assessment.proposal_id.as_str(), "prop-123");
    assert_eq!(assessment.suggested_priority, Priority::High);
    assert_eq!(assessment.priority_score, 75);
    assert_eq!(assessment.factors.dependency_factor.score, 20);
    assert_eq!(assessment.factors.critical_path_factor.path_length, 2);
}

#[test]
fn priority_assessment_roundtrip_serialization() {
    let proposal_id = TaskProposalId::new();
    let factors = PriorityAssessmentFactors {
        dependency_factor: DependencyFactor::calculate(3),
        critical_path_factor: CriticalPathFactor::calculate(true, 4),
        business_value_factor: BusinessValueFactor::calculate("critical blocker"),
        complexity_factor: ComplexityFactor::calculate(Complexity::Trivial),
        user_hint_factor: UserHintFactor::calculate("urgent asap"),
    };
    let original = PriorityAssessment::new(proposal_id, factors);

    let json = serde_json::to_string(&original).unwrap();
    let restored: PriorityAssessment = serde_json::from_str(&json).unwrap();

    assert_eq!(original.proposal_id, restored.proposal_id);
    assert_eq!(original.suggested_priority, restored.suggested_priority);
    assert_eq!(original.priority_score, restored.priority_score);
    assert_eq!(
        original.factors.dependency_factor.score,
        restored.factors.dependency_factor.score
    );
}

#[test]
fn priority_assessment_high_score_yields_critical_priority() {
    let proposal_id = TaskProposalId::new();
    let factors = PriorityAssessmentFactors {
        dependency_factor: DependencyFactor::new(30, 5, ""),
        critical_path_factor: CriticalPathFactor::new(25, true, 5, ""),
        business_value_factor: BusinessValueFactor::new(20, vec![], ""),
        complexity_factor: ComplexityFactor::new(15, Complexity::Trivial, ""),
        user_hint_factor: UserHintFactor::new(10, vec![], ""),
    };
    let assessment = PriorityAssessment::new(proposal_id, factors);

    assert_eq!(assessment.priority_score, 100);
    assert_eq!(assessment.suggested_priority, Priority::Critical);
}

// ==========================================
// MessageRole Tests
// ==========================================

#[test]
fn message_role_default_is_user() {
    assert_eq!(MessageRole::default(), MessageRole::User);
}

#[test]
fn message_role_display_user() {
    assert_eq!(format!("{}", MessageRole::User), "user");
}

#[test]
fn message_role_display_orchestrator() {
    assert_eq!(format!("{}", MessageRole::Orchestrator), "orchestrator");
}

#[test]
fn message_role_display_system() {
    assert_eq!(format!("{}", MessageRole::System), "system");
}

#[test]
fn message_role_from_str_user() {
    let role: MessageRole = "user".parse().unwrap();
    assert_eq!(role, MessageRole::User);
}

#[test]
fn message_role_from_str_orchestrator() {
    let role: MessageRole = "orchestrator".parse().unwrap();
    assert_eq!(role, MessageRole::Orchestrator);
}

#[test]
fn message_role_from_str_system() {
    let role: MessageRole = "system".parse().unwrap();
    assert_eq!(role, MessageRole::System);
}

#[test]
fn message_role_from_str_invalid() {
    let result: Result<MessageRole, _> = "invalid".parse();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.value, "invalid");
    assert!(err.to_string().contains("unknown message role"));
}

#[test]
fn message_role_serializes_to_json() {
    let json = serde_json::to_string(&MessageRole::Orchestrator).unwrap();
    assert_eq!(json, "\"orchestrator\"");
}

#[test]
fn message_role_deserializes_from_json() {
    let role: MessageRole = serde_json::from_str("\"system\"").unwrap();
    assert_eq!(role, MessageRole::System);
}

#[test]
fn message_role_clone_works() {
    let role = MessageRole::Orchestrator;
    #[allow(clippy::clone_on_copy)]
    let cloned = role.clone();
    assert_eq!(role, cloned);
}

// ==========================================
// ChatMessage Tests
// ==========================================

#[test]
fn chat_message_user_in_session() {
    let session_id = IdeationSessionId::new();
    let msg = ChatMessage::user_in_session(session_id.clone(), "Hello world");

    assert_eq!(msg.session_id, Some(session_id));
    assert!(msg.project_id.is_none());
    assert!(msg.task_id.is_none());
    assert_eq!(msg.role, MessageRole::User);
    assert_eq!(msg.content, "Hello world");
    assert!(msg.metadata.is_none());
    assert!(msg.parent_message_id.is_none());
}

#[test]
fn chat_message_orchestrator_in_session() {
    let session_id = IdeationSessionId::new();
    let msg = ChatMessage::orchestrator_in_session(session_id.clone(), "I can help with that");

    assert_eq!(msg.session_id, Some(session_id));
    assert_eq!(msg.role, MessageRole::Orchestrator);
    assert_eq!(msg.content, "I can help with that");
}

#[test]
fn chat_message_system_in_session() {
    let session_id = IdeationSessionId::new();
    let msg = ChatMessage::system_in_session(session_id.clone(), "Session started");

    assert_eq!(msg.role, MessageRole::System);
    assert_eq!(msg.content, "Session started");
}

#[test]
fn chat_message_user_in_project() {
    let project_id = ProjectId::new();
    let msg = ChatMessage::user_in_project(project_id.clone(), "Project question");

    assert!(msg.session_id.is_none());
    assert_eq!(msg.project_id, Some(project_id));
    assert!(msg.task_id.is_none());
    assert_eq!(msg.role, MessageRole::User);
}

#[test]
fn chat_message_user_about_task() {
    let task_id = TaskId::new();
    let msg = ChatMessage::user_about_task(task_id.clone(), "Task question");

    assert!(msg.session_id.is_none());
    assert!(msg.project_id.is_none());
    assert_eq!(msg.task_id, Some(task_id));
    assert_eq!(msg.role, MessageRole::User);
}

#[test]
fn chat_message_with_metadata() {
    let session_id = IdeationSessionId::new();
    let msg = ChatMessage::user_in_session(session_id, "Test").with_metadata(r#"{"key": "value"}"#);

    assert_eq!(msg.metadata, Some(r#"{"key": "value"}"#.to_string()));
}

#[test]
fn chat_message_with_parent() {
    let session_id = IdeationSessionId::new();
    let parent_id = ChatMessageId::new();
    let msg = ChatMessage::user_in_session(session_id, "Reply").with_parent(parent_id.clone());

    assert_eq!(msg.parent_message_id, Some(parent_id));
}

#[test]
fn chat_message_is_user_true() {
    let msg = ChatMessage::user_in_session(IdeationSessionId::new(), "Test");
    assert!(msg.is_user());
    assert!(!msg.is_orchestrator());
    assert!(!msg.is_system());
}

#[test]
fn chat_message_is_orchestrator_true() {
    let msg = ChatMessage::orchestrator_in_session(IdeationSessionId::new(), "Test");
    assert!(!msg.is_user());
    assert!(msg.is_orchestrator());
    assert!(!msg.is_system());
}

#[test]
fn chat_message_is_system_true() {
    let msg = ChatMessage::system_in_session(IdeationSessionId::new(), "Test");
    assert!(!msg.is_user());
    assert!(!msg.is_orchestrator());
    assert!(msg.is_system());
}

#[test]
fn chat_message_serializes() {
    let msg = ChatMessage::user_in_session(IdeationSessionId::new(), "Serialize test");
    let json = serde_json::to_string(&msg).unwrap();

    assert!(json.contains("\"role\":\"user\""));
    assert!(json.contains("\"content\":\"Serialize test\""));
}

#[test]
fn chat_message_deserializes() {
    let json = r#"{
            "id": "msg-123",
            "session_id": "sess-456",
            "project_id": null,
            "task_id": null,
            "role": "orchestrator",
            "content": "Hello there",
            "metadata": null,
            "parent_message_id": null,
            "created_at": "2026-01-24T12:00:00Z"
        }"#;

    let msg: ChatMessage = serde_json::from_str(json).unwrap();

    assert_eq!(msg.id.as_str(), "msg-123");
    assert_eq!(msg.session_id.as_ref().unwrap().as_str(), "sess-456");
    assert_eq!(msg.role, MessageRole::Orchestrator);
    assert_eq!(msg.content, "Hello there");
}

#[test]
fn chat_message_roundtrip_serialization() {
    let original = ChatMessage::user_in_session(IdeationSessionId::new(), "Roundtrip")
        .with_metadata("some meta");

    let json = serde_json::to_string(&original).unwrap();
    let restored: ChatMessage = serde_json::from_str(&json).unwrap();

    assert_eq!(original.id, restored.id);
    assert_eq!(original.role, restored.role);
    assert_eq!(original.content, restored.content);
    assert_eq!(original.metadata, restored.metadata);
}

#[test]
fn chat_message_from_row_works() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        r#"CREATE TABLE chat_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                project_id TEXT,
                task_id TEXT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT,
                parent_message_id TEXT,
                created_at TEXT NOT NULL
            )"#,
        [],
    )
    .unwrap();

    conn.execute(
            r#"INSERT INTO chat_messages (id, session_id, project_id, task_id, role, content, metadata, parent_message_id, created_at)
               VALUES ('msg-1', 'sess-1', NULL, NULL, 'user', 'Test message', NULL, NULL, '2026-01-24T10:00:00Z')"#,
            [],
        ).unwrap();

    let msg: ChatMessage = conn
        .query_row(
            "SELECT * FROM chat_messages WHERE id = 'msg-1'",
            [],
            |row| ChatMessage::from_row(row),
        )
        .unwrap();

    assert_eq!(msg.id.as_str(), "msg-1");
    assert_eq!(msg.session_id.as_ref().unwrap().as_str(), "sess-1");
    assert_eq!(msg.role, MessageRole::User);
    assert_eq!(msg.content, "Test message");
}

#[test]
fn chat_message_from_row_with_task_context() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        r#"CREATE TABLE chat_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                project_id TEXT,
                task_id TEXT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT,
                parent_message_id TEXT,
                created_at TEXT NOT NULL
            )"#,
        [],
    )
    .unwrap();

    conn.execute(
            r#"INSERT INTO chat_messages (id, session_id, project_id, task_id, role, content, metadata, parent_message_id, created_at)
               VALUES ('msg-2', NULL, 'proj-1', 'task-1', 'orchestrator', 'Task help', '{"foo":"bar"}', NULL, '2026-01-24T11:00:00Z')"#,
            [],
        ).unwrap();

    let msg: ChatMessage = conn
        .query_row(
            "SELECT * FROM chat_messages WHERE id = 'msg-2'",
            [],
            |row| ChatMessage::from_row(row),
        )
        .unwrap();

    assert!(msg.session_id.is_none());
    assert_eq!(msg.project_id.as_ref().unwrap().as_str(), "proj-1");
    assert_eq!(msg.task_id.as_ref().unwrap().as_str(), "task-1");
    assert_eq!(msg.role, MessageRole::Orchestrator);
    assert_eq!(msg.metadata, Some(r#"{"foo":"bar"}"#.to_string()));
}

// ==========================================
// DependencyGraphNode Tests
// ==========================================

#[test]
fn dependency_graph_node_new() {
    let node = DependencyGraphNode::new(TaskProposalId::from_string("prop-1"), "Test Task");

    assert_eq!(node.proposal_id.as_str(), "prop-1");
    assert_eq!(node.title, "Test Task");
    assert_eq!(node.in_degree, 0);
    assert_eq!(node.out_degree, 0);
}

#[test]
fn dependency_graph_node_with_degrees() {
    let node = DependencyGraphNode::new(TaskProposalId::new(), "Task")
        .with_in_degree(2)
        .with_out_degree(3);

    assert_eq!(node.in_degree, 2);
    assert_eq!(node.out_degree, 3);
}

#[test]
fn dependency_graph_node_is_root() {
    let root = DependencyGraphNode::new(TaskProposalId::new(), "Root").with_in_degree(0);
    let not_root = DependencyGraphNode::new(TaskProposalId::new(), "Not Root").with_in_degree(1);

    assert!(root.is_root());
    assert!(!not_root.is_root());
}

#[test]
fn dependency_graph_node_is_leaf() {
    let leaf = DependencyGraphNode::new(TaskProposalId::new(), "Leaf").with_out_degree(0);
    let not_leaf = DependencyGraphNode::new(TaskProposalId::new(), "Not Leaf").with_out_degree(1);

    assert!(leaf.is_leaf());
    assert!(!not_leaf.is_leaf());
}

#[test]
fn dependency_graph_node_is_blocker() {
    let blocker = DependencyGraphNode::new(TaskProposalId::new(), "Blocker").with_out_degree(2);
    let not_blocker =
        DependencyGraphNode::new(TaskProposalId::new(), "Not Blocker").with_out_degree(0);

    assert!(blocker.is_blocker());
    assert!(!not_blocker.is_blocker());
}

#[test]
fn dependency_graph_node_serializes() {
    let node = DependencyGraphNode::new(TaskProposalId::from_string("prop-1"), "Serialize")
        .with_in_degree(1)
        .with_out_degree(2);
    let json = serde_json::to_string(&node).unwrap();

    assert!(json.contains("\"proposal_id\":\"prop-1\""));
    assert!(json.contains("\"title\":\"Serialize\""));
    assert!(json.contains("\"in_degree\":1"));
    assert!(json.contains("\"out_degree\":2"));
}

#[test]
fn dependency_graph_node_deserializes() {
    let json = r#"{
            "proposal_id": "prop-123",
            "title": "Test Node",
            "in_degree": 3,
            "out_degree": 1
        }"#;

    let node: DependencyGraphNode = serde_json::from_str(json).unwrap();

    assert_eq!(node.proposal_id.as_str(), "prop-123");
    assert_eq!(node.title, "Test Node");
    assert_eq!(node.in_degree, 3);
    assert_eq!(node.out_degree, 1);
}

#[test]
fn dependency_graph_node_equality() {
    let id = TaskProposalId::from_string("same-id");
    let node1 = DependencyGraphNode::new(id.clone(), "Node 1").with_in_degree(1);
    let node2 = DependencyGraphNode::new(id.clone(), "Node 1").with_in_degree(1);
    let node3 = DependencyGraphNode::new(id, "Node 1").with_in_degree(2);

    assert_eq!(node1, node2);
    assert_ne!(node1, node3);
}

// ==========================================
// DependencyGraphEdge Tests
// ==========================================

#[test]
fn dependency_graph_edge_new() {
    let from = TaskProposalId::from_string("from-1");
    let to = TaskProposalId::from_string("to-1");
    let edge = DependencyGraphEdge::new(from.clone(), to.clone());

    assert_eq!(edge.from, from);
    assert_eq!(edge.to, to);
}

#[test]
fn dependency_graph_edge_serializes() {
    let edge = DependencyGraphEdge::new(
        TaskProposalId::from_string("prop-a"),
        TaskProposalId::from_string("prop-b"),
    );
    let json = serde_json::to_string(&edge).unwrap();

    assert!(json.contains("\"from\":\"prop-a\""));
    assert!(json.contains("\"to\":\"prop-b\""));
}

#[test]
fn dependency_graph_edge_deserializes() {
    let json = r#"{"from": "edge-from", "to": "edge-to"}"#;
    let edge: DependencyGraphEdge = serde_json::from_str(json).unwrap();

    assert_eq!(edge.from.as_str(), "edge-from");
    assert_eq!(edge.to.as_str(), "edge-to");
}

#[test]
fn dependency_graph_edge_equality() {
    let edge1 = DependencyGraphEdge::new(
        TaskProposalId::from_string("a"),
        TaskProposalId::from_string("b"),
    );
    let edge2 = DependencyGraphEdge::new(
        TaskProposalId::from_string("a"),
        TaskProposalId::from_string("b"),
    );
    let edge3 = DependencyGraphEdge::new(
        TaskProposalId::from_string("a"),
        TaskProposalId::from_string("c"),
    );

    assert_eq!(edge1, edge2);
    assert_ne!(edge1, edge3);
}

// ==========================================
// DependencyGraph Tests
// ==========================================

#[test]
fn dependency_graph_new() {
    let graph = DependencyGraph::new();

    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());
    assert!(graph.critical_path.is_empty());
    assert!(!graph.has_cycles);
    assert!(graph.cycles.is_none());
}

#[test]
fn dependency_graph_default() {
    let graph: DependencyGraph = Default::default();
    assert!(graph.is_empty());
}

#[test]
fn dependency_graph_with_nodes_and_edges() {
    let nodes = vec![
        DependencyGraphNode::new(TaskProposalId::from_string("n1"), "Node 1"),
        DependencyGraphNode::new(TaskProposalId::from_string("n2"), "Node 2"),
    ];
    let edges = vec![DependencyGraphEdge::new(
        TaskProposalId::from_string("n2"),
        TaskProposalId::from_string("n1"),
    )];

    let graph = DependencyGraph::with_nodes_and_edges(nodes, edges);

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 1);
}

#[test]
fn dependency_graph_add_node() {
    let mut graph = DependencyGraph::new();
    graph.add_node(DependencyGraphNode::new(TaskProposalId::new(), "Added"));

    assert_eq!(graph.node_count(), 1);
}

#[test]
fn dependency_graph_add_edge() {
    let mut graph = DependencyGraph::new();
    graph.add_edge(DependencyGraphEdge::new(
        TaskProposalId::from_string("a"),
        TaskProposalId::from_string("b"),
    ));

    assert_eq!(graph.edge_count(), 1);
}

#[test]
fn dependency_graph_set_critical_path() {
    let mut graph = DependencyGraph::new();
    let path = vec![
        TaskProposalId::from_string("step1"),
        TaskProposalId::from_string("step2"),
        TaskProposalId::from_string("step3"),
    ];

    graph.set_critical_path(path.clone());

    assert_eq!(graph.critical_path, path);
    assert_eq!(graph.critical_path_length(), 3);
}

#[test]
fn dependency_graph_set_cycles() {
    let mut graph = DependencyGraph::new();
    let cycles = vec![vec![
        TaskProposalId::from_string("a"),
        TaskProposalId::from_string("b"),
        TaskProposalId::from_string("a"),
    ]];

    graph.set_cycles(cycles.clone());

    assert!(graph.has_cycles);
    assert!(graph.cycles.is_some());
    assert_eq!(graph.cycles.as_ref().unwrap().len(), 1);
}

#[test]
fn dependency_graph_set_empty_cycles() {
    let mut graph = DependencyGraph::new();
    graph.set_cycles(vec![]);

    assert!(!graph.has_cycles);
    assert!(graph.cycles.is_none());
}

#[test]
fn dependency_graph_get_node() {
    let id = TaskProposalId::from_string("find-me");
    let mut graph = DependencyGraph::new();
    graph.add_node(DependencyGraphNode::new(id.clone(), "Find Me"));

    let found = graph.get_node(&id);
    assert!(found.is_some());
    assert_eq!(found.unwrap().title, "Find Me");

    let not_found = graph.get_node(&TaskProposalId::from_string("not-there"));
    assert!(not_found.is_none());
}

#[test]
fn dependency_graph_get_dependencies() {
    let id = TaskProposalId::from_string("a");
    let mut graph = DependencyGraph::new();
    graph.add_edge(DependencyGraphEdge::new(
        id.clone(),
        TaskProposalId::from_string("b"),
    ));
    graph.add_edge(DependencyGraphEdge::new(
        id.clone(),
        TaskProposalId::from_string("c"),
    ));
    graph.add_edge(DependencyGraphEdge::new(
        TaskProposalId::from_string("d"),
        TaskProposalId::from_string("a"),
    ));

    let deps = graph.get_dependencies(&id);
    assert_eq!(deps.len(), 2);
}

#[test]
fn dependency_graph_get_dependents() {
    let id = TaskProposalId::from_string("target");
    let mut graph = DependencyGraph::new();
    graph.add_edge(DependencyGraphEdge::new(
        TaskProposalId::from_string("a"),
        id.clone(),
    ));
    graph.add_edge(DependencyGraphEdge::new(
        TaskProposalId::from_string("b"),
        id.clone(),
    ));

    let deps = graph.get_dependents(&id);
    assert_eq!(deps.len(), 2);
}

#[test]
fn dependency_graph_get_roots() {
    let mut graph = DependencyGraph::new();
    graph.add_node(DependencyGraphNode::new(TaskProposalId::new(), "Root 1").with_in_degree(0));
    graph.add_node(DependencyGraphNode::new(TaskProposalId::new(), "Root 2").with_in_degree(0));
    graph.add_node(DependencyGraphNode::new(TaskProposalId::new(), "Not Root").with_in_degree(1));

    let roots = graph.get_roots();
    assert_eq!(roots.len(), 2);
}

#[test]
fn dependency_graph_get_leaves() {
    let mut graph = DependencyGraph::new();
    graph.add_node(DependencyGraphNode::new(TaskProposalId::new(), "Leaf 1").with_out_degree(0));
    graph.add_node(DependencyGraphNode::new(TaskProposalId::new(), "Not Leaf").with_out_degree(2));

    let leaves = graph.get_leaves();
    assert_eq!(leaves.len(), 1);
}

#[test]
fn dependency_graph_is_on_critical_path() {
    let on_path = TaskProposalId::from_string("on-path");
    let off_path = TaskProposalId::from_string("off-path");
    let mut graph = DependencyGraph::new();
    graph.set_critical_path(vec![on_path.clone(), TaskProposalId::from_string("other")]);

    assert!(graph.is_on_critical_path(&on_path));
    assert!(!graph.is_on_critical_path(&off_path));
}

#[test]
fn dependency_graph_serializes() {
    let mut graph = DependencyGraph::new();
    graph.add_node(DependencyGraphNode::new(
        TaskProposalId::from_string("p1"),
        "Node 1",
    ));
    graph.add_edge(DependencyGraphEdge::new(
        TaskProposalId::from_string("p2"),
        TaskProposalId::from_string("p1"),
    ));

    let json = serde_json::to_string(&graph).unwrap();

    assert!(json.contains("\"nodes\":["));
    assert!(json.contains("\"edges\":["));
    assert!(json.contains("\"has_cycles\":false"));
}

#[test]
fn dependency_graph_deserializes() {
    let json = r#"{
            "nodes": [
                {"proposal_id": "p1", "title": "Node 1", "in_degree": 0, "out_degree": 1}
            ],
            "edges": [
                {"from": "p2", "to": "p1"}
            ],
            "critical_path": ["p1"],
            "has_cycles": false,
            "cycles": null
        }"#;

    let graph: DependencyGraph = serde_json::from_str(json).unwrap();

    assert_eq!(graph.node_count(), 1);
    assert_eq!(graph.edge_count(), 1);
    assert_eq!(graph.critical_path_length(), 1);
    assert!(!graph.has_cycles);
}

#[test]
fn dependency_graph_roundtrip_serialization() {
    let mut original = DependencyGraph::new();
    original.add_node(DependencyGraphNode::new(
        TaskProposalId::from_string("a"),
        "A",
    ));
    original.add_node(DependencyGraphNode::new(
        TaskProposalId::from_string("b"),
        "B",
    ));
    original.add_edge(DependencyGraphEdge::new(
        TaskProposalId::from_string("b"),
        TaskProposalId::from_string("a"),
    ));
    original.set_critical_path(vec![
        TaskProposalId::from_string("a"),
        TaskProposalId::from_string("b"),
    ]);

    let json = serde_json::to_string(&original).unwrap();
    let restored: DependencyGraph = serde_json::from_str(&json).unwrap();

    assert_eq!(original.node_count(), restored.node_count());
    assert_eq!(original.edge_count(), restored.edge_count());
    assert_eq!(original.critical_path, restored.critical_path);
}

// ===== VerificationStatus Tests =====

#[test]
fn verification_status_default_is_unverified() {
    assert_eq!(
        VerificationStatus::default(),
        VerificationStatus::Unverified
    );
}

#[test]
fn verification_status_display_roundtrip() {
    let cases = [
        (VerificationStatus::Unverified, "unverified"),
        (VerificationStatus::Reviewing, "reviewing"),
        (VerificationStatus::Verified, "verified"),
        (VerificationStatus::NeedsRevision, "needs_revision"),
        (VerificationStatus::Skipped, "skipped"),
        (VerificationStatus::ImportedVerified, "imported_verified"),
    ];
    for (status, expected) in cases {
        assert_eq!(format!("{}", status), expected);
        let parsed: VerificationStatus = expected.parse().expect("should parse");
        assert_eq!(parsed, status);
    }
}

#[test]
fn imported_verified_display_and_from_str_round_trip() {
    let status = VerificationStatus::ImportedVerified;
    let s = status.to_string();
    assert_eq!(s, "imported_verified");
    let parsed: VerificationStatus = s.parse().unwrap();
    assert_eq!(parsed, VerificationStatus::ImportedVerified);
}

#[test]
fn imported_verified_serializes_to_snake_case() {
    let json = serde_json::to_string(&VerificationStatus::ImportedVerified).unwrap();
    assert_eq!(json, "\"imported_verified\"");
    let back: VerificationStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, VerificationStatus::ImportedVerified);
}

#[test]
fn verification_status_fromstr_unknown_errors() {
    let result = "unknown_status".parse::<VerificationStatus>();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("unknown verification status"));
}

#[test]
fn verification_status_serde_snake_case() {
    let json = serde_json::to_string(&VerificationStatus::NeedsRevision).unwrap();
    assert_eq!(json, "\"needs_revision\"");
    let back: VerificationStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, VerificationStatus::NeedsRevision);
}

// ===== Verification Snapshot Tests =====

#[test]
fn verification_round_snapshot_serde_defaults_parse_failed() {
    let json = r#"{"round":2,"gap_score":13,"fingerprints":["abc123"],"gaps":[]}"#;
    let round: VerificationRoundSnapshot = serde_json::from_str(json).unwrap();
    assert_eq!(round.round, 2);
    assert_eq!(round.gap_score, 13);
    assert_eq!(round.fingerprints, vec!["abc123".to_string()]);
    assert!(!round.parse_failed);
}

#[test]
fn verification_run_snapshot_serde_roundtrip() {
    let snapshot = VerificationRunSnapshot {
        generation: 4,
        status: VerificationStatus::NeedsRevision,
        in_progress: false,
        current_round: 3,
        max_rounds: 5,
        current_gaps: vec![VerificationGap {
            severity: "critical".to_string(),
            category: "security".to_string(),
            description: "Missing auth check".to_string(),
            why_it_matters: Some("Allows unauthorized access".to_string()),
            source: None,
        }],
        rounds: vec![VerificationRoundSnapshot {
            round: 3,
            gap_score: 13,
            fingerprints: vec!["abc123".to_string()],
            gaps: vec![],
            parse_failed: true,
        }],
        convergence_reason: Some("zero_blocking".to_string()),
        best_round_index: Some(2),
    };
    let json = serde_json::to_string(&snapshot).unwrap();
    let restored: VerificationRunSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.generation, 4);
    assert_eq!(restored.current_round, 3);
    assert_eq!(restored.rounds.len(), 1);
    assert_eq!(restored.rounds[0].gap_score, 13);
    assert!(restored.rounds[0].parse_failed);
    assert_eq!(restored.current_gaps[0].severity, "critical");
    assert_eq!(
        restored.convergence_reason,
        Some("zero_blocking".to_string())
    );
}

// ===== VerificationError Tests =====

#[test]
fn verification_error_display_not_verified() {
    let err = VerificationError::NotVerified;
    assert_eq!(err.to_string(), "Plan must be verified before accepting");
}

#[test]
fn verification_error_display_in_progress() {
    let err = VerificationError::InProgress {
        round: 2,
        max_rounds: 5,
    };
    assert_eq!(
        err.to_string(),
        "Plan verification is in progress (round 2/5)"
    );
}

#[test]
fn verification_error_display_has_unresolved_gaps() {
    let err = VerificationError::HasUnresolvedGaps { count: 3 };
    assert_eq!(err.to_string(), "Plan has 3 unresolved gaps");
}

#[test]
fn verification_error_display_skipped_cannot_update() {
    let err = VerificationError::SkippedCannotUpdate;
    assert_eq!(
        err.to_string(),
        "Verification was skipped — cannot update from critic"
    );
}

#[test]
fn verification_error_display_invalid_transition() {
    let err = VerificationError::InvalidTransition {
        from: "unverified".to_string(),
        to: "verified".to_string(),
    };
    assert!(err.to_string().contains("unverified"));
    assert!(err.to_string().contains("verified"));
}

#[test]
fn verification_error_display_round_exceeds_max() {
    let err = VerificationError::RoundExceedsMax { round: 6, max: 5 };
    assert_eq!(err.to_string(), "Round 6 exceeds max_rounds (5)");
}

#[test]
fn verification_error_display_agent_crashed() {
    let err = VerificationError::AgentCrashed { round: 3 };
    assert_eq!(err.to_string(), "Verification agent crashed during round 3");
}
