use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Manager;

use crate::application::chat_service::ClaudeChatService;
use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    ChatConversationId, ProjectId, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus,
    SkillUsageEvent, SkillUsageInjectionKind,
};
use crate::domain::repositories::{SkillUsageEventRepository, SkillUsageListOptions};
use crate::infrastructure::memory::MemorySkillUsageEventRepository;

struct LaunchUsageFixture {
    app: tauri::App<MockRuntime>,
    usage_repo: Arc<MemorySkillUsageEventRepository>,
}

impl LaunchUsageFixture {
    fn new() -> Self {
        let mut app_state = AppState::new_test();
        let usage_repo = Arc::new(MemorySkillUsageEventRepository::new());
        app_state.skill_usage_event_repo = Arc::clone(&usage_repo) as _;
        let app = mock_builder()
            .manage(app_state)
            .build(mock_context(noop_assets()))
            .expect("mock app");
        Self { app, usage_repo }
    }

    fn service(&self) -> ClaudeChatService<MockRuntime> {
        let handle = self.app.handle().clone();
        let state = handle.state::<AppState>();
        ClaudeChatService::<MockRuntime>::new(
            Arc::clone(&state.chat_message_repo),
            Arc::clone(&state.chat_attachment_repo),
            Arc::clone(&state.artifact_repo),
            Arc::clone(&state.chat_conversation_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.project_repo),
            Arc::clone(&state.task_repo),
            Arc::clone(&state.task_dependency_repo),
            Arc::clone(&state.ideation_session_repo),
            Arc::clone(&state.delegated_session_repo),
            Arc::clone(&state.activity_event_repo),
            Arc::clone(&state.message_queue),
            Arc::clone(&state.running_agent_registry),
            Arc::clone(&state.memory_event_repo),
            Arc::clone(&state.project_memory_settings_repo),
        )
        .with_app_handle(handle.clone())
    }

    async fn seed_skill(&self, project_id: &ProjectId) -> ProjectSkill {
        let handle = self.app.handle().clone();
        let state = handle.state::<AppState>();
        let skill = build_skill(project_id);
        state
            .project_skill_repo
            .create(skill.clone())
            .await
            .expect("seed project skill");
        skill
    }

    async fn usage(&self, project_id: &ProjectId) -> Vec<SkillUsageEvent> {
        self.usage_repo
            .list_by_project(project_id, SkillUsageListOptions::default())
            .await
            .expect("list usage")
    }
}

fn build_skill(project_id: &ProjectId) -> ProjectSkill {
    let now = chrono::Utc::now();
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id: project_id.clone(),
        title: "Use planning constraints".to_string(),
        bucket: "planning".to_string(),
        stage: "planning".to_string(),
        status: ProjectSkillLifecycleStatus::Approved,
        pinned: false,
        archived: false,
        scope_paths: Vec::new(),
        compact_guidance: "Carry approved planning constraints into the next turn.".to_string(),
        body_markdown: "Detailed guidance".to_string(),
        predicted_effect: Some("Avoids dropping accepted planning constraints.".to_string()),
        provenance_json: serde_json::json!({ "test": true }),
        companion_of_skill_id: None,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
    }
}

fn kinds_of(events: &[SkillUsageEvent]) -> Vec<SkillUsageInjectionKind> {
    let mut kinds: Vec<_> = events.iter().map(|event| event.injection_kind).collect();
    kinds.sort_by_key(|kind| format!("{kind:?}"));
    kinds
}

fn event_of<'a>(
    events: &'a [SkillUsageEvent],
    kind: SkillUsageInjectionKind,
) -> &'a SkillUsageEvent {
    events
        .iter()
        .find(|event| event.injection_kind == kind)
        .unwrap_or_else(|| panic!("expected a {kind:?} usage event"))
}

#[tokio::test]
async fn test_launch_usage_records_compact_index_and_composer_directive_rows() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let skill = fixture.seed_skill(&project_id).await;
    let injected_name = format!("learned:{}", skill.id.as_str());

    service
        .record_learned_skill_usage_for_launch(
            Some(project_id.as_str()),
            &conversation_id,
            "run-1",
            AgentHarnessKind::Claude,
            &[injected_name.clone(), "internal-skill-name".to_string()],
            &[skill.clone()],
        )
        .await;

    let usage = fixture.usage(&project_id).await;
    assert_eq!(
        usage.len(),
        2,
        "one compact-index and one composer-directive row; the internal name records nothing"
    );
    assert_eq!(
        kinds_of(&usage),
        vec![
            SkillUsageInjectionKind::CompactIndex,
            SkillUsageInjectionKind::ComposerDirective,
        ]
    );

    for event in &usage {
        assert_eq!(event.project_skill_id, skill.id);
        assert_eq!(
            event.conversation_id.as_deref(),
            Some(conversation_id.as_str().as_str())
        );
        assert_eq!(event.agent_run_id.as_deref(), Some("run-1"));
        assert_eq!(
            event.provider_harness.as_deref(),
            Some(AgentHarnessKind::Claude.to_string().as_str())
        );
        assert_eq!(event.stage.as_deref(), Some(skill.stage.as_str()));
        assert_eq!(event.bucket.as_deref(), Some(skill.bucket.as_str()));
        assert_eq!(event.outcome_id, None);
        assert_eq!(event.metadata_json["scoring_eligible"], true);
    }
}

