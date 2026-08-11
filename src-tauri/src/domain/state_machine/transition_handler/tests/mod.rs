// transition_handler test modules
//
// Organized by source module and concern area.
// Each file contains tests for a specific subsystem.
// Shared helpers: use super::helpers::*;

mod helpers;

// Tests extracted from side_effects.rs embedded #[cfg(test)] block
mod commit_message;
mod merge_arbitration;
mod merge_helpers_branch_resolution;
mod merge_helpers_metadata;
mod merge_helpers_workflow;
mod merge_validation_tests;

// Tests extracted from tests.rs (integration tests with mock services)
mod execution_recovery_fallback_tests;
mod execution_state;
mod integration_branch_discovery;
mod merge_retry;
mod merge_workflow;
mod metadata_skip_guard;
mod structural_git_error_tests;
mod transitions_agents;
mod transitions_basic;
mod transitions_branchless_merge;

// Tests for merge-hang fixes (step 0, deadline, timeouts, config)
mod merge_cleanup;

// Test quality overhaul: adversarial mocks, ordering assertions, outcome coverage
mod test_quality_overhaul;

// Real git repo integration tests: merge strategy dispatch with actual git operations
mod real_git_integration;

// Tests for update_plan_from_main: bringing plan branches up-to-date before task merge
mod plan_update_from_main;

// Tests extracted from inline #[cfg(test)] blocks in production modules
mod merge_helpers_inline;
mod merge_helpers_inline_2;
mod merge_validation_inline;
mod metadata_builder_tests;
mod metadata_builder_tests_2;
mod on_enter_states_tests;
mod symlink_idempotent;
mod worktree_setup_collision;

// Tests for has_prior_validation_failure guard (validation metadata checks)
mod merge_helpers_validation_guard;

// Tests for update_source_from_target: bringing feature branches up-to-date before merge
mod source_update_from_target;

// Tests for merge progress event emission and phase ordering
mod progress_events;

// Regression tests: on_enter_dispatch coverage for all Merging state entry paths
mod merge_dispatch_agent_spawn;

// Orchestration chain integration tests: real git + real DB + MockChatService
// Verifies merger agent is spawned: B2 (merge conflict) and C1 (AutoFix validation failure)
mod orchestration_chain_tests;

// Tests for spawn failure recovery: on_enter(Merging) + failing chat service
// Covers Fix 2 (commit 189e8eaf): AttemptFailed events + retry budget exhaustion
mod spawn_failure_recovery_tests;

// Tests for reviewer spawn failure recovery: on_enter(Reviewing) with missing worktree or failing chat service
mod reviewer_spawn_failure_tests;

// Tests for was_queued guard in on_enter for all 4 agent-spawning states
// RC#2: double on_enter (Ok(was_queued: true)) returns no-op instead of recording spawn failure
mod on_enter_already_running_tests;

// RC#6: plan_update_conflict must create merge-* worktree before spawning merger agent
mod plan_update_conflict_worktree;

// Merge pipeline gap tests: 6 gaps from test audit (real git + real DB + MockChatService)
// Gap 1: E2E conflict resolved → Merged
// Gap 2: Two-phase plan-update + task-merge
// Gap 3: Worktree cleanup assertion in pre_merge_cleanup
// Gap 4: Post-conflict auto-complete failure
// Gap 5: Attempt counter persistence across retry cycles
// Gap 6: Source update with existing worktree fallback
mod merge_pipeline_gaps;

// RC#8 + RC#9 + RC#10: task worktree cleanup, source update fallback, metadata preservation
mod rc8_rc9_rc10_regression;

// RC#4: rebase worktree double-delete ownership contract
// Inner (try_rebase_squash_merge_in_worktree Step 5) removed — outer caller owns lifecycle.
// pre_delete_worktree guards with exists() to skip paths never created.
mod rc4_rebase_double_delete;

// RC#12 + RC#13: stale merge worktree between merge phases
// RC#12: merge-{id} leftover from plan_update phase blocks task_merge retry
// RC#13: source_update_conflict doesn't clean stale merge-{id} (same as RC#6 pattern)
// Bonus: pre_merge_cleanup aborts stale MERGE_HEAD in task worktree
mod rc12_rc13_stale_worktree;

