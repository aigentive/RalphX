use crate::domain::entities::{ProjectId, ProjectSkillSettings};
use crate::domain::repositories::ProjectSkillSettingsRepository;
use crate::infrastructure::sqlite::SqliteProjectSkillSettingsRepository;
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn sqlite_project_skill_settings_upserts_export_opt_in() {
    let db = SqliteTestDb::new("sqlite_project_skill_settings_upsert");
    let project = db.seed_project("Skill Settings");
    let project_id = project.id;
    let repo = SqliteProjectSkillSettingsRepository::from_shared(db.shared_conn());

    assert!(repo.get_for_project(&project_id).await.unwrap().is_none());

    let saved = repo
        .upsert(ProjectSkillSettings {
            project_id: project_id.clone(),
            export_enabled: true,
        })
        .await
        .unwrap();
    assert!(saved.export_enabled);

    let loaded = repo
        .get_for_project(&project_id)
        .await
        .unwrap()
        .expect("settings row");
    assert!(loaded.export_enabled);

    repo.upsert(ProjectSkillSettings {
        project_id: project_id.clone(),
        export_enabled: false,
    })
    .await
    .unwrap();
    let loaded = repo.get_for_project(&project_id).await.unwrap().unwrap();
    assert!(!loaded.export_enabled);
}

#[tokio::test]
async fn project_skill_settings_defaults_to_export_disabled() {
    let settings = ProjectSkillSettings::default_for_project(ProjectId::from_string(
        "project-default".to_string(),
    ));

    assert!(!settings.export_enabled);
}
