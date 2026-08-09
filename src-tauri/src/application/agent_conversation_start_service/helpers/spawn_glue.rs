use super::*;

pub(crate) fn normalized_effort_for_supported(
    requested: Option<LogicalEffort>,
    supported_efforts: &[LogicalEffort],
    default_effort: LogicalEffort,
) -> LogicalEffort {
    requested
        .filter(|effort| supported_efforts.contains(effort))
        .unwrap_or(default_effort)
}

pub(crate) async fn normalize_agent_runtime_selection(
    state: &AppState,
    provider: Option<AgentHarnessKind>,
    model_override: Option<String>,
    effort_override: Option<LogicalEffort>,
) -> Result<(Option<String>, Option<LogicalEffort>), String> {
    let Some(provider) = provider else {
        return Ok((model_override, effort_override));
    };

    let custom_models = state
        .agent_model_registry_repo
        .list_custom_models()
        .await
        .map_err(|error| format!("Failed to fetch custom agent models: {error}"))?;
    let snapshot = AgentModelRegistrySnapshot::merged(custom_models);
    if let Some(model_id) = model_override {
        if let Some(model) = snapshot.find_enabled(provider, &model_id) {
            let effort = normalized_effort_for_supported(
                effort_override,
                &model.supported_efforts,
                model.default_effort,
            );
            return Ok((Some(model_id), Some(effort)));
        }

        let effort = normalized_effort_for_supported(
            effort_override,
            default_efforts_for_provider(provider),
            default_effort_for_provider(provider),
        );
        return Ok((Some(model_id), Some(effort)));
    }

    let effort = if let Some(default_model) = snapshot.default_for_provider(provider) {
        normalized_effort_for_supported(
            effort_override,
            &default_model.supported_efforts,
            default_model.default_effort,
        )
    } else {
        normalized_effort_for_supported(
            effort_override,
            default_efforts_for_provider(provider),
            default_effort_for_provider(provider),
        )
    };

    Ok((None, Some(effort)))
}

pub(crate) fn log_start_agent_conversation_phase(
    project_id: &str,
    conversation_id: Option<&ChatConversationId>,
    phase: &'static str,
    started: Instant,
) {
    tracing::info!(
        project_id,
        conversation_id = ?conversation_id.map(ChatConversationId::as_str),
        phase,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "start_agent_conversation phase completed"
    );
}

#[derive(Clone, Debug, Serialize)]
struct AgentStartupProgressPayload<'a> {
    conversation_id: String,
    context_type: &'static str,
    context_id: &'a str,
    stage: &'static str,
    label: &'static str,
}

pub(crate) fn emit_start_agent_conversation_progress(
    events: &dyn ralphx_events::EventSink,
    context_type: &'static str,
    context_id: &str,
    conversation_id: &ChatConversationId,
    stage: &'static str,
    label: &'static str,
) {
    // This emitter is only called after a conversation id exists. Standalone
    // conversations are self-keyed, so never leak the pre-creation tracing
    // sentinel into a correlatable progress event.
    let context_id = if context_type == "standalone" {
        conversation_id.as_str()
    } else {
        context_id.to_string()
    };
    let payload = AgentStartupProgressPayload {
        conversation_id: conversation_id.as_str(),
        context_type,
        context_id: &context_id,
        stage,
        label,
    };
    let _ = ralphx_events::emit_serialized(events, "agent:startup_progress", &payload);
}
