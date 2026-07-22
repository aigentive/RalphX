use std::sync::Arc;

use crate::domain::entities::{ProjectId, ProjectSkillSettings, ProjectSkillSettingsPatch};
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
            export_enabled: true,
            ..ProjectSkillSettings::default_for_project(project_id.clone())
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
        export_enabled: false,
        ..loaded
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

#[tokio::test]
async fn sqlite_project_skill_settings_patch_preserves_omitted_and_rejects_invalid() {
    let db = SqliteTestDb::new("sqlite_project_skill_settings_patch");
    let project_id = db.seed_project("Skill Settings Patch").id;
    let repo = SqliteProjectSkillSettingsRepository::from_shared(db.shared_conn());

    let saved = repo
        .patch(
            &project_id,
            ProjectSkillSettingsPatch {
                auto_inject: Some(true),
                injection_max_chars: Some(7_500),
                export_enabled: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(saved.auto_inject);
    assert_eq!(saved.injection_max_chars, 7_500);
    assert!(saved.export_enabled);
    assert_eq!(saved.injection_max_skills, 4);

    let error = repo
        .patch(
            &project_id,
            ProjectSkillSettingsPatch {
                injection_max_skills: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect_err("invalid patch must fail");
    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(
        repo.get_for_project(&project_id).await.unwrap().unwrap(),
        saved
    );
}

#[tokio::test]
async fn sqlite_project_skill_settings_concurrent_disjoint_patches_do_not_lose_updates() {
    let db = SqliteTestDb::new("sqlite_project_skill_settings_concurrent_patch");
    let project_id = db.seed_project("Concurrent Skill Settings").id;
    let repo = Arc::new(SqliteProjectSkillSettingsRepository::from_shared(
        db.shared_conn(),
    ));
    let first_repo = Arc::clone(&repo);
    let first_id = project_id.clone();
    let first = tokio::spawn(async move {
        first_repo
            .patch(
                &first_id,
                ProjectSkillSettingsPatch {
                    auto_inject: Some(true),
                    ..Default::default()
                },
            )
            .await
    });
    let second_repo = Arc::clone(&repo);
    let second_id = project_id.clone();
    let second = tokio::spawn(async move {
        second_repo
            .patch(
                &second_id,
                ProjectSkillSettingsPatch {
                    export_enabled: Some(true),
                    ..Default::default()
                },
            )
            .await
    });
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    let loaded = repo.get_for_project(&project_id).await.unwrap().unwrap();
    assert!(loaded.auto_inject);
    assert!(loaded.export_enabled);
}

#[tokio::test]
async fn sqlite_project_skill_settings_rejects_malformed_persisted_booleans() {
    let db = SqliteTestDb::new("sqlite_project_skill_settings_malformed_boolean");
    let project_id = db.seed_project("Malformed Skill Settings").id;
    let repo = SqliteProjectSkillSettingsRepository::from_shared(db.shared_conn());
    repo.patch(
        &project_id,
        ProjectSkillSettingsPatch {
            enabled: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    db.with_connection(|conn| {
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .unwrap();
        conn.execute(
            "UPDATE project_skill_settings SET enabled = 2 WHERE project_id = ?1",
            [project_id.as_str()],
        )
        .unwrap();
        conn.execute("PRAGMA ignore_check_constraints = OFF", [])
            .unwrap();
    });

    let error = repo
        .get_for_project(&project_id)
        .await
        .expect_err("malformed persisted booleans must fail closed");
    assert!(matches!(error, crate::error::AppError::Database(_)));
}
