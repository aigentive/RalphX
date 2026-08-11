use super::*;

pub(super) struct ProjectSetupInput {
    pub(super) project_id_opt: Option<ProjectId>,
    pub(super) project: Option<crate::domain::entities::Project>,
    pub(super) source_persona_id: Option<PersonaId>,
    pub(super) composer_integration_references: Vec<ComposerIntegrationReference>,
    pub(super) should_create_workspace: bool,
    pub(super) mode: AgentConversationWorkspaceMode,
    pub(super) base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
    pub(super) base_branch_mode: Option<AgentConversationWorkspaceBranchMode>,
    pub(super) base_ref: Option<String>,
    pub(super) base_display_name: Option<String>,
    pub(super) ticket_branch_name_hint: Option<AgentConversationWorkspaceBranchNameHint>,
    pub(super) source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
    pub(super) persona_id: Option<PersonaId>,
    pub(super) draft_conversation_id: Option<ChatConversationId>,
    pub(super) context_type: ChatContextType,
    pub(super) context_log_id: String,
}

pub(super) struct ProjectSetupOutput {
    pub(super) project: Option<crate::domain::entities::Project>,
    pub(super) validated_clickup_task:
        Option<crate::application::clickup_integration_service::ClickUpTaskContent>,
    pub(super) base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
    pub(super) base_branch_mode: Option<AgentConversationWorkspaceBranchMode>,
    pub(super) base_ref: Option<String>,
    pub(super) base_display_name: Option<String>,
    pub(super) ticket_branch_name_hint: Option<AgentConversationWorkspaceBranchNameHint>,
    pub(super) source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
}