// Fix 5: source_update_conflict completion path cleans up merge worktree
// Verifies: worktree deleted, task.worktree_path = None, conflict_type cleared from metadata
mod source_update_conflict_worktree_cleanup;

// RC#14: Layer 2 self-healing guard in on_enter(Executing) for deleted branches
// Self-heal detects deleted branch, cleans orphaned worktrees, creates fresh branch
// Tests: branch detection, orphaned worktree cleanup, stored vs expected path cleanup
mod rc14_deleted_branch_self_heal;

// Merge target resolution regression tests: task with plan branch must merge to plan, not main
// Test 1: task_with_plan_branch_merges_to_plan_not_main
// Test 2: check_already_merged_detects_prior_merge_on_plan_branch
// Test 3: metadata_toctou_guard_survives_conflict_metadata
// Test 4: plan_update_conflict_retry_uses_correct_target
// Test 5: plan_branch_repo_none_fallback_uses_metadata_guard
mod merge_target_resolution;

// Branch freshness timeout tests: config, env override, YAML deserialization
mod branch_freshness_timeout;

// Merge pipeline timeout integration tests: real git + memory repos + mock agents
// Covers: lsof timeout, pre-merge cleanup, stale index.lock, stale worktree, full E2E
mod merge_pipeline_timeout_tests;

// PlanMerge check_already_merged guard: ensures PlanMerge tasks are not falsely
// completed by the tautological plan-branch defense-in-depth check
mod plan_merge_already_merged;

// Integration tests for merge pipeline failure scenarios from logs-21:
//   RC2 authoritative deferral gate (running_count > 0 defers merge to main)
//   RC5 retry pipeline (MergeIncomplete → PendingMerge → Merged)
//   RC5 log message distinctness (structural grep-ability guard)
mod merge_pipeline_failure_scenarios;

// RC pipeline integration tests: RC1 lsof kill_on_drop + RC2 TOCTOU retry scenarios
// Scenario 1 (RC1): cleanup timeout doesn't kill merge attempt; kill_on_drop terminates lsof
// Scenario 2 (RC2): try_retry_main_merges always fires regardless of running_count
mod rc_pipeline_integration_tests;

// Merge pipeline round 2 tests: cleanup timeout config, step_0b timeout config,
// state freshness guard (ghost merge prevention)
mod merge_pipeline_round2_tests;

// Merge pipeline round 3 tests: os_thread_timeout, lsof +d, spawn_blocking,
// prior rebase conflict detection, force branch deletion
mod merge_pipeline_round3_tests;

// Merge pipeline round 4 tests: Step 3 prune_worktrees timeout via run_cleanup_step,
// settle sleep watchdog (os_thread_timeout immune to tokio timer starvation)
mod merge_pipeline_round4_tests;

// Post-merge cascade stop + resolve_task_base_branch merged branch guard
// Fix C: cascade stop sibling tasks after plan merge
// Fix D: refuse to resurrect merged plan branches
mod post_merge_cascade_tests;

// Plan branch status guard tests: on_enter(Executing/ReExecuting) blocks Merged/Abandoned branches
mod plan_branch_guard_tests;

// Step reset on re-execution entry tests: reset_all_to_pending called in Executing/ReExecuting
// Verifies stale steps are reset to Pending and step:updated event is emitted
mod step_reset_on_enter_tests;

// Integration tests: merged-branch guards working together (cascade + on_enter + base branch)
// Tests Guards B/C/D/E cooperating across multi-task plan scenarios
mod merged_branch_guard_integration;

// Regression tests for source_conflict_resolved metadata flag.
// Prevents the source_update_conflict retry loop: after agent resolves source←target conflict,
// retry must use squash-only (not rebase) to avoid dropping the agent's merge commit.
mod source_conflict_resolved_tests;

// Fast cleanup tests: remove_worktree_fast (rm -rf + prune) and conditional settle sleep
mod fast_cleanup_tests;

// Transient merge error inline retry tests (ROOT CAUSE #5):
// is_transient_merge_error classification, deferred vs MergeIncomplete, branch re-check
mod merge_outcome_transient_retry_tests;

// BranchFreshnessConflict transition tests: execution/review states must not bypass the
// dedicated branch-update workflow, plus event classification validation.
mod transitions_freshness;

