import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";

import { useRoleRuntimeConfirmation } from "./useRoleRuntimeConfirmation";
import { PlanContinuationCommittedError } from "./agentPlanProposalActivation";

function recoverCommittedContinuation(error: unknown) {
  if (!(error instanceof PlanContinuationCommittedError)) return null;
  return {
    description: error.message,
    confirmDisabled: false,
    bodyDisabled: true,
  };
}

export function useApprovedPlanContinuation({
  conversationId,
  projectId,
}: {
  conversationId: string | null;
  projectId: string | null;
}) {
  const runtimeConfirmation = useRoleRuntimeConfirmation({
    conversationId,
    projectId,
  });

  return {
    confirmImplementDirectly: (
      onConfirm: (selection: ManualRoleRuntimeSelection) => Promise<unknown>,
    ) =>
      runtimeConfirmation.confirmRoleRuntime({
        role: "workspace_edit",
        title: "Implement approved plan directly?",
        description:
          "Review the Workspace → Edit runtime before leaving Plan mode and starting implementation.",
        confirmText: "Implement directly",
        pendingText: "Starting implementation…",
        onConfirm,
        recoverFromError: recoverCommittedContinuation,
      }),
    confirmCreateProposals: (
      onConfirm: (selection: ManualRoleRuntimeSelection) => Promise<unknown>,
    ) =>
      runtimeConfirmation.confirmRoleRuntime({
        role: "ideation_primary",
        title: "Create task proposals from this plan?",
        description:
          "Review the Ideation → Primary runtime. The workspace will enter Tasks mode while this role creates proposals.",
        confirmText: "Create proposals",
        pendingText: "Creating proposals…",
        onConfirm,
        recoverFromError: recoverCommittedContinuation,
      }),
    confirmationDialogProps: runtimeConfirmation.confirmationDialogProps,
    ConfirmationDialog: runtimeConfirmation.ConfirmationDialog,
  };
}
