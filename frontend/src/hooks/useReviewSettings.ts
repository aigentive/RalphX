import { invoke } from "@tauri-apps/api/core";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

// ============================================================================
// Types (snake_case from Rust — no rename_all on response structs)
// ============================================================================

export interface ReviewSettings {
  require_human_review: boolean;
  require_workspace_review: boolean;
  max_fix_attempts: number;
  max_revision_cycles: number;
  /** Stored-only; follow-up decision pending */
  ai_review_enabled: boolean;
  /** Stored-only; follow-up decision pending */
  ai_review_auto_fix: boolean;
  /** Stored-only; follow-up decision pending */
  require_fix_approval: boolean;
  auto_create_followup_agent_conversation: boolean;
  autofix_workspace_review_blocking_findings: boolean;
  workspace_review_fixer_cycle_cap: number;
  run_task_validations: boolean;
}

/** Only the primary policy fields are accepted for update. */
export interface UpdateReviewSettingsInput {
  requireHumanReview?: boolean;
  requireWorkspaceReview?: boolean;
  maxFixAttempts?: number;
  maxRevisionCycles?: number;
  autoCreateFollowupAgentConversation?: boolean;
  autofixWorkspaceReviewBlockingFindings?: boolean;
  workspaceReviewFixerCycleCap?: number;
  runTaskValidations?: boolean;
}

// ============================================================================
// Query key
// ============================================================================

export const reviewSettingsKeys = {
  all: ["review-settings"] as const,
};

// ============================================================================
// useReviewSettings
// ============================================================================

/**
 * Fetch the global review policy settings.
 */
export function useReviewSettings() {
  return useQuery<ReviewSettings>({
    queryKey: reviewSettingsKeys.all,
    queryFn: () => invoke<ReviewSettings>("get_review_settings"),
  });
}

// ============================================================================
// useUpdateReviewSettings
// ============================================================================

/**
 * Mutation to update primary review policy fields.
 * Ballast fields (ai_review_enabled etc.) are preserved server-side.
 */
export function useUpdateReviewSettings() {
  const queryClient = useQueryClient();

  return useMutation<ReviewSettings, string, UpdateReviewSettingsInput>({
    mutationFn: (input: UpdateReviewSettingsInput) =>
      invoke<ReviewSettings>("update_review_settings", { input }),
    onSuccess: (updated) => {
      queryClient.setQueryData<ReviewSettings>(reviewSettingsKeys.all, updated);
    },
  });
}
