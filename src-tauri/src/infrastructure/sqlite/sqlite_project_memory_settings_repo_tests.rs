use crate::domain::repositories::ProjectMemorySettingsRepository;
use crate::infrastructure::sqlite::SqliteProjectMemorySettingsRepository;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn sqlite_project_memory_settings_reads_seeded_defaults() {
    let db = SqliteTestDb::new("sqlite_project_memory_settings_repo_defaults");
    let project = db.seed_project("Memory Defaults");
    let project_id = project.id;
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO project_memory_settings (
                project_id,
                enabled,
                maintenance_categories_json,
                capture_categories_json
            ) VALUES (?1, 1, ?2, ?3)",
            rusqlite::params![
                project_id.as_str(),
                r#"["execution","review","merge"]"#,
                r#"["planning","execution","review"]"#,
            ],
        )
        .expect("insert project memory settings");
    });

    let repo = SqliteProjectMemorySettingsRepository::from_shared(db.shared_conn());
    let settings = repo
        .get_for_project(&project_id)
        .await
        .unwrap()
        .expect("settings row");

    assert!(settings.enabled);
    assert!(settings
        .maintenance_categories
        .contains(&"execution".to_string()));
    assert!(settings
        .capture_categories
        .contains(&"planning".to_string()));
}
