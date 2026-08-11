use super::*;

async fn abort_new_conversation_after_failure(
    state: &AppState,
    conversation_id: &ChatConversationId,
    failed_phase: &'static str,
) {
    if let Err(cleanup_error) = abort_seeded_agent_conversation(state, conversation_id).await {
        tracing::warn!(
            %conversation_id,
            %cleanup_error,
            failed_phase,
            "Failed to abort a newly created conversation after start preparation failed"
        );
    }
}

async fn restore_existing_conversation_after_failure(
    state: &AppState,
    previous: &ChatConversation,
    failed_phase: &'static str,
) {
    let mode_result = state
        .chat_conversation_repo
        .update_agent_mode(&previous.id, previous.agent_mode)
        .await;
    let coordination_result = state
        .chat_conversation_repo
        .update_coordination_mode(&previous.id, previous.coordination_mode)
        .await;
    if let Err(cleanup_error) = mode_result.and(coordination_result) {
        tracing::error!(
            conversation_id = %previous.id,
            %cleanup_error,
            failed_phase,
            "Failed to restore an existing conversation after start preparation failed"
        );
    }
}

async fn remove_new_private_workspace_after_failure(
    state: &AppState,
    conversation_id: &ChatConversationId,
    private_workspace_existed: bool,
    failed_phase: &'static str,
) {
    if private_workspace_existed {
        return;
    }
    if let Err(cleanup_error) =
        remove_workspace_if_present(state.app_paths.app_data_dir(), &conversation_id.as_str())
    {
        tracing::warn!(
            %conversation_id,
            %cleanup_error,
            failed_phase,
            "Failed to remove a newly prepared private workspace after start preparation failed"
        );
    }
}

pub(super) struct FinishFlow {
    pub(super) input: StartAgentConversationInput,
    pub(super) command_started: Instant,
    pub(super) context_type: ChatContextType,
    pub(super) context_type_label: &'static str,
    pub(super) context_log_id: String,
    pub(super) conversation: ChatConversation,
    pub(super) should_create_conversation: bool,
    pub(super) workspace: Option<AgentConversationWorkspace>,
    pub(super) source_persona_id: Option<PersonaId>,
    pub(super) persona_id: Option<PersonaId>,
    pub(super) validated_clickup_task:
        Option<crate::application::clickup_integration_service::ClickUpTaskContent>,
    pub(super) project: Option<crate::domain::entities::Project>,
    pub(super) requested_coordination_mode: Option<CoordinationMode>,
    pub(super) harness_override: Option<AgentHarnessKind>,
    pub(super) effective_model_override: Option<String>,
    pub(super) effective_logical_effort: Option<LogicalEffort>,
    pub(super) effective_service_tier_override: Option<String>,
    pub(super) effective_team_intent: Option<TeamIntent>,
    pub(super) mode: AgentConversationWorkspaceMode,
    pub(super) composer_artifact_references: Vec<ComposerArtifactReference>,
}

