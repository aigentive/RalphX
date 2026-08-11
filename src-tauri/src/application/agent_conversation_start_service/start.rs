use super::*;

impl<'a, R: Runtime + 'static> AgentConversationStartService<'a, R> {
    pub fn new(deps: AgentConversationStartDeps<'a, R>) -> Self {
        Self { deps }
    }

    pub async fn start(
        self,
        input: StartAgentConversationInput,
    ) -> Result<AgentConversationStartResult, String> {
        let finish_input = input.clone();
        let command_started = Instant::now();
        let context_type = if input.project_id.is_some() {
            ChatContextType::Project
        } else {
            ChatContextType::Standalone
        };
        // Stable label for tracing/progress before a conversation (and thus a
        // standalone self-key) exists; never used as a lookup key.
        let context_log_id: String = input
            .project_id
            .clone()
            .unwrap_or_else(|| STANDALONE_CONTEXT_LOG_LABEL.to_string());
        let context_type_label: &'static str = if context_type == ChatContextType::Standalone {
            "standalone"
        } else {
            "project"
        };
        tracing::info!(
            context_id = %context_log_id,
            context_type = context_type_label,
            content_len = input.content.len(),
            mode = ?input.mode,
            base_ref_kind = ?input.base_ref_kind,
            base_ref = ?input.base_ref,
            "[START_AGENT_CONVERSATION] command invoked"
        );

        if context_type == ChatContextType::Standalone {
            if !standalone_conversations_enabled() {
                return Err(STANDALONE_CONVERSATIONS_DISABLED_ERROR.to_string());
            }
            if input.conversation_id.is_none()
                && !matches!(
                    input.mode.as_deref().map(str::trim),
                    Some("chat" | "persona_builder")
                )
            {
                return Err(STANDALONE_MODE_NOT_ALLOWED_ERROR.to_string());
            }
            if let Some(team_intent) = input.team_intent.as_ref() {
                if !team_intent.is_solo() {
                    return Err(STANDALONE_TEAM_INTENT_REJECTED_ERROR.to_string());
                }
            }
            if input.parent_conversation_id.is_some() {
                return Err(STANDALONE_PARENT_CONVERSATION_REJECTED_ERROR.to_string());
            }
        }

        let parse_runtime_started = Instant::now();
        let mode = parse_agent_workspace_mode(input.mode.as_deref())?;
        if mode == AgentConversationWorkspaceMode::Tasks {
            return Err(
                "Tasks mode is available only for an existing attached task pipeline".to_string(),
            );
        }
        if mode == AgentConversationWorkspaceMode::Autopilot
            && !self.deps.state.agent_capability_gate.autopilot_enabled()
        {
            return Err("Autopilot is disabled in Agent conversation capabilities".to_string());
        }
        let draft_conversation_id = input
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|conversation_id| !conversation_id.is_empty())
            .map(ChatConversationId::from_string);
        let seeded_conversation = if let Some(conversation_id) = draft_conversation_id.as_ref() {
            Some(
                self.deps
                    .state
                    .chat_conversation_repo
                    .get_by_id(conversation_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Conversation not found: {conversation_id}"))?,
            )
        } else {
            None
        };
        if input.source_persona_id.is_some()
            && seeded_conversation
                .as_ref()
                .is_some_and(|conversation| conversation.builder_draft_id.is_some())
        {
            return Err(
                "source_persona_id cannot replace an existing bound persona draft".to_string(),
            );
        }
        if seeded_conversation.as_ref().is_some_and(|conversation| {
            matches!(
                conversation.agent_mode,
                Some(
                    AgentConversationWorkspaceMode::PersonaBuilder
                        | AgentConversationWorkspaceMode::Automation
                )
            ) && conversation.agent_mode != Some(mode)
        }) {
            return Err(format!(
                "{SEEDED_CONVERSATION_MODE_LOCKED_ERROR_CODE} conversation mode is locked"
            ));
        }
        if let Some(conversation) = seeded_conversation.as_ref().filter(|conversation| {
            mode == AgentConversationWorkspaceMode::PersonaBuilder
                && conversation.agent_mode != Some(AgentConversationWorkspaceMode::PersonaBuilder)
        }) {
            let has_messages = !self
                .deps
                .state
                .chat_message_repo
                .get_by_conversation(&conversation.id)
                .await
                .map_err(|error| error.to_string())?
                .is_empty();
            if has_messages {
                return Err(format!(
                    "{SEEDED_CONVERSATION_MODE_LOCKED_ERROR_CODE} conversation mode is locked"
                ));
            }
        }
        if context_type == ChatContextType::Standalone
            && !matches!(
                input.mode.as_deref().map(str::trim),
                Some("chat" | "persona_builder")
            )
        {
            return Err(STANDALONE_MODE_NOT_ALLOWED_ERROR.to_string());
        }
        let explicitly_requested_coordination_mode = input
            .team_intent
            .as_ref()
            .map(|intent| intent.coordination_mode);
        if mode == AgentConversationWorkspaceMode::PersonaBuilder
            && explicitly_requested_coordination_mode
                .is_some_and(|mode| mode != CoordinationMode::Solo)
        {
            return Err(PERSONA_BUILDER_TEAM_INTENT_REJECTED_ERROR.to_string());
        }
        if input.source_persona_id.is_some()
            && mode != AgentConversationWorkspaceMode::PersonaBuilder
        {
            return Err(PERSONA_BUILDER_SOURCE_MODE_ERROR.to_string());
        }
        let source_persona_id = match input.source_persona_id.as_deref() {
            Some(source_id) if source_id.trim().is_empty() => {
                return Err("source_persona_id cannot be empty".to_string())
            }
            Some(source_id) => Some(PersonaId::from_string(source_id.trim().to_string())),
            None => None,
        };
        log_start_agent_conversation_phase(
            &context_log_id,
            None,
            "parse_runtime_selection",
            parse_runtime_started,
        );

        let parse_input_started = Instant::now();
        let base_ref_kind = parse_agent_workspace_base_kind(input.base_ref_kind.as_deref())?;
        let base_branch_mode =
            parse_agent_workspace_branch_mode(input.base_branch_mode.as_deref())?;
        let base_ref = trim_optional_input(input.base_ref);
        let base_display_name = trim_optional_input(input.base_display_name);
        let parent_conversation_id = trim_optional_input(input.parent_conversation_id);
        let conversation_title = trim_optional_input(input.title);
        let ticket_branch_name_hint =
            first_ticket_branch_name_hint(&input.composer_integration_references);
        let source_pull_request = normalize_agent_workspace_source_pull_request(
            input.base_source_pull_request,
            base_ref_kind,
            base_ref.as_deref(),
        )?;
        validate_review_pr_workspace_source_pull_request(mode, source_pull_request.as_ref())
            .map_err(|error| error.to_string())?;
        let selected_plan_reference = selected_plan_reference(&input.composer_artifact_references)?;
        // Standalone never creates a project-rooted workspace. Its CWD resolves to
        // the private standalone workspace via `send_message`'s Standalone arm.
        let should_create_workspace = context_type == ChatContextType::Project
            && agent_mode_should_create_workspace(
                mode,
                source_pull_request.as_ref(),
                selected_plan_reference.is_some(),
            );
        let project_id_opt = input
            .project_id
            .as_ref()
            .map(|id| ProjectId::from_string(id.clone()));
        log_start_agent_conversation_phase(
            &context_log_id,
            None,
            "parse_input",
            parse_input_started,
        );

        let project_lookup_started = Instant::now();
        let project = match project_id_opt.as_ref() {
            Some(project_id) => Some(
                self.deps
                    .state
                    .project_repo
                    .get_by_id(project_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Project not found: {context_log_id}"))?,
            ),
            None => None,
        };
        log_start_agent_conversation_phase(
            &context_log_id,
            None,
            "load_project",
            project_lookup_started,
        );

        let has_explicit_composer_override = input.provider_harness.is_some()
            || input.model_override.is_some()
            || input.logical_effort.is_some()
            || input.codex_fast_mode.is_some()
            || input.persona_id.is_some()
            || input.team_intent.is_some();
        let role_default = if has_explicit_composer_override {
            None
        } else {
            let role = crate::application::agent_lane_resolution::routing_role_for_chat_launch(
                agent_name_for_workspace_mode(mode),
                context_type,
                None,
                Some(mode),
                false,
            );
            let project_root = project
                .as_ref()
                .map(|project| std::path::Path::new(&project.working_directory));
            Some(
                self.deps
                    .state
                    .manual_role_default_service()
                    .resolve(input.project_id.as_deref(), project_root, role)
                    .await
                    .map_err(|error| {
                        format!("Failed to resolve manual default for {role}: {error}")
                    })?,
            )
        };
        let role_value = role_default.as_ref().map(|resolved| &resolved.value);
        let harness_override = match input.provider_harness.as_deref() {
            Some(provider) => Some(provider.parse::<AgentHarnessKind>()?),
            None => role_value.map(|value| value.harness),
        };
        let effective_model_override = input
            .model_override
            .clone()
            .or_else(|| role_value.and_then(|value| value.model.clone()));
        let effective_logical_effort = input
            .logical_effort
            .or_else(|| role_value.and_then(|value| value.effort));
        let effective_service_tier_override = match input.codex_fast_mode {
            Some(fast) => {
                crate::application::chat_service::codex_fast_mode_service_tier_override(Some(fast))
            }
            None => role_value.and_then(|value| match value.service_tier {
                ManualServiceTier::ProviderDefault => None,
                ManualServiceTier::Standard => Some("standard".to_string()),
                ManualServiceTier::Fast => Some("fast".to_string()),
            }),
        };
        let effective_team_intent = input.team_intent.clone().or_else(|| {
            role_value
                .and_then(|value| value.coordination_mode)
                .map(|coordination_mode| TeamIntent {
                    coordination_mode,
                    strategy: None,
                })
        });
        let effective_persona_id = input.persona_id.clone().or_else(|| {
            (mode != AgentConversationWorkspaceMode::PersonaBuilder && agent_personas_enabled())
                .then(|| role_value.and_then(|value| value.persona_id.as_ref()))
                .flatten()
                .map(ToString::to_string)
        });
        let persona_id = trim_optional_input(effective_persona_id).map(PersonaId::from_string);
        let requested_coordination_mode = effective_team_intent
            .as_ref()
            .map(|intent| intent.coordination_mode);
        if mode == AgentConversationWorkspaceMode::PersonaBuilder
            && requested_coordination_mode.is_some_and(|mode| mode != CoordinationMode::Solo)
        {
            return Err(PERSONA_BUILDER_TEAM_INTENT_REJECTED_ERROR.to_string());
        }

        let validate_runtime_started = Instant::now();
        let validated_harness =
            crate::application::validate_chat_runtime_for_context_with_override(
                self.deps.state,
                context_type,
                &context_log_id,
                "start_agent_conversation",
                harness_override,
            )
            .await?;
        let requested_capability = requested_coordination_mode.unwrap_or_default();
        let requested_harness = harness_override.unwrap_or(validated_harness);
        let codex_ultra_supported = (requested_capability == CoordinationMode::CodexNativeUltra)
            .then(|| {
                crate::application::agent_capability_validation::codex_ultra_support_for_model(
                    requested_harness,
                    effective_model_override.as_deref(),
                )
            })
            .flatten();
        crate::application::agent_capability_validation::validate_agent_capability(
            requested_capability,
            requested_harness,
            &self.deps.state.agent_capability_gate,
            codex_ultra_supported,
        )
        .map_err(|error| error.to_string())?;
        crate::application::managed_team::validate_native_team_intent(
            effective_team_intent.as_ref(),
            requested_harness,
        )
        .map_err(|error| error.to_string())?;
        log_start_agent_conversation_phase(
            &context_log_id,
            None,
            "validate_chat_runtime",
            validate_runtime_started,
        );

        let mcp_preflight_started = Instant::now();
        self.deps
            .state
            .mcp_policy_service()
            .resolve_launch_policy(
                requested_harness,
                input.project_id.as_deref(),
                project
                    .as_ref()
                    .map(|project| std::path::Path::new(&project.working_directory)),
            )
            .await
            .map_err(|error| error.to_string())?;
        log_start_agent_conversation_phase(
            &context_log_id,
            None,
            "mcp_setup_preflight",
            mcp_preflight_started,
        );

        let ProjectSetupOutput {
            project,
            validated_clickup_task,
            base_ref_kind,
            base_branch_mode,
            base_ref,
            base_display_name,
            ticket_branch_name_hint,
            source_pull_request,
        } = self
            .resolve_project_setup(ProjectSetupInput {
                project_id_opt: project_id_opt.clone(),
                project,
                source_persona_id: source_persona_id.clone(),
                composer_integration_references: input.composer_integration_references.clone(),
                should_create_workspace,
                mode,
                base_ref_kind,
                base_branch_mode,
                base_ref,
                base_display_name,
                ticket_branch_name_hint,
                source_pull_request,
                persona_id: persona_id.clone(),
                draft_conversation_id: draft_conversation_id.clone(),
                context_type,
                context_log_id: context_log_id.clone(),
            })
            .await?;

        let conversation_resolve_started = Instant::now();
        let mut conversation = if let Some(conversation) = seeded_conversation {
            if mode == AgentConversationWorkspaceMode::PersonaBuilder
                && conversation.coordination_mode != CoordinationMode::Solo
            {
                return Err(PERSONA_BUILDER_TEAM_INTENT_REJECTED_ERROR.to_string());
            }
            match context_type {
                ChatContextType::Project => {
                    if conversation.context_type != ChatContextType::Project
                        || Some(conversation.context_id.as_str()) != input.project_id.as_deref()
                    {
                        return Err(format!(
                            "Conversation {} does not belong to project {}",
                            conversation.id, context_log_id
                        ));
                    }
                }
                _ => {
                    // Seeded-standalone ownership rule (D3.6): valid iff the seed is
                    // truly a self-keyed Standalone row and this start carries no
                    // project_id. Any mismatch on context_type, context_id, or a
                    // supplied project_id is rejected.
                    if conversation.context_type != ChatContextType::Standalone
                        || conversation.context_id != conversation.id.as_str()
                        || conversation.coordination_mode != CoordinationMode::Solo
                        || input.project_id.is_some()
                    {
                        return Err(format!(
                            "Conversation {} is not a valid standalone seed",
                            conversation.id
                        ));
                    }
                }
            }
            conversation
        } else {
            match context_type {
                ChatContextType::Project => ChatConversation::new_project(
                    project_id_opt
                        .clone()
                        .expect("Project context requires project_id"),
                ),
                _ => ChatConversation::new_standalone(),
            }
        };
        conversation.set_agent_mode(Some(mode));
        if let Some(coordination_mode) = requested_coordination_mode {
            conversation.set_coordination_mode(coordination_mode);
        }
        let should_create_conversation = draft_conversation_id.is_none();
        if let Some(parent_conversation_id) = parent_conversation_id.as_deref() {
            if should_create_conversation {
                let parent_id = ChatConversationId::from_string(parent_conversation_id.to_string());
                let parent = self
                    .deps
                    .state
                    .chat_conversation_repo
                    .get_by_id(&parent_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Parent conversation not found: {}", parent_id))?;
                if parent.context_type != ChatContextType::Project
                    || Some(parent.context_id.as_str()) != input.project_id.as_deref()
                {
                    return Err(format!(
                        "Parent conversation {} does not belong to project {}",
                        parent.id, context_log_id
                    ));
                }
                conversation.parent_conversation_id = Some(parent.id.as_str());
            }
        }
        if should_create_conversation {
            if let Some(title) = conversation_title {
                conversation.set_title(title);
            }
        }
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "resolve_conversation",
            conversation_resolve_started,
        );

        let workspace_prepare_started = Instant::now();
        if should_create_conversation {
            emit_start_agent_conversation_progress(
                &self.deps.app_handle,
                context_type_label,
                &context_log_id,
                &conversation.id,
                "resolve_conversation",
                "Creating chat",
            );
        }
        if should_create_workspace {
            emit_start_agent_conversation_progress(
                &self.deps.app_handle,
                context_type_label,
                &context_log_id,
                &conversation.id,
                "prepare_workspace",
                "Setup workspace",
            );
        }
        let mut composer_artifact_references = input.composer_artifact_references.clone();
        let workspace = if should_create_workspace {
            // should_create_workspace implies context_type == Project.
            let project = project
                .as_ref()
                .expect("should_create_workspace implies a Project context");
            let pr_automation_defaults =
                agent_workspace_pr_automation_defaults_for_project(self.deps.state, &project.id)
                    .await?;
            let mut workspace =
                match prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint(
                    project,
                    &conversation.id,
                    mode,
                    AgentConversationWorkspaceBaseSelection {
                        kind: base_ref_kind,
                        branch_mode: base_branch_mode,
                        base_ref,
                        display_name: base_display_name,
                        source_pull_request,
                    },
                    AgentConversationWorkspaceSetupMode::Deferred,
                    pr_automation_defaults,
                    // Automation runs (setup + successors) prefer the advanced
                    // remote-tracking base so successor worktrees build on merged work
                    // (integration-branch model). Non-automation chats keep the local
                    // start-point.
                    conversation.automation_id.is_some(),
                    ticket_branch_name_hint.clone(),
                )
                .await
                {
                    Ok(workspace) => workspace,
                    Err(error) => {
                        let mut error = error.to_string();
                        if !should_create_conversation {
                            if let Err(archive_error) =
                                archive_empty_seeded_draft_after_setup_failure(
                                    self.deps.state,
                                    &conversation,
                                )
                                .await
                            {
                                error = format!(
                                    "{error}; failed to archive failed draft: {archive_error}",
                                );
                            }
                        }
                        return Err(
                            if base_branch_mode
                                == Some(AgentConversationWorkspaceBranchMode::Linked)
                            {
                                linked_setup_failure_error(error)
                            } else {
                                error
                            },
                        );
                    }
                };
            if let Some(plan_reference) = selected_plan_reference.as_ref() {
                let import = import_agent_conversation_plan_reference(
                    self.deps.state,
                    project,
                    &mut workspace,
                    plan_reference,
                )
                .await?;
                composer_artifact_references = rewrite_imported_plan_references(
                    &composer_artifact_references,
                    plan_reference,
                    &import.composer_references,
                );
            } else {
                ensure_plan_workspace_planning_session_link(
                    self.deps.state,
                    project,
                    &mut workspace,
                )
                .await?;
            }
            Some(workspace)
        } else {
            None
        };
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "prepare_workspace",
            workspace_prepare_started,
        );

        self.persist_and_spawn(FinishFlow {
            input: finish_input,
            command_started,
            context_type,
            context_type_label,
            context_log_id,
            conversation,
            should_create_conversation,
            workspace,
            source_persona_id,
            persona_id,
            validated_clickup_task,
            project,
            requested_coordination_mode,
            harness_override,
            effective_model_override,
            effective_logical_effort,
            effective_service_tier_override,
            effective_team_intent,
            mode,
            composer_artifact_references,
        })
        .await
    }
}
