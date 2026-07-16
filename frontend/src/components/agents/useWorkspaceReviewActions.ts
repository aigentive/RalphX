import { useCallback } from "react";

import {
  AgentWorkspaceHttpError,
  chatApi,
  type AgentWorkspaceReviewStartConfirmation,
  type AgentWorkspaceReviewStartPreview,
} from "@/api/chat";
import { useConfirmation } from "@/hooks/useConfirmation";

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
  onStartReview,
}: {
  conversationId: string | null;
  onStartReview: (input: {
    force: boolean;
    confirmation: AgentWorkspaceReviewStartConfirmation;
  }) => Promise<unknown>;
}) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();

  const startReview = useCallback(
    (force: boolean) => {
      if (!conversationId) return;
      let preview: AgentWorkspaceReviewStartPreview | null = null;
      void confirm({
        title: "Start Workspace Review?",
        description: "Checking the current review target and GitHub auto-merge state…",
        confirmText: "Start review",
        pendingText: "Starting review…",
        prepare: async () => {
          preview = await chatApi.getAgentWorkspaceReviewStartPreview(conversationId);
          return { description: reviewStartDescription(preview) };
        },
        onConfirm: async () => {
          if (!preview) {
            throw new Error("Workspace Review preparation did not finish");
          }
          await onStartReview({ force, confirmation: preview.confirmation });
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
    [confirm, conversationId, onStartReview],
  );

  return {
    startReview,
    confirmationDialogProps,
    ConfirmationDialog,
  };
}