impl<'a> AgentConversationStartService<'a> {
    pub(super) async fn persist_and_spawn(
        self,
        flow: FinishFlow,
    ) -> Result<AgentConversationStartResult, String> {
        let FinishFlow {
            input,
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
        } = flow;
        let conversation_persist_started = Instant::now();
        emit_start_agent_conversation_progress(
            self.deps.events.as_ref(),
            context_type_label,
            &context_log_id,
            &conversation.id,
            "persist_conversation",
            "Saving chat",
        );
        let persisted_before_start = if should_create_conversation {
            None
        } else {
            Some(
                self.deps
                    .state
                    .chat_conversation_repo
                    .get_by_id(&conversation.id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Conversation not found: {}", conversation.id))?,
            )
        };
        let mut conversation = if should_create_conversation {
            self.deps
                .state
                .chat_conversation_repo
                .create(conversation)
                .await
                .map_err(|error| error.to_string())?
        } else {
            conversation
        };
        let needs_private_workspace = context_type == ChatContextType::Standalone
            || mode == AgentConversationWorkspaceMode::PersonaBuilder;
        let private_workspace_existed = needs_private_workspace
            && resolve_workspace(
                self.deps.state.app_paths.app_data_dir(),
                &conversation.id.as_str(),
            )
            .is_ok();
        if needs_private_workspace {
            if let Err(error) = create_workspace(
                self.deps.state.app_paths.app_data_dir(),
                &conversation.id.as_str(),
            ) {
                if should_create_conversation {
                    abort_new_conversation_after_failure(
                        self.deps.state,
                        &conversation.id,
                        "create_private_workspace",
                    )
                    .await;
                } else if !private_workspace_existed {
                    if let Err(cleanup_error) = remove_workspace_if_present(
                        self.deps.state.app_paths.app_data_dir(),
                        &conversation.id.as_str(),
                    ) {
                        tracing::warn!(
                            conversation_id = %conversation.id,
                            %cleanup_error,
                            "Failed to remove a partial private workspace after workspace creation failed"
                        );
                    }
                }
                return Err(error.to_string());
            }
        }
        if mode == AgentConversationWorkspaceMode::PersonaBuilder {
            if let Err(error) = sync_builder_attachments(
                self.deps.state.app_paths.app_data_dir(),
                &self.deps.state.attachment_storage_path,
                &conversation.id,
                Arc::clone(&self.deps.state.chat_attachment_repo),
            )
            .await
            {
                if should_create_conversation {
                    abort_new_conversation_after_failure(
                        self.deps.state,
                        &conversation.id,
                        "sync_builder_attachments",
                    )
                    .await;
                } else if !private_workspace_existed {
                    if let Err(cleanup_error) = remove_workspace_if_present(
                        self.deps.state.app_paths.app_data_dir(),
                        &conversation.id.as_str(),
                    ) {
                        tracing::warn!(
                            conversation_id = %conversation.id,
                            %cleanup_error,
                            "Failed to remove a newly prepared builder workspace after attachment sync failed"
                        );
                    }
                }
                return Err(error.to_string());
            }
        }
        if let Some(previous) = persisted_before_start.as_ref() {
            if let Err(error) = self
                .deps
                .state
                .chat_conversation_repo
                .update_agent_mode(&conversation.id, Some(mode))
                .await
            {
                remove_new_private_workspace_after_failure(
                    self.deps.state,
                    &conversation.id,
                    private_workspace_existed,
                    "persist_agent_mode",
                )
                .await;
                return Err(error.to_string());
            }
            if let Some(coordination_mode) = requested_coordination_mode {
                if previous.coordination_mode == CoordinationMode::RxNativeTeam
                    && coordination_mode != CoordinationMode::RxNativeTeam
                {
                    match self
                        .deps
                        .state
                        .managed_team
                        .exit_team_before_coordination_change(
                            &crate::application::AgentTaskService::new(
                                self.deps.state.agent_task_repo.clone(),
                            ),
                            &conversation.id,
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(error) => {
                            let rollback_error = self
                                .deps
                                .state
                                .chat_conversation_repo
                                .update_agent_mode(&conversation.id, previous.agent_mode)
                                .await
                                .err();
                            if let Some(rollback_error) = rollback_error.as_ref() {
                                tracing::error!(
                                    conversation_id = %conversation.id,
                                    %rollback_error,
                                    "Failed to restore conversation mode after Team exit failed"
                                );
                            }
                            remove_new_private_workspace_after_failure(
                                self.deps.state,
                                &conversation.id,
                                private_workspace_existed,
                                "exit_team_before_coordination_mode",
                            )
                            .await;
                            return match rollback_error {
                                Some(rollback_error) => Err(format!(
                                    "{error}; failed to restore prior conversation mode: {rollback_error}"
                                )),
                                None => Err(error.to_string()),
                            };
                        }
                    }
                }
                if let Err(error) = self
                    .deps
                    .state
                    .chat_conversation_repo
                    .update_coordination_mode(&conversation.id, coordination_mode)
                    .await
                {
                    let rollback_error = self
                        .deps
                        .state
                        .chat_conversation_repo
                        .update_agent_mode(&conversation.id, previous.agent_mode)
                        .await
                        .err();
                    if let Some(rollback_error) = rollback_error.as_ref() {
                        tracing::error!(
                            conversation_id = %conversation.id,
                            %rollback_error,
                            "Failed to restore conversation mode after coordination persistence failed"
                        );
                    }
                    remove_new_private_workspace_after_failure(
                        self.deps.state,
                        &conversation.id,
                        private_workspace_existed,
                        "persist_coordination_mode",
                    )
                    .await;
                    return match rollback_error {
                        Some(rollback_error) => Err(format!(
                            "{error}; failed to restore prior conversation mode: {rollback_error}"
                        )),
                        None => Err(error.to_string()),
                    };
                }
            }
        }
        if let Some(source_persona_id) = source_persona_id.as_ref() {
            if conversation.builder_draft_id.is_some() {
                return Err(
                    "source_persona_id cannot replace an existing bound persona draft".to_string(),
                );
            }
            let persona_service = PersonaService::new(
                self.deps.state.db.clone(),
                Arc::clone(&self.deps.state.persona_repo),
                Arc::clone(&self.deps.state.chat_conversation_repo),
            );
            if let Err(error) = persona_service
                .create_seeded_bound_draft(
                    agent_personas_enabled(),
                    &conversation,
                    source_persona_id,
                )
                .await
            {
                if should_create_conversation {
                    abort_new_conversation_after_failure(
                        self.deps.state,
                        &conversation.id,
                        "seed_persona_draft",
                    )
                    .await;
                } else if let Some(previous) = persisted_before_start.as_ref() {
                    restore_existing_conversation_after_failure(
                        self.deps.state,
                        previous,
                        "seed_persona_draft",
                    )
                    .await;
                    remove_new_private_workspace_after_failure(
                        self.deps.state,
                        &conversation.id,
                        private_workspace_existed,
                        "seed_persona_draft",
                    )
                    .await;
                }
                return Err(error.to_string());
            }
            conversation = self
                .deps
                .state
                .chat_conversation_repo
                .get_by_id(&conversation.id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    "PersonaBuilder conversation disappeared after draft seeding".to_string()
                })?;
        }
        if let Some(persona_id) = persona_id.as_ref() {
            self.deps
                .state
                .chat_conversation_repo
                .update_persona_binding(&conversation.id, Some(persona_id.as_str()))
                .await
                .map_err(|error| error.to_string())?;
            conversation.persona_id = Some(persona_id.to_string());
        }
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "persist_conversation",
            conversation_persist_started,
        );

