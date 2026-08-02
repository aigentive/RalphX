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
import { WORKSPACE_REVIEW_AUTOMATION_COPY } from "./workspaceReviewAutomationCopy";

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
  reviewAutomation = null,
}: {
  conversationId: string | null;
  projectId: string | null;
  onStartReview: (input: {
    force: boolean;
    confirmation: AgentWorkspaceReviewStartConfirmation;
    runtimeOverride: ManualRoleRuntimeSelection;
    enableReviewAutomation?: boolean;
  }) => Promise<unknown>;
  reviewAutomation?: {
    effectiveLoopActive: boolean;
    overrideOn: boolean;
  } | null;
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
        recoverFromPrepareError: (error) => {
          const description = blockedWorkspaceReviewCopy(error);
          return description ? { description, confirmDisabled: true } : null;
        },
        ...(reviewAutomation
          ? {
              optIn: {
                title: "Auto Review & Fix until passing",
                description: `${WORKSPACE_REVIEW_AUTOMATION_COPY} Applies to this conversation and stays on until you turn it off.`,
                initialValue: reviewAutomation.overrideOn,
                hidden: reviewAutomation.effectiveLoopActive,
              },
            }
          : {}),
        onConfirm: async (runtimeOverride, optInEnabled) => {
          if (!preview) {
            throw new Error("Workspace Review preparation did not finish");
          }
          await onStartReview({
            force,
            confirmation: preview.confirmation,
            runtimeOverride,
            ...(!reviewAutomation?.overrideOn && optInEnabled
              ? { enableReviewAutomation: true }
              : {}),
          });
        },
        recoverFromError: async (error) => {
          if (!isWorkspaceReviewStartConflict(error)) {
            return null;
          }
          try {
            const refreshedPreview =
              await chatApi.getAgentWorkspaceReviewStartPreview(conversationId);
            preview = refreshedPreview;
            return {
              description: `The review target changed. ${reviewStartDescription(refreshedPreview)} Confirm the updated details to start the review.`,
              confirmDisabled: false,
            };
          } catch (refreshError) {
            preview = null;
            return {
              description:
                blockedWorkspaceReviewCopy(refreshError) ?? GENERIC_PREPARATION_ERROR,
              confirmDisabled: true,
            };
          }
        },
      });
    },
    [confirmRoleRuntime, conversationId, onStartReview, reviewAutomation],
  );

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
    startFixer,
    confirmationDialogProps,
    ConfirmationDialog,
  };
}
