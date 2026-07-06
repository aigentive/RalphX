use super::*;

/// Result returned by `execute_team_spawn` — consumed by approve callers to
/// build their HTTP responses.
pub(super) struct TeamSpawnResult {
    pub(super) team_name: String,
    pub(super) spawned_teammates: Vec<SpawnedTeammateInfo>,
    pub(super) spawned_count: usize,
    pub(super) message: String,
}

/// Shared spawn logic — called by both auto-approve and manual approve paths.
///
/// Receives an already-taken `PendingTeamPlan` (removed from the tracker) and
/// executes team creation, working directory resolution, the full spawn loop,
/// rollback on partial failure, and watch channel resolution.
///
/// Channel lifecycle:
/// - Success: resolves the watch channel (does NOT remove it — Phase 2 does that)
/// - Failure/rollback: removes the channel so Phase 2 gets a fast 404 instead of
///   hanging for 840s
///
/// Context fields (`context_type`, `context_id`) are taken from `plan` — the stored
/// plan is the single source of truth for both auto-approve and manual approve paths.
pub(super) async fn execute_team_spawn(
    state: &HttpServerState,
    plan: PendingTeamPlan,
    plan_id: &str,
) -> Result<TeamSpawnResult, (StatusCode, String)> {
    ensure_team_mode_supported_for_context(state, &plan.context_type, &plan.context_id).await?;

    // 1. Create team (or find existing) — via TeamService for DB persistence + events
    // team_name comes from the lead agent's TeamCreate call (required field).
    let team_name = plan.team_name.clone();
    let team_exists = state.team_service.team_exists(&team_name).await;
    if !team_exists {
        state
            .team_service
            .create_team(&team_name, &plan.context_id, &plan.context_type)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to create team");
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            })?;
    }

    // 2. Resolve the working directory (worktree-aware for task contexts)
    let working_dir = resolve_teammate_working_dir(state, &plan.context_type, &plan.context_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to resolve teammate working directory");
            (StatusCode::CONFLICT, e)
        })?;
    info!(
        plan_id = %plan_id,
        context_type = %plan.context_type,
        context_id = %plan.context_id,
        working_dir = %working_dir.display(),
        "Resolved teammate working directory"
    );

    // 2b. Resolve project ID for RALPHX_PROJECT_ID env var on teammates
    let project_id = resolve_teammate_project_id(state, &plan.context_type, &plan.context_id).await;

    // 3. Register, spawn, and stream each teammate as a separate CLI process.
    //    This is the ONLY place where teammate worker processes are created.
    //    The lead agent's Task tool creates in-process subagents within its own
    //    Claude session, but these separate CLI processes are what the frontend tracks.
    let mut spawned_teammates: Vec<SpawnedTeammateInfo> = Vec::new();
    let mut spawn_error: Option<String> = None;

    'spawn_loop: for pending in &plan.teammates {
        let teammate_name = generate_unique_teammate_name(state, &team_name, &pending.role).await;
        let teammate_color = assign_teammate_color(state, &team_name).await;

        // Register teammate in DB via TeamService (persistence + events)
        let _ = state
            .team_service
            .add_teammate(
                &team_name,
                &teammate_name,
                &teammate_color,
                &pending.model,
                &pending.role,
            )
            .await;

        // Derive MCP agent type from process: worker-* processes use worker-team-member,
        // all others use ideation-team-member (the default in TeammateSpawnConfig::new).
        // Prefer the preset field when available — it carries the specific agent name from
        // the team lead's spawn request (e.g. "ralphx-ideation-specialist-backend").
        let mcp_type = if plan.process.starts_with("worker") {
            pending.preset.as_deref().unwrap_or("worker-team-member")
        } else {
            pending.preset.as_deref().unwrap_or("ideation-team-member")
        };

        // Use the lead agent's session ID passed through the MCP flow.
        // Fallback chain: MCP env var → Claude Code team config file → context_id.
        let source;
        let parent_session_id = if let Some(ref sid) = plan.lead_session_id {
            source = "mcp_env_var";
            sid.clone()
        } else if let Some(sid) = resolve_lead_session_from_config(&team_name) {
            source = "team_config_file";
            sid
        } else {
            source = "context_id_fallback";
            plan.context_id.clone()
        };

        tracing::info!(
            parent_session_id = %parent_session_id,
            source = %source,
            lead_session_id_from_mcp = ?plan.lead_session_id,
            context_id = %plan.context_id,
            team = %team_name,
            "[TEAM_SPAWN] Resolved parent_session_id for teammate (plan flow)"
        );

        // Build RalphX session context (separate from parent_session_id)
        let teammate_context = TeammateContext {
            context_id: plan.context_id.clone(),
            context_type: plan.context_type.clone(),
            project_id: project_id.clone(),
        };

        // Spawn an interactive CLI worker process for this teammate
        // (uses spawn_teammate_interactive for piped stdin — keeps process alive for messaging)

        // Always inject team communication tools regardless of plan specification.
        // Without these, teammates cannot send messages or coordinate via task lists.
        let mut tools = pending.tools.clone();
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

        let spawn_config = TeammateSpawnConfig::new(&teammate_name, &team_name, &pending.prompt)
            .with_parent_session_id(&parent_session_id)
            .with_context(teammate_context)
            .with_model(&pending.model)
            .with_tools(tools)
            .with_mcp_tools(pending.mcp_tools.clone())
            .with_color(&teammate_color)
            .with_mcp_agent_type(mcp_type)
            .with_effort(resolve_effort(Some(mcp_type)))
            .with_working_dir(working_dir.clone())
            .with_plugin_dir(resolve_teammate_plugin_dir(&working_dir));
        let spawn_config = load_claude_provider_env_for_teammate_spawn(
            &state.app_state.agent_provider_settings_repo,
            spawn_config,
        )
        .await?;

        let client = ClaudeCodeClient::new();
        match client.spawn_teammate_interactive(spawn_config).await {
            Ok(spawn_result) => {
                info!(
                    teammate = %teammate_name,
                    team = %team_name,
                    pid = ?spawn_result.child.id(),
                    "Teammate worker process spawned in execute_team_spawn"
                );

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

                // Start background stream processor for teammate stdout
                let stream_task = match (stdout, &state.app_state.app_handle) {
                    (Some(stdout), Some(app_handle)) => Some(
                        crate::application::team_stream_processor::start_teammate_stream(
                            stdout,
                            exit_rx,
                            team_name.clone(),
                            teammate_name.clone(),
                            plan.context_type.clone(),
                            plan.context_id.clone(),
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
                    _ => {
                        warn!(
                            teammate = %teammate_name,
                            "No stdout/app_handle for teammate stream processing"
                        );
                        None
                    }
                };

                let handle = TeammateHandle {
                    kill_tx: Some(kill_tx),
                    stream_task,
                    stdin: Some(spawn_result.stdin),
                    child_pid,
                };

                let _ = state
                    .team_service
                    .set_teammate_handle(&team_name, &teammate_name, handle)
                    .await;

                let _ = state
                    .team_service
                    .update_teammate_status(&team_name, &teammate_name, TeammateStatus::Running)
                    .await;
            }
            Err(e) => {
                error!(
                    teammate = %teammate_name,
                    team = %team_name,
                    error = %e,
                    "Failed to spawn teammate worker process — rolling back already-spawned teammates"
                );

                let _ = state
                    .team_service
                    .update_teammate_status(&team_name, &teammate_name, TeammateStatus::Failed)
                    .await;

                crate::http_server::emit_http_event(
                    state,
                    "team:teammate_spawn_failed",
                    serde_json::json!({
                        "team_name": team_name,
                        "teammate_name": teammate_name,
                        "error": e.to_string(),
                    }),
                );

                spawn_error = Some(e.to_string());
                break 'spawn_loop;
            }
        }

        spawned_teammates.push(SpawnedTeammateInfo {
            name: teammate_name,
            role: pending.role.clone(),
            model: pending.model.clone(),
            color: teammate_color,
        });
    }

    // Partial spawn rollback: if any spawn failed, kill already-spawned teammates.
    // Also removes the watch channel so Phase 2 gets a fast 404 instead of hanging.
    if let Some(err) = spawn_error {
        error!(
            plan_id = %plan_id,
            team = %team_name,
            already_spawned = spawned_teammates.len(),
            "Rolling back partial spawn due to failure"
        );
        for t in &spawned_teammates {
            let _ = state.team_service.stop_teammate(&team_name, &t.name).await;
        }
        state.team_tracker.remove_plan_channel(plan_id).await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Teammate spawn failed: {err}"),
        ));
    }

    let spawned_count = spawned_teammates.len();
    let total_count = plan.teammates.len();

    info!(
        plan_id = %plan_id,
        team = %team_name,
        spawned = spawned_count,
        total = total_count,
        "Team plan approved — teammates spawned as CLI worker processes"
    );

    // Signal the blocking request_team_plan handler with the spawn results.
    // Success path: resolve the channel but do NOT remove it — Phase 2 removes it
    // after receiving the decision via `await_team_plan`.
    let decision_teammates: Vec<PlanDecisionTeammate> = spawned_teammates
        .iter()
        .map(|t| PlanDecisionTeammate {
            name: t.name.clone(),
            role: t.role.clone(),
            model: t.model.clone(),
            color: t.color.clone(),
        })
        .collect();

    let message = format!(
        "{}/{} teammates registered successfully",
        spawned_count, total_count
    );

    state
        .team_tracker
        .resolve_plan(
            plan_id,
            PlanDecision {
                approved: spawned_count > 0,
                team_name: Some(team_name.clone()),
                teammates_spawned: decision_teammates,
                message: message.clone(),
            },
        )
        .await;

    Ok(TeamSpawnResult {
        team_name,
        spawned_teammates,
        spawned_count,
        message,
    })
}
