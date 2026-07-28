# PR 3.1 — Facade coverage census (P-11 gap work manifest)

> GENERATED — do not edit by hand. Regenerate: `node scripts/generate-remote-coverage-census.mjs`. Staleness gate: `--check`.
> This is the PR 3.1-a planning artifact. It registers nothing. Every class here is the ledger's CURRENT value; the per-command hand audit (§3.3) and the P-17 detector run own the final one.

## 1. Scan state

```
PASS: remote transport drift — 498 invoke command name(s), 0 dynamic, 0 seam bypasses; 435 unclassified (baseline, → 0 in PR 3.1).
```

| Measure | Count | Source |
|---|---|---|
| Invoke command names in `frontend/src` | 498 | drift scan (AST) |
| Dynamic / unresolvable expressions | 0 | drift scan — must stay 0 |
| Transport seam bypasses | 0 | drift scan — must stay 0 |
| Remote-registered (`remote_commands!`) | 34 | `docs/generated/remote-commands.json` |
| Reason-coded local-only rows | 29 | `frontend/src/lib/remote/local-only-commands.ts` |
| Ledger rows (exhaustive over `generate_handler!`) | 540 | `docs/generated/remote-commands.json` |
| **Unclassified — the 3.1 gap** | **435** | `scripts/remote-transport-drift-baseline.json` |

## 2. What the gap is made of

The gap is not 447 registrations. Routing each name mechanically through the ledger splits it into four very different kinds of work:

| Disposition | Count | Rule |
|---|---|---|
| register-candidate | 276 | ledgered AgentControl (or lower) with no SpawnsProcess capability — eligible for a hand-audited `remote_commands!` entry under `ui:agent` |
| host-denied (class: denied) | 84 | `class_permits` returns false for Denied at any capability set — registering it fails compilation. Resolves for P-11 through the manifest, never through a local-only reason (phase doc key point 6) |
| host-denied (SpawnsProcess) | 49 | carries `SpawnsProcess`; `class_permits(AgentControl, [SpawnsProcess])` is false and Elevated is a v1 non-goal, so it is not exposable on the v1 facade at any scope (`remote_server/registry.rs` detector-(c) note) |
| v1-deferred (Elevated) | 26 | ledgered Elevated without SpawnsProcess — reachable only under `ui:elevated`, which §1 excludes from v1; deferred, not denied |
| orphan invoke (no local handler) | 0 | invoked by the frontend but absent from `generate_handler!` and from the ledger — it cannot be registered remotely because it does not exist locally either |

**159 of the 435 gap names can never be registered in v1** — they are host-side commands the facade denies or defers. They are not client-local either, so today's scan (registered OR local-only) has no way to classify them and P-11 cannot reach zero. Phase-doc key point 6 fixes the intended resolution: they resolve through the ledger rows the manifest renders. **That mechanism does not exist yet — it is batch B0, and it blocks every other batch's measurable progress.**

**276 names are registration candidates**, and `register-candidate` means eligible for a hand audit, not approved: detector (c) has already rejected ledgered-`AgentControl` commands whose process authority the manifest cannot see (`resume_task`, `apply_proposals_to_kanban`, `set_agent_conversation_workspace_auto_publish`). Expect a non-empty rejection subset in every registration batch.

## 3. Recommended batch order

| # | Batch | Title | Cmds | Register-candidates | Not registering | Modules |
|---|---|---|---|---|---|---|
| 1 | `B0` | P-11 third-disposition mechanism (prerequisite, no registrations) | 0 | 0 | 0 | 0 |
| 2 | `B1` | Task core — lifecycle, steps, execution, gates | 36 | 35 | 1 | 5 |
| 3 | `B2` | Chat + agent conversation surface (unblocks PR 3.2) | 56 | 53 | 3 | 6 |
| 4 | `B3` | Review, QA, merge pipeline, validation | 31 | 31 | 0 | 4 |
| 5 | `B4` | Ideation, plans, methodology, workflow | 64 | 63 | 1 | 5 |
| 6 | `B5` | Automation, research, metrics, activity | 36 | 34 | 2 | 4 |
| 7 | `B6` | Personas, role defaults, MCP policy, review settings | 28 | 27 | 1 | 4 |
| 8 | `B7` | Artifacts, task context, notifications, app chrome | 33 | 33 | 0 | 6 |
| 9 | `D1` | Credential + integration surface (disposition only, no registrations) | 72 | 0 | 72 | 8 |
| 10 | `D2` | Process-launch getters and git/gh surface (disposition only) | 60 | 0 | 60 | 8 |
| 11 | `R1` | `get_project` / `list_projects` — spawn-free read path | 2 | 0 | 2 | 1 |
| 12 | `D3` | Host chrome, terminal, repository settings, test data (disposition only) | 14 | 0 | 14 | 4 |
| 13 | `A1` | Chat attachments — disposition + remote rendering (deferred from 2.6/review-4) | 3 | 0 | 3 | 1 |
| 14 | `X1` | Orphan invokes — no local handler exists | 0 | 0 | 0 | 0 |

