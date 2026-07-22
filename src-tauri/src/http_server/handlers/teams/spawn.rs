use super::*;

pub async fn request_teammate_spawn(
    State(state): State<HttpServerState>,
    Json(req): Json<RequestTeammateSpawnRequest>,
) -> Result<Json<RequestTeammateSpawnResponse>, (StatusCode, String)> {
    info!(
        role = %req.role,
        model = %req.model,
        tool_count = req.tools.len(),
        mcp_tool_count = req.mcp_tools.len(),
        "Teammate spawn requested — validating and spawning"
    );

    // 1. Validate against constraints (use "ideation" as default process)
    let config = team_constraints_config();
    let constraints = get_team_constraints(config, "ideation");

    // Validate individual teammate spawn request
    let spawn_req = TeammateSpawnRequest {
        role: req.role.clone(),
        prompt: Some(req.prompt.clone()),
        preset: req.preset.clone(),
        tools: req.tools.clone(),
        mcp_tools: req.mcp_tools.clone(),
        model: req.model.clone(),
        prompt_summary: None,
    };

    // Validate as a single-teammate plan
    let _approved = validate_team_plan(&constraints, "ideation", &[spawn_req]).map_err(|e| {
        warn!(role = %req.role, error = %e, "Teammate spawn validation failed");
        (
            StatusCode::BAD_REQUEST,
            format!("Spawn validation failed: {e}"),
        )
    })?;

    // 2. Find the active team
    let (team_name, context_id, context_type) = find_active_team(&state).await.map_err(|e| {
        error!(error = %e, "No active team found for teammate spawn");
        (StatusCode::CONFLICT, e)
    })?;
    ensure_team_mode_supported_for_context(&state, &context_type, &context_id).await?;

    // Resolve working directory (worktree-aware for task contexts)
    let working_dir = resolve_teammate_working_dir(&state, &context_type, &context_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to resolve teammate working directory");
            (StatusCode::CONFLICT, e)
        })?;

    // 3. Generate unique teammate name (add suffix for uniqueness)
    let teammate_name = generate_unique_teammate_name(&state, &team_name, &req.role).await;
    let teammate_color = assign_teammate_color(&state, &team_name).await;

    // 4. Check teammate count against constraint max
    let current_count = state
        .team_service
        .get_teammate_count(&team_name)
        .await
        .unwrap_or(0);
    if current_count >= constraints.max_teammates as usize {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Max teammates exceeded: {current_count} >= {}",
                constraints.max_teammates
            ),
        ));
    }

    // 5. Register teammate via TeamService (persists to DB + emits events)
    state
        .team_service
        .add_teammate(
            &team_name,
            &teammate_name,
            &teammate_color,
            &req.model,
            &req.role,
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to register teammate");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    // 6. Build spawn config and spawn the process.
    //    Resolve lead session ID from Claude Code team config, falling back to context_id.
    let source;
    let parent_session_id = if let Some(sid) = resolve_lead_session_from_config(&team_name) {
        source = "team_config_file";
        sid
    } else {
        source = "context_id_fallback";
        context_id.clone()
    };

    tracing::info!(
        parent_session_id = %parent_session_id,
        source = %source,
        context_id = %context_id,
        team = %team_name,
        "[TEAM_SPAWN] Resolved parent_session_id for teammate (ad-hoc flow)"
    );

    // Resolve project ID for RALPHX_PROJECT_ID env var on teammates
    let project_id = resolve_teammate_project_id(&state, &context_type, &context_id).await;

    let teammate_context = TeammateContext {
        context_id: context_id.clone(),
        context_type: context_type.clone(),
        project_id: project_id.clone(),
    };

    // Always inject team communication tools regardless of spawn request specification.
    // Without these, teammates cannot send messages or coordinate via task lists.
    let mut tools = req.tools.clone();
    for required in [
        "SendMessage",
        "TaskCreate",
        "TaskUpdate",
        "TaskList",
        "TaskGet",
    ] {
        if !tools.contains(&required.to_string()) {
            tools.push(required.to_string());
        }
    }

    let mcp_type = req.preset.as_deref().unwrap_or("ideation-team-member");

    let spawn_config = TeammateSpawnConfig::new(&teammate_name, &team_name, &req.prompt)
        .with_parent_session_id(&parent_session_id)
        .with_context(teammate_context)
        .with_model(&req.model)
        .with_tools(tools)
        .with_mcp_tools(req.mcp_tools.clone())
        .with_color(&teammate_color)
        .with_mcp_agent_type(mcp_type)
        .with_effort(resolve_effort(Some(mcp_type)))
        .with_working_dir(working_dir.clone())
        .with_plugin_dir(resolve_teammate_plugin_dir(&working_dir));
    let mut spawn_config = load_claude_provider_env_for_teammate_spawn(
        &state.app_state.agent_provider_settings_repo,
        spawn_config,
    )
    .await?;
    let mcp_policy = state
        .app_state
        .mcp_policy_service()
        .resolve_launch_policy(
            AgentHarnessKind::Claude,
            project_id.as_deref(),
            Some(&working_dir),
        )
        .await
        .map_err(|error| (StatusCode::CONFLICT, error.to_string()))?;
    spawn_config = spawn_config.with_mcp_launch_policy(mcp_policy);

    let client = ClaudeCodeClient::new();
    match client.spawn_teammate_interactive(spawn_config).await {
        Ok(spawn_result) => {
            info!(
                teammate = %teammate_name,
                team = %team_name,
                pid = ?spawn_result.child.id(),
                "Teammate process spawned successfully"
            );

            // 7. Take stdout/stderr from child for stream processing, then create handle
            let child_pid = spawn_result.child.id();
            let mut child = spawn_result.child;
            let stdout = child.stdout.take();

            // Drain stderr in background to capture crash messages without deadlock.
            // Logs any output at error level when the process exits.
            if let Some(stderr) = child.stderr.take() {
                let name = teammate_name.clone();
                let team = team_name.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncBufReadExt, BufReader};
                    let mut lines = BufReader::new(stderr).lines();
                    let mut output = Vec::new();
                    while let Ok(Some(line)) = lines.next_line().await {
                        output.push(line);
                    }
                    if !output.is_empty() {
                        tracing::error!(
                            teammate = %name,
                            team = %team,
                            stderr = %output.join("\n"),
                            "Teammate process stderr (crash/MCP init error)"
                        );
                    }
                });
            }

            // Process monitor: owns child, signals stream processor when Claude exits.
            // Prevents 3600s timeout when a grandchild (e.g., Node.js MCP server) inherits
            // the stdout pipe and holds it open after Claude exits.
            let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
            let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<()>();
            {
                let monitor_name = teammate_name.clone();
                let monitor_team = team_name.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        biased;
                        _ = kill_rx => {
                            tracing::info!(
                                teammate = %monitor_name,
                                team = %monitor_team,
                                "Teammate process kill signal received"
                            );
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                        }
                        status = child.wait() => {
                            tracing::info!(
                                teammate = %monitor_name,
                                team = %monitor_team,
                                status = ?status,
                                "Teammate process exited naturally"
                            );
                        }
                    }
                    // Signal stream processor to stop (pipe inheritance guard)
                    let _ = exit_tx.send(());
                });
            }

            // 8. Start background stream processor if we have both stdout and app_handle
            let stream_task = match (stdout, &state.app_state.app_handle) {
                (Some(stdout), Some(app_handle)) => Some(
                    crate::application::team_stream_processor::start_teammate_stream(
                        stdout,
                        exit_rx,
                        team_name.clone(),
                        teammate_name.clone(),
                        "ideation".to_string(),
                        context_id.clone(),
                        app_handle.clone(),
                        std::sync::Arc::new(state.team_tracker.clone()),
                        Some(state.team_service.clone()),
                        Some(std::sync::Arc::clone(
                            &state.app_state.chat_conversation_repo,
                        )),
                        Some(std::sync::Arc::clone(&state.app_state.chat_message_repo)),
                        Some(std::sync::Arc::clone(
                            &state.app_state.interactive_process_registry,
                        )),
                        Some(std::sync::Arc::clone(&state.execution_state)),
                    ),
                ),
                (None, _) => {
                    warn!(
                        teammate = %teammate_name,
                        "No stdout pipe available for teammate stream processing"
                    );
                    None
                }
                (_, None) => {
                    warn!(
                        teammate = %teammate_name,
                        "No AppHandle available for teammate event emission"
                    );
                    None
                }
            };

            // 9. Create TeammateHandle and register via TeamService
            let handle = TeammateHandle {
                kill_tx: Some(kill_tx),
                stream_task,
                stdin: Some(spawn_result.stdin),
                child_pid,
            };

            state
                .team_service
                .set_teammate_handle(&team_name, &teammate_name, handle)
                .await
                .map_err(|e| {
                    error!(error = %e, "Failed to set teammate handle");
                    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                })?;

            // 10. Update status to Running (via TeamService for persistence + events)
            let _ = state
                .team_service
                .update_teammate_status(&team_name, &teammate_name, TeammateStatus::Running)
                .await;

            Ok(Json(RequestTeammateSpawnResponse {
                success: true,
                message: format!(
                    "Teammate '{}' spawned for team '{}'",
                    teammate_name, team_name
                ),
                teammate_name,
            }))
        }
        Err(e) => {
            // Spawn failed — update status and return error
            warn!(
                teammate = %teammate_name,
                team = %team_name,
                error = %e,
                "Teammate spawn failed"
            );

            let _ = state
                .team_service
                .update_teammate_status(&team_name, &teammate_name, TeammateStatus::Failed)
                .await;

            crate::http_server::emit_http_event(
                &state,
                "team:teammate_spawn_failed",
                serde_json::json!({
                    "team_name": team_name,
                    "teammate_name": teammate_name,
                    "error": e.to_string(),
                }),
            );

            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Teammate spawn failed: {e}"),
            ))
        }
    }
}