#[tokio::test]
async fn test_launch_usage_records_metadata_provenance_per_kind() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let skill = fixture.seed_skill(&project_id).await;
    let injected_name = format!("learned:{}", skill.id.as_str());

    service
        .record_learned_skill_usage_for_launch(
            Some(project_id.as_str()),
            &conversation_id,
            "run-1",
            AgentHarnessKind::Claude,
            &[injected_name.clone()],
            &[skill.clone()],
        )
        .await;

    let usage = fixture.usage(&project_id).await;
    let compact = event_of(&usage, SkillUsageInjectionKind::CompactIndex);
    assert_eq!(
        compact.metadata_json["source"],
        "pre_execution_project_skill_injection"
    );
    assert_eq!(compact.metadata_json["injected_skill_name"], injected_name);

    let composer = event_of(&usage, SkillUsageInjectionKind::ComposerDirective);
    assert_eq!(
        composer.metadata_json["source"],
        "ralphx_project_skill_directive"
    );
}

#[tokio::test]
async fn test_launch_usage_same_run_retry_is_idempotent_and_distinct_runs_are_not_collapsed() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let skill = fixture.seed_skill(&project_id).await;
    let injected_name = format!("learned:{}", skill.id.as_str());

    for _ in 0..2 {
        service
            .record_learned_skill_usage_for_launch(
                Some(project_id.as_str()),
                &conversation_id,
                "run-1",
                AgentHarnessKind::Claude,
                &[injected_name.clone()],
                &[skill.clone()],
            )
            .await;
    }
    assert_eq!(
        fixture.usage(&project_id).await.len(),
        2,
        "retrying the same run must not duplicate rows"
    );

    service
        .record_learned_skill_usage_for_launch(
            Some(project_id.as_str()),
            &conversation_id,
            "run-2",
            AgentHarnessKind::Claude,
            &[injected_name],
            &[skill],
        )
        .await;
    assert_eq!(
        fixture.usage(&project_id).await.len(),
        4,
        "a distinct run must record its own rows"
    );
}

#[tokio::test]
async fn test_launch_usage_unresolved_injected_skill_suppresses_whole_batch() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let skill = fixture.seed_skill(&project_id).await;

    service
        .record_learned_skill_usage_for_launch(
            Some(project_id.as_str()),
            &conversation_id,
            "run-1",
            AgentHarnessKind::Claude,
            &["learned:does-not-exist".to_string()],
            &[skill],
        )
        .await;

    assert!(
        fixture.usage(&project_id).await.is_empty(),
        "an unresolved injected skill suppresses the entire launch batch"
    );
}

#[tokio::test]
async fn test_launch_usage_suppresses_cross_project_injected_skill() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let other_project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let foreign_skill = fixture.seed_skill(&other_project_id).await;
    let injected_name = format!("learned:{}", foreign_skill.id.as_str());

    service
        .record_learned_skill_usage_for_launch(
            Some(project_id.as_str()),
            &conversation_id,
            "run-1",
            AgentHarnessKind::Claude,
            &[injected_name],
            &[],
        )
        .await;

    assert!(fixture.usage(&project_id).await.is_empty());
    assert!(fixture.usage(&other_project_id).await.is_empty());
}

#[tokio::test]
async fn test_launch_usage_suppresses_cross_project_selected_skill() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let other_project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let local_skill = fixture.seed_skill(&project_id).await;
    let foreign_skill = fixture.seed_skill(&other_project_id).await;
    let injected_name = format!("learned:{}", local_skill.id.as_str());

    service
        .record_learned_skill_usage_for_launch(
            Some(project_id.as_str()),
            &conversation_id,
            "run-1",
            AgentHarnessKind::Claude,
            &[injected_name],
            &[foreign_skill],
        )
        .await;

    assert!(fixture.usage(&project_id).await.is_empty());
    assert!(fixture.usage(&other_project_id).await.is_empty());
}

#[tokio::test]
async fn test_launch_usage_requires_project_scope() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let skill = fixture.seed_skill(&project_id).await;
    let injected_name = format!("learned:{}", skill.id.as_str());

    for scope in [None, Some("  ")] {
        service
            .record_learned_skill_usage_for_launch(
                scope,
                &conversation_id,
                "run-1",
                AgentHarnessKind::Claude,
                &[injected_name.clone()],
                &[skill.clone()],
            )
            .await;
        assert!(
            fixture.usage(&project_id).await.is_empty(),
            "a missing or blank project scope must record nothing"
        );
    }
}

#[tokio::test]
async fn test_launch_usage_persistence_failure_is_contained() {
    let fixture = LaunchUsageFixture::new();
    let service = fixture.service();
    let project_id = ProjectId::new();
    let conversation_id = ChatConversationId::new();
    let skill = fixture.seed_skill(&project_id).await;
    let injected_name = format!("learned:{}", skill.id.as_str());

    fixture.usage_repo.fail_next_batch_for_test();
    service
        .record_learned_skill_usage_for_launch(
            Some(project_id.as_str()),
            &conversation_id,
            "run-1",
            AgentHarnessKind::Claude,
            &[injected_name],
            &[skill],
        )
        .await;

    assert!(
        fixture.usage(&project_id).await.is_empty(),
        "an atomic telemetry failure leaves zero rows and is not an execution gate"
    );
}
