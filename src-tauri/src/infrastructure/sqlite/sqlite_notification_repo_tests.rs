use chrono::{Duration, Utc};

use super::SqliteNotificationRepository;
use crate::domain::entities::{
    ChatConversation, NewNotification, NotificationCategory, NotificationSeverity,
    NotificationTarget, NotificationTargetKind,
};
use crate::domain::repositories::NotificationRepository;
use crate::testing::SqliteTestDb;

fn notification(
    key: &str,
    created_at: chrono::DateTime<Utc>,
) -> crate::domain::entities::Notification {
    NewNotification {
        project_id: Some("project-a".into()),
        category: NotificationCategory::TaskFailed,
        severity: NotificationSeverity::Warning,
        title: key.into(),
        body: None,
        target: NotificationTarget::none(),
        dedupe_key: Some(key.into()),
    }
    .into_notification(created_at)
}

#[tokio::test]
async fn sqlite_notification_repo_dedupes_and_prunes_with_shared_fixture() {
    let db = SqliteTestDb::new("sqlite-notification-repo");
    let repo = SqliteNotificationRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    let old = notification("old", now - Duration::days(40));
    assert!(repo.create_with_dedupe(old.clone()).await.unwrap());
    assert!(!repo.create_with_dedupe(old.clone()).await.unwrap());
    assert!(repo
        .mark_read(&old.id, now - Duration::days(35))
        .await
        .unwrap()
        .is_some());
    let newest = notification("newest", now);
    assert!(repo.create_with_dedupe(newest.clone()).await.unwrap());
    repo.prune(now - Duration::days(30), 1).await.unwrap();
    let page = repo.list(None, None, 50).await.unwrap();
    assert_eq!(page.notifications.len(), 1);
    assert_eq!(page.notifications[0].id, newest.id);
}

#[tokio::test]
async fn sqlite_notification_repo_excludes_archived_conversation_targets_from_history_and_reads() {
    let db = SqliteTestDb::new("sqlite-notification-archive-filter");
    let repo = SqliteNotificationRepository::from_shared(db.shared_conn());
    let project = db.seed_project("Notification archive filter");
    let now = Utc::now();

    let active_conversation =
        db.insert_conversation(ChatConversation::new_project(project.id.clone()));
    let mut archived_conversation = ChatConversation::new_project(project.id.clone());
    archived_conversation.archived_at = Some(now);
    let archived_conversation = db.insert_conversation(archived_conversation);
    let workspace_archived_conversation =
        db.insert_conversation(ChatConversation::new_project(project.id.clone()));
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO agent_conversation_workspaces (
                conversation_id, project_id, mode, base_ref_kind, base_ref, branch_name,
                worktree_path, status, created_at, updated_at
             ) VALUES (?1, ?2, 'edit', 'project_default', 'main', 'archive-filter',
                '/tmp/archive-filter', 'archived', ?3, ?3)",
            rusqlite::params![
                workspace_archived_conversation.id.as_str(),
                project.id.as_str(),
                now.to_rfc3339(),
            ],
        )
        .unwrap();
    });

    let cases = [
        (
            "active conversation",
            NotificationTarget {
                kind: NotificationTargetKind::AgentConversation,
                project_id: Some(project.id.to_string()),
                task_id: None,
                conversation_id: Some(active_conversation.id.to_string()),
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
        ),
        (
            "archived chat conversation",
            NotificationTarget {
                kind: NotificationTargetKind::AgentConversation,
                project_id: Some(project.id.to_string()),
                task_id: None,
                conversation_id: Some(archived_conversation.id.to_string()),
                setup_conversation_id: None,
                automation_id: None,
                run_id: None,
            },
        ),
        (
            "archived workspace setup conversation",
            NotificationTarget {
                kind: NotificationTargetKind::AutomationRun,
                project_id: Some(project.id.to_string()),
                task_id: None,
                conversation_id: None,
                setup_conversation_id: Some(workspace_archived_conversation.id.to_string()),
                automation_id: Some("automation-1".to_string()),
                run_id: Some("run-1".to_string()),
            },
        ),
        ("no conversation", NotificationTarget::none()),
    ];
    for (title, target) in cases {
        let row = NewNotification {
            project_id: Some(project.id.to_string()),
            category: NotificationCategory::TaskFailed,
            severity: NotificationSeverity::Warning,
            title: title.to_string(),
            body: None,
            target,
            dedupe_key: Some(title.to_string()),
        }
        .into_notification(now);
        assert!(repo.create_with_dedupe(row).await.unwrap());
    }

    let page = repo.list(None, None, 50).await.unwrap();
    let mut visible_titles: Vec<_> = page
        .notifications
        .iter()
        .map(|notification| notification.title.as_str())
        .collect();
    visible_titles.sort_unstable();
    assert_eq!(visible_titles, ["active conversation", "no conversation"]);
    assert_eq!(repo.unread_count(None).await.unwrap(), 2);

    assert_eq!(repo.mark_all_read(None, now).await.unwrap(), 2);
    assert_eq!(repo.unread_count(None).await.unwrap(), 0);
    let hidden_unread_count: i64 = db.with_connection(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM notifications WHERE read_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap()
    });
    assert_eq!(hidden_unread_count, 2);
}