// FreshnessMetadata unit tests: from_task_metadata, merge_into, clear_from, serde round-trips
mod freshness_metadata_tests;

// Stale freshness metadata cleanup on on_enter(ReviewPassed):
//   clears freshness_origin_state + freshness_count_incremented_by via RoutingOnly scope
mod review_passed_metadata_cleanup_tests;

// Freshness return routing unit tests: routing decision for each origin state,
// metadata clearing on return, flag detection for early-return trigger
mod merge_freshness_return_tests;

// Phase 1 GUARD tests: first-attempt skip, parallel worktree deletion, deferred orphan scan
mod phase1_guard_tests;

// Integration tests for freshness return-path routing:
//   test 1: executing origin (plan_update_conflict) → Ready after merge resolution
//   test 2: reviewing origin (source_update_conflict) → PendingReview after merge resolution
mod freshness_return_path_integration_tests;

// Phase 2 MERGE + Phase 3 CLEANUP tests: immediate Merged status, deferred cleanup,
// pending_cleanup metadata, startup resumption
mod phase2_phase3_merge_tests;

// Locked worktree tests: empirical verification of unlock + double-force behavior
// RC1: single --force fails on locked worktrees; -f -f and unlock+prune succeed
mod locked_worktree_tests;

// Integration tests for conflict marker scan before reviewer spawn (Fix 2) and
// BranchFreshnessConflict metadata persistence during on_enter(Reviewing)
mod reviewing_conflict_marker_tests;

// Compatibility tests for recovering persisted pre-dedicated-workflow merge worktrees.
// New freshness conflicts never enter Merging; these cases retain safe recovery for
// legacy rows with freshness-conflict metadata.
mod freshness_merge_worktree_tests;

// Cascade stop poller cleanup + post_merge_cleanup PR fork (AD11, AD17):
//   1. cascade_stop calls stop_polling for each sibling BEFORE persist_status_change
//   2. cascade_stop decrements running_count for PR-mode Merging siblings
//   3. post_merge_cleanup PR mode: delete_remote_branch for plan + task branches
//   4. push-to-main mode: delete_remote_branch NOT called
//   5. no poller registry: cascade proceeds normally without panic
mod cascade_poller_cleanup_tests;

// TOCTOU guard tests for pre_merge_cleanup Steps 2 and 4:
//   step2_guard_merging_skips_task_worktree_deletion
//   step2_guard_pending_merge_proceeds_with_task_worktree_deletion
//   step4_guard_merging_skips_merge_worktree_deletion
//   step4_guard_pending_merge_proceeds_with_merge_worktree_deletion
mod toctou_merge_guard_tests;

// PR-mode state machine integration tests (Phase 3 GitHub PR integration):
//   1. existing pr_number → push_branch + mark_pr_ready (no create_draft_pr)
//   2. no pr_number → create_draft_pr + mark_pr_ready
//   3. pr_eligible=false → skips PR path entirely
//   4. re-entry guard: pr_polling_active=true, no registry → proceeds
//   5. AD14: PR-polling task in Merging does not block PendingMerge task
//   6. post_merge_cleanup idempotency: Merged plan branch → early return
//   7. no github_service wired → falls through to push-to-main
mod pr_mode_state_machine_tests;

// Resilience tests for pre_delete_worktree second-chance fallback (Fix 3):
//   1. Removes a real registered git worktree (happy path)
//   2. No-op when path doesn't exist
//   3. Removes a plain (non-registered) directory via rm-rf fallback
//   4. Second-chance after transient lock: permissions restored during 100ms sleep (Unix)
mod pre_delete_worktree_resilience_tests;

// Unit tests for is_merge_worktree_path(): prefix detection for all merge-pipeline
// worktree basename prefixes (merge-, rebase-, source-update-, plan-update-).
mod is_merge_worktree_path_tests;

// Integration tests for worktree restoration — three layers:
//   Test 1 (re_review_worktree_restore): restore_task_worktree() direct call (L3)
//   Test 2 (merging_to_pending_review_worktree_path_reset): L1 via state machine transition
//   Test 3 (on_enter_reviewing_restores_merge_prefixed_worktree): L2 via on_enter(Reviewing)
mod worktree_restore_tests;
