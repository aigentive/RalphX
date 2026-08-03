import { useCallback, useEffect, useRef } from "react";

import {
  AgentWorkspaceHttpError,
  chatApi,
  type AgentWorkspaceReviewStartConfirmation,
  type AgentWorkspaceReviewStartPreview,
  type AgentWorkspaceReviewContext,
  type AgentWorkspaceReviewFixerConfirmation,
} from "@/api/chat";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import { extractErrorMessage } from "@/lib/errors";
import {
  agentWorkspaceOperationToastId,
  startAgentWorkspaceOperationToast,
  type AgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";
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

function fixerConfirmation(
  context: AgentWorkspaceReviewContext,
): AgentWorkspaceReviewFixerConfirmation | null {
  const target = context.target;
  const monitor = context.monitor;
  if (
    !target ||
    !monitor.reviewArtifactId ||
    !monitor.reviewArtifactVersion ||
    !monitor.reviewBlockingFingerprint
  ) {
    return null;
  }
  return {
    targetScope: target.scope,
    diffFingerprint: target.diffFingerprint,
    artifactId: monitor.reviewArtifactId,
    artifactVersion: monitor.reviewArtifactVersion,
    blockingFingerprint: monitor.reviewBlockingFingerprint,
  };
}

function fixerDescription(context: AgentWorkspaceReviewContext): string {
  return context.monitor.reviewBlockingSummary
    ? `The Repair agent will address: ${context.monitor.reviewBlockingSummary}`
    : "The Repair agent will address the current blocking findings.";
}
function blockedWorkspaceReviewCopy(error: unknown): string | null {
  return error instanceof AgentWorkspaceHttpError &&
    error.status === 409 &&
    error.detail
    ? error.detail
    : null;
}

const GENERIC_PREPARATION_ERROR =
  "Could not prepare this action. Cancel and try again.";

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
  const reviewPreviewRef = useRef<{
    conversationId: string;
    promise: Promise<AgentWorkspaceReviewStartPreview>;
  } | null>(null);
  const reviewToastRef = useRef<AgentWorkspaceOperationToast | null>(null);
  const startReviewRef = useRef<(force: boolean) => void>(() => undefined);

  const loadReviewPreview = useCallback(() => {
    if (!conversationId) {
      return Promise.reject(new Error("Workspace Review is unavailable"));
    }
    const current = reviewPreviewRef.current;
    if (current?.conversationId === conversationId) {
      return current.promise;
    }
    const promise = chatApi.getAgentWorkspaceReviewStartPreview(conversationId);
    reviewPreviewRef.current = { conversationId, promise };
    void promise.catch(() => {
      if (reviewPreviewRef.current?.promise === promise) {
        reviewPreviewRef.current = null;
      }
    });
    return promise;
  }, [conversationId]);

  const clearReviewPreview = useCallback(() => {
    if (reviewPreviewRef.current?.conversationId === conversationId) {
      reviewPreviewRef.current = null;
    }
  }, [conversationId]);

  const startReviewToast = useCallback(
    (detail: string) => {
      if (!conversationId) return null;
      reviewToastRef.current?.dismiss();
      const nextToast = startAgentWorkspaceOperationToast({
        detail,
        id: agentWorkspaceOperationToastId(conversationId, "workspace-review"),
        title: "Starting Workspace Review",
      });
      reviewToastRef.current = nextToast;
      return nextToast;
    },
    [conversationId],
  );

  const startReview = useCallback(
    (force: boolean) => {
      if (!conversationId) return;
      void confirmRoleRuntime({
        role: "workspace_reviewer",
        title: "Start Workspace Review?",
        description: "Preparing the reviewer runtime…",
        confirmText: "Start review",
        pendingText: "Checking review details…",
        prepareDescription: async () => {
          const preview = await loadReviewPreview();
          return reviewStartDescription(preview);
        },
        recoverFromPrepareError: (error) => {
          const description = blockedWorkspaceReviewCopy(error);
          return description ? { description, confirmDisabled: true } : null;
        },
        closeOnConfirm: true,
        onIntent: () => {
          startReviewToast("Checking the current review target and auto-merge state");
        },
        onConfirm: async (runtimeOverride) => {
          const preview = await loadReviewPreview();
          const reviewToast = reviewToastRef.current;
          reviewToast?.update({
            detail: "Submitting the current review receipt",
            title: "Starting Workspace Review",
          });
          await onStartReview({
            force,
            confirmation: preview.confirmation,
            runtimeOverride,
          });
          if (reviewToastRef.current === reviewToast) {
            reviewToastRef.current = null;
          }
          reviewToast?.success("Workspace Review started");
        },
        onErrorAfterClose: (error) => {
          const reviewToast = reviewToastRef.current;
          if (reviewToastRef.current === reviewToast) {
            reviewToastRef.current = null;
          }
          if (!isWorkspaceReviewStartConflict(error)) {
            reviewToast?.error("Workspace Review did not start", {
              detail: extractErrorMessage(
                error,
                "Check the Review tab and try again.",
              ),
            });
            return;
          }
          reviewToast?.info("Review details changed", {
            detail: "Refreshing the confirmation before retrying.",
          });
          clearReviewPreview();
          startReviewRef.current(force);
        },
      });
    },
    [
      clearReviewPreview,
      confirmRoleRuntime,
      conversationId,
      loadReviewPreview,
      onStartReview,
      startReviewToast,
    ],
  );
  useEffect(() => {
    startReviewRef.current = startReview;
  }, [startReview]);

  const prefetchStartReview = useCallback(() => {
    void loadReviewPreview().catch(() => undefined);
  }, [loadReviewPreview]);

  const startFixer = useCallback(
    (context: AgentWorkspaceReviewContext) => {
      if (!conversationId) return;
      let fixerContext = context;
      let confirmation = fixerConfirmation(fixerContext);
      if (!confirmation) return;
      void confirmRoleRuntime({
        role: "workspace_repair",
        title: "Fix blocking Workspace Review issues?",
        description: fixerDescription(fixerContext),
        confirmText: "Fix issues",
        pendingText: "Starting repair…",
        onConfirm: (runtimeOverride) => {
          if (!confirmation) {
            throw new Error("Workspace Review blocker is no longer actionable");
          }
          return onStartFixer({ confirmation, runtimeOverride });
        },
        recoverFromError: async (error) => {
          if (!isWorkspaceReviewStartConflict(error)) {
            return null;
          }
          try {
            fixerContext = await chatApi.getAgentWorkspaceReviewContext(
              conversationId,
              { refreshTarget: true },
            );
          } catch (refreshError) {
            confirmation = null;
            return {
              description:
                blockedWorkspaceReviewCopy(refreshError) ?? GENERIC_PREPARATION_ERROR,
              confirmDisabled: true,
            };
          }
          confirmation = fixerConfirmation(fixerContext);
          if (!confirmation) {
            return {
              description:
                "The blocking Review changed and is no longer actionable. Refresh the Review tab before trying again.",
              confirmDisabled: true,
            };
          }
          return {
            description: `The blocking Review changed. ${fixerDescription(fixerContext)} Confirm the updated details to start the repair.`,
          };
        },
      });
    },
    [confirmRoleRuntime, conversationId, onStartFixer],
  );

  return {
    startReview,
    prefetchStartReview,
    startFixer,
    confirmationDialogProps,
    ConfirmationDialog,
  };
}
