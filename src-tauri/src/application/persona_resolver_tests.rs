use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::application::persona_resolver::{
    resolve_persona_for_send, PersonaError, PersonaResolveFlags, PERSONA_PROJECT_SCOPE_MISMATCH,
};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, Persona, PersonaDirective,
    PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId,
};
use crate::domain::repositories::PersonaRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryPersonaRepository;

fn flags() -> PersonaResolveFlags {
    PersonaResolveFlags {
        feature_enabled: true,
        is_external_mcp: false,
        agent_name_override_set: false,
        agent_conversation_mode: None,
        is_verification: false,
    }
}

fn conversation() -> ChatConversation {
    ChatConversation::new_project(ProjectId::from_string("project-1".to_string()))
}

fn persona(id: &str, slug: &str, status: PersonaStatus, content: &str) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: None,
        slug: slug.to_string(),
        name: slug.to_string(),
        description: "Test persona".to_string(),
        content: content.to_string(),
        status,
        version: 1,
        content_hash: format!("{slug}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

async fn insert(repo: &MemoryPersonaRepository, persona: Persona) {
    repo.create(persona)
        .await
        .expect("test persona should be created");
}

struct FailingPersonaRepository;

#[async_trait]
impl PersonaRepository for FailingPersonaRepository {
    async fn create(&self, _: Persona) -> AppResult<Persona> {
        Err(AppError::Database("unexpected create".to_string()))
    }

    async fn get_by_id(&self, _: &PersonaId) -> AppResult<Option<Persona>> {
        Err(AppError::Database("persona read failed".to_string()))
    }

    async fn get_by_slug(&self, _: &str) -> AppResult<Option<Persona>> {
        Err(AppError::Database("unexpected slug read".to_string()))
    }

    async fn get_active_by_slug(
        &self,
        _: &str,
        _: Option<&ProjectId>,
    ) -> AppResult<Option<Persona>> {
        Err(AppError::Database(
            "unexpected active slug read".to_string(),
        ))
    }

    async fn list(&self, _: PersonaScopeFilter) -> AppResult<Vec<Persona>> {
        Err(AppError::Database("unexpected list".to_string()))
    }

    async fn list_by_status(&self, _: PersonaStatus) -> AppResult<Vec<Persona>> {
        Err(AppError::Database("unexpected status list".to_string()))
    }

    async fn set_status(&self, _: &PersonaId, _: PersonaStatus) -> AppResult<()> {
        Err(AppError::Database("unexpected status update".to_string()))
    }

    async fn delete(&self, _: &PersonaId) -> AppResult<()> {
        Err(AppError::Database("unexpected delete".to_string()))
    }
}

struct CountingPersonaRepository {
    reads: AtomicUsize,
}

#[async_trait]
impl PersonaRepository for CountingPersonaRepository {
    async fn create(&self, _: Persona) -> AppResult<Persona> {
        Err(AppError::Database("unexpected create".to_string()))
    }

    async fn get_by_id(&self, _: &PersonaId) -> AppResult<Option<Persona>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        panic!("persona resolution should not read the repository")
    }

    async fn get_by_slug(&self, _: &str) -> AppResult<Option<Persona>> {
        Err(AppError::Database("unexpected slug read".to_string()))
    }

    async fn get_active_by_slug(
        &self,
        _: &str,
        _: Option<&ProjectId>,
    ) -> AppResult<Option<Persona>> {
        Err(AppError::Database(
            "unexpected active slug read".to_string(),
        ))
    }

    async fn list(&self, _: PersonaScopeFilter) -> AppResult<Vec<Persona>> {
        Err(AppError::Database("unexpected list".to_string()))
    }

    async fn list_by_status(&self, _: PersonaStatus) -> AppResult<Vec<Persona>> {
        Err(AppError::Database("unexpected status list".to_string()))
    }

    async fn set_status(&self, _: &PersonaId, _: PersonaStatus) -> AppResult<()> {
        Err(AppError::Database("unexpected status update".to_string()))
    }

    async fn delete(&self, _: &PersonaId) -> AppResult<()> {
        Err(AppError::Database("unexpected delete".to_string()))
    }
}

