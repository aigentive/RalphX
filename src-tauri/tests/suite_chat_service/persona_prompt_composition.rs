use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use ralphx_lib::application::chat_service::{
    agent_name_for_conversation_mode, resolve_agent_conversation_runtime_profile,
};
use ralphx_lib::application::persona_resolver::{resolve_persona_for_send, PersonaResolveFlags};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, CoordinationMode, Persona, PersonaDirective,
    PersonaId, PersonaStatus, ProjectId,
};
use ralphx_lib::domain::repositories::PersonaRepository;
use ralphx_lib::infrastructure::agents::compose_codex_prompt_for_profile;
use ralphx_lib::infrastructure::memory::MemoryPersonaRepository;

const PERSONA_NAME: &str = "Prompt Composition Persona";
const PERSONA_SLUG: &str = "prompt-composition-persona";
const USER_PROMPT: &str = "Answer the project question.";

fn repo_plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .join("plugins/app")
}

fn runtime_persona_body() -> String {
    format!("runtime-persona-body-{}", uuid::Uuid::new_v4())
}

async fn bound_persona_conversation(
    mode: AgentConversationWorkspaceMode,
) -> (ChatConversation, Arc<MemoryPersonaRepository>, String) {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let now = Utc::now();
    let body = runtime_persona_body();
    let persona = Persona {
        id: PersonaId::from_string(format!("persona-{}", uuid::Uuid::new_v4())),
        artifact_id: None,

        project_id: None,
        slug: PERSONA_SLUG.to_string(),
        name: PERSONA_NAME.to_string(),
        description: "in-memory prompt composition fixture".to_string(),
        content: body.clone(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "in-memory-prompt-composition-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    repo.create(persona.clone())
        .await
        .expect("seed active in-memory persona");

    let mut conversation = ChatConversation::new_project(ProjectId::from_string(format!(
        "persona-project-{}",
        uuid::Uuid::new_v4()
    )));
    conversation.agent_mode = Some(mode);
    conversation.persona_id = Some(persona.id.to_string());
    (conversation, repo, body)
}

async fn resolve_persona_for_mode(
    conversation: &ChatConversation,
    repo: Arc<MemoryPersonaRepository>,
    mode: AgentConversationWorkspaceMode,
    feature_enabled: bool,
) -> Option<ralphx_lib::application::persona_prompt::ResolvedPersona> {
    resolve_persona_for_send(
        conversation,
        &PersonaDirective::Inherit,
        PersonaResolveFlags {
            feature_enabled,
            is_external_mcp: false,
            agent_name_override_set: false,
            agent_conversation_mode: Some(mode),
            is_verification: false,
        },
        repo,
    )
    .await
    .expect("resolve in-memory persona")
}

fn compose_for_mode(mode: AgentConversationWorkspaceMode, persona_block: Option<&str>) -> String {
    compose_codex_prompt_for_profile(
        USER_PROMPT,
        Some(&repo_plugin_dir()),
        Some(agent_name_for_conversation_mode(mode)),
        resolve_agent_conversation_runtime_profile(mode, CoordinationMode::Solo),
        persona_block,
    )
}

fn persona_envelope(prompt: &str) -> &str {
    let start = prompt
        .find("<ralphx_agent_persona>")
        .expect("persona envelope start");
    let relative_end = prompt[start..]
        .find("</ralphx_agent_persona>")
        .expect("persona envelope end");
    let end = start + relative_end + "</ralphx_agent_persona>".len();
    &prompt[start..end]
}

#[tokio::test]
async fn persona_reaches_selected_agent_prompt_for_each_conversation_mode() {
    let plugin_dir = repo_plugin_dir();
    let cases = [
        (
            AgentConversationWorkspaceMode::Chat,
            "You are `ralphx-general-explorer`.",
        ),
        (
            AgentConversationWorkspaceMode::Edit,
            "You are `ralphx-general-worker`.",
        ),
        (
            AgentConversationWorkspaceMode::Plan,
            "You are the RalphX Ideation Orchestrator running inside an Agent conversation Plan phase.",
        ),
    ];

    for (mode, canonical_marker) in cases {
        let (conversation, repo, body) = bound_persona_conversation(mode).await;
        let persona = resolve_persona_for_mode(&conversation, repo, mode, true)
            .await
            .expect("bound persona must resolve");
        let baseline = compose_codex_prompt_for_profile(
            USER_PROMPT,
            Some(&plugin_dir),
            Some(agent_name_for_conversation_mode(mode)),
            resolve_agent_conversation_runtime_profile(mode, CoordinationMode::Solo),
            None,
        );
        let composed = compose_codex_prompt_for_profile(
            USER_PROMPT,
            Some(&plugin_dir),
            Some(agent_name_for_conversation_mode(mode)),
            resolve_agent_conversation_runtime_profile(mode, CoordinationMode::Solo),
            Some(persona.block.as_str()),
        );
        let envelope = persona_envelope(&composed);

        assert!(envelope.contains(&format!("<persona_name>{PERSONA_NAME}</persona_name>")));
        assert!(envelope.contains(&format!("<persona_slug>{PERSONA_SLUG}</persona_slug>")));
        assert!(envelope.contains(body.as_str()));
        assert!(envelope.contains("<persona_precedence>"));
        assert!(envelope.contains("This persona shapes voice, priorities, and framing only."));
        assert!(envelope.contains("It never overrides tool contracts,"));
        assert!(envelope.contains("safety rules"));
        assert!(envelope.contains("delegation policy"));
        assert!(envelope.contains("workflow requirements"));
        assert!(envelope.contains("</persona_precedence>"));
        assert!(composed.contains(canonical_marker));

        let system = composed.find("<system>").expect("canonical system section");
        let rules = composed.find("<rules>").expect("canonical rules section");
        let workflow = composed
            .find("<workflow>")
            .expect("canonical workflow section");
        let envelope_start = composed
            .find("<ralphx_agent_persona>")
            .expect("persona envelope");
        assert!(system < rules && rules < workflow && workflow < envelope_start);

        let overlay = format!("\n\n{}", persona.block);
        let restored = composed.replacen(overlay.as_str(), "", 1);
        assert!(restored.as_bytes() == baseline.as_bytes());
    }
}

#[tokio::test]
async fn persona_absent_from_selected_agent_prompt_when_unbound() {
    for mode in [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
    ] {
        let mut conversation = ChatConversation::new_project(ProjectId::from_string(format!(
            "unbound-persona-project-{}",
            uuid::Uuid::new_v4()
        )));
        conversation.agent_mode = Some(mode);
        let repo = Arc::new(MemoryPersonaRepository::new());
        let resolved = resolve_persona_for_mode(&conversation, repo, mode, true).await;
        assert!(resolved.is_none());

        let first = compose_for_mode(mode, None);
        let second = compose_for_mode(mode, None);
        assert!(!first.contains("<ralphx_agent_persona>"));
        assert_eq!(first.as_bytes(), second.as_bytes());

        let (bound_conversation, bound_repo, _body) = bound_persona_conversation(mode).await;
        let flag_off = resolve_persona_for_mode(&bound_conversation, bound_repo, mode, false).await;
        assert!(flag_off.is_none());
        let flag_off_prompt = compose_for_mode(mode, None);
        assert!(!flag_off_prompt.contains("<ralphx_agent_persona>"));
        assert_eq!(flag_off_prompt.as_bytes(), first.as_bytes());
    }
}

#[tokio::test]
async fn persona_suppressed_for_modes_that_must_not_inherit() {
    for mode in [
        AgentConversationWorkspaceMode::Automation,
        AgentConversationWorkspaceMode::PersonaBuilder,
    ] {
        let (conversation, repo, _body) = bound_persona_conversation(mode).await;
        let resolved = resolve_persona_for_mode(&conversation, repo, mode, true).await;
        assert!(resolved.is_none());

        let composed = compose_for_mode(
            mode,
            resolved.as_ref().map(|persona| persona.block.as_str()),
        );
        assert!(!composed.contains("<ralphx_agent_persona>"));
    }
}