impl<'a, R: Runtime + 'static> AgentConversationStartService<'a, R> {
    pub(super) async fn resolve_project_setup(
        &self,
        input: ProjectSetupInput,
    ) -> Result<ProjectSetupOutput, String> {
        let ProjectSetupInput {
            project_id_opt,
            project,
            source_persona_id,
            composer_integration_references,
            should_create_workspace,
            mode,
            mut base_ref_kind,
            mut base_branch_mode,
            mut base_ref,
            mut base_display_name,
            mut ticket_branch_name_hint,
            mut source_pull_request,
            persona_id,
            draft_conversation_id,
            context_type,
            context_log_id,
        } = input;

        if let Some(source_persona_id) = source_persona_id.as_ref() {
            PersonaService::new(
                self.deps.state.db.clone(),
                Arc::clone(&self.deps.state.persona_repo),
                Arc::clone(&self.deps.state.chat_conversation_repo),
            )
            .validate_refine_source(
                agent_personas_enabled(),
                source_persona_id,
                project_id_opt.as_ref(),
            )
            .await
            .map_err(|error| error.to_string())?;
        }
        // ClickUp ticket-start resolution requires a Project context; standalone
        // starts never look up or link a ticket.
        let validated_clickup_task = if let Some(lookup_key) =
            project
                .as_ref()
                .and(clickup_task_lookup_key_from_references(
                    &composer_integration_references,
                )?) {
            let task = self
                .deps
                .state
                .clickup_integration_service
                .fetch_task(&lookup_key)
                .await?;
            let identity = clickup_identity_from_task(&task);
            ticket_branch_name_hint = Some(AgentConversationWorkspaceBranchNameHint {
                provider: "clickup".to_string(),
                ticket_token: identity.preferred_token(),
            });

            // should_create_workspace can only be true when context_type == Project
            // (see its derivation above), so `project` is guaranteed Some whenever
            // this branch runs.
            let should_auto_resolve_ticket_base = should_create_workspace
                && matches!(
                    mode,
                    AgentConversationWorkspaceMode::Edit
                        | AgentConversationWorkspaceMode::Plan
                        | AgentConversationWorkspaceMode::Ideation
                )
                && matches!(
                    base_ref_kind,
                    None | Some(IdeationAnalysisBaseRefKind::ProjectDefault)
                        | Some(IdeationAnalysisBaseRefKind::CurrentBranch)
                );
            if should_auto_resolve_ticket_base {
                let project = project
                    .as_ref()
                    .expect("should_create_workspace implies a Project context");
                match resolve_clickup_ticket_start(
                    &identity,
                    std::path::Path::new(&project.working_directory),
                    self.deps.state.github_service.as_deref(),
                )
                .await?
                {
                    ClickUpTicketStartResolution::NoMatch => {}
                    ClickUpTicketStartResolution::Unique(candidate) => {
                        base_ref_kind = Some(IdeationAnalysisBaseRefKind::LocalBranch);
                        base_branch_mode = Some(AgentConversationWorkspaceBranchMode::Linked);
                        base_ref = Some(candidate.branch_name.clone());
                        base_display_name = Some(format!(
                            "ClickUp {} ({})",
                            identity.preferred_token(),
                            candidate.branch_name
                        ));
                        source_pull_request = candidate.pull_request.map(|pull_request| {
                            AgentWorkspaceSourcePullRequest {
                                number: pull_request.number,
                                url: Some(pull_request.url),
                                title: Some(pull_request.title),
                                head_ref_name: pull_request.head_ref_name,
                                base_ref_name: Some(pull_request.base_ref_name),
                                head_ref_oid: pull_request.head_ref_oid,
                            }
                        });
                    }
                    ClickUpTicketStartResolution::Ambiguous { branch_names } => {
                        return Err(format!(
                            "ClickUp task {} matches multiple open PRs or branches ({}); select the intended branch explicitly",
                            identity.preferred_token(),
                            branch_names.join(", ")
                        ));
                    }
                }
            }
            Some(task)
        } else {
            None
        };

        if let Some(persona_id) = persona_id.as_ref() {
            if let Some(conversation_id) = draft_conversation_id.as_ref() {
                let existing = self
                    .deps
                    .state
                    .chat_conversation_repo
                    .get_by_id(conversation_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Conversation not found: {conversation_id}"))?;
                ensure_persona_binding_project_context(existing.context_type)
                    .map_err(|error| error.to_string())?;
            } else {
                ensure_persona_binding_project_context(context_type)
                    .map_err(|error| error.to_string())?;
            }

            // The context check above already rejects any context_type != Project,
            // so a Project's project_id is guaranteed present here.
            let persona_project_id = project_id_opt
                .as_ref()
                .expect("persona binding validated Project context above");
            PersonaService::new(
                self.deps.state.db.clone(),
                Arc::clone(&self.deps.state.persona_repo),
                Arc::clone(&self.deps.state.chat_conversation_repo),
            )
            .ensure_bindable(agent_personas_enabled(), persona_id, persona_project_id)
            .await
            .map_err(|error| error.to_string())?;
        }

        if should_create_workspace {
            // should_create_workspace implies context_type == Project (derivation
            // above), so project_id_opt is guaranteed Some here.
            let workspace_project_id = project_id_opt
                .as_ref()
                .expect("should_create_workspace implies a Project context");
            if let Err(error) = ensure_linked_branch_workspace_available(
                self.deps.state,
                workspace_project_id,
                draft_conversation_id.as_ref(),
                base_branch_mode,
                base_ref.as_deref(),
                source_pull_request.as_ref(),
            )
            .await
            {
                if let Some(conversation_id) = draft_conversation_id.as_ref() {
                    if let Err(archive_error) = archive_supplied_seeded_draft_after_setup_failure(
                        self.deps.state,
                        &context_log_id,
                        conversation_id,
                    )
                    .await
                    {
                        return Err(linked_setup_failure_error(format!(
                            "{error}; failed to archive failed draft: {archive_error}",
                        )));
                    }
                }
                return Err(linked_setup_failure_error(error));
            }
        }
        source_pull_request = if let Some(project) = project.as_ref() {
            hydrate_linked_branch_source_pull_request(
                self.deps.state,
                project,
                base_branch_mode,
                base_ref.as_deref(),
                source_pull_request,
            )
            .await?
        } else {
            source_pull_request
        };

        Ok(ProjectSetupOutput {
            project,
            validated_clickup_task,
            base_ref_kind,
            base_branch_mode,
            base_ref,
            base_display_name,
            ticket_branch_name_hint,
            source_pull_request,
        })
    }
}