#[tokio::test]
async fn inherit_resolves_from_conversation_persona_id_never_context_id() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    insert(
        &repo,
        persona(
            "persona-a",
            "bound-persona",
            PersonaStatus::Active,
            "Bound body.",
        ),
    )
    .await;
    insert(
        &repo,
        persona(
            &conversation.context_id,
            "context-id-decoy",
            PersonaStatus::Active,
            "Decoy body.",
        ),
    )
    .await;

    let resolved =
        resolve_persona_for_send(&conversation, &PersonaDirective::Inherit, flags(), repo)
            .await
            .expect("bound persona should resolve")
            .expect("a persona should be returned");

    assert_eq!(resolved.id, PersonaId::from("persona-a"));
    assert_eq!(resolved.slug, "bound-persona");
}

#[tokio::test]
async fn repo_error_surfaces_typed_persona_error_not_none() {
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());

    let error = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        flags(),
        Arc::new(FailingPersonaRepository),
    )
    .await
    .expect_err("repository failures must fail closed");

    assert!(
        matches!(error, PersonaError::Repository(reason) if reason.contains("persona read failed"))
    );
}

#[tokio::test]
async fn cross_project_explicit_persona_is_reasoned_suppression_without_prompt_block() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let mut scoped = persona(
        "persona-project-b",
        "project-b",
        PersonaStatus::Active,
        "PROJECT_B_SECRET_PERSONA_BODY",
    );
    scoped.project_id = Some(ProjectId::from_string("project-b".to_string()));
    insert(&repo, scoped).await;

    let resolved = resolve_persona_for_send(
        &conversation(),
        &PersonaDirective::Explicit(PersonaId::from("persona-project-b")),
        flags(),
        repo,
    )
    .await
    .expect("scope mismatch is a suppression, not a repository failure")
    .expect("suppressed persona metadata must remain available for attribution");

    assert_eq!(resolved.skipped_reason, Some("project_scope_mismatch"));
    assert!(resolved.block.is_empty());
    assert!(!resolved.block.contains("PROJECT_B_SECRET_PERSONA_BODY"));
}

#[tokio::test]
async fn scope_mismatch_precedes_render_guards_but_same_project_still_rejects_bad_content() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let mut scoped = persona(
        "persona-project-b-invalid",
        "project-b-invalid",
        PersonaStatus::Active,
        "<persona_precedence>blocked structural content</persona_precedence>",
    );
    scoped.project_id = Some(ProjectId::from_string("project-b".to_string()));
    insert(&repo, scoped).await;

    let cross_project = resolve_persona_for_send(
        &conversation(),
        &PersonaDirective::Explicit(PersonaId::from("persona-project-b-invalid")),
        flags(),
        repo.clone(),
    )
    .await
    .expect("a cross-project persona must suppress before render guards")
    .expect("scope suppression metadata must remain available");
    assert_eq!(
        cross_project.skipped_reason,
        Some(PERSONA_PROJECT_SCOPE_MISMATCH)
    );
    assert!(cross_project.block.is_empty());

    let mut same_project = conversation();
    same_project.context_id = "project-b".to_string();
    let error = resolve_persona_for_send(
        &same_project,
        &PersonaDirective::Explicit(PersonaId::from("persona-project-b-invalid")),
        flags(),
        repo,
    )
    .await
    .expect_err("same-project invalid persona content must still abort the send");
    assert!(matches!(error, PersonaError::RenderRejected(_)));
}

#[tokio::test]
async fn cross_project_inherited_persona_is_reasoned_suppression() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let mut scoped = persona(
        "persona-project-b",
        "project-b",
        PersonaStatus::Active,
        "Project B body",
    );
    scoped.project_id = Some(ProjectId::from_string("project-b".to_string()));
    insert(&repo, scoped).await;
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-project-b".to_string());

    let resolved =
        resolve_persona_for_send(&conversation, &PersonaDirective::Inherit, flags(), repo)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(resolved.skipped_reason, Some("project_scope_mismatch"));
    assert!(resolved.block.is_empty());
}