        let workspace_persist_started = Instant::now();
        if workspace.is_some() {
            emit_start_agent_conversation_progress(
                self.deps.events.as_ref(),
                context_type_label,
                &context_log_id,
                &conversation.id,
                "persist_workspace",
                "Saving chat",
            );
        }
        let workspace = match workspace {
            Some(workspace) => match self
                .deps
                .state
                .agent_conversation_workspace_repo
                .create_or_update(workspace)
                .await
            {
                Ok(workspace) => Some(workspace),
                Err(error) => {
                    if should_create_conversation {
                        if let Err(cleanup_error) = self
                            .deps
                            .state
                            .chat_conversation_repo
                            .delete(&conversation.id)
                            .await
                        {
                            tracing::warn!(
                                conversation_id = %conversation.id,
                                error = %cleanup_error,
                                "Failed to delete newly created conversation after workspace persistence failed"
                            );
                        }
                    }
                    return Err(error.to_string());
                }
            },
            None => None,
        };
        let review_pr_monitor = ensure_review_pr_monitor_for_workspace(
            self.deps.state.agent_conversation_workspace_repo.as_ref(),
            workspace.as_ref(),
        )
        .await?;
        if let (Some(workspace), Some(monitor)) = (workspace.as_ref(), review_pr_monitor.as_ref()) {
            use crate::application::services::pr_merge_poller::AgentWorkspacePrPollerStart;

            match crate::application::services::pr_merge_poller::start_review_pr_lifecycle_polling(
                self.deps.state,
                workspace,
                monitor,
            )
            .await
            .map_err(|error| error.to_string())?
            {
                AgentWorkspacePrPollerStart::Started
                | AgentWorkspacePrPollerStart::AlreadyRunning => {}
                // The persisted monitor is authoritative. A temporarily unavailable
                // poller can be started by lifecycle recovery once GitHub is available.
                AgentWorkspacePrPollerStart::Unavailable => {}
            }
        }
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "persist_workspace",
            workspace_persist_started,
        );

        // validated_clickup_task can only be Some when `project` was resolved
        // (ClickUp start flows require a Project context).
        if let (Some(clickup_task), Some(project)) =
            (validated_clickup_task.as_ref(), project.as_ref())
        {
            let head_sha = match workspace.as_ref() {
                Some(workspace) => {
                    match resolve_valid_agent_conversation_workspace_path(project, workspace).await
                    {
                        Ok(worktree_path) => GitService::get_head_sha(&worktree_path).await.ok(),
                        Err(_) => None,
                    }
                }
                None => None,
            };
            let metadata_json = serde_json::json!({
                "source": "ticket_start",
                "title": clickup_task.name,
                "branch": workspace.as_ref().map(|workspace| workspace.branch_name.as_str()),
                "pr_number": workspace.as_ref().and_then(|workspace| {
                    workspace.source_pull_request.as_ref().map(|pull_request| pull_request.number)
                }),
                "validated_at": chrono::Utc::now().to_rfc3339(),
            })
            .to_string();
            self.deps
                .state
                .external_issue_link_service
                .upsert_ticket_conversation_link(TicketConversationLinkInput {
                    provider: "clickup".to_string(),
                    external_kind: "clickup".to_string(),
                    external_id: clickup_task.id.clone(),
                    external_key: clickup_task.custom_id.clone(),
                    external_url: clickup_task.url.clone(),
                    conversation_id: conversation.id.as_str(),
                    project_id: project.id.to_string(),
                    local_sha: head_sha,
                    local_state: Some("active".to_string()),
                    metadata_json: Some(metadata_json),
                })
                .await
                .map_err(|error| error.to_string())?;
            let _ = ralphx_events::emit_serialized(
                self.deps.events.as_ref(),
                "ticketing:cache_invalidated",
                &serde_json::json!({
                    "provider": "clickup",
                    "ticketId": clickup_task.id,
                    "ticketKey": clickup_task.custom_id,
                    "projectId": project.id.to_string(),
                    "reason": "conversation_started",
                    "invalidatedAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
        }

        // Standalone is self-keyed: its send/event context_id is the conversation's
        // own id, not a project id.
        let send_context_id: String = if context_type == ChatContextType::Standalone {
            conversation.id.as_str()
        } else {
            input
                .project_id
                .clone()
                .expect("Project context requires project_id")
        };

        let event_emit_started = Instant::now();
        if should_create_conversation {
            let _ = ralphx_events::emit_serialized(
                self.deps.events.as_ref(),
                "agent:conversation_created",
                &AgentConversationCreatedPayload {
                    conversation_id: conversation.id.as_str(),
                    context_type: context_type.to_string(),
                    context_id: send_context_id.clone(),
                },
            );
        }
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "emit_conversation_created",
            event_emit_started,
        );

        let service_create_started = Instant::now();
        let service = self
            .deps
            .state
            .build_chat_service_with_execution_state(Arc::clone(self.deps.execution_state));
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "create_chat_service",
            service_create_started,
        );

        let runtime_override_prepare_started = Instant::now();
        let model_override = effective_model_override
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string);
        let working_directory_override = workspace
            .as_ref()
            .map(|workspace| PathBuf::from(&workspace.worktree_path));
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "prepare_runtime_overrides",
            runtime_override_prepare_started,
        );

        let runtime_normalize_started = Instant::now();
        let (model_override, logical_effort_override) = normalize_agent_runtime_selection(
            self.deps.state,
            harness_override,
            model_override,
            effective_logical_effort,
        )
        .await?;
        let service_tier_override = effective_service_tier_override;
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "normalize_runtime_selection",
            runtime_normalize_started,
        );

        let send_message_started = Instant::now();
        emit_start_agent_conversation_progress(
            self.deps.events.as_ref(),
            context_type_label,
            &context_log_id,
            &conversation.id,
            "send_message",
            "Starting agent",
        );
        let send_result = service
            .send_message(
                context_type,
                &send_context_id,
                &input.content,
                SendMessageOptions {
                    harness_override,
                    agent_name_override: Some(agent_name_for_workspace_mode(mode).to_string()),
                    persona_directive: persona_id
                        .as_ref()
                        .map(|persona_id| PersonaDirective::Explicit(persona_id.clone()))
                        .unwrap_or_default(),
                    model_override,
                    logical_effort_override,
                    service_tier_override,
                    conversation_id_override: Some(conversation.id),
                    working_directory_override,
                    composer_project_references: input.composer_project_references.clone(),
                    composer_integration_references: input.composer_integration_references.clone(),
                    composer_artifact_references,
                    composer_selection_snapshot: input.composer_selection_snapshot.clone(),
                    team_intent: effective_team_intent,
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "send_message",
            send_message_started,
        );
        log_start_agent_conversation_phase(
            &context_log_id,
            Some(&conversation.id),
            "command_total",
            command_started,
        );

        Ok(AgentConversationStartResult {
            conversation,
            workspace,
            send_result,
        })
    }
}