Ordering logic: **B0 first** (nothing is measurable without the third disposition) → **B1** (smallest parity risk, reuses 1.5-A's proven injection shapes) → **B2** (unblocks PR 3.2, which cannot start until chat send answers `REMOTE_FORBIDDEN` instead of `REMOTE_COMMAND_UNAVAILABLE`) → **B3–B7** registration batches by falling audit risk → **D1/D2/D3** disposition-only batches, which retire large blocks with zero registration risk and can run in parallel with any registration batch once B0 lands → **R1** (a code change, not a registration, and gated on an owner call) → **A1** (blocked on 1.5-C) → **X1** (live defects, independent of remote work).

## 4. Batches

### 1. `B0` — P-11 third-disposition mechanism (prerequisite, no registrations)

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Why here:** Today the drift scan classifies a name as remote-registered OR local-only. 159 of the 447 gap names are neither and never will be: they are host-side commands the facade denies (Denied class, SpawnsProcess) or defers (Elevated). Phase doc key point 6 fixes the intended resolution — those names resolve for P-11 'via the module-`Denied` rows the ledger renders into `remote-commands.json`', explicitly NOT via a client-local reason. The scan must learn to read the manifest as a third classification source before ANY batch can move the unclassified count to zero. Landing this first also makes every later batch's delta measurable.

**Work:**

- Extend `scripts/check-remote-transport-drift.mjs` to read `docs/generated/remote-commands.json` and treat a name as classified when its ledger row is host-denied (class `denied`, or any capability set `class_permits` rejects at v1 classes) — with the ledger row, not a name list, as the authority.
- Decide and encode the `Elevated`/v1-deferred disposition: same manifest path with a distinct reason, or an explicit deferred list that CI shrinks. Do not let it fall into `local-only-commands.ts` (key point 6).
- Add self-test detector cases: a manifest-denied name classifies; a name absent from every source still fails; a name that is BOTH registered and manifest-denied fails (that is a ledger/registry contradiction).
- Keep the ratchet: the baseline file may only shrink, and is deleted when the count reaches zero.

**Gate:** Scan self-test grows by the new detector cases; the PASS line reports the unclassified count falling by exactly the manifest-resolved set (expected −159 with no registrations).

### 2. `B1` — Task core — lifecycle, steps, execution, gates

**Commands:** 36 · **Register-candidates:** 35 · **Risk classes:** register-candidate 35 · host-denied (class: denied) 1

**Why here:** The 1.5-A surface already registered the neighbouring commands (`move_task`, `unblock_task`, `answer_user_question`, the brakes), so the injection table, the `authz:` predicate shape and the P-4 parity rows for these argument shapes are proven on this exact module family. Lowest parity risk, highest reuse — the right batch to shake out the per-batch harness before it meets 41-command modules.

**Work:**

- Hand-audit each command's downstream authority (detector (a) transitions, detector (b) spawn-triggering state writes, content-surface writes) and assign class + capability set in `capability_ledger.rs`.
- P-4 parity rows FIRST (flat args, struct-wrapped, camelCase, `Option`, error path) per C-11.
- Confirm the brakes in these modules stay `ui:operate` (A-14) and that no arming transition lands below the `AgentControl` floor.

**Gate:** P-17 suite green; P-17b generated scope entries exist for every new AgentControl member; C-9 dual-lens review recorded.

<details><summary>Members by module</summary>

- **`execution_commands`** (13) — `get_execution_settings`, `get_execution_status`, `get_global_execution_settings`, `get_running_processes`, `pause_execution`, `recover_task_execution`, `resolve_recovery_prompt`, `restart_task`, `resume_execution`, `set_active_project`, `stop_execution`, `update_execution_settings`, `update_global_execution_settings`
- **`permission_commands`** (2) — `get_pending_permissions`, `resolve_permission_request`
- **`question_commands`** (2) — `get_pending_questions`, `resolve_user_question`
- **`task_commands`** (12) — `archive_task`, `archive_tasks_in_group`, `cancel_tasks_in_group`, `cleanup_task`, `cleanup_tasks_in_group`, `pause_execution_plan`, `restore_task`, `resume_execution_plan`, `resume_task`, `resume_tasks_in_group`, `retry_branch_update`, `stop_execution_plan`
- **`task_step_commands`** (7) — `complete_step`, `fail_step`, `get_step_progress`, `get_task_steps`, `reorder_task_steps`, `skip_step`, `start_step`

</details>

### 3. `B2` — Chat + agent conversation surface (unblocks PR 3.2)

**Commands:** 56 · **Register-candidates:** 53 · **Risk classes:** register-candidate 53 · host-denied (class: denied) 3

**Why here:** PR 3.2's whole premise is that chat send paths answer `REMOTE_FORBIDDEN` without `ui:agent` rather than `REMOTE_COMMAND_UNAVAILABLE` — which requires them registered. 2.6 shipped the honest interim (composer renders UNAVAILABLE remotely) and its product note says it 'flips with no client change when 3.1 registers them'. This is the batch that flips it, so it must land before 3.2 starts. It is also the highest-risk batch: `send_message` is a detector-(a) steer sink and the module contains the workspace-publish `git push` surface that stays denied.

**Work:**

- Split the module by authority: the send/steer commands register as `AgentControl`; the publish/PR surface (`publish_agent_conversation_workspace`, `update_agent_conversation_workspace_from_base`, `close_agent_workspace_pr`) stays denied, and `set_agent_conversation_workspace_auto_publish` is an already-proven detector-(c) rejection.
- Verify per command that the process-launch sink sits BEYOND the steer-sink cut (`chat_service.send_message`) rather than inside the command's own closure — the cut is what makes chat send registerable while `resume_task` is not. Any command whose own closure resolves a CLI path is a detector-(c) rejection, not a registration.
- P-4 rows must cover `SendAgentMessageInput`'s optional/override fields (the `runtimeOverride` vs legacy-field rejection is an error-path parity row).

**Gate:** P-17 green; C-9 dual-lens review recorded; the five 2.6-surfaced ops resolve per this census's `resolvedItems.unregisteredUiAgentOps`.

<details><summary>Members by module</summary>

- **`agent_composer_commands`** (3) — `list_agent_composer_skills`, `search_agent_composer_entries`, `search_agent_composer_plan_references`
- **`agent_model_commands`** (3) — `delete_custom_agent_model`, `list_agent_models`, `upsert_custom_agent_model`
- **`agent_sidebar_commands`** (2) — `get_bulk_workspace_publication_states`, `list_agent_sidebar_conversations`
- **`conversation_folder_reference_commands`** (3) — `add_conversation_folder_reference`, `list_conversation_folder_references`, `remove_conversation_folder_reference`
- **`conversation_stats_commands`** (4) — `get_agent_conversation_stats`, `get_insights_chat_usage_stats`, `get_project_chat_usage_stats`, `get_task_chat_usage_stats`
- **`unified_chat_commands`** (41) — `abort_seeded_agent_conversation`, `archive_agent_conversation`, `close_agent_workspace_pr`, `commit_agent_conversation_workspace_locally`, `create_agent_conversation`, `delete_queued_agent_message`, `fork_agent_conversation`, `get_agent_conversation`, `get_agent_conversation_messages_page`, `get_agent_conversation_runtime_index`, `get_agent_conversation_runtime_statuses`, `get_agent_conversation_summary`, `get_agent_conversation_timeline_page`, `get_agent_conversation_workspace`, `get_agent_conversation_workspace_freshness`, `get_agent_message_tool_call_detail`, `get_agent_run_status_unified`, `get_agent_running_states`, `get_agent_timeline_item_tool_call_detail`, `get_queued_agent_messages`, `is_agent_running`, `is_chat_service_available`, `list_agent_conversation_workspace_publication_events`, `list_agent_conversation_workspaces_by_project`, `list_agent_conversations`, `list_agent_conversations_page`, `precompute_agent_conversation_workspace_pr_description`, `publish_agent_conversation_workspace`, `reconcile_agent_conversation_workspace_publication`, `restore_agent_conversation`, `send_agent_message`, `send_queued_agent_message_now`, `set_agent_conversation_workspace_auto_publish`, `set_agent_conversation_workspace_pr_supervision`, `start_agent_conversation`, `stop_agent`, `switch_agent_conversation_mode`, `switch_agent_conversation_persona`, `update_agent_conversation_coordination_mode`, `update_agent_conversation_title`, `update_agent_conversation_workspace_from_base`

</details>

### 4. `B3` — Review, QA, merge pipeline, validation

**Commands:** 31 · **Register-candidates:** 31 · **Risk classes:** register-candidate 31

**Why here:** Approval/review commands write the agent-consumed content surface (`MutatesAgentConsumedContent` already appears on 6 of them), which is exactly the capability whose floor P-17d enforces. Grouping them keeps that audit in one review rather than spread across batches.

**Work:**

- Confirm every content-surface writer keeps `MutatesAgentConsumedContent` and lands at or above the AgentControl floor.
- Check the merge-pipeline members against the destructive-git deny list (`cleanup_task_branch`, `resolve_merge_conflict` are Denied and must not ride in on module similarity).

**Gate:** P-17d floor diff clean; C-9 review recorded.

<details><summary>Members by module</summary>

- **`merge_pipeline_commands`** (3) — `get_merge_phase_list`, `get_merge_pipeline`, `get_merge_progress`
- **`qa_commands`** (6) — `get_qa_results`, `get_qa_settings`, `get_task_qa`, `retry_qa`, `skip_qa`, `update_qa_settings`
- **`review_commands`** (21) — `approve_fix_task`, `approve_review`, `get_fix_task_attempts`, `get_issue_progress`, `get_pending_reviews`, `get_review_by_id`, `get_review_settings`, `get_reviews_by_task_id`, `get_task_issues`, `get_task_state_history`, `mark_issue_addressed`, `mark_issue_in_progress`, `re_review_task_from_escalated`, `reject_fix_task`, `reject_review`, `reopen_issue`, `request_changes`, `request_task_changes_for_review`, `request_task_changes_from_reviewing`, `update_review_settings`, `verify_issue`
- **`validation_commands`** (1) — `get_task_validation_summary`

</details>

### 5. `B4` — Ideation, plans, methodology, workflow

**Commands:** 64 · **Register-candidates:** 63 · **Risk classes:** register-candidate 63 · host-denied (class: denied) 1

**Why here:** The largest single module in the gap (42). It is also where the known detector-(c) rejection `apply_proposals_to_kanban` lives, so the batch must be sized to absorb a mid-batch reclassification without stalling the others.

**Work:**

- Expect a non-empty detector-(c) rejection subset; record each rejection in the manifest disposition rather than downgrading the class.
- `delete_task_proposal` is Denied (deletesEntity) — it stays a manifest disposition inside this batch.

**Gate:** P-17 green; C-9 review recorded; rejected members appear as manifest dispositions, never as local-only rows.

<details><summary>Members by module</summary>

- **`agent_plan_commands`** (5) — `activate_agent_plan_direct_implementation`, `activate_agent_task_pipeline`, `copy_agent_conversation_plan`, `import_agent_conversation_plan`, `start_agent_task_pipeline`
- **`ideation_commands`** (42) — `analyze_dependencies`, `apply_proposals_to_kanban`, `archive_ideation_session`, `assess_all_priorities`, `assess_proposal_priority`, `create_cross_project_session`, `create_ideation_session`, `create_task_proposal`, `delete_task_proposal`, `export_ideation_session`, `get_agent_harness_availability`, `get_agent_lane_settings`, `get_blocked_tasks`, `get_child_sessions`, `get_ideation_agent_workspace`, `get_ideation_effort_settings`, `get_ideation_model_settings`, `get_ideation_session`, `get_ideation_session_with_data`, `get_ideation_settings`, `get_latest_child_session_id`, `get_proposal_dependencies`, `get_proposal_dependents`, `get_session_group_counts`, `get_task_blockers`, `get_task_proposal`, `get_tasks_disable_impact`, `import_ideation_session`, `list_ideation_sessions`, `list_session_proposals`, `list_sessions_by_group`, `remove_proposal_dependency`, `reopen_ideation_session`, `reorder_proposals`, `restart_ideation_implementation`, `set_tasks_feature_enabled`, `spawn_session_namer`, `update_agent_lane_settings`, `update_ideation_effort_settings`, `update_ideation_model_settings`, `update_ideation_session_title`, `update_ideation_settings`
- **`methodology_commands`** (4) — `activate_methodology`, `deactivate_methodology`, `get_active_methodology`, `get_methodologies`
- **`plan_commands`** (5) — `clear_active_plan`, `get_active_execution_plan`, `get_active_plan`, `list_plan_selector_candidates`, `set_active_plan`
- **`workflow_commands`** (8) — `create_workflow`, `get_active_workflow_columns`, `get_builtin_workflows`, `get_workflow`, `get_workflows`, `seed_builtin_workflows`, `set_default_workflow`, `update_workflow`

</details>

### 6. `B5` — Automation, research, metrics, activity

**Commands:** 36 · **Register-candidates:** 34 · **Risk classes:** register-candidate 34 · host-denied (class: denied) 2

**Why here:** Automation run/restart are two of the five 2.6-surfaced ops; the rest are read-shaped commands that were swept to the conservative module default and are the cheapest reclassification wins in the gap.

**Work:**

- Re-audit the conservative-module-default rows: a genuinely inert read here may drop to `Read`/`Operate`, but only with sink evidence — the floor may not be undershot.
- `trigger_automation_run_now` / `restart_automation` route through the scheduler seam; confirm the arming-transition targets are visible to detector (a) before assigning.

**Gate:** P-17 green; C-9 review recorded.

<details><summary>Members by module</summary>

- **`activity_commands`** (5) — `count_session_activity_events`, `count_task_activity_events`, `list_all_activity_events`, `list_session_activity_events`, `list_task_activity_events`
- **`automation_commands`** (15) — `cancel_automation_run`, `create_automation_draft`, `delete_automation`, `delete_automation_run`, `get_automation`, `list_automations`, `pause_automation`, `restart_automation`, `resume_automation_run`, `retry_automation_judge`, `retry_automation_plan_judge`, `skip_automation_judge`, `stop_automation`, `trigger_automation_run_now`, `update_automation_settings`
- **`metrics_commands`** (9) — `get_insights_pr_insights`, `get_insights_stats`, `get_insights_trends`, `get_metrics_config`, `get_project_pr_insights`, `get_project_stats`, `get_project_trends`, `get_task_metrics`, `save_metrics_config`
- **`research_commands`** (7) — `get_research_presets`, `get_research_process`, `get_research_processes`, `pause_research`, `resume_research`, `start_research`, `stop_research`

</details>

### 7. `B6` — Personas, role defaults, MCP policy, review settings

**Commands:** 28 · **Register-candidates:** 27 · **Risk classes:** register-candidate 27 · host-denied (class: denied) 1

**Why here:** Configuration-of-future-authority shapes cluster here: a persona/role/policy write does not act now but changes what a later spawn is allowed to do. This is the `update_custom_analysis` family of risk (§3.3 backstop-1 residual), so it gets one focused dual-lens review instead of being sprinkled across batches.

**Work:**

- For each command ask the deferred-authority question explicitly: does this write change what a FUTURE agent process may do? If yes it is at least `AgentControl` with `ConfiguresFutureProcessAuthority`, regardless of how inert the immediate action looks.
- `delete_persona`-shaped members stay Denied (deletesEntity).

**Gate:** P-17 green; C-9 review recorded with the deferred-authority lens explicitly exercised.

<details><summary>Members by module</summary>

- **`manual_role_default_commands`** (6) — `clear_manual_role_default`, `get_agent_conversation_role_default`, `get_manual_role_defaults`, `get_start_composer_role_default`, `reset_agent_conversation_role_default`, `update_manual_role_default`
- **`mcp_policy_commands`** (7) — `clear_mcp_server_override`, `clear_mcp_tool_override`, `get_mcp_catalog`, `refresh_mcp_catalog`, `retry_legacy_mcp_registration_repair`, `update_mcp_server_override`, `update_mcp_tool_override`
- **`persona_commands`** (13) — `approve_persona`, `approve_persona_as_new`, `archive_persona`, `create_persona_draft`, `delete_persona_draft`, `get_persona`, `list_persona_usage`, `list_personas`, `preview_persona_overlay`, `reseed_persona_draft`, `unarchive_persona`, `update_persona`, `update_persona_draft`
- **`workspace_review_settings_commands`** (2) — `get_workspace_review_runtime_settings`, `update_workspace_review_runtime_settings`

</details>

### 8. `B7` — Artifacts, task context, notifications, app chrome

**Commands:** 33 · **Register-candidates:** 33 · **Risk classes:** register-candidate 33

**Why here:** The tail. Mixed reads and small writes; also the batch that must decide which names are genuinely CLIENT-LOCAL (updater channel, window/dock chrome) and therefore belong in `local-only-commands.ts` with an honest reason — the only batch expected to add local-only rows.

**Work:**

- Split client-local from host-owned per command: `update_channel_commands` and parts of `ui_commands` are plausible `local-only` rows; artifacts and task context are host state and must register or be manifest-disposed.
- `get_task_context` and the prompt-builder reads are content-surface members (ledger-soundness round found 5 dropped worker content reads) — re-check the surface enumeration before assigning.
- Every local-only row gets an honest client-local reason; 'hard to classify' is never valid.

**Gate:** P-17 green; every new local-only row has a reason; C-9 review recorded.

<details><summary>Members by module</summary>

- **`artifact_commands`** (11) — `archive_artifact`, `create_bucket`, `get_artifact`, `get_artifact_at_version`, `get_artifact_relations`, `get_artifact_version_history`, `get_artifacts`, `get_artifacts_by_bucket`, `get_artifacts_by_task`, `get_buckets`, `get_system_buckets`
- **`notification_commands`** (8) — `get_notification_settings`, `get_unread_notification_count`, `list_attention_items`, `list_notifications`, `mark_all_notifications_read`, `mark_notification_read`, `set_dock_badge_count`, `update_notification_settings`
- **`release_notes_commands`** (5) — `get_current_release_notes`, `get_last_seen_release_notes_version`, `get_release_notes_for_version`, `list_release_notes_versions`, `mark_release_notes_seen`
- **`task_context_commands`** (5) — `get_artifact_full`, `get_artifact_version`, `get_related_artifacts`, `get_task_context`, `search_artifacts`
- **`ui_commands`** (2) — `get_ui_feature_flags`, `update_ui_feature_flags`
- **`update_channel_commands`** (2) — `get_update_channel`, `set_update_channel`

</details>

### 9. `D1` — Credential + integration surface (disposition only, no registrations)

**Commands:** 72 · **Register-candidates:** 0 · **Risk classes:** v1-deferred (Elevated) 20 · host-denied (class: denied) 52

**Why here:** Every member is `TouchesCredentials` or `ConfiguresFutureProcessAuthority`. API-key management is compile-denied from the facade (§4.3) and the integration-settings saves are the round-3 module deny list. Nothing here registers in v1; the entire batch is manifest disposition, so it is pure throughput once B0 lands — 72 names retired with zero registration risk.

**Work:**

- Confirm each ledger row already carries the denying capability; add missing rows rather than adding local-only reasons.
- The ticketing reads are Elevated-not-Denied (they read a credentialed provider): decide once, for the whole module, whether v1 defers them or the reads split from the writes in a later phase. Record the decision in the ledger reason.

**Gate:** Manifest regenerated and diff-clean; unclassified count drops by exactly this batch's size; zero new local-only rows.

<details><summary>Members by module</summary>

- **`api_key_commands`** (7) — `create_api_key`, `get_api_key_audit_log`, `list_api_keys`, `revoke_api_key`, `rotate_api_key`, `update_api_key_permissions`, `update_api_key_projects`
- **`atlassian_commands`** (15) — `assign_agent_conversation_jira_issue`, `assign_agent_conversation_jira_issue_to_me`, `build_atlassian_oauth_authorization_url`, `clear_agent_conversation_jira_issue`, `complete_atlassian_oauth_local_callback`, `disconnect_atlassian_integration`, `exchange_atlassian_oauth_code`, `get_agent_conversation_jira_issue`, `get_atlassian_integration_settings`, `refresh_agent_conversation_jira_issue`, `resolve_atlassian_resource_urls`, `save_atlassian_integration_settings`, `search_atlassian_resources`, `start_atlassian_oauth_local_callback`, `validate_atlassian_integration`
- **`clickup_commands`** (6) — `disconnect_clickup_integration`, `get_clickup_integration_settings`, `list_clickup_workspaces`, `save_clickup_integration_settings`, `search_clickup_tasks`, `validate_clickup_integration`
- **`external_mcp_commands`** (2) — `get_external_mcp_config`, `update_external_mcp_config`
- **`granola_commands`** (9) — `assign_agent_conversation_granola_note`, `clear_agent_conversation_granola_note`, `get_agent_conversation_granola_note`, `get_granola_integration_settings`, `get_granola_note_detail`, `list_granola_notes`, `refresh_agent_conversation_granola_note`, `save_granola_integration_settings`, `validate_granola_integration_settings`
- **`harness_provider_commands`** (2) — `get_agent_provider_settings`, `update_agent_provider_settings`
- **`linear_commands`** (11) — `assign_agent_conversation_linear_issue`, `clear_agent_conversation_linear_issue`, `disconnect_linear_integration`, `get_agent_conversation_linear_issue`, `get_linear_integration_settings`, `get_linear_webhook_config`, `refresh_agent_conversation_linear_issue`, `save_linear_integration_settings`, `save_linear_webhook_signing_secret`, `search_linear_issues`, `validate_linear_integration`
- **`ticketing_commands`** (20) — `add_ticket_comment`, `assign_ticket`, `clear_ticket_assignee`, `get_conversation_ticket`, `get_ticket_associations`, `get_ticket_detail`, `list_ticket_filter_options`, `list_ticket_labels`, `list_ticket_transitions`, `list_ticketing_columns`, `list_ticketing_containers`, `list_ticketing_providers`, `list_ticketing_status_catalog`, `list_tickets`, `refresh_ticketing_status_catalog`, `refresh_tickets`, `set_ticket_labels`, `start_ralphx_work_from_ticket`, `transition_ticket_status`, `update_ticketing_status_presentation`

</details>

### 10. `D2` — Process-launch getters and git/gh surface (disposition only)

**Commands:** 60 · **Register-candidates:** 0 · **Risk classes:** host-denied (SpawnsProcess) 47 · host-denied (class: denied) 13

**Why here:** The 'getter that shells out' family plus the destructive-git and installer surfaces. `SpawnsProcess` is not exposable at any v1 scope, so these are dispositions, not registrations. `get_project`/`list_projects` are carved out into R1 because they are the one case where the spawn is removable rather than inherent.

**Work:**

- Verify each row carries `SpawnsProcess` (detector (c) is the floor: a Read/Operate row reaching a launch sink fails CI).
- `get_task_file_changes` / `get_file_diff` / `get_codex_cli_diagnostics` are the named getter-spawns — they stay denied even though they read like reads.

**Gate:** Manifest diff-clean; detector-(c) floor test green; unclassified count drops by exactly this batch's size.

<details><summary>Members by module</summary>

- **`agent_issue_report_commands`** (2) — `build_agent_issue_report`, `submit_agent_issue_report`
- **`diff_commands`** (27) — `detect_merge_conflicts`, `get_agent_conversation_workspace_change_summary`, `get_agent_conversation_workspace_commit_file_changes`, `get_agent_conversation_workspace_commit_file_diff`, `get_agent_conversation_workspace_commits`, `get_agent_conversation_workspace_cumulative_file_changes`, `get_agent_conversation_workspace_cumulative_file_diff`, `get_agent_conversation_workspace_file_changes`, `get_agent_conversation_workspace_file_diff`, `get_agent_conversation_workspace_pr_annotations`, `get_agent_conversation_workspace_repair_change_summary`, `get_agent_conversation_workspace_repair_conflict_file_diff`, `get_agent_conversation_workspace_repair_staged_file_changes`, `get_agent_conversation_workspace_repair_staged_file_diff`, `get_agent_conversation_workspace_repair_unstaged_file_changes`, `get_agent_conversation_workspace_repair_unstaged_file_diff`, `get_agent_conversation_workspace_review`, `get_agent_conversation_workspace_review_hunk_annotations`, `get_agent_conversation_workspace_staged_file_changes`, `get_agent_conversation_workspace_staged_file_diff`, `get_agent_conversation_workspace_unstaged_file_changes`, `get_agent_conversation_workspace_unstaged_file_diff`, `get_commit_file_changes`, `get_commit_file_diff`, `get_conflict_file_diff`, `get_file_diff`, `get_task_file_changes`
- **`git_commands`** (3) — `get_task_commits`, `resolve_merge_conflict`, `retry_merge`
- **`github_commands`** (3) — `get_github_branch_overview`, `get_github_connection_status`, `get_pull_request_detail`
- **`plan_branch_commands`** (4) — `enable_feature_branch`, `get_plan_branch`, `get_plan_branch_by_task_id`, `get_project_plan_branches`
- **`project_commands`** (17) — `archive_project`, `create_project`, `get_git_auth_diagnostics`, `get_git_branches`, `get_git_current_branch`, `get_git_default_branch`, `get_git_remote_url`, `login_gh_with_browser`, `read_pr_template`, `resume_deferred_git_startup`, `search_github_pull_requests`, `setup_gh_git_auth`, `switch_git_origin_to_ssh`, `update_custom_analysis`, `update_github_pr_enabled`, `update_project`, `write_pr_template`
- **`provider_cli_management_commands`** (3) — `auto_update_managed_provider_clis`, `get_managed_provider_cli_status`, `install_or_update_managed_provider_cli`
- **`workspace_open_commands`** (1) — `list_workspace_open_targets`

</details>

### 11. `R1` — `get_project` / `list_projects` — spawn-free read path

**Commands:** 2 · **Register-candidates:** 0 · **Risk classes:** host-denied (SpawnsProcess) 2

**Why here:** The only commands in the gap whose process authority is INCIDENTAL. Both are pure repository reads; the single spawning field is `repository_capability`, computed per project by shelling out to git in `project_response()`. Removing that inline shell-out makes the highest-traffic read on the whole remote surface registerable as `Read`. See `resolvedItems.projectGetters` for the proposed path and the rejected alternatives.

**Work:**

- Land the cache-backed capability read (option A in `resolvedItems.projectGetters`) as its own change with its own tests — NOT inside a registration batch.
- Only after the shell-out is gone: re-run detector (c), drop the `SpawnsProcess` capability, reclassify to `Read`, register both.
- If option A is rejected by the owner, both names fall back into D2 as v1-deferred dispositions and the frontend project list stays a fetch-route question (3.1 open question 4).

**Gate:** Detector (c) reports no launch sink in either closure; P-4 parity rows for both; the manifest shows class `read` with an empty capability set.

<details><summary>Members by module</summary>

- **`project_commands`** (2) — `get_project`, `list_projects`

</details>

### 12. `D3` — Host chrome, terminal, repository settings, test data (disposition only)

**Commands:** 14 · **Register-candidates:** 0 · **Risk classes:** host-denied (class: denied) 8 · v1-deferred (Elevated) 6

**Why here:** Terminal is the phase doc's worked example of the third disposition: its invoke names resolve for P-11 through the module-`Denied` (`PtyControl`) rows, NEVER through a client-local reason. Test data is hard-denied outright (total-data-loss blast radius). Startup/repository settings are `HostManagement`/`ConfiguresFutureProcessAuthority` — v1-deferred.

**Work:**

- Assert the terminal names resolve through the manifest path introduced in B0; a `local-only` row for any of them is a defect, not a shortcut.
- Keep `report_startup_frontend_milestone` honest: it is a client-originated report about the LOCAL app boot — check whether it is genuinely client-local (local-only row) rather than host-deferred.

**Gate:** Manifest diff-clean; a planted local-only row for a terminal command fails CI.

<details><summary>Members by module</summary>

- **`agent_terminal_commands`** (5) — `clear_agent_terminal`, `close_agent_terminal`, `resize_agent_terminal`, `restart_agent_terminal`, `write_agent_terminal`
- **`repository_settings_commands`** (2) — `get_repository_settings`, `update_repository_settings`
- **`startup_commands`** (4) — `get_startup_diagnostics`, `get_startup_status`, `report_startup_frontend_milestone`, `retry_startup`
- **`test_data_commands`** (3) — `clear_test_data`, `seed_test_data`, `seed_visual_audit_data`

</details>

### 13. `A1` — Chat attachments — disposition + remote rendering (deferred from 2.6/review-4)

**Commands:** 3 · **Register-candidates:** 0 · **Risk classes:** host-denied (class: denied) 3

**Why here:** 2.6-a shipped the honest interim: under a remote environment `getImagePreviewSrc()` returns `null` and every attachment renders as a placeholder card, because `convertFileSrc` mints an `asset://` URL for a path on the CLIENT's filesystem while `attachment.filePath` names a file on the HOST. The comment at `MessageAttachments.tsx:92-94` defers the real fix to 3.1. The three attachment commands are Denied (`writesArbitraryPath` / `deletesEntity`) and stay dispositions — the rendering work is a FETCH-path change, not a command registration, which is exactly 3.1 open question 4 and needs an explicit call.

**Work:**

- Blocked on 1.5-C: `/remote/v1/attachments/{id}` does not exist on this base (no `attachments` route in `remote_server/`). Do not start A1 until the 1.5 lane lands it.
- Branch preview-source resolution on env kind in BOTH renderers — `MessageAttachments.tsx:115` and `ChatAttachmentGallery.tsx:97` (2.6 only hardened the first; the gallery still calls `convertFileSrc` unconditionally, which is a live gap this census surfaces).
- Route the remote branch through the scoped endpoint under `ui:read` with a binary-safe body and 2.7's response-header envelope; never through JSON `/invoke` (C-16).
- Resolve open question 4 explicitly: extending the §3.5 fetch-route remount allowlist rides 3.1, or it is a separate change against P-1's checked-in allowlist. Record the call.
- Disposition the three commands through the manifest (upload/delete are `writesArbitraryPath` fs sinks; `list_message_attachments` is denied by module).

**Gate:** A remote attachment renders through the scoped endpoint; a local one still uses `convertFileSrc`; P-1 route-allowlist equality still holds; the three commands are manifest-disposed with zero local-only rows.

<details><summary>Members by module</summary>

- **`chat_attachment_commands`** (3) — `delete_chat_attachment`, `list_message_attachments`, `upload_chat_attachment`

</details>

### 14. `X1` — Orphan invokes — no local handler exists

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Why here:** RESOLVED in PR 3.1-b. Five gap names were absent from `src-tauri/src/commands/registry.rs` `generate_handler!` AND from the ledger (which is exhaustive over it), so every call rejected at runtime with no remote environment involved. The reachability audit found none of the five was wired to any component or event handler — the wrappers were dead code kept alive only by their own unit tests — so all five were resolved by DELETING the call site rather than by minting host authority the product does not use. This batch is now empty and stays here as the record of that call.

**Work:**

- `add_proposal_dependency` — deleted at both call sites (`api/ideation.ts` `dependencies.add`, `api/proposal.ts` `addProposalDependency`) plus the `addDependency` mutation in `hooks/useDependencyGraph.ts`. Its owning hook `useDependencyMutations` had no consumer at all: the UI reads the dependency graph and never writes edges, so there was no product asymmetry to fix by adding the missing command.
- `create_child_session` / `get_parent_session_context` — deleted from `api/ideation.ts` (zero callers, zero tests). The capability is not lost: both are live HTTP routes (`POST /api/create_child_session`, `GET /api/parent_session_context/:session_id`), which is how the backend actually reaches them.
- `delete_project` — deleted from `api/projects.ts`; `projectsApi.archive` is the live removal path.
- `delete_task` — deleted from `api/tasks.ts` and `hooks/useTaskMutation.ts`; it was already `@deprecated Use cleanupTask instead`, and every component destructures `cleanupTaskMutation`.
- Regression guard: `frontend/src/api/orphan-invokes.test.ts` asserts each wrapper stays absent while its surviving sibling (`remove`, `getChildren`, `archive`, `cleanupTask`) stays present, so the test cannot pass by the namespace disappearing.

**Gate:** Each of the five is deleted at the call site with a regression test; the P-11 scan sees zero orphans.

## 5. Resolved items

### 5.1 `get_project` / `list_projects` — the getter that shells out

**Status:** proposal — needs an owner call before 3.1-b starts R1 · **Batch:** `R1`

**Finding.** Both are pure repository reads (`project_commands.rs:211-240`): `project_repo.get_all()` / `get_by_id()`, then `project_response()` per row. The ONLY process authority is one response field — `repository_capability`, produced by `inspect_repository_capability()` (`infrastructure/git_auth.rs`), which runs `git remote get-url origin` and `git remote get-url --push origin` through `resolve_git_cli_path()` with a 5s deadline, once PER PROJECT. That is what makes a getter a `SpawnsProcess`/Elevated command, and it is incidental to the read, not inherent to it.

**Proposal.** Option A — cache the capability, do not compute it in the getter. Persist the inspected `repository_capability` (plus `inspected_at`) alongside the project row, write it from the paths that already have process authority and already shell out (project create/update, `change_project_git_mode`, `setup_gh_git_auth`, `switch_git_origin_to_ssh`, `reanalyze_project`) plus one background refresh whose loop root is declared in the manifest's `background_loop_inventory`, and have `project_response()` READ the cached value. `list_projects`/`get_project` then hold no launch sink in their closure, detector (c) goes quiet, the `SpawnsProcess` capability drops, and both classify as `Read` — registerable on the v1 facade at `ui:read`, with zero `generate_handler!` edits and zero command-fn forks (A-7). The response shape is unchanged, so P-4 parity and every existing caller are untouched; only the freshness semantics change, and a stale-capability value is strictly safer than the current InspectionFailed-on-timeout behaviour (`inspect_repository_capability` already returns `InspectionFailed{message}` rather than erroring, so consumers already handle a non-authoritative value).

**Rejected alternatives:**

- A response projection that omits `repository_capability` for remote callers — that is a command-fn fork, which A-7 forbids, and it would break P-4 byte-identity between local IPC and remote dispatch (the whole point of the parity suite).
- A pinned facade op — pins fix ARGUMENTS (`approve_permission_request` / `deny_permission_request`), not response shape; there is no pin that removes a field.
- Registering as Elevated — `ui:elevated` is a §1 v1 non-goal; this would ship a scope nothing can hold.
- Serving the project list over a remounted fetch route instead — `http_server/handlers/projects.rs` computes the SAME capability inline, so the spawn moves rather than disappears, and it opens 3.1 open question 4 unnecessarily.

**If the owner rejects option A.** Both names fall back to D2 as v1-deferred dispositions. That is not cost-free: the project list is the entry point of nearly every remote screen, so a remote client would have to hydrate projects through a fetch route (open question 4) or run with no project list at all.

### 5.2 The five 2.6-surfaced unregistered `ui:agent` ops

**Status:** resolved — all five are registration candidates; none is a detector-(c) rejection on current evidence

| Command | Ledger class | Capabilities | Batch | Resolution |
|---|---|---|---|---|
| `send_agent_message` | agentControl | agentControl | `B2` | register (`ui:agent`), pending detector-(c) confirmation |
| `start_agent_conversation` | agentControl | agentControl | `B2` | register (`ui:agent`), pending detector-(c) confirmation |
| `skip_step` | agentControl | agentControl | `B1` | register (`ui:agent`), pending detector-(c) confirmation |
| `trigger_automation_run_now` | agentControl | agentControl | `B5` | register (`ui:agent`), pending detector-(c) confirmation |
| `restart_automation` | agentControl | agentControl | `B5` | register (`ui:agent`), pending detector-(c) confirmation |

**Briefing correction.** The 3.1-a brief states three of these five are detector-(c)-rejected. That does not match the code: the detector-(c) trio is `resume_task`, `apply_proposals_to_kanban`, `set_agent_conversation_workspace_auto_publish` (`remote_server/registry.rs` NOT-registered note; `frontend/src/lib/remote/agent-gate.test.ts:114-124` uses exactly those three as the unavailable-by-ABSENCE fixture). None of the five 2.6-surfaced ops appears in that set. The two lists were conflated — they are different trios, and 2.6's tracker note lists the five as ops that 'flip with no client change when 3.1 registers them', i.e. registration is the intended resolution.

**Evidence:**

- 2.6 tracker product note: 'with `ui:agent` granted, chat send / start composer / skip_step / automation run+restart render UNAVAILABLE remotely — send_agent_message etc. are unregistered in 1.5-A's 27-op surface. Honest against this build; flips with no client change when 3.1 registers them.'
- Phase 3 doc, PR 3.2 key point 4: 'Chat send paths (`start_agent_conversation`, `send_agent_message` + variants, …) are `AgentControl` — a device without `ui:agent` gets `REMOTE_FORBIDDEN`'. `REMOTE_FORBIDDEN` (not `REMOTE_COMMAND_UNAVAILABLE`) is only reachable for a REGISTERED command, so 3.2 requires these registered.
- All five are ledgered `class: agentControl`, `capabilities: [agentControl]`, reason `conservative-module-default` — none carries `SpawnsProcess`.
- `send_agent_message` reaches `chat_service.send_message` (`unified_chat_commands/mod.rs`), which is a detector-(a) STEER sink. `all_cut_sinks()` CUTS the closure at steer sinks, so the provider process launch beyond it is outside the command's own closure — which is precisely why chat send is registerable while `resume_task` (whose closure resolves a CLI path directly) is not.

**Obligation on 3.1-b.** This is a static read of the call graph, not a detector run. 3.1-b must confirm each of the five against the live P-17 detector-(c) output as the first step of its batch, and demote any that come back positive to a manifest disposition — the class is decided by the detector, never by this census.

**Client impact.** No client change is needed: `agent-gate.ts` derives availability from ABSENCE in `facade_ops`, so each op flips from `unavailable` to `gated`/`enabled` the moment its registration lands in the regenerated manifest.

### 5.3 Remote attachment rendering

**Status:** scoped into batch A1; BLOCKED on the 1.5-C endpoint · **Batch:** `A1`

**Finding.** Deferred here from 2.6-a and the review-4 round. Current behaviour is the honest interim, not a bug: `getImagePreviewSrc()` (`frontend/src/components/Chat/MessageAttachments.tsx:99-116`) returns `null` whenever the active environment is remote, so every host attachment renders as a placeholder card instead of a broken image — `convertFileSrc` would mint an `asset://` URL for a path on the CLIENT's disk while `attachment.filePath` names a file on the HOST.

**Blockers:**

- `/remote/v1/attachments/{id}` does not exist on this base — there is no attachments route in `src-tauri/src/remote_server/`. It is 1.5-C's deliverable (live in the `rme-pr-1-5` lane). A1 cannot start until it lands.
- 2.7's response-header envelope and a binary-safe body are prerequisites; binary must never travel through JSON `/invoke` (C-16).

**New gap this census found.** 2.6 hardened only ONE of the two renderers. `ChatAttachmentGallery.tsx:97` still calls `convertFileSrc(attachment.filePath)` with no env-kind branch, so the gallery surface renders broken images under a remote environment where `MessageAttachments` renders placeholders. A1 must fix both, and the 2.6 negative test (`host-affordance-gating.test.tsx`, which asserts `convertFileSrc` was NOT called) should be extended to cover the gallery.

**Open question.** Phase-3 open question 4 applies verbatim: attachment rendering is a FETCH route, not an invoke command, and the source does not say whether extending the §3.5 remount allowlist rides 3.1 or requires a separate change against P-1's checked-in allowlist. A1 must record the call before it writes a route.

**Command side.** The three attachment COMMANDS in the gap (`upload_chat_attachment`, `delete_chat_attachment`, `list_message_attachments`) are all ledgered `denied` (`writesArbitraryPath` / `deletesEntity`) and stay manifest dispositions — no registration, and specifically no local-only rows.

## 6. Reconciliation

| Check | Result |
|---|---|
| Drift scan passes | yes (this file is not emitted otherwise) |
| Scan unclassified count == baseline size | 435 == 435 |
| Every gap command in exactly one batch | 435 / 435 |
| Disposition totals sum to the gap | 435 == 435 |
| Batch plan claims no empty module and pins no absent command | enforced by the generator |

Machine-readable companion for 3.1-b/c: [`remote-coverage-census.json`](./remote-coverage-census.json) — same batches, plus per-command `{batch, module, ledgerClass, capabilities, disposition}` rows.
