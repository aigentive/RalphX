import { useCallback } from "react";

import {
  AgentWorkspaceHttpError,
  chatApi,
  type AgentWorkspaceReviewStartConfirmation,
  type AgentWorkspaceReviewStartPreview,
  type AgentWorkspaceReviewContext,
  type AgentWorkspaceReviewFixerConfirmation,
} from "@/api/chat";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import { useRoleRuntimeConfirmation } from "./useRoleRuntimeConfirmation";

function reviewTargetLabel(preview: AgentWorkspaceReviewStartPreview): string {
  const target = preview.target;
  if (!target) return "the current workspace changes";
  return target.scope === "selected_source"
    ? `the selected source (${target.headRef})`
    : "the current workspace changes";
}

function reviewStartDescription(preview: AgentWorkspaceReviewStartPreview): string {
  const target = reviewTargetLabel(preview);
  if (!preview.willDisableAutoMerge) {
    return `A reviewer run will start for ${target}.`;
  }
  const pr = preview.prNumber ? ` on PR #${preview.prNumber}` : "";
  const restoreTiming = preview.restoreAfterPublish
    ? "It will resume after the reviewed local changes are published."
    : "It will resume after this remote-head Review passes.";
  return `GitHub auto-merge is enabled${pr}. RalphX will temporarily disable it before starting a reviewer run for ${target}. ${restoreTiming}`;
}

function isWorkspaceReviewStartConflict(error: unknown): boolean {
  return error instanceof AgentWorkspaceHttpError && error.status === 409;
}

export function useWorkspaceReviewActions({
  conversationId,
  projectId,
  onStartReview,
  onStartFixer,
}: {
  conversationId: string | null;
  projectId: string | null;
  onStartReview: (input: {
    force: boolean;
    confirmation: AgentWorkspaceReviewStartConfirmation;
    runtimeOverride: ManualRoleRuntimeSelection;
  }) => Promise<unknown>;
  onStartFixer: (input: {
    confirmation: AgentWorkspaceReviewFixerConfirmation;
    runtimeOverride: ManualRoleRuntimeSelection;
  }) => Promise<unknown>;
}) {
  const { confirmRoleRuntime, confirmationDialogProps, ConfirmationDialog } =
    useRoleRuntimeConfirmation({ conversationId, projectId });

  const startReview = useCallback(
    (force: boolean) => {
      if (!conversationId) return;
      let preview: AgentWorkspaceReviewStartPreview | null = null;
      void confirmRoleRuntime({
        role: "workspace_reviewer",
        title: "Start Workspace Review?",
        description: "Checking the current review target and GitHub auto-merge state…",
        confirmText: "Start review",
        pendingText: "Starting review…",
        prepareDescription: async () => {
          preview = await chatApi.getAgentWorkspaceReviewStartPreview(conversationId);
          return reviewStartDescription(preview);
        },
        onConfirm: async (runtimeOverride) => {
          if (!preview) {
            throw new Error("Workspace Review preparation did not finish");
          }
          await onStartReview({
            force,
            confirmation: preview.confirmation,
            runtimeOverride,
          });
        },
        recoverFromError: async (error) => {
          if (!isWorkspaceReviewStartConflict(error)) {
            return null;
          }
          const refreshedPreview =
            await chatApi.getAgentWorkspaceReviewStartPreview(conversationId);
          preview = refreshedPreview;
          return {
            description: `The review target changed. ${reviewStartDescription(refreshedPreview)} Confirm the updated details to start the review.`,
          };
        },
      });
    },
    [confirmRoleRuntime, conversationId, onStartReview],
  );

  const startFixer = useCallback(
    (context: AgentWorkspaceReviewContext) => {
      if (!conversationId) return;
      const target = context.target;
      const monitor = context.monitor;
      if (
        !target ||
        !monitor.reviewArtifactId ||
        !monitor.reviewArtifactVersion ||
        !monitor.reviewBlockingFingerprint
      ) return;
      const confirmation: AgentWorkspaceReviewFixerConfirmation = {
        targetScope: target.scope,
        diffFingerprint: target.diffFingerprint,
        artifactId: monitor.reviewArtifactId,
        artifactVersion: monitor.reviewArtifactVersion,
        blockingFingerprint: monitor.reviewBlockingFingerprint,
      };
      void confirmRoleRuntime({
        role: "workspace_repair",
        title: "Fix blocking Workspace Review issues?",
        description: monitor.reviewBlockingSummary
          ? `The Repair agent will address: ${monitor.reviewBlockingSummary}`
          : "The Repair agent will address the current blocking findings.",
        confirmText: "Fix issues",
        pendingText: "Starting repair…",
        onConfirm: (runtimeOverride) =>
          onStartFixer({ confirmation, runtimeOverride }),
      });
    },
    [confirmRoleRuntime, conversationId, onStartFixer],
  );

  return {
    startReview,
    startFixer,
    confirmationDialogProps,
    ConfirmationDialog,
  };
}
