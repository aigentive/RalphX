use super::*;
use crate::domain::entities::Project;
use crate::domain::entities::{ArchiveJobPayload, ArchiveJobType, MemoryArchiveJob};
use crate::domain::entities::{MemoryBucket, MemoryEntry};
use crate::domain::repositories::{MemoryArchiveRepository, ProjectRepository};
use crate::infrastructure::sqlite::connection::open_memory_connection;
use crate::testing::SqliteTestDb;
use async_trait::async_trait;

// Mock project repository for testing
struct MockProjectRepository {
    project: Option<Project>,
}

#[async_trait]
impl ProjectRepository for MockProjectRepository {
    async fn create(&self, project: Project) -> crate::error::AppResult<Project> {
        Ok(project)
    }

    async fn get_by_id(&self, id: &ProjectId) -> crate::error::AppResult<Option<Project>> {
        Ok(self.project.clone().filter(|project| &project.id == id))
    }

    async fn get_all(&self) -> crate::error::AppResult<Vec<Project>> {
        Ok(vec![])
    }

    async fn update(&self, _project: &Project) -> crate::error::AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &ProjectId) -> crate::error::AppResult<()> {
        Ok(())
    }

    async fn get_by_working_directory(
        &self,
        _path: &str,
    ) -> crate::error::AppResult<Option<Project>> {
        Ok(None)
    }

    async fn archive(&self, _id: &ProjectId) -> crate::error::AppResult<Project> {
        unimplemented!()
    }
}

fn create_format_test_service() -> MemoryArchiveService {
    let archive_conn = open_memory_connection().unwrap();
    let entry_conn = open_memory_connection().unwrap();

    MemoryArchiveService::new_with_archive_root(
        Arc::new(crate::infrastructure::sqlite::SqliteMemoryArchiveRepository::new(archive_conn)),
        Arc::new(crate::infrastructure::sqlite::SqliteMemoryEntryRepository::new(entry_conn)),
        Arc::new(MockProjectRepository { project: None }),
        PathBuf::from("/ralphx-app-data/artifacts/memory-archive"),
    )
}

#[test]
fn test_format_memory_snapshot() {
    let project_id = ProjectId::from_string("test-project".to_string());
    let entry = MemoryEntry::new(
        project_id,
        MemoryBucket::ImplementationDiscoveries,
        "Test Memory".to_string(),
        "Brief summary".to_string(),
        "# Full Details\n\nMore info here".to_string(),
        vec!["src/**/*.rs".to_string()],
        "hash123".to_string(),
    );

    // These formatting tests do not hit the DB, so lightweight in-memory repos are sufficient.
    let service = create_format_test_service();

    let snapshot = service.format_memory_snapshot(&entry).unwrap();

    assert!(snapshot.contains("memory_id:"));
    assert!(snapshot.contains("# Test Memory"));
    assert!(snapshot.contains("**Summary:** Brief summary"));
    assert!(snapshot.contains("# Full Details"));
}

#[test]
fn test_format_memory_snapshot_deterministic() {
    // Test that formatting is deterministic
    let project_id = ProjectId::from_string("test-project".to_string());
    let entry = MemoryEntry::new(
        project_id,
        MemoryBucket::ArchitecturePatterns,
        "Architecture Test".to_string(),
        "Summary text".to_string(),
        "Details content".to_string(),
        vec!["src/**/*.rs".to_string(), "tests/**/*.rs".to_string()],
        "hash456".to_string(),
    );

    let service = create_format_test_service();

    let snapshot1 = service.format_memory_snapshot(&entry).unwrap();
    let snapshot2 = service.format_memory_snapshot(&entry).unwrap();

    // Should produce identical output
    assert_eq!(snapshot1, snapshot2);

    // Verify scope_paths are included and sorted
    assert!(snapshot1.contains("scope_paths:"));
    assert!(snapshot1.contains("src/**/*.rs"));
    assert!(snapshot1.contains("tests/**/*.rs"));
}

