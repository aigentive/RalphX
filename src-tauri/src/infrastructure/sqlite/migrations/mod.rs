// Database migrations for SQLite
//
// # Migration System Design
//
// ## Adding a new migration
//
// 1. Create a new file: `vN_description.rs` (e.g., `v2_add_user_preferences.rs`)
// 2. Implement a `pub fn migrate(conn: &Connection) -> AppResult<()>` function
// 3. Register it in the MIGRATIONS array below
// 4. Bump SCHEMA_VERSION
//
// ## Guidelines
//
// - Use `IF NOT EXISTS` for CREATE TABLE/INDEX to make migrations idempotent
// - Use helpers::add_column_if_not_exists for ALTER TABLE ADD COLUMN
// - Keep migrations focused - one logical change per migration
// - Test migrations work on both fresh databases and existing ones
//
// ## For existing databases
//
// Existing databases have schema_migrations tracking applied versions.
// Any registered migration that is not recorded will run, even if a later
// version was already applied by another branch.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub mod helpers;
mod v10_execution_settings;
mod v11_per_project_execution_settings;
mod v12_fix_worktree_project_settings;
mod v13_plan_branches;
mod v14_app_state;
mod v15_task_ideation_session_id;
mod v16_plan_branch_session_index;
mod v17_running_agents;
mod v18_task_metadata;
mod v19_project_analysis;
mod v1_initial_schema;
mod v20260325120000_app_state_execution_halt_mode;
mod v20260325131500_execution_ideation_allocation_settings;
mod v20260327233752_pending_initial_prompt;
mod v20260328194000_ideation_followup_provenance;
mod v20260329103000_review_note_followup_session;
mod v20260329113000_ideation_blocker_fingerprint;
mod v20_merge_validation_mode;
mod v21_questions_permissions;
mod v22_project_active_plan;
mod v23_plan_selection_stats;
mod v24_memory_framework;
mod v25_seed_artifact_buckets;
mod v26_running_agent_worktree;
mod v27_merge_strategy;
mod v28_default_rebase_squash;
mod v29_repair_schema_drift;
mod v2_add_dependency_reason;
mod v30_update_max_concurrent_default;
mod v31_session_linking;
mod v32_fix_task_fk_constraints;
mod v33_agent_run_chain_ids;
mod v34_chat_attachments;
mod v35_step_substeps;
mod v36_spawn_orchestrator_jobs;
mod v37_team_sessions;
mod v38_ideation_team_mode;
mod v39_conversation_parent_id;
mod v3_add_activity_events;
mod v40_dependency_source;
mod v41_activity_events_merge_index;
mod v42_running_agent_heartbeat;
mod v43_session_title_source;
mod v44_remove_local_git_mode;
mod v45_drop_task_blockers;
mod v46_execution_plans;
mod v47_plan_branches_execution_plan_id;
mod v48_tasks_execution_plan_id;
mod v49_backfill_execution_plans;
mod v4_add_blocked_reason;
mod v50_active_plan_execution_plan_id;
mod v51_repair_plan_branches;
mod v52_cleanup_stale_execution_plans;
mod v53_merge_pipeline_active_column;
mod v54_inherited_plan_artifact_id;
mod v55_drop_spawn_orchestrator_jobs;
mod v56_api_keys;
mod v57_plan_verification;
mod v58_metrics_index;
mod v59_project_metrics_config;
mod v5_add_review_summary_issues;
mod v60_metrics_working_days;
mod v61_ideation_settings_verification;
mod v62_api_key_admin_permissions;
mod v63_auto_verify_generation;
mod v64_github_pr_settings;
mod v65_unique_working_directory;
mod v66_cross_project_import;
mod v67_tasks_session_status_index;
mod v68_session_purpose;
mod v69_soft_delete_archived_at;
mod v6_review_issues;
mod v70_plan_branch_base_override;
mod v71_add_target_project_to_proposals;
mod v72_cross_project_check;
mod v73_proposal_migrated_from;
mod v74_permission_identity;
mod v75_plan_version_last_read;
mod v76_session_origin;
mod v77_expected_proposal_count;
mod v78_webhook_registrations;
mod v79_external_session_reliability;
mod v7_session_status_converted_to_accepted;
mod v80_dependencies_acknowledged;
mod v81_external_session_reliability_backfill;
mod v8_task_git_fields;
mod v9_project_git_fields;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod v10_execution_settings_tests;
#[cfg(test)]
mod v11_per_project_execution_settings_tests;
#[cfg(test)]
mod v12_fix_worktree_project_settings_tests;
#[cfg(test)]
mod v13_plan_branches_tests;
#[cfg(test)]
mod v14_app_state_tests;
#[cfg(test)]
mod v15_task_ideation_session_id_tests;
#[cfg(test)]
mod v16_plan_branch_session_index_tests;
#[cfg(test)]
mod v17_running_agents_tests;
#[cfg(test)]
mod v18_task_metadata_tests;
#[cfg(test)]
mod v19_project_analysis_tests;
#[cfg(test)]
mod v1_initial_schema_tests;
#[cfg(test)]
mod v20260325120000_app_state_execution_halt_mode_tests;
#[cfg(test)]
mod v20260325131500_execution_ideation_allocation_settings_tests;
#[cfg(test)]
mod v20260327233752_pending_initial_prompt_tests;
#[cfg(test)]
mod v20260328194000_ideation_followup_provenance_tests;
mod v20260328210000_proposal_affected_paths;
#[cfg(test)]
mod v20260328210000_proposal_affected_paths_tests;
mod v20260329080000_acceptance_status;
#[cfg(test)]
mod v20260329080000_acceptance_status_tests;
#[cfg(test)]
mod v20260329103000_review_note_followup_session_tests;
#[cfg(test)]
mod v20260329113000_ideation_blocker_fingerprint_tests;
mod v20260330000000_verification_confirmation_status;
#[cfg(test)]
mod v20260330000000_verification_confirmation_status_tests;
mod v20260330000001_ideation_effort_settings;
#[cfg(test)]
mod v20260330000001_ideation_effort_settings_tests;
mod v20260330000002_ideation_model_settings;
mod v20260405045108_ideation_external_overrides;
#[cfg(test)]
mod v20260405045108_ideation_external_overrides_tests;
mod v20260406000000_verifier_subagent_model;
#[cfg(test)]
mod v20260406000000_verifier_subagent_model_tests;
mod v20260406043151_add_last_effective_model_to_ideation_sessions;
#[cfg(test)]
mod v20260406043151_add_last_effective_model_to_ideation_sessions_tests;
mod v20260406043153_add_model_to_running_agents;
#[cfg(test)]
mod v20260406043153_add_model_to_running_agents_tests;
mod v20260406120000_add_ideation_subagent_model;
#[cfg(test)]
mod v20260406120000_add_ideation_subagent_model_tests;
mod v20260407073000_provider_harness_metadata;
#[cfg(test)]
mod v20260407073000_provider_harness_metadata_tests;
mod v20260407103000_agent_lane_settings;
#[cfg(test)]
mod v20260407103000_agent_lane_settings_tests;
mod v20260410093000_chat_attribution_backfill_state;
#[cfg(test)]
mod v20260410093000_chat_attribution_backfill_state_tests;
mod v20260410101500_chat_message_attribution;
#[cfg(test)]
mod v20260410101500_chat_message_attribution_tests;
mod v20260410113000_agent_run_usage;
#[cfg(test)]
mod v20260410113000_agent_run_usage_tests;
mod v20260410124500_chat_message_usage;
#[cfg(test)]
mod v20260410124500_chat_message_usage_tests;
mod v20260410143000_upstream_provider_metadata;
#[cfg(test)]
mod v20260410143000_upstream_provider_metadata_tests;
mod v20260410153000_chat_conversation_provider_origin;
#[cfg(test)]
mod v20260410153000_chat_conversation_provider_origin_tests;
mod v20260411190000_delegated_sessions;
#[cfg(test)]
mod v20260411190000_delegated_sessions_tests;
mod v20260413043153_drop_agent_lane_settings_fallback_harness;
#[cfg(test)]
mod v20260413043153_drop_agent_lane_settings_fallback_harness_tests;
mod v20260414060000_verification_run_store;
#[cfg(test)]
mod v20260414060000_verification_run_store_tests;
mod v20260414123000_drop_verification_metadata_column;
#[cfg(test)]
mod v20260414123000_drop_verification_metadata_column_tests;
mod v20260415164250_merge_validation_mode_off;
#[cfg(test)]
mod v20260415164250_merge_validation_mode_off_tests;
mod v20260422140039_chat_conversation_archived_at;
#[cfg(test)]
mod v20260422140039_chat_conversation_archived_at_tests;
mod v20260424090000_ideation_analysis_base;
#[cfg(test)]
mod v20260424090000_ideation_analysis_base_tests;
mod v20260424150000_agent_conversation_workspaces;
mod v20260424193000_chat_conversation_agent_mode;
#[cfg(test)]
mod v20260424193000_chat_conversation_agent_mode_tests;
mod v20260425154500_agent_workspace_chat_mode;
#[cfg(test)]
mod v20260425154500_agent_workspace_chat_mode_tests;
mod v20260426093000_agent_workspace_publication_events;
#[cfg(test)]
mod v20260426093000_agent_workspace_publication_events_tests;
mod v20260505090000_agent_model_registry;
#[cfg(test)]
mod v20260505090000_agent_model_registry_tests;
mod v20260506131356_agent_workspace_pr_descriptions;
#[cfg(test)]
mod v20260506131356_agent_workspace_pr_descriptions_tests;
mod v20260508103000_agent_provider_settings;
#[cfg(test)]
mod v20260508103000_agent_provider_settings_tests;
mod v20260509090000_release_notes_seen_version;
#[cfg(test)]
mod v20260509090000_release_notes_seen_version_tests;
mod v20260510185257_chat_message_blocks_timeline;
#[cfg(test)]
mod v20260510185257_chat_message_blocks_timeline_tests;
mod v20260730025727_chat_message_blocks_thinking_kind;
#[cfg(test)]
mod v20260730025727_chat_message_blocks_thinking_kind_tests;
mod v20260512093000_startup_local_cleanup_markers;
#[cfg(test)]
mod v20260512093000_startup_local_cleanup_markers_tests;
mod v20260513143000_orphan_worktree_cleanup_markers;
#[cfg(test)]
mod v20260513143000_orphan_worktree_cleanup_markers_tests;
mod v20260517153000_agent_workspace_pr_supervision;
#[cfg(test)]
mod v20260517153000_agent_workspace_pr_supervision_tests;
mod v20260518113000_agent_workspace_pr_supervision_recovery_index;
#[cfg(test)]
mod v20260518113000_agent_workspace_pr_supervision_recovery_index_tests;
mod v20260518230038_agent_workspace_pr_comment_evidence;
#[cfg(test)]
mod v20260518230038_agent_workspace_pr_comment_evidence_tests;
mod v20260519180000_agent_tasks;
#[cfg(test)]
mod v20260519180000_agent_tasks_tests;
mod v20260520125526_atlassian_integrations;
#[cfg(test)]
mod v20260520125526_atlassian_integrations_tests;
mod v20260520150000_atlassian_oauth;
#[cfg(test)]
mod v20260520150000_atlassian_oauth_tests;
mod v20260521150003_agent_workspace_source_pull_request;
#[cfg(test)]
mod v20260521150003_agent_workspace_source_pull_request_tests;
mod v20260521222911_agent_plan_mode;
#[cfg(test)]
mod v20260521222911_agent_plan_mode_tests;
mod v20260522090000_agent_workspace_state_history;
#[cfg(test)]
mod v20260522090000_agent_workspace_state_history_tests;
mod v20260522093000_ideation_session_flow;
#[cfg(test)]
mod v20260522093000_ideation_session_flow_tests;
mod v20260523070000_plan_artifact_approvals;
#[cfg(test)]
mod v20260523070000_plan_artifact_approvals_tests;
mod v20260523145711_plan_complexity_assessments;
#[cfg(test)]
mod v20260523145711_plan_complexity_assessments_tests;
mod v20260523152748_agent_task_list_slices;
#[cfg(test)]
mod v20260523152748_agent_task_list_slices_tests;
mod v20260524170000_execution_workspace_capacity;
#[cfg(test)]
mod v20260524170000_execution_workspace_capacity_tests;
mod v20260527033000_agent_workspace_auto_publish;
#[cfg(test)]
mod v20260527033000_agent_workspace_auto_publish_tests;
mod v20260611110952_question_skip_progress;
#[cfg(test)]
mod v20260611110952_question_skip_progress_tests;
mod v20260611152000_question_metadata;
#[cfg(test)]
mod v20260611152000_question_metadata_tests;
mod v20260611191722_agent_workspace_pr_automation_defaults;
#[cfg(test)]
mod v20260611191722_agent_workspace_pr_automation_defaults_tests;
mod v20260612124826_provider_cli_management_policy;
#[cfg(test)]
mod v20260612124826_provider_cli_management_policy_tests;
mod v20260616182441_external_issue_links;
#[cfg(test)]
mod v20260616182441_external_issue_links_tests;
mod v20260616182951_linear_webhook_reconciliation;
#[cfg(test)]
mod v20260616182951_linear_webhook_reconciliation_tests;
mod v20260617121800_agent_conversation_jira_issue_links;
#[cfg(test)]
mod v20260617121800_agent_conversation_jira_issue_links_tests;
mod v20260617122100_linear_integration_settings;
#[cfg(test)]
mod v20260617122100_linear_integration_settings_tests;
mod v20260617122430_agent_workspace_initial_auto_publish;
#[cfg(test)]
mod v20260617122430_agent_workspace_initial_auto_publish_tests;
mod v20260618123000_agent_workspace_pr_review_monitoring;
#[cfg(test)]
mod v20260618123000_agent_workspace_pr_review_monitoring_tests;
mod v20260618134600_review_pr_mode_checks;
#[cfg(test)]
mod v20260618134600_review_pr_mode_checks_tests;
mod v20260618181405_agent_conversation_linear_issue_links;
#[cfg(test)]
mod v20260618181405_agent_conversation_linear_issue_links_tests;
mod v20260619093000_agent_workspace_pr_review_artifacts;
#[cfg(test)]
mod v20260619093000_agent_workspace_pr_review_artifacts_tests;
mod v20260619144000_durable_queued_messages;
#[cfg(test)]
mod v20260619144000_durable_queued_messages_tests;
mod v20260620075610_provider_ticket_operations;
#[cfg(test)]
mod v20260620075610_provider_ticket_operations_tests;
mod v20260621201947_ticket_canonical_branches;
#[cfg(test)]
mod v20260621201947_ticket_canonical_branches_tests;
mod v20260622103000_agent_workspace_reviews;
#[cfg(test)]
mod v20260622103000_agent_workspace_reviews_tests;
mod v20260622162352_agent_workspace_followup_provenance;
#[cfg(test)]
mod v20260622162352_agent_workspace_followup_provenance_tests;
mod v20260623074101_clickup_integration_settings;
#[cfg(test)]
mod v20260623074101_clickup_integration_settings_tests;
mod v20260624114053_granola_integration_settings;
#[cfg(test)]
mod v20260624114053_granola_integration_settings_tests;
mod v20260625115000_custom_provider_binary;
#[cfg(test)]
mod v20260625115000_custom_provider_binary_tests;
mod v20260625153000_agent_conversation_issues;
#[cfg(test)]
mod v20260625153000_agent_conversation_issues_tests;
mod v20260626092500_custom_provider_env_file;
#[cfg(test)]
mod v20260626092500_custom_provider_env_file_tests;
mod v20260626191358_codex_service_tier_settings;
#[cfg(test)]
mod v20260626191358_codex_service_tier_settings_tests;
mod v20260627104500_agent_conversation_granola_note_links;
mod v20260627183000_agent_workspace_branch_mode;
#[cfg(test)]
mod v20260627183000_agent_workspace_branch_mode_tests;
mod v20260628010000_workspace_review_child_conversation;
#[cfg(test)]
mod v20260628010000_workspace_review_child_conversation_tests;
mod v20260629101000_workspace_review_gate;
#[cfg(test)]
mod v20260629101000_workspace_review_gate_tests;
mod v20260630120000_ticketing_status_catalog;
#[cfg(test)]
mod v20260630120000_ticketing_status_catalog_tests;
mod v20260630123000_workspace_review_policy_setting;
#[cfg(test)]
mod v20260630123000_workspace_review_policy_setting_tests;
mod v20260701143000_workspace_review_runtime_settings;
#[cfg(test)]
mod v20260701143000_workspace_review_runtime_settings_tests;
mod v20260701152000_workspace_review_runtime_global_scope;
#[cfg(test)]
mod v20260701152000_workspace_review_runtime_global_scope_tests;
mod v20260701174810_workspace_review_hunk_annotations;
#[cfg(test)]
mod v20260701174810_workspace_review_hunk_annotations_tests;
mod v20260703143000_task_validation_runs;
mod v20260704193000_automations_p1;
#[cfg(test)]
mod v20260704193000_automations_p1_tests;
mod v20260706113000_agent_conversation_issue_identity;
#[cfg(test)]
mod v20260706113000_agent_conversation_issue_identity_tests;
mod v20260707113000_automation_agent_completed_signal;
#[cfg(test)]
mod v20260707113000_automation_agent_completed_signal_tests;
mod v20260707120000_automations_spec_artifact_id;
#[cfg(test)]
mod v20260707120000_automations_spec_artifact_id_tests;
mod v20260708120000_automation_run_plan_gate;
#[cfg(test)]
mod v20260708120000_automation_run_plan_gate_tests;
mod v20260708130511_workspace_review_autofix_setting;
#[cfg(test)]
mod v20260708130511_workspace_review_autofix_setting_tests;
mod v20260708131548_chat_conversation_coordination_mode;
#[cfg(test)]
mod v20260708131548_chat_conversation_coordination_mode_tests;
mod v20260710000000_task_branch_base;
#[cfg(test)]
mod v20260710000000_task_branch_base_tests;
mod v20260710003315_execution_plan_halt_mode;
#[cfg(test)]
mod v20260710003315_execution_plan_halt_mode_tests;
mod v20260710134609_notifications_table;
#[cfg(test)]
mod v20260710134609_notifications_table_tests;
mod v20260710201548_notification_settings;
#[cfg(test)]
mod v20260710201548_notification_settings_tests;
mod v20260711151804_personas;
#[cfg(test)]
mod v20260711151804_personas_tests;
mod v20260712090000_validation_run_content_fingerprints;
mod v20260712153932_agent_workspace_pr_review_auto_approve;
#[cfg(test)]
mod v20260712153932_agent_workspace_pr_review_auto_approve_tests;
mod v20260712155425_ui_feature_flag_overrides;
#[cfg(test)]
mod v20260712155425_ui_feature_flag_overrides_tests;
mod v20260712162657_persona_builder_agent_mode;
#[cfg(test)]
mod v20260712162657_persona_builder_agent_mode_tests;
mod v20260712190416_branch_update_authority;
#[cfg(test)]
mod v20260712190416_branch_update_authority_tests;
mod v20260713063349_persona_run_attribution;
#[cfg(test)]
mod v20260713063349_persona_run_attribution_tests;
mod v20260713131052_disable_auto_followup_by_default;
#[cfg(test)]
mod v20260713131052_disable_auto_followup_by_default_tests;
mod v20260714184430_workspace_review_auto_merge_guard;
#[cfg(test)]
mod v20260714184430_workspace_review_auto_merge_guard_tests;
mod v20260715013854_model_native_plan_verification;
#[cfg(test)]
mod v20260715013854_model_native_plan_verification_tests;
mod v20260715170000_automation_authoring_state;
#[cfg(test)]
mod v20260715170000_automation_authoring_state_tests;
mod v20260715172058_persona_update_draft_provenance;
#[cfg(test)]
mod v20260715172058_persona_update_draft_provenance_tests;
mod v20260715181627_agent_conversation_capabilities;
#[cfg(test)]
mod v20260715181627_agent_conversation_capabilities_tests;
mod v20260715183000_automation_ideation_signal;
#[cfg(test)]
mod v20260715183000_automation_ideation_signal_tests;
mod v20260715194617_scripted_agent_workflows;
#[cfg(test)]
mod v20260715194617_scripted_agent_workflows_tests;
mod v20260716154318_manual_role_defaults;
#[cfg(test)]
mod v20260716154318_manual_role_defaults_tests;
mod v20260716170840_persona_project_scope;
#[cfg(test)]
mod v20260716170840_persona_project_scope_tests;
mod v20260716202015_workspace_review_bypass_and_bound_agent;
#[cfg(test)]
mod v20260716202015_workspace_review_bypass_and_bound_agent_tests;
mod v20260716204027_conversation_folder_references;
#[cfg(test)]
mod v20260716204027_conversation_folder_references_tests;
mod v20260716210000_supervised_native_task_pipeline;
#[cfg(test)]
mod v20260716210000_supervised_native_task_pipeline_tests;
mod v20260717152713_persona_builder_result_binding;
#[cfg(test)]
mod v20260717152713_persona_builder_result_binding_tests;
mod v20260717152714_persona_artifact_history;
#[cfg(test)]
mod v20260717152714_persona_artifact_history_tests;
mod v20260717235338_github_cli_token_environment_setting;
#[cfg(test)]
mod v20260717235338_github_cli_token_environment_setting_tests;
mod v20260718014631_mcp_policy_overrides;
#[cfg(test)]
mod v20260718014631_mcp_policy_overrides_tests;
mod v20260718162852_clear_detected_validation_commands;
#[cfg(test)]
mod v20260718162852_clear_detected_validation_commands_tests;
mod v20260718182035_add_tasks_enabled_setting;
#[cfg(test)]
mod v20260718182035_add_tasks_enabled_setting_tests;
mod v20260720102513_add_tasks_feature_state;
#[cfg(test)]
mod v20260720102513_add_tasks_feature_state_tests;
mod v20260720131416_review_pr_disable_pr_automation;
#[cfg(test)]
mod v20260720131416_review_pr_disable_pr_automation_tests;
mod v20260720140000_remove_legacy_claude_team;
#[cfg(test)]
mod v20260720140000_remove_legacy_claude_team_tests;
mod v20260720200633_auto_verify_draft_plans;
#[cfg(test)]
mod v20260720200633_auto_verify_draft_plans_tests;
mod v20260721190000_workspace_review_fixer_attempt;
#[cfg(test)]
mod v20260721190000_workspace_review_fixer_attempt_tests;
mod v20260722022339_usage_capture_provenance_and_raw_snapshots;
#[cfg(test)]
mod v20260722022339_usage_capture_provenance_and_raw_snapshots_tests;
mod v20260722132100_automation_run_goal_item;
#[cfg(test)]
mod v20260722132100_automation_run_goal_item_tests;
mod v20260723012559_agent_workspace_pr_metadata_decision;
#[cfg(test)]
mod v20260723012559_agent_workspace_pr_metadata_decision_tests;
mod v20260723065349_pr_autofix_completed_supervision_history;
#[cfg(test)]
mod v20260723065349_pr_autofix_completed_supervision_history_tests;
mod v20260723100604_app_state_update_channel;
#[cfg(test)]
mod v20260723100604_app_state_update_channel_tests;
mod v20260724113627_agent_task_delegate_assignments;
#[cfg(test)]
mod v20260724113627_agent_task_delegate_assignments_tests;
mod v20260724130000_plan_blueprints;
#[cfg(test)]
mod v20260724130000_plan_blueprints_tests;
mod v20260724141500_workspace_review_requested_changes;
#[cfg(test)]
mod v20260724141500_workspace_review_requested_changes_tests;
mod v20260724222347_agent_task_assignment_planned_run_identity;
#[cfg(test)]
mod v20260724222347_agent_task_assignment_planned_run_identity_tests;
mod v20260725164704_agent_workspace_repair_attempts;
#[cfg(test)]
mod v20260725164704_agent_workspace_repair_attempts_tests;
mod v20260727115037_agent_workspace_publication_metadata_receipts;
#[cfg(test)]
mod v20260727115037_agent_workspace_publication_metadata_receipts_tests;
mod v20260728155615_agent_conversation_mutes;
#[cfg(test)]
mod v20260728155615_agent_conversation_mutes_tests;
mod v20260728162405_rx_native_team_runtime;
#[cfg(test)]
mod v20260728162405_rx_native_team_runtime_tests;
mod v20260728183000_workspace_review_plan_context;
#[cfg(test)]
mod v20260728183000_workspace_review_plan_context_tests;
mod v20260730000304_chat_message_blocks_created_at_index;
#[cfg(test)]
mod v20260730000304_chat_message_blocks_created_at_index_tests;
mod v20260730151837_agent_workspace_repair_ci_rerun_reservations;
#[cfg(test)]
mod v20260730151837_agent_workspace_repair_ci_rerun_reservations_tests;
mod v20260730161032_agent_workspace_pr_autofix_completion_evidence;
mod v20260731023949_agent_run_identity;
mod v20260731111346_purge_empty_thinking_blocks;
#[cfg(test)]
mod v20260730161032_agent_workspace_pr_autofix_completion_evidence_tests;
#[cfg(test)]
mod v20260731023949_agent_run_identity_tests;
#[cfg(test)]
mod v20260731111346_purge_empty_thinking_blocks_tests;
#[cfg(test)]
pub(super) fn migrate_scripted_agent_workflows_for_test(conn: &Connection) -> AppResult<()> {
    v20260715194617_scripted_agent_workflows::migrate(conn)
}
#[cfg(test)]
mod v20_merge_validation_mode_tests;
#[cfg(test)]
mod v21_questions_permissions_tests;
#[cfg(test)]
mod v22_project_active_plan_tests;
#[cfg(test)]
mod v23_plan_selection_stats_tests;
#[cfg(test)]
mod v24_memory_framework_tests;
#[cfg(test)]
mod v26_running_agent_worktree_tests;
#[cfg(test)]
mod v27_merge_strategy_tests;
#[cfg(test)]
mod v2_add_dependency_reason_tests;
#[cfg(test)]
mod v31_session_linking_tests;
#[cfg(test)]
mod v32_fix_task_fk_constraints_tests;
#[cfg(test)]
mod v33_agent_run_chain_ids_tests;
#[cfg(test)]
mod v34_chat_attachments_tests;
#[cfg(test)]
mod v35_step_substeps_tests;
#[cfg(test)]
mod v37_team_sessions_tests;
#[cfg(test)]
mod v38_ideation_team_mode_tests;
#[cfg(test)]
mod v39_conversation_parent_id_tests;
#[cfg(test)]
mod v3_add_activity_events_tests;
#[cfg(test)]
mod v40_dependency_source_tests;
#[cfg(test)]
mod v43_session_title_source_tests;
#[cfg(test)]
mod v44_remove_local_git_mode_tests;
#[cfg(test)]
mod v49_backfill_execution_plans_tests;
#[cfg(test)]
mod v4_add_blocked_reason_tests;
#[cfg(test)]
mod v51_repair_plan_branches_tests;
#[cfg(test)]
mod v56_api_keys_tests;
#[cfg(test)]
mod v58_metrics_index_tests;
#[cfg(test)]
mod v59_project_metrics_config_tests;
#[cfg(test)]
mod v60_metrics_working_days_tests;
#[cfg(test)]
mod v61_ideation_settings_verification_tests;
#[cfg(test)]
mod v62_api_key_admin_permissions_tests;
#[cfg(test)]
mod v63_auto_verify_generation_tests;
#[cfg(test)]
mod v65_unique_working_directory_tests;
#[cfg(test)]
mod v66_cross_project_import_tests;
#[cfg(test)]
mod v67_tasks_session_status_index_tests;
#[cfg(test)]
mod v68_session_purpose_tests;
#[cfg(test)]
mod v69_soft_delete_archived_at_tests;
#[cfg(test)]
mod v6_review_issues_tests;
#[cfg(test)]
mod v71_add_target_project_to_proposals_tests;
#[cfg(test)]
mod v72_cross_project_check_tests;
#[cfg(test)]
mod v73_proposal_migrated_from_tests;
#[cfg(test)]
mod v76_session_origin_tests;
#[cfg(test)]
mod v7_session_status_converted_to_accepted_tests;
#[cfg(test)]
mod v81_external_session_reliability_backfill_tests;
#[cfg(test)]
mod v8_task_git_fields_tests;
#[cfg(test)]
mod v9_project_git_fields_tests;

