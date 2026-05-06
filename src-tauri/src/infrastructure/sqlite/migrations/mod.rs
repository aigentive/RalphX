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
// Existing databases have schema_migrations tracking what version they're at.
// Only migrations newer than their current version will run.

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
mod v20_merge_validation_mode;
mod v21_questions_permissions;
mod v22_project_active_plan;
mod v23_plan_selection_stats;
mod v24_memory_framework;
mod v26_running_agent_worktree;
mod v25_seed_artifact_buckets;
mod v30_update_max_concurrent_default;
mod v27_merge_strategy;
mod v28_default_rebase_squash;
mod v29_repair_schema_drift;
mod v2_add_dependency_reason;
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
mod v5_add_review_summary_issues;
mod v6_review_issues;
mod v7_session_status_converted_to_accepted;
mod v8_task_git_fields;
mod v9_project_git_fields;
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
mod v80_dependencies_acknowledged;
mod v81_external_session_reliability_backfill;
mod v20260325120000_app_state_execution_halt_mode;
mod v20260327233752_pending_initial_prompt;
mod v20260325131500_execution_ideation_allocation_settings;
mod v20260328194000_ideation_followup_provenance;
mod v20260329103000_review_note_followup_session;
mod v20260329113000_ideation_blocker_fingerprint;

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
mod v4_add_blocked_reason_tests;
#[cfg(test)]
mod v6_review_issues_tests;
#[cfg(test)]
mod v7_session_status_converted_to_accepted_tests;
#[cfg(test)]
mod v8_task_git_fields_tests;
#[cfg(test)]
mod v9_project_git_fields_tests;
#[cfg(test)]
mod v49_backfill_execution_plans_tests;
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
mod v71_add_target_project_to_proposals_tests;
#[cfg(test)]
mod v72_cross_project_check_tests;
#[cfg(test)]
mod v73_proposal_migrated_from_tests;
#[cfg(test)]
mod v76_session_origin_tests;
#[cfg(test)]
mod v81_external_session_reliability_backfill_tests;
#[cfg(test)]
mod v20260325120000_app_state_execution_halt_mode_tests;
#[cfg(test)]
mod v20260325131500_execution_ideation_allocation_settings_tests;
#[cfg(test)]
mod v20260327233752_pending_initial_prompt_tests;
#[cfg(test)]
mod v20260328194000_ideation_followup_provenance_tests;
#[cfg(test)]
mod v20260329103000_review_note_followup_session_tests;
#[cfg(test)]
mod v20260329113000_ideation_blocker_fingerprint_tests;
mod v20260328210000_proposal_affected_paths;
#[cfg(test)]
mod v20260328210000_proposal_affected_paths_tests;
mod v20260329080000_acceptance_status;
#[cfg(test)]
mod v20260329080000_acceptance_status_tests;
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
mod v20260410124500_chat_message_usage;
#[cfg(test)]
mod v20260410124500_chat_message_usage_tests;
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

/// Current schema version - bump this when adding a new migration
pub const SCHEMA_VERSION: i64 = 20260505090000;

/// Migration function signature
type MigrationFn = fn(&Connection) -> AppResult<()>;

/// Migration definition
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
];

/// Run all pending migrations on the database
pub fn run_migrations(conn: &Connection) -> AppResult<()> {
    // Create migrations table if it doesn't exist
    create_migrations_table(conn)?;

    // Get current version
    let current_version = get_schema_version(conn)?;

    // Run migrations sequentially
    for migration in MIGRATIONS {
        if current_version < migration.version {
            tracing::info!(
                "Running migration v{}: {}",
                migration.version,
                migration.name
            );

            (migration.migrate)(conn)?;
            set_schema_version(conn, migration.version)?;

            tracing::info!("Migration v{} complete", migration.version);
        }
    }

    Ok(())
}

#[cfg(test)]
pub(super) fn run_migrations_through(conn: &Connection, target_version: i64) -> AppResult<()> {
    create_migrations_table(conn)?;

    let mut current_version = get_schema_version(conn)?;

    for migration in MIGRATIONS {
        if current_version < migration.version && migration.version <= target_version {
            (migration.migrate)(conn)?;
            set_schema_version(conn, migration.version)?;
            current_version = migration.version;
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

/// Set the schema version after a migration
fn set_schema_version(conn: &Connection, version: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO schema_migrations (version) VALUES (?1)",
        [version],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}