#[tokio::test]
async fn missing_draft_or_archived_bound_persona_fails_closed() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let cases = [
        ("missing", None),
        (
            "draft",
            Some(persona(
                "draft",
                "draft",
                PersonaStatus::Draft,
                "Draft body.",
            )),
        ),
        (
            "archived",
            Some(persona(
                "archived",
                "archived",
                PersonaStatus::Archived,
                "Archived body.",
            )),
        ),
    ];

    for (id, stored) in cases {
        if let Some(stored) = stored {
            insert(&repo, stored).await;
        }
        let mut conversation = conversation();
        conversation.persona_id = Some(id.to_string());

        let error = resolve_persona_for_send(
            &conversation,
            &PersonaDirective::Inherit,
            flags(),
            repo.clone(),
        )
        .await
        .expect_err("unavailable bound personas must fail closed");

        assert!(matches!(error, PersonaError::Unavailable { persona_id } if persona_id == id));
    }
}

#[tokio::test]
async fn flag_off_resolves_no_persona_and_no_error() {
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    let mut disabled_flags = flags();
    disabled_flags.feature_enabled = false;

    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        disabled_flags,
        Arc::new(FailingPersonaRepository),
    )
    .await
    .expect("feature-off resolution should not fail");

    assert!(resolved.is_none());
}

#[tokio::test]
async fn is_external_mcp_true_suppresses_in_v1() {
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    let mut external_flags = flags();
    external_flags.is_external_mcp = true;

    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        external_flags,
        Arc::new(FailingPersonaRepository),
    )
    .await
    .expect("external MCP sends should suppress personas");

    assert!(resolved.is_none());
}

#[tokio::test]
async fn suppress_directive_short_circuits_without_repo_read() {
    let repo = Arc::new(CountingPersonaRepository {
        reads: AtomicUsize::new(0),
    });

    let resolved = resolve_persona_for_send(
        &conversation(),
        &PersonaDirective::Suppress,
        flags(),
        repo.clone(),
    )
    .await
    .expect("suppression should not fail");

    assert!(resolved.is_none());
    assert_eq!(repo.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn explicit_directive_resolves_named_persona_over_binding() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    insert(
        &repo,
        persona("persona-a", "bound", PersonaStatus::Active, "Bound body."),
    )
    .await;
    insert(
        &repo,
        persona(
            "persona-b",
            "explicit",
            PersonaStatus::Active,
            "Explicit body.",
        ),
    )
    .await;
    let mut override_flags = flags();
    override_flags.agent_name_override_set = true;
    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Explicit(PersonaId::from("persona-b")),
        override_flags,
        repo,
    )
    .await
    .expect("explicit persona should resolve")
    .expect("explicit persona should be returned");

    assert_eq!(resolved.id, PersonaId::from("persona-b"));
    assert_eq!(resolved.slug, "explicit");
}