/// Current schema version - bump this when adding a new migration
pub const SCHEMA_VERSION: i64 = 20260731111346;

/// Migration function signature
type MigrationFn = fn(&Connection) -> AppResult<()>;

/// Migration definition
#[derive(Clone, Copy)]
struct Migration {
    version: i64,
    name: &'static str,
    migrate: MigrationFn,
}

/// All migrations in order
/// Add new migrations here - they will be run in version order
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        migrate: v1_initial_schema::migrate,
    },
    Migration {
        version: 2,
        name: "add_dependency_reason",
        migrate: v2_add_dependency_reason::migrate,
    },
    Migration {
        version: 3,
        name: "add_activity_events",
        migrate: v3_add_activity_events::migrate,
    },
    Migration {
        version: 4,
        name: "add_blocked_reason",
        migrate: v4_add_blocked_reason::migrate,
    },
    Migration {
        version: 5,
        name: "add_review_summary_issues",
        migrate: v5_add_review_summary_issues::migrate,
    },
    Migration {
        version: 6,
        name: "review_issues",
        migrate: v6_review_issues::migrate,
    },
    Migration {
        version: 7,
        name: "session_status_converted_to_accepted",
        migrate: v7_session_status_converted_to_accepted::migrate,
    },
    Migration {
        version: 8,
        name: "task_git_fields",
        migrate: v8_task_git_fields::migrate,
    },
    Migration {
        version: 9,
        name: "project_git_fields",
        migrate: v9_project_git_fields::migrate,
    },
    Migration {
        version: 10,
        name: "execution_settings",
        migrate: v10_execution_settings::migrate,
    },
    Migration {
        version: 11,
        name: "per_project_execution_settings",
        migrate: v11_per_project_execution_settings::migrate,
    },
    Migration {
        version: 12,
        name: "fix_worktree_project_settings",
        migrate: v12_fix_worktree_project_settings::migrate,
    },
    Migration {
        version: 13,
        name: "plan_branches",
        migrate: v13_plan_branches::migrate,
    },
    Migration {
        version: 14,
        name: "app_state",
        migrate: v14_app_state::migrate,
    },
    Migration {
        version: 15,
        name: "task_ideation_session_id",
        migrate: v15_task_ideation_session_id::migrate,
    },
    Migration {
        version: 16,
        name: "plan_branch_session_index",
        migrate: v16_plan_branch_session_index::migrate,
    },
    Migration {
        version: 17,
        name: "running_agents",
        migrate: v17_running_agents::migrate,
    },
    Migration {
        version: 18,
        name: "task_metadata",
        migrate: v18_task_metadata::migrate,
    },
    Migration {
        version: 19,
        name: "project_analysis",
        migrate: v19_project_analysis::migrate,
    },
    Migration {
        version: 20,
        name: "merge_validation_mode",
        migrate: v20_merge_validation_mode::migrate,
    },
    Migration {
        version: 21,
        name: "questions_permissions",
        migrate: v21_questions_permissions::migrate,
    },
    Migration {
        version: 22,
        name: "project_active_plan",
        migrate: v22_project_active_plan::migrate,
    },
    Migration {
        version: 23,
        name: "plan_selection_stats",
        migrate: v23_plan_selection_stats::migrate,
    },
    Migration {
        version: 24,
        name: "memory_framework",
        migrate: v24_memory_framework::migrate,
    },
    Migration {
        version: 25,
        name: "seed_artifact_buckets",
        migrate: v25_seed_artifact_buckets::migrate,
    },
    Migration {
        version: 26,
        name: "running_agent_worktree",
        migrate: v26_running_agent_worktree::migrate,
    },
    Migration {
        version: 27,
        name: "merge_strategy",
        migrate: v27_merge_strategy::migrate,
    },
    Migration {
        version: 28,
        name: "default_rebase_squash",
        migrate: v28_default_rebase_squash::migrate,
    },
    Migration {
        version: 29,
        name: "repair_schema_drift",
        migrate: v29_repair_schema_drift::migrate,
    },
    Migration {
        version: 30,
        name: "update_max_concurrent_default",
        migrate: v30_update_max_concurrent_default::migrate,
    },
    Migration {
        version: 31,
        name: "session_linking",
        migrate: v31_session_linking::migrate,
    },
    Migration {
        version: 32,
        name: "fix_task_fk_constraints",
        migrate: v32_fix_task_fk_constraints::migrate,
    },
    Migration {
        version: 33,
        name: "agent_run_chain_ids",
        migrate: v33_agent_run_chain_ids::migrate,
    },
    Migration {
        version: 34,
        name: "chat_attachments",
        migrate: v34_chat_attachments::migrate,
    },
    Migration {
        version: 35,
        name: "step_substeps",
        migrate: v35_step_substeps::migrate,
    },
    Migration {
        version: 36,
        name: "spawn_orchestrator_jobs",
        migrate: v36_spawn_orchestrator_jobs::migrate,
    },
    Migration {
        version: 37,
        name: "team_sessions",
        migrate: v37_team_sessions::migrate,
    },
    Migration {
        version: 38,
        name: "ideation_team_mode",
        migrate: v38_ideation_team_mode::migrate,
    },
    Migration {
        version: 39,
        name: "conversation_parent_id",
        migrate: v39_conversation_parent_id::migrate,
    },
    Migration {
        version: 40,
        name: "dependency_source",
        migrate: v40_dependency_source::migrate,
    },
    Migration {
        version: 41,
        name: "activity_events_merge_index",
        migrate: v41_activity_events_merge_index::migrate,
    },
    Migration {
        version: 42,
        name: "running_agent_heartbeat",
        migrate: v42_running_agent_heartbeat::migrate,
    },
    Migration {
        version: 43,
        name: "session_title_source",
        migrate: v43_session_title_source::migrate,
    },
    Migration {
        version: 44,
        name: "remove_local_git_mode",
        migrate: v44_remove_local_git_mode::migrate,
    },
    Migration {
        version: 45,
        name: "drop_task_blockers",
        migrate: v45_drop_task_blockers::migrate,
    },
    Migration {
        version: 46,
        name: "execution_plans",
        migrate: v46_execution_plans::migrate,
    },
    Migration {
        version: 47,
        name: "plan_branches_execution_plan_id",
        migrate: v47_plan_branches_execution_plan_id::migrate,
    },
    Migration {
        version: 48,
        name: "tasks_execution_plan_id",
        migrate: v48_tasks_execution_plan_id::migrate,
    },
    Migration {
        version: 49,
        name: "backfill_execution_plans",
        migrate: v49_backfill_execution_plans::migrate,
    },
    Migration {
        version: 50,
        name: "active_plan_execution_plan_id",
        migrate: v50_active_plan_execution_plan_id::migrate,
    },
    Migration {
        version: 51,
        name: "repair_plan_branches",
        migrate: v51_repair_plan_branches::migrate,
    },
    Migration {
        version: 52,
        name: "cleanup_stale_execution_plans",
        migrate: v52_cleanup_stale_execution_plans::migrate,
    },
    Migration {
        version: 53,
        name: "merge_pipeline_active_column",
        migrate: v53_merge_pipeline_active_column::migrate,
    },
    Migration {
        version: 54,
        name: "inherited_plan_artifact_id",
        migrate: v54_inherited_plan_artifact_id::migrate,
    },
    Migration {
        version: 55,
        name: "drop_spawn_orchestrator_jobs",
        migrate: v55_drop_spawn_orchestrator_jobs::migrate,
    },
    Migration {
        version: 56,
        name: "api_keys",
        migrate: v56_api_keys::migrate,
    },
    Migration {
        version: 57,
        name: "plan_verification",
        migrate: v57_plan_verification::migrate,
    },
    Migration {
        version: 58,
        name: "metrics_index",
        migrate: v58_metrics_index::migrate,
    },
    Migration {
        version: 59,
        name: "project_metrics_config",
        migrate: v59_project_metrics_config::migrate,
    },
    Migration {
        version: 60,
        name: "metrics_working_days",
        migrate: v60_metrics_working_days::migrate,
    },
    Migration {
        version: 61,
        name: "ideation_settings_verification",
        migrate: v61_ideation_settings_verification::migrate,
    },
    Migration {
        version: 62,
        name: "api_key_admin_permissions",
        migrate: v62_api_key_admin_permissions::migrate,
    },
    Migration {
        version: 63,
        name: "auto_verify_generation",
        migrate: v63_auto_verify_generation::migrate,
    },
    Migration {
        version: 64,
        name: "github_pr_settings",
        migrate: v64_github_pr_settings::migrate,
    },
    Migration {
        version: 65,
        name: "unique_working_directory",
        migrate: v65_unique_working_directory::migrate,
    },
    Migration {
        version: 66,
        name: "cross_project_import",
        migrate: v66_cross_project_import::migrate,
    },
    Migration {
        version: 67,
        name: "tasks_session_status_index",
        migrate: v67_tasks_session_status_index::migrate,
    },
    Migration {
        version: 68,
        name: "session_purpose",
        migrate: v68_session_purpose::migrate,
    },
    Migration {
        version: 69,
        name: "soft_delete_archived_at",
        migrate: v69_soft_delete_archived_at::migrate,
    },
    Migration {
        version: 70,
        name: "plan_branch_base_override",
        migrate: v70_plan_branch_base_override::migrate,
    },
    Migration {
        version: 71,
        name: "add_target_project_to_proposals",
        migrate: v71_add_target_project_to_proposals::migrate,
    },
    Migration {
        version: 72,
        name: "cross_project_check",
        migrate: v72_cross_project_check::migrate,
    },
    Migration {
        version: 73,
        name: "proposal_migrated_from",
        migrate: v73_proposal_migrated_from::migrate,
    },
    Migration {
        version: 74,
        name: "permission_identity",
        migrate: v74_permission_identity::migrate,
    },
    Migration {
        version: 75,
        name: "plan_version_last_read",
        migrate: v75_plan_version_last_read::migrate,
    },
    Migration {
        version: 76,
        name: "session_origin",
        migrate: v76_session_origin::migrate,
    },
    Migration {
        version: 77,
        name: "expected_proposal_count",
        migrate: v77_expected_proposal_count::migrate,
    },
    Migration {
        version: 78,
        name: "webhook_registrations",
        migrate: v78_webhook_registrations::migrate,
    },
    Migration {
        version: 79,
        name: "external_session_reliability",
        migrate: v79_external_session_reliability::migrate,
    },
    Migration {
        version: 80,
        name: "dependencies_acknowledged",
        migrate: v80_dependencies_acknowledged::migrate,
    },
    Migration {
        version: 81,
        name: "external_session_reliability_backfill",
        migrate: v81_external_session_reliability_backfill::migrate,
    },
    Migration {
        version: 20260325120000,
        name: "app_state_execution_halt_mode",
        migrate: v20260325120000_app_state_execution_halt_mode::migrate,
    },
    Migration {
        version: 20260325131500,
        name: "execution_ideation_allocation_settings",
        migrate: v20260325131500_execution_ideation_allocation_settings::migrate,
    },
    Migration {
        version: 20260327233752,
        name: "pending_initial_prompt",
        migrate: v20260327233752_pending_initial_prompt::migrate,
    },
    Migration {
        version: 20260328194000,
        name: "ideation_followup_provenance",
        migrate: v20260328194000_ideation_followup_provenance::migrate,
    },
    Migration {
        version: 20260328210000,
        name: "proposal_affected_paths",
        migrate: v20260328210000_proposal_affected_paths::migrate,
    },
    Migration {
        version: 20260329080000,
        name: "acceptance_status",
        migrate: v20260329080000_acceptance_status::migrate,
    },
    Migration {
        version: 20260329103000,
        name: "review_note_followup_session",
        migrate: v20260329103000_review_note_followup_session::migrate,
    },
    Migration {
        version: 20260329113000,
        name: "ideation_blocker_fingerprint",
        migrate: v20260329113000_ideation_blocker_fingerprint::migrate,
    },
    Migration {
        version: 20260330000000,
        name: "verification_confirmation_status",
        migrate: v20260330000000_verification_confirmation_status::migrate,
    },
    Migration {
        version: 20260330000001,
        name: "ideation_effort_settings",
        migrate: v20260330000001_ideation_effort_settings::migrate,
    },
    Migration {
        version: 20260330000002,
        name: "ideation_model_settings",
        migrate: v20260330000002_ideation_model_settings::migrate,
    },
    Migration {
        version: 20260405045108,
        name: "ideation_external_overrides",
        migrate: v20260405045108_ideation_external_overrides::migrate,
    },
    Migration {
        version: 20260406000000,
        name: "verifier_subagent_model",
        migrate: v20260406000000_verifier_subagent_model::migrate,
    },
    Migration {
        version: 20260406043151,
        name: "add_last_effective_model_to_ideation_sessions",
        migrate: v20260406043151_add_last_effective_model_to_ideation_sessions::migrate,
    },
    Migration {
        version: 20260406043153,
        name: "add_model_to_running_agents",
        migrate: v20260406043153_add_model_to_running_agents::migrate,
    },
    Migration {
        version: 20260406120000,
        name: "add_ideation_subagent_model",
        migrate: v20260406120000_add_ideation_subagent_model::migrate,
    },
    Migration {
        version: 20260407073000,
        name: "provider_harness_metadata",
        migrate: v20260407073000_provider_harness_metadata::migrate,
    },
    Migration {
        version: 20260407103000,
        name: "agent_lane_settings",
        migrate: v20260407103000_agent_lane_settings::migrate,
    },
    Migration {
        version: 20260410093000,
        name: "chat_attribution_backfill_state",
        migrate: v20260410093000_chat_attribution_backfill_state::migrate,
    },
    Migration {
        version: 20260410101500,
        name: "chat_message_attribution",
        migrate: v20260410101500_chat_message_attribution::migrate,
    },
    Migration {
        version: 20260410113000,
        name: "agent_run_usage",
        migrate: v20260410113000_agent_run_usage::migrate,
    },
    Migration {
        version: 20260410124500,
        name: "chat_message_usage",
        migrate: v20260410124500_chat_message_usage::migrate,
    },
    Migration {
        version: 20260410143000,
        name: "upstream_provider_metadata",
        migrate: v20260410143000_upstream_provider_metadata::migrate,
    },
    Migration {
        version: 20260410153000,
        name: "chat_conversation_provider_origin",
        migrate: v20260410153000_chat_conversation_provider_origin::migrate,
    },
    Migration {
        version: 20260411190000,
        name: "delegated_sessions",
        migrate: v20260411190000_delegated_sessions::migrate,
    },
    Migration {
        version: 20260413043153,
        name: "drop_agent_lane_settings_fallback_harness",
        migrate: v20260413043153_drop_agent_lane_settings_fallback_harness::migrate,
    },
    Migration {
        version: 20260414060000,
        name: "verification_run_store",
        migrate: v20260414060000_verification_run_store::migrate,
    },
    Migration {
        version: 20260414123000,
        name: "drop_verification_metadata_column",
        migrate: v20260414123000_drop_verification_metadata_column::migrate,
    },
    Migration {
        version: 20260415164250,
        name: "merge_validation_mode_off",
        migrate: v20260415164250_merge_validation_mode_off::migrate,
    },
    Migration {
        version: 20260422140039,
        name: "chat_conversation_archived_at",
        migrate: v20260422140039_chat_conversation_archived_at::migrate,
    },
    Migration {
        version: 20260424090000,
        name: "ideation_analysis_base",
        migrate: v20260424090000_ideation_analysis_base::migrate,
    },
    Migration {
        version: 20260424150000,
        name: "agent_conversation_workspaces",
        migrate: v20260424150000_agent_conversation_workspaces::migrate,
    },
    Migration {
        version: 20260424193000,
        name: "chat_conversation_agent_mode",
        migrate: v20260424193000_chat_conversation_agent_mode::migrate,
    },
    Migration {
        version: 20260425154500,
        name: "agent_workspace_chat_mode",
        migrate: v20260425154500_agent_workspace_chat_mode::migrate,
    },
    Migration {
        version: 20260426093000,
        name: "agent_workspace_publication_events",
        migrate: v20260426093000_agent_workspace_publication_events::migrate,
    },
    Migration {
        version: 20260505090000,
        name: "agent_model_registry",
        migrate: v20260505090000_agent_model_registry::migrate,
    },
    Migration {
        version: 20260506131356,
        name: "agent_workspace_pr_descriptions",
        migrate: v20260506131356_agent_workspace_pr_descriptions::migrate,
    },
    Migration {
        version: 20260508103000,
        name: "agent_provider_settings",
        migrate: v20260508103000_agent_provider_settings::migrate,
    },
    Migration {
        version: 20260509090000,
        name: "release_notes_seen_version",
        migrate: v20260509090000_release_notes_seen_version::migrate,
    },
    Migration {
        version: 20260510185257,
        name: "chat_message_blocks_timeline",
        migrate: v20260510185257_chat_message_blocks_timeline::migrate,
    },
    Migration {
        version: 20260512093000,
        name: "startup_local_cleanup_markers",
        migrate: v20260512093000_startup_local_cleanup_markers::migrate,
    },
    Migration {
        version: 20260513143000,
        name: "orphan_worktree_cleanup_markers",
        migrate: v20260513143000_orphan_worktree_cleanup_markers::migrate,
    },
    Migration {
        version: 20260517153000,
        name: "agent_workspace_pr_supervision",
        migrate: v20260517153000_agent_workspace_pr_supervision::migrate,
    },
    Migration {
        version: 20260518113000,
        name: "agent_workspace_pr_supervision_recovery_index",
        migrate: v20260518113000_agent_workspace_pr_supervision_recovery_index::migrate,
    },
    Migration {
        version: 20260518230038,
        name: "agent_workspace_pr_comment_evidence",
        migrate: v20260518230038_agent_workspace_pr_comment_evidence::migrate,
    },
    Migration {
        version: 20260519180000,
        name: "agent_tasks",
        migrate: v20260519180000_agent_tasks::migrate,
    },
    Migration {
        version: 20260520125526,
        name: "atlassian_integrations",
        migrate: v20260520125526_atlassian_integrations::migrate,
    },
    Migration {
        version: 20260520150000,
        name: "atlassian_oauth",
        migrate: v20260520150000_atlassian_oauth::migrate,
    },
    Migration {
        version: 20260521150003,
        name: "agent_workspace_source_pull_request",
        migrate: v20260521150003_agent_workspace_source_pull_request::migrate,
    },
    Migration {
        version: 20260521222911,
        name: "agent_plan_mode",
        migrate: v20260521222911_agent_plan_mode::migrate,
    },
    Migration {
        version: 20260522090000,
        name: "agent_workspace_state_history",
        migrate: v20260522090000_agent_workspace_state_history::migrate,
    },
    Migration {
        version: 20260522093000,
        name: "ideation_session_flow",
        migrate: v20260522093000_ideation_session_flow::migrate,
    },
    Migration {
        version: 20260523070000,
        name: "plan_artifact_approvals",
        migrate: v20260523070000_plan_artifact_approvals::migrate,
    },
    Migration {
        version: 20260523145711,
        name: "plan_complexity_assessments",
        migrate: v20260523145711_plan_complexity_assessments::migrate,
    },
    Migration {
        version: 20260523152748,
        name: "agent_task_list_slices",
        migrate: v20260523152748_agent_task_list_slices::migrate,
    },
    Migration {
        version: 20260524170000,
        name: "execution_workspace_capacity",
        migrate: v20260524170000_execution_workspace_capacity::migrate,
    },
    Migration {
        version: 20260527033000,
        name: "agent_workspace_auto_publish",
        migrate: v20260527033000_agent_workspace_auto_publish::migrate,
    },
    Migration {
        version: 20260611110952,
        name: "question_skip_progress",
        migrate: v20260611110952_question_skip_progress::migrate,
    },
    Migration {
        version: 20260611152000,
        name: "question_metadata",
        migrate: v20260611152000_question_metadata::migrate,
    },
    Migration {
        version: 20260611191722,
        name: "agent_workspace_pr_automation_defaults",
        migrate: v20260611191722_agent_workspace_pr_automation_defaults::migrate,
    },
    Migration {
        version: 20260612124826,
        name: "provider_cli_management_policy",
        migrate: v20260612124826_provider_cli_management_policy::migrate,
    },
    Migration {
        version: 20260616182441,
        name: "external_issue_links",
        migrate: v20260616182441_external_issue_links::migrate,
    },
    Migration {
        version: 20260616182951,
        name: "linear_webhook_reconciliation",
        migrate: v20260616182951_linear_webhook_reconciliation::migrate,
    },
    Migration {
        version: 20260617121800,
        name: "agent_conversation_jira_issue_links",
        migrate: v20260617121800_agent_conversation_jira_issue_links::migrate,
    },
    Migration {
        version: 20260617122100,
        name: "linear_integration_settings",
        migrate: v20260617122100_linear_integration_settings::migrate,
    },
    Migration {
        version: 20260617122430,
        name: "agent_workspace_initial_auto_publish",
        migrate: v20260617122430_agent_workspace_initial_auto_publish::migrate,
    },
    Migration {
        version: 20260618123000,
        name: "agent_workspace_pr_review_monitoring",
        migrate: v20260618123000_agent_workspace_pr_review_monitoring::migrate,
    },
    Migration {
        version: 20260618134600,
        name: "review_pr_mode_checks",
        migrate: v20260618134600_review_pr_mode_checks::migrate,
    },
    Migration {
        version: 20260618181405,
        name: "agent_conversation_linear_issue_links",
        migrate: v20260618181405_agent_conversation_linear_issue_links::migrate,
    },
    Migration {
        version: 20260619093000,
        name: "agent_workspace_pr_review_artifacts",
        migrate: v20260619093000_agent_workspace_pr_review_artifacts::migrate,
    },
    Migration {
        version: 20260619144000,
        name: "durable_queued_messages",
        migrate: v20260619144000_durable_queued_messages::migrate,
    },
    Migration {
        version: 20260620075610,
        name: "provider_ticket_operations",
        migrate: v20260620075610_provider_ticket_operations::migrate,
    },
    Migration {
        version: 20260621201947,
        name: "ticket_canonical_branches",
        migrate: v20260621201947_ticket_canonical_branches::migrate,
    },
    Migration {
        version: 20260622103000,
        name: "agent_workspace_reviews",
        migrate: v20260622103000_agent_workspace_reviews::migrate,
    },
    Migration {
        version: 20260622162352,
        name: "agent_workspace_followup_provenance",
        migrate: v20260622162352_agent_workspace_followup_provenance::migrate,
    },
    Migration {
        version: 20260623074101,
        name: "clickup_integration_settings",
        migrate: v20260623074101_clickup_integration_settings::migrate,
    },
    Migration {
        version: 20260624114053,
        name: "granola_integration_settings",
        migrate: v20260624114053_granola_integration_settings::migrate,
    },
    Migration {
        version: 20260625115000,
        name: "custom_provider_binary",
        migrate: v20260625115000_custom_provider_binary::migrate,
    },
    Migration {
        version: 20260625153000,
        name: "agent_conversation_issues",
        migrate: v20260625153000_agent_conversation_issues::migrate,
    },
    Migration {
        version: 20260626092500,
        name: "custom_provider_env_file",
        migrate: v20260626092500_custom_provider_env_file::migrate,
    },
    Migration {
        version: 20260626191358,
        name: "codex_service_tier_settings",
        migrate: v20260626191358_codex_service_tier_settings::migrate,
    },
    Migration {
        version: 20260627104500,
        name: "agent_conversation_granola_note_links",
        migrate: v20260627104500_agent_conversation_granola_note_links::migrate,
    },
    Migration {
        version: 20260627183000,
        name: "agent_workspace_branch_mode",
        migrate: v20260627183000_agent_workspace_branch_mode::migrate,
    },
    Migration {
        version: 20260628010000,
        name: "workspace_review_child_conversation",
        migrate: v20260628010000_workspace_review_child_conversation::migrate,
    },
    Migration {
        version: 20260629101000,
        name: "workspace_review_gate",
        migrate: v20260629101000_workspace_review_gate::migrate,
    },
    Migration {
        version: 20260630120000,
        name: "ticketing_status_catalog",
        migrate: v20260630120000_ticketing_status_catalog::migrate,
    },
    Migration {
        version: 20260630123000,
        name: "workspace_review_policy_setting",
        migrate: v20260630123000_workspace_review_policy_setting::migrate,
    },
    Migration {
        version: 20260701143000,
        name: "workspace_review_runtime_settings",
        migrate: v20260701143000_workspace_review_runtime_settings::migrate,
    },
    Migration {
        version: 20260701152000,
        name: "workspace_review_runtime_global_scope",
        migrate: v20260701152000_workspace_review_runtime_global_scope::migrate,
    },
    Migration {
        version: 20260701174810,
        name: "workspace_review_hunk_annotations",
        migrate: v20260701174810_workspace_review_hunk_annotations::migrate,
    },
    Migration {
        version: 20260703143000,
        name: "task_validation_runs",
        migrate: v20260703143000_task_validation_runs::migrate,
    },
    Migration {
        version: 20260704193000,
        name: "automations_p1",
        migrate: v20260704193000_automations_p1::migrate,
    },
    Migration {
        version: 20260706113000,
        name: "agent_conversation_issue_identity",
        migrate: v20260706113000_agent_conversation_issue_identity::migrate,
    },
    Migration {
        version: 20260707113000,
        name: "automation_agent_completed_signal",
        migrate: v20260707113000_automation_agent_completed_signal::migrate,
    },
    Migration {
        version: 20260707120000,
        name: "automations_spec_artifact_id",
        migrate: v20260707120000_automations_spec_artifact_id::migrate,
    },
    Migration {
        version: 20260708120000,
        name: "automation_run_plan_gate",
        migrate: v20260708120000_automation_run_plan_gate::migrate,
    },
    Migration {
        version: 20260708130511,
        name: "workspace_review_autofix_setting",
        migrate: v20260708130511_workspace_review_autofix_setting::migrate,
    },
    Migration {
        version: 20260708131548,
        name: "chat_conversation_coordination_mode",
        migrate: v20260708131548_chat_conversation_coordination_mode::migrate,
    },
    Migration {
        version: 20260710000000,
        name: "task_branch_base",
        migrate: v20260710000000_task_branch_base::migrate,
    },
    Migration {
        version: 20260710003315,
        name: "execution_plan_halt_mode",
        migrate: v20260710003315_execution_plan_halt_mode::migrate,
    },
    Migration {
        version: 20260710134609,
        name: "notifications_table",
        migrate: v20260710134609_notifications_table::migrate,
    },
    Migration {
        version: 20260710201548,
        name: "notification_settings",
        migrate: v20260710201548_notification_settings::migrate,
    },
    Migration {
        version: 20260711151804,
        name: "personas",
        migrate: v20260711151804_personas::migrate,
    },
    Migration {
        version: 20260712090000,
        name: "validation_run_content_fingerprints",
        migrate: v20260712090000_validation_run_content_fingerprints::migrate,
    },
    Migration {
        version: 20260712153932,
        name: "agent_workspace_pr_review_auto_approve",
        migrate: v20260712153932_agent_workspace_pr_review_auto_approve::migrate,
    },
    Migration {
        version: 20260712155425,
        name: "ui_feature_flag_overrides",
        migrate: v20260712155425_ui_feature_flag_overrides::migrate,
    },
    Migration {
        version: 20260712162657,
        name: "persona_builder_agent_mode",
        migrate: v20260712162657_persona_builder_agent_mode::migrate,
    },
    Migration {
        version: 20260712190416,
        name: "branch_update_authority",
        migrate: v20260712190416_branch_update_authority::migrate,
    },
    Migration {
        version: 20260713063349,
        name: "persona_run_attribution",
        migrate: v20260713063349_persona_run_attribution::migrate,
    },
    Migration {
        version: 20260713131052,
        name: "disable_auto_followup_by_default",
        migrate: v20260713131052_disable_auto_followup_by_default::migrate,
    },
    Migration {
        version: 20260714184430,
        name: "workspace_review_auto_merge_guard",
        migrate: v20260714184430_workspace_review_auto_merge_guard::migrate,
    },
    Migration {
        version: 20260715013854,
        name: "model_native_plan_verification",
        migrate: v20260715013854_model_native_plan_verification::migrate,
    },
    Migration {
        version: 20260715170000,
        name: "automation_authoring_state",
        migrate: v20260715170000_automation_authoring_state::migrate,
    },
    Migration {
        version: 20260715172058,
        name: "persona_update_draft_provenance",
        migrate: v20260715172058_persona_update_draft_provenance::migrate,
    },
    Migration {
        version: 20260715181627,
        name: "agent_conversation_capabilities",
        migrate: v20260715181627_agent_conversation_capabilities::migrate,
    },
    Migration {
        version: 20260715183000,
        name: "automation_ideation_signal",
        migrate: v20260715183000_automation_ideation_signal::migrate,
    },
    Migration {
        version: 20260715194617,
        name: "scripted_agent_workflows",
        migrate: v20260715194617_scripted_agent_workflows::migrate,
    },
    Migration {
        version: 20260716154318,
        name: "manual_role_defaults",
        migrate: v20260716154318_manual_role_defaults::migrate,
    },
    Migration {
        version: 20260716170840,
        name: "persona_project_scope",
        migrate: v20260716170840_persona_project_scope::migrate,
    },
    Migration {
        version: 20260716202015,
        name: "workspace_review_bypass_and_bound_agent",
        migrate: v20260716202015_workspace_review_bypass_and_bound_agent::migrate,
    },
    Migration {
        version: 20260716204027,
        name: "conversation_folder_references",
        migrate: v20260716204027_conversation_folder_references::migrate,
    },
    Migration {
        version: 20260716210000,
        name: "supervised_native_task_pipeline",
        migrate: v20260716210000_supervised_native_task_pipeline::migrate,
    },
    Migration {
        version: 20260717152713,
        name: "persona_builder_result_binding",
        migrate: v20260717152713_persona_builder_result_binding::migrate,
    },
    Migration {
        version: 20260717152714,
        name: "persona_artifact_history",
        migrate: v20260717152714_persona_artifact_history::migrate,
    },
    Migration {
        version: 20260717235338,
        name: "github_cli_token_environment_setting",
        migrate: v20260717235338_github_cli_token_environment_setting::migrate,
    },
    Migration {
        version: 20260718014631,
        name: "mcp_policy_overrides",
        migrate: v20260718014631_mcp_policy_overrides::migrate,
    },
    Migration {
        version: 20260718162852,
        name: "clear_detected_validation_commands",
        migrate: v20260718162852_clear_detected_validation_commands::migrate,
    },
    Migration {
        version: 20260718182035,
        name: "add_tasks_enabled_setting",
        migrate: v20260718182035_add_tasks_enabled_setting::migrate,
    },
    Migration {
        version: 20260720102513,
        name: "add_tasks_feature_state",
        migrate: v20260720102513_add_tasks_feature_state::migrate,
    },
    Migration {
        version: 20260720131416,
        name: "review_pr_disable_pr_automation",
        migrate: v20260720131416_review_pr_disable_pr_automation::migrate,
    },
    Migration {
        version: 20260720140000,
        name: "remove_legacy_claude_team",
        migrate: v20260720140000_remove_legacy_claude_team::migrate,
    },
    Migration {
        version: 20260720200633,
        name: "auto_verify_draft_plans",
        migrate: v20260720200633_auto_verify_draft_plans::migrate,
    },
    Migration {
        version: 20260721190000,
        name: "workspace_review_fixer_attempt",
        migrate: v20260721190000_workspace_review_fixer_attempt::migrate,
    },
    Migration {
        version: 20260722022339,
        name: "usage_capture_provenance_and_raw_snapshots",
        migrate: v20260722022339_usage_capture_provenance_and_raw_snapshots::migrate,
    },
    Migration {
        version: 20260722132100,
        name: "automation_run_goal_item",
        migrate: v20260722132100_automation_run_goal_item::migrate,
    },
    Migration {
        version: 20260723012559,
        name: "agent_workspace_pr_metadata_decision",
        migrate: v20260723012559_agent_workspace_pr_metadata_decision::migrate,
    },
    Migration {
        version: 20260723065349,
        name: "pr_autofix_completed_supervision_history",
        migrate: v20260723065349_pr_autofix_completed_supervision_history::migrate,
    },
    Migration {
        version: 20260723100604,
        name: "app_state_update_channel",
        migrate: v20260723100604_app_state_update_channel::migrate,
    },
    Migration {
        version: 20260724113627,
        name: "agent_task_delegate_assignments",
        migrate: v20260724113627_agent_task_delegate_assignments::migrate,
    },
    Migration {
        version: 20260724130000,
        name: "plan_blueprints",
        migrate: v20260724130000_plan_blueprints::migrate,
    },
    Migration {
        version: 20260724141500,
        name: "workspace_review_requested_changes",
        migrate: v20260724141500_workspace_review_requested_changes::migrate,
    },
    Migration {
        version: 20260724222347,
        name: "agent_task_assignment_planned_run_identity",
        migrate: v20260724222347_agent_task_assignment_planned_run_identity::migrate,
    },
    Migration {
        version: 20260725164704,
        name: "agent_workspace_repair_attempts",
        migrate: v20260725164704_agent_workspace_repair_attempts::migrate,
    },
    Migration {
        version: 20260727115037,
        name: "agent_workspace_publication_metadata_receipts",
        migrate: v20260727115037_agent_workspace_publication_metadata_receipts::migrate,
    },
    Migration {
        version: 20260728155615,
        name: "agent_conversation_mutes",
        migrate: v20260728155615_agent_conversation_mutes::migrate,
    },
    Migration {
        version: 20260728162405,
        name: "rx_native_team_runtime",
        migrate: v20260728162405_rx_native_team_runtime::migrate,
    },
    Migration {
        version: 20260728183000,
        name: "workspace_review_plan_context",
        migrate: v20260728183000_workspace_review_plan_context::migrate,
    },
    Migration {
        version: 20260730000304,
        name: "chat_message_blocks_created_at_index",
        migrate: v20260730000304_chat_message_blocks_created_at_index::migrate,
    },
    Migration {
        version: 20260730025727,
        name: "chat_message_blocks_thinking_kind",
        migrate: v20260730025727_chat_message_blocks_thinking_kind::migrate,
    },
    Migration {
        version: 20260730151837,
        name: "agent_workspace_repair_ci_rerun_reservations",
        migrate: v20260730151837_agent_workspace_repair_ci_rerun_reservations::migrate,
    },
    Migration {
        version: 20260730161032,
        name: "agent_workspace_pr_autofix_completion_evidence",
        migrate: v20260730161032_agent_workspace_pr_autofix_completion_evidence::migrate,
    },
    Migration {
        version: 20260731023949,
        name: "agent_run_identity",
        migrate: v20260731023949_agent_run_identity::migrate,
    },
    Migration {
        version: 20260731111346,
        name: "purge_empty_thinking_blocks",
        migrate: v20260731111346_purge_empty_thinking_blocks::migrate,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationProgress {
    pub completed_units: u32,
    pub total_units: u32,
    pub elapsed_ms: u128,
}

/// Run all pending migrations on the database.
pub fn run_migrations(conn: &Connection) -> AppResult<()> {
    run_migrations_with_observer(conn, |_| {})
}

/// Runs pending migrations and reports real completed/pending units.
///
/// # Errors
///
/// Returns the first migration or schema-version persistence error. The
/// observer is never advanced for the failed migration.
pub fn run_migrations_with_observer(
    conn: &Connection,
    observer: impl FnMut(MigrationProgress),
) -> AppResult<()> {
    run_pending_migrations(conn, MIGRATIONS, observer)
}

fn run_pending_migrations(
    conn: &Connection,
    migrations: &[Migration],
    mut observer: impl FnMut(MigrationProgress),
) -> AppResult<()> {
    let started_at = std::time::Instant::now();
    // Create migrations table if it doesn't exist
    create_migrations_table(conn)?;

    let mut applied_versions = get_applied_migration_versions(conn)?;
    let pending = migrations
        .iter()
        .filter(|migration| !applied_versions.contains(&migration.version))
        .collect::<Vec<_>>();
    let total_units = u32::try_from(pending.len()).unwrap_or(u32::MAX);
    let mut completed_units = 0u32;
    observer(MigrationProgress {
        completed_units,
        total_units,
        elapsed_ms: started_at.elapsed().as_millis(),
    });

    // Run registered migrations sequentially. Membership checks repair dev and
    // branch databases that have a later version recorded while missing an
    // earlier migration added on this branch.
    for migration in pending {
        tracing::info!(
            "Running migration v{}: {}",
            migration.version,
            migration.name
        );

        if let Err(error) = (migration.migrate)(conn) {
            tracing::error!(
                migration_version = migration.version,
                completed_units,
                total_units,
                elapsed_ms = started_at.elapsed().as_millis(),
                "Database migration failed"
            );
            return Err(error);
        }
        set_schema_version(conn, migration.version)?;
        applied_versions.insert(migration.version);
        completed_units = completed_units.saturating_add(1);
        observer(MigrationProgress {
            completed_units,
            total_units,
            elapsed_ms: started_at.elapsed().as_millis(),
        });

        tracing::info!("Migration v{} complete", migration.version);
    }

    tracing::info!(
        completed_units,
        total_units,
        elapsed_ms = started_at.elapsed().as_millis(),
        "Database migration pass completed"
    );
    Ok(())
}

#[cfg(test)]
pub(super) fn run_migrations_through(conn: &Connection, target_version: i64) -> AppResult<()> {
    create_migrations_table(conn)?;

    let mut applied_versions = get_applied_migration_versions(conn)?;

    for migration in MIGRATIONS {
        if migration.version <= target_version && !applied_versions.contains(&migration.version) {
            (migration.migrate)(conn)?;
            set_schema_version(conn, migration.version)?;
            applied_versions.insert(migration.version);
        }
    }

    Ok(())
}

#[cfg(test)]
pub(super) fn latest_registered_migration_version() -> i64 {
    MIGRATIONS
        .last()
        .map(|migration| migration.version)
        .expect("migration registry should not be empty")
}

#[cfg(test)]
mod migration_progress_tests;

/// Create the migrations tracking table
fn create_migrations_table(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

/// Get the current schema version
pub fn get_schema_version(conn: &Connection) -> AppResult<i64> {
    let result: Result<i64, _> = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    );

    result.map_err(|e| AppError::Database(e.to_string()))
}

fn get_applied_migration_versions(conn: &Connection) -> AppResult<HashSet<i64>> {
    let mut statement = conn
        .prepare("SELECT version FROM schema_migrations")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let versions = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|e| AppError::Database(e.to_string()))?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(versions)
}

/// Set the schema version after a migration
fn set_schema_version(conn: &Connection, version: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO schema_migrations (version) VALUES (?1)",
        [version],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}