#[test]
fn test_format_project_snapshot_deterministic() {
    // Test that project snapshots are deterministic and group by bucket
    let project_id = ProjectId::from_string("test-project".to_string());

    let entry1 = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::ImplementationDiscoveries,
        "Discovery 1".to_string(),
        "Summary 1".to_string(),
        "Details 1".to_string(),
        vec!["src/**/*.rs".to_string()],
        "hash1".to_string(),
    );

    let entry2 = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::ArchitecturePatterns,
        "Pattern 1".to_string(),
        "Summary 2".to_string(),
        "Details 2".to_string(),
        vec!["src/**/*.rs".to_string()],
        "hash2".to_string(),
    );

    let service = create_format_test_service();

    // Pass entries in different orders, should produce same output
    let entries1 = vec![entry1.clone(), entry2.clone()];
    let entries2 = vec![entry2.clone(), entry1.clone()];

    let snapshot1 = service
        .format_project_snapshot(&project_id, &entries1)
        .unwrap();
    let snapshot2 = service
        .format_project_snapshot(&project_id, &entries2)
        .unwrap();

    // Strip snapshot_date lines since Utc::now() differs between calls
    // We're testing content ordering determinism, not timestamp equality
    let strip_date = |s: &str| -> String {
        s.lines()
            .filter(|line| !line.starts_with("snapshot_date:"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Should produce identical output regardless of input order (ignoring timestamp)
    assert_eq!(strip_date(&snapshot1), strip_date(&snapshot2));

    // Verify bucket ordering is deterministic
    assert!(snapshot1.contains("architecture_patterns"));
    assert!(snapshot1.contains("implementation_discoveries"));

    // Find positions to verify architecture_patterns comes before implementation_discoveries
    let arch_pos = snapshot1.find("architecture_patterns").unwrap();
    let impl_pos = snapshot1.find("implementation_discoveries").unwrap();
    assert!(arch_pos < impl_pos, "Buckets should be in fixed order");
}

#[test]
fn test_format_rule_snapshot_sorting() {
    // Test that rule snapshots sort entries by ID for determinism
    let project_id = ProjectId::from_string("test-project".to_string());

    let entry1 = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::ImplementationDiscoveries,
        "Memory Z".to_string(),
        "Last alphabetically".to_string(),
        "Details Z".to_string(),
        vec!["src/**/*.rs".to_string()],
        "hashZ".to_string(),
    );

    let entry2 = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::ImplementationDiscoveries,
        "Memory A".to_string(),
        "First alphabetically".to_string(),
        "Details A".to_string(),
        vec!["src/**/*.rs".to_string()],
        "hashA".to_string(),
    );

    let service = create_format_test_service();

    // Pass entries in unsorted order
    let entries = vec![entry1.clone(), entry2.clone()];
    let snapshot = service
        .format_rule_snapshot("test_rule.md", &entries)
        .unwrap();

    // Find positions of entry IDs in snapshot
    let pos1 = snapshot.find(&entry1.id.0).unwrap();
    let pos2 = snapshot.find(&entry2.id.0).unwrap();

    // Verify entries are sorted by ID (lexicographically)
    if entry1.id.0 < entry2.id.0 {
        assert!(pos1 < pos2, "Entries should be sorted by ID");
    } else {
        assert!(pos2 < pos1, "Entries should be sorted by ID");
    }
}

#[test]
fn test_archive_snapshot_paths_use_app_owned_root_and_hashed_components() {
    let service = create_format_test_service();
    let project_id = ProjectId::from_string("../target-project".to_string());

    let memory_path = service
        .get_memory_snapshot_path(&project_id, "../memory/unsafe")
        .unwrap();
    let rule_path = service
        .get_rule_snapshot_path(&project_id, "../rules/unsafe.md", "20260516_120000")
        .unwrap();
    let project_path = service
        .get_project_snapshot_path(&project_id, "20260516_120000")
        .unwrap();

    for path in [&memory_path, &rule_path, &project_path] {
        let rendered = path.to_string_lossy();
        assert!(rendered.starts_with("/ralphx-app-data/artifacts/memory-archive"));
        assert!(!rendered.contains("../target-project"));
        assert!(!rendered.contains("../memory"));
        assert!(!rendered.contains("../rules"));
        assert!(!rendered.contains(".claude/memory-archive"));
    }
}

#[tokio::test]
async fn process_next_job_checks_project_before_writing_snapshots() {
    let db = SqliteTestDb::new("memory-archive-service-project-check");
    let project_id = ProjectId::from_string("missing-project".to_string());
    let mut project = Project::new("Missing Project".to_string(), "/tmp/missing".to_string());
    project.id = project_id.clone();
    db.insert_project(project);

    let archive_repo = Arc::new(
        crate::infrastructure::sqlite::SqliteMemoryArchiveRepository::from_shared(db.shared_conn()),
    );
    let archive_root = tempfile::tempdir().unwrap();

    archive_repo
        .create(MemoryArchiveJob::new(
            project_id.clone(),
            ArchiveJobType::MemorySnapshot,
            ArchiveJobPayload::memory_snapshot("memory-1"),
        ))
        .await
        .unwrap();

    let service = MemoryArchiveService::new_with_archive_root(
        archive_repo,
        Arc::new(
            crate::infrastructure::sqlite::SqliteMemoryEntryRepository::from_shared(
                db.shared_conn(),
            ),
        ),
        Arc::new(MockProjectRepository { project: None }),
        archive_root.path().join("memory-archive"),
    );

    let error = service.process_next_job().await.unwrap_err();

    assert!(error
        .to_string()
        .contains("Project missing-project not found for archive job"));
    assert!(!archive_root.path().join("memory-archive").exists());
}