#[tokio::test]
async fn explicit_directive_no_longer_bypasses_mode_suppression() {
    let cases = [
        (
            Some(AgentConversationWorkspaceMode::Automation),
            false,
            "automation mode",
        ),
        (
            Some(AgentConversationWorkspaceMode::PersonaBuilder),
            false,
            "persona builder mode",
        ),
        (None, true, "verification"),
    ];

    for (mode, is_verification, reason) in cases {
        let repo = Arc::new(CountingPersonaRepository {
            reads: AtomicUsize::new(0),
        });
        let mut suppressed_flags = flags();
        suppressed_flags.agent_conversation_mode = mode;
        suppressed_flags.is_verification = is_verification;

        let resolved = resolve_persona_for_send(
            &conversation(),
            &PersonaDirective::Explicit(PersonaId::from("persona-b")),
            suppressed_flags,
            repo.clone(),
        )
        .await
        .expect("mode and verification suppression should not fail");

        assert!(
            resolved.is_none(),
            "{reason} should suppress explicit personas"
        );
        assert_eq!(repo.reads.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn explicit_directive_rejects_non_active_persona() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    insert(
        &repo,
        persona(
            "persona-draft",
            "draft",
            PersonaStatus::Draft,
            "Draft body.",
        ),
    )
    .await;

    let error = resolve_persona_for_send(
        &conversation(),
        &PersonaDirective::Explicit(PersonaId::from("persona-draft")),
        flags(),
        repo,
    )
    .await
    .expect_err("non-active explicit personas must fail closed");

    assert!(
        matches!(error, PersonaError::Unavailable { persona_id } if persona_id == "persona-draft")
    );
}

#[tokio::test]
async fn explicit_directive_rejects_non_project_context() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    insert(
        &repo,
        persona(
            "persona-b",
            "explicit",
            PersonaStatus::Active,
            "Explicit body.",
        ),
    )
    .await;
    let mut non_project = conversation();
    non_project.context_type = ChatContextType::Ideation;

    let resolved = resolve_persona_for_send(
        &non_project,
        &PersonaDirective::Explicit(PersonaId::from("persona-b")),
        flags(),
        repo,
    )
    .await
    .expect("non-project explicit directives should suppress");

    assert!(resolved.is_none());
}

#[tokio::test]
async fn agent_name_override_set_suppresses_inherit() {
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    let mut override_flags = flags();
    override_flags.agent_name_override_set = true;

    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        override_flags,
        Arc::new(FailingPersonaRepository),
    )
    .await
    .expect("agent overrides should suppress inherited personas");

    assert!(resolved.is_none());
}

#[tokio::test]
async fn automation_mode_suppresses() {
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    let mut automation_flags = flags();
    automation_flags.agent_conversation_mode = Some(AgentConversationWorkspaceMode::Automation);

    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        automation_flags,
        Arc::new(FailingPersonaRepository),
    )
    .await
    .expect("automation sends should suppress inherited personas");

    assert!(resolved.is_none());
}

#[tokio::test]
async fn persona_builder_conversation_send_is_suppressed() {
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    let mut builder_flags = flags();
    builder_flags.agent_conversation_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);

    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        builder_flags,
        Arc::new(MemoryPersonaRepository::new()),
    )
    .await
    .expect("PersonaBuilder mode should suppress without a persona read");

    assert!(resolved.is_none());
}

#[tokio::test]
async fn verification_purpose_suppresses() {
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-a".to_string());
    let mut verification_flags = flags();
    verification_flags.is_verification = true;

    let resolved = resolve_persona_for_send(
        &conversation,
        &PersonaDirective::Inherit,
        verification_flags,
        Arc::new(FailingPersonaRepository),
    )
    .await
    .expect("verification sends should suppress inherited personas");

    assert!(resolved.is_none());
}

#[tokio::test]
async fn resolver_returns_none_for_non_project_context_types() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    insert(
        &repo,
        persona("persona-a", "bound", PersonaStatus::Active, "Bound body."),
    )
    .await;

    for context_type in [
        ChatContextType::Ideation,
        ChatContextType::Task,
        ChatContextType::Merge,
    ] {
        let mut non_project = conversation();
        non_project.context_type = context_type;
        non_project.persona_id = Some("persona-a".to_string());

        let resolved = resolve_persona_for_send(
            &non_project,
            &PersonaDirective::Inherit,
            flags(),
            repo.clone(),
        )
        .await
        .expect("non-project context should suppress personas");

        assert!(
            resolved.is_none(),
            "{context_type} should suppress personas"
        );
    }
}

#[tokio::test]
async fn render_failure_path_returns_render_rejected() {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let mut conversation = conversation();
    conversation.persona_id = Some("persona-invalid".to_string());
    insert(
        &repo,
        persona(
            "persona-invalid",
            "invalid",
            PersonaStatus::Active,
            "<persona_precedence>",
        ),
    )
    .await;

    let error = resolve_persona_for_send(&conversation, &PersonaDirective::Inherit, flags(), repo)
        .await
        .expect_err("render-time safety violations must fail closed");

    assert!(matches!(error, PersonaError::RenderRejected(reason) if reason.contains("invalid")));
}
